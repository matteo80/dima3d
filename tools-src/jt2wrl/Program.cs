// jt2wrl: converte un file JT (Jupiter Tessellation) in VRML 2.0 per il Gestore WRL.
//
// Uso:    jt2wrl <entrata.jt> <uscita.wrl>
// Uscita: righe "PROGRESS <0..1> <testo>" su stdout durante il lavoro, "DONE <shape> <triangoli>
//         <saltate>" alla fine; avvisi ed errori su stderr. Codice di uscita 0 se riuscito.
//
// Per ogni pezzo si prende il livello di dettaglio più alto; trasformazioni e materiali si
// accumulano scendendo nell'albero e le coordinate sono scritte già in world-space (niente
// Transform nel WRL). Le normali del file passano con `normalIndex`, così restano gli spigoli vivi.

using DLAT.JTReader;
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace Jt2Wrl {

    /// Trasformazione affine: x' = m0 x + m1 y + m2 z + m3, e così via per y' e z'.
    readonly struct Affine {
        public readonly double[] M;
        public Affine(double[] m) { M = m; }
        public static Affine Identity => new Affine(new double[] { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0 });

        /// Matrice JT (16 valori). Lo standard JT usa vettori riga (traslazione in 12..14); se la
        /// traslazione è invece nell'ultima colonna (3, 7, 11) si legge per colonne.
        public static Affine FromJt(Mx4F64 mx) {
            var r = mx.raw;
            bool rowVectors = Math.Abs(r[12]) + Math.Abs(r[13]) + Math.Abs(r[14]) > 0
                              || Math.Abs(r[3]) + Math.Abs(r[7]) + Math.Abs(r[11]) == 0;
            return rowVectors
                ? new Affine(new[] { r[0], r[4], r[8], r[12], r[1], r[5], r[9], r[13], r[2], r[6], r[10], r[14] })
                : new Affine(new[] { r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10], r[11] });
        }

        /// Applica prima `local`, poi questa trasformazione.
        public Affine Then(Affine local) {
            var a = M; var b = local.M; var m = new double[12];
            for (int row = 0; row < 3; row++) {
                for (int col = 0; col < 4; col++) {
                    double v = a[row * 4 + 0] * b[0 * 4 + col] + a[row * 4 + 1] * b[1 * 4 + col] + a[row * 4 + 2] * b[2 * 4 + col];
                    if (col == 3) v += a[row * 4 + 3];
                    m[row * 4 + col] = v;
                }
            }
            return new Affine(m);
        }

        public void Point(double x, double y, double z, out double ox, out double oy, out double oz) {
            ox = M[0] * x + M[1] * y + M[2] * z + M[3];
            oy = M[4] * x + M[5] * y + M[6] * z + M[7];
            oz = M[8] * x + M[9] * y + M[10] * z + M[11];
        }

        /// Direzione (normale): parte lineare, poi normalizzata. Corretta per rotazioni e scale
        /// uniformi, i casi delle matrici di assieme.
        public void Direction(double x, double y, double z, out double ox, out double oy, out double oz) {
            ox = M[0] * x + M[1] * y + M[2] * z;
            oy = M[4] * x + M[5] * y + M[6] * z;
            oz = M[8] * x + M[9] * y + M[10] * z;
            double len = Math.Sqrt(ox * ox + oy * oy + oz * oz);
            if (len > 0) { ox /= len; oy /= len; oz /= len; }
        }
    }

    /// Materiale letto al posto di `MaterialAttributeData` di JTReader. Nei JT 9 con materiale in
    /// versione 2 JTReader legge ogni colore come un solo grigio (un F32), mentre i colori sono
    /// RGBA completi salvo i bit "colore comune" dei Data Flags (come nei JT 8): i valori
    /// slittavano e il diffuso finiva quasi sempre a 0.2, cioè pezzi neri. JT 8 e 10 restano a
    /// JTReader.
    sealed class JtMaterial {
        public ushort dataFlags;
        public short version;
        public RGBA ambientColor, diffuseColor, specularColor, emissionColor;
        public float shininess, reflectivity;
        /// Byte non letti alla fine dell'elemento (0 se il formato è stato letto per intero).
        public long leftover;

        public JtMaterial(Element ele) {
            if (ele.majorVersion != 9) {
                var m = new MaterialAttributeData(ele);
                dataFlags = m.dataFlags;
                ambientColor = m.ambientColor; diffuseColor = m.diffuseColor;
                specularColor = m.specularColor; emissionColor = m.emissionColor;
                shininess = m.shiniess; reflectivity = m.reflectivity;
                return;
            }
            var data = ele.dataStream;
            new BaseAttributeData(ele);
            version = data.ReadI16();
            dataFlags = data.ReadU16();
            ambientColor = (dataFlags & 0x2) != 0 ? new RGBA(data.ReadF32()) : new RGBA(data);
            diffuseColor = new RGBA(data);
            specularColor = (dataFlags & 0x8) != 0 ? new RGBA(data.ReadF32()) : new RGBA(data);
            emissionColor = (dataFlags & 0x4) != 0 ? new RGBA(data.ReadF32()) : new RGBA(data);
            shininess = data.ReadF32();
            if (version >= 2) reflectivity = data.ReadF32();
            leftover = data.Length - data.Position;
        }

        static readonly GUID TypeId = new GUID("10DD1030-2AC8-11D1-9B-6B-00-80-C7-BB-59-97");

        /// Da chiamare prima di aprire qualsiasi file: JTReader crea gli elementi dal tipo
        /// registrato per il loro GUID.
        public static void Register() => ObjectTypeIdentifiers.types[TypeId] = typeof(JtMaterial);
    }

    static class JtFix {
        /// JTReader mette in un solo dizionario gli elementi di tutti i segmenti, tenendo il primo per
        /// ogni identificativo; nei JT 10 i segmenti di proprietà letti prima del grafo riusano gli
        /// stessi identificativi dei nodi, e l'albero finiva per puntare a una proprietà di testo.
        /// Gli elementi del segmento del grafo hanno la precedenza.
        public static LSG ReadGraph(JTFile jt) {
            if (jt.LSGSegment?.elements != null) {
                foreach (var e in jt.LSGSegment.elements)
                    if (e.objectID != -1)
                        jt.elements[e.objectID] = e;
            }
            return new LSG(jt);
        }
    }

    sealed class Converter {
        readonly TextWriter w;
        readonly HashSet<string> openFiles = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        readonly Dictionary<string, int> names = new Dictionary<string, int>();
        static readonly CultureInfo Inv = CultureInfo.InvariantCulture;

        public int Shapes, Triangles, Skipped;
        int visited, totalNodes = 1;
        double lastProgress = -1;

        public Converter(TextWriter writer) { w = writer; }

        public static void Progress(double value, string text) {
            Console.Out.WriteLine("PROGRESS " + value.ToString("0.000", Inv) + " " + text);
            Console.Out.Flush();
        }

        public void ConvertFile(string path, Affine xf, int depth, bool root) {
            var full = Path.GetFullPath(path);
            if (!openFiles.Add(full)) {
                Console.Error.WriteLine("Avviso: riferimento circolare a " + Path.GetFileName(path) + ", ignorato");
                return;
            }
            if (root) Progress(0.02, "Lettura di " + Path.GetFileName(path));
            var jt = new JTFile(full);
            if (root) Progress(0.10, "Struttura del file (JT " + jt.Version + ")");
            var lsg = JtFix.ReadGraph(jt);
            if (root) totalNodes = Math.Max(1, lsg.nodes.Count);
            Visit(lsg, lsg.root, xf, null, depth, Path.GetDirectoryName(full), root);
            openFiles.Remove(full);
        }

        void Visit(LSG lsg, LSGNode node, Affine xf, RGBA color, int depth, string dir, bool countProgress) {
            // Un nodo istanziato più volte è letto una volta sola: le altre copie sono vuote e
            // rimandano a quella completa (stesso id)
            var full = lsg.nodes.TryGetValue(node.id, out var f) ? f : node;
            if (countProgress) {
                visited++;
                double p = 0.10 + 0.88 * Math.Min(1.0, visited / (double)totalNodes);
                if (p - lastProgress >= 0.01) {
                    lastProgress = p;
                    Progress(p, "Conversione: " + Shapes + " pezzi");
                }
            }

            foreach (var attr in full.attributes) {
                object data;
                try { data = attr?.elementData; } catch { continue; }
                if (data is GeometricTransformAttributeData g && g.matrix != null)
                    xf = xf.Then(Affine.FromJt(g.matrix));
                else if (data is JtMaterial m && m.diffuseColor != null)
                    color = m.diffuseColor;
            }

            var elementData = full.elementData;
            var name = NodeName(full);

            if (elementData is BaseShapeNodeData) {
                WriteShape(full, xf, color, name, depth);
                return;
            }

            IEnumerable<LSGNode> children = full.child;
            if (elementData is LODNodeData) {
                // Il primo figlio di un LOD è il livello più dettagliato
                children = full.child.Count > 0 ? new[] { full.child[0] } : Array.Empty<LSGNode>();
            } else if (elementData is SwitchNodeData sw) {
                children = sw.selectedChild < full.child.Count ? new[] { full.child[(int)sw.selectedChild] } : Array.Empty<LSGNode>();
            }

            // Partizione senza figli: l'assieme continua in un altro file JT
            if (elementData is PartitionNodeData part && full.child.Count == 0 && !string.IsNullOrWhiteSpace(part.fileName)) {
                var other = ResolvePartition(dir, part.fileName);
                if (other == null) {
                    Console.Error.WriteLine("Avviso: file dell'assieme non trovato: " + part.fileName);
                    Skipped++;
                    return;
                }
                OpenGroup(name ?? Path.GetFileNameWithoutExtension(other), depth);
                try {
                    ConvertFile(other, xf, depth + 1, false);
                } catch (Exception e) {
                    Console.Error.WriteLine("Avviso: " + Path.GetFileName(other) + " non letto: " + e.Message);
                    Skipped++;
                }
                CloseGroup(depth);
                return;
            }

            if (name != null) OpenGroup(name, depth);
            foreach (var c in children)
                Visit(lsg, c, xf, color, name != null ? depth + 1 : depth, dir, countProgress);
            if (name != null) CloseGroup(depth);
        }

        static string ResolvePartition(string dir, string fileName) {
            var candidates = new[] {
                Path.Combine(dir, fileName),
                Path.Combine(dir, Path.GetFileName(fileName.Replace('/', '\\'))),
            };
            foreach (var c in candidates)
                if (File.Exists(c)) return c;
            return null;
        }

        static string NodeName(LSGNode node) {
            foreach (var key in new[] { "JT_PROP_NAME", "JT_PROP_NAME_PATH" }) {
                if (node.property.TryGetValue(key, out var v) && v != null) {
                    var s = v.ToString();
                    if (!string.IsNullOrWhiteSpace(s) && !(v is DataSegment)) {
                        // "Nome;1;0:" → "Nome"
                        int cut = s.IndexOf(';');
                        return (cut > 0 ? s.Substring(0, cut) : s).Trim();
                    }
                }
            }
            return null;
        }

        void WriteShape(LSGNode node, Affine xf, RGBA color, string name, int depth) {
            List<float> vertices = null, normals = null;
            List<List<int>> indices = null;
            try {
                foreach (var value in node.property.Values) {
                    if (!(value is DataSegment seg) || seg.elements == null) continue;
                    foreach (var ele in seg.elements) {
                        object d;
                        try { d = ele.elementData; } catch { continue; }
                        if (d is Tri_StripSetShapeLODData ts) {
                            vertices = ts.GetVertices(); normals = ts.GetNormals(); indices = ts.GetIndices();
                        } else if (d is VertexShapeLODData vs) {
                            vertices = vs.GetVertices(); normals = vs.GetNormals(); indices = vs.GetIndices();
                        }
                        if (vertices != null) break;
                    }
                    if (vertices != null) break;
                }
            } catch (Exception e) {
                Console.Error.WriteLine("Avviso: pezzo " + (name ?? "senza nome") + " non decodificato: " + e.Message);
                Skipped++;
                return;
            }
            if (vertices == null || indices == null || indices.Count == 0 || vertices.Count < 9) {
                Skipped++;
                return;
            }

            int vertexCount = vertices.Count / 3;
            var tri = indices[0];
            var ni = indices.Count > 1 ? indices[1] : null;
            int normalCount = normals != null ? normals.Count / 3 : 0;
            bool useNormals = ni != null && ni.Count == tri.Count && normalCount > 0;

            // Solo triangoli con indici validi
            var keep = new List<int>(tri.Count / 3);
            for (int t = 0; t + 2 < tri.Count; t += 3) {
                bool ok = true;
                for (int k = 0; k < 3; k++) {
                    int v = tri[t + k];
                    if (v < 0 || v >= vertexCount) ok = false;
                    if (useNormals && (ni[t + k] < 0 || ni[t + k] >= normalCount)) useNormals = false;
                }
                if (ok) keep.Add(t);
            }
            if (keep.Count == 0) {
                Skipped++;
                return;
            }

            // Scrittura compatta: niente rientri né virgole, 7 cifre significative per le
            // coordinate (0,001 mm su 4 m) e 3 decimali per le normali. Il file è meno della metà.
            var ind = "";
            var sb = new StringBuilder(64 + vertexCount * 30 + keep.Count * 24);
            sb.Append(ind).Append("DEF ").Append(Unique(name ?? "Pezzo")).Append(" Shape {\n");
            float r = 0.75f, g = 0.78f, b = 0.84f, a = 1f;
            if (color != null) { r = color.r; g = color.g; b = color.b; a = color.a; }
            float transparency = Math.Clamp(1f - a, 0f, 1f);
            sb.Append(ind).Append("appearance Appearance { material Material { diffuseColor ")
              .Append(F(r)).Append(' ').Append(F(g)).Append(' ').Append(F(b));
            if (transparency > 0.0005f) sb.Append(" transparency ").Append(F(transparency));
            sb.Append(" } }\n");
            sb.Append(ind).Append("geometry IndexedFaceSet {\n");
            sb.Append(ind).Append("solid FALSE\n");
            sb.Append(ind).Append("coord Coordinate { point [\n");
            for (int v = 0; v < vertexCount; v++) {
                xf.Point(vertices[v * 3], vertices[v * 3 + 1], vertices[v * 3 + 2], out var x, out var y, out var z);
                sb.Append(F((float)x)).Append(' ').Append(F((float)y)).Append(' ').Append(F((float)z)).Append('\n');
            }
            sb.Append(ind).Append("] }\n");
            sb.Append(ind).Append("coordIndex [\n");
            foreach (int t in keep)
                sb.Append(tri[t]).Append(' ').Append(tri[t + 1]).Append(' ').Append(tri[t + 2]).Append(" -1\n");
            sb.Append(ind).Append("]\n");
            if (useNormals) {
                sb.Append(ind).Append("normal Normal { vector [\n");
                for (int n = 0; n < normalCount; n++) {
                    xf.Direction(normals[n * 3], normals[n * 3 + 1], normals[n * 3 + 2], out var x, out var y, out var z);
                    sb.Append(N((float)x)).Append(' ').Append(N((float)y)).Append(' ').Append(N((float)z)).Append('\n');
                }
                sb.Append(ind).Append("] }\n");
                sb.Append(ind).Append("normalIndex [\n");
                foreach (int t in keep)
                    sb.Append(ni[t]).Append(' ').Append(ni[t + 1]).Append(' ').Append(ni[t + 2]).Append(" -1\n");
                sb.Append(ind).Append("]\n");
            } else {
                sb.Append(ind).Append("creaseAngle 0.5\n");
            }
            sb.Append(ind).Append("}\n");
            sb.Append(ind).Append("}\n");
            w.Write(sb.ToString());
            Shapes++;
            Triangles += keep.Count;
        }

        void OpenGroup(string name, int depth) {
            w.Write("DEF " + Unique(name) + " Group { children [\n");
        }

        void CloseGroup(int depth) {
            w.Write("] }\n");
        }

        /// Coordinata con 7 cifre significative, senza zeri inutili.
        static string F(float v) => v == 0 ? "0" : v.ToString("G7", Inv);

        /// Componente di una normale con 3 decimali (errore massimo di circa 0,05°).
        static string N(float v) {
            var s = v.ToString("0.###", Inv);
            return s == "-0" ? "0" : s;
        }

        /// Identificatore VRML valido e unico nel file (come `sanitize_identifier` del Gestore).
        string Unique(string label) {
            var sb = new StringBuilder(label.Length + 1);
            foreach (var c in label)
                sb.Append(char.IsLetterOrDigit(c) || c == '_' || c == '-' || c == '.' ? c : '_');
            var s = sb.Length == 0 ? "_" : sb.ToString();
            if (char.IsDigit(s[0]) || s[0] == '-' || s[0] == '.' || s[0] == '+') s = "_" + s;
            switch (s) {
                case "DEF": case "USE": case "PROTO": case "EXTERNPROTO": case "IS": case "ROUTE": case "TO":
                case "TRUE": case "FALSE": case "NULL": s = "_" + s; break;
            }
            names.TryGetValue(s, out var n);
            names[s] = n + 1;
            return n == 0 ? s : s + "_" + (n + 1);
        }
    }

    static class Program {
        /// `jt2wrl --diag file.jt`: albero del file con tipi, proprietà e contenuto dei segmenti.
        static void Diagnose(string path) {
            var jt = new JTFile(path);
            Console.WriteLine("Versione " + jt.Version + ", segmenti " + jt.segments.Count + ", elementi " + jt.elements.Count);
            var lsg = JtFix.ReadGraph(jt);
            var seen = new HashSet<LSGNode>();
            void Walk(LSGNode n, int d) {
                var full = lsg.nodes.TryGetValue(n.id, out var f) ? f : n;
                var ind = new string(' ', d * 2);
                Console.WriteLine(ind + full.id + " " + (full.elementData?.GetType().Name ?? "null") + " attr=" + full.attributes.Count + " figli=" + full.child.Count);
                foreach (var a in full.attributes) {
                    var ad = a?.elementData;
                    var extra = "";
                    if (ad is JtMaterial m) {
                        string C(RGBA c) => c == null ? "-" : string.Format(CultureInfo.InvariantCulture, "{0:0.###} {1:0.###} {2:0.###} {3:0.###}", c.r, c.g, c.b, c.a);
                        extra = " JT" + a.majorVersion + " v" + m.version + " flags=0x" + m.dataFlags.ToString("X") + " amb=" + C(m.ambientColor) + " dif=" + C(m.diffuseColor)
                                + " spe=" + C(m.specularColor) + " emi=" + C(m.emissionColor) + " shin=" + m.shininess.ToString(CultureInfo.InvariantCulture) + (m.leftover != 0 ? " AVANZO=" + m.leftover : "");
                    }
                    Console.WriteLine(ind + "  @ " + (ad?.GetType().Name ?? "null") + extra);
                }
                foreach (var p in full.property) {
                    var v = p.Value;
                    string desc = v is DataSegment s
                        ? "segmento tipo " + s.segmentType + " elementi: " + string.Join(",", (s.elements ?? new List<Element>()).ConvertAll(e => { try { return e.elementData?.GetType().Name ?? ("? " + e.objectTypeID); } catch (Exception ex) { return "ERRORE " + ex.Message; } }))
                        : (v?.GetType().Name + " = " + v);
                    Console.WriteLine(ind + "  # " + p.Key + " : " + desc);
                }
                if (!seen.Add(full)) return;
                foreach (var c in full.child) Walk(c, d + 1);
            }
            Walk(lsg.root, 0);
            var counts = new Dictionary<string, int>();
            foreach (var seg in jt.segments.Values) {
                if (seg.elements == null) continue;
                foreach (var e in seg.elements) {
                    string k; try { k = "tipo seg " + seg.segmentType + ": " + (e.elementData?.GetType().Name ?? ("sconosciuto " + e.objectTypeID)); } catch (Exception ex) { k = "tipo seg " + seg.segmentType + ": ERRORE " + ex.GetType().Name + " " + ex.Message; }
                    counts.TryGetValue(k, out var n); counts[k] = n + 1;
                }
            }
            Console.WriteLine("--- elementi nei segmenti:");
            foreach (var kv in counts) Console.WriteLine("  " + kv.Value + " × " + kv.Key);
        }

        static int Main(string[] args) {
            JtMaterial.Register();
            if (args.Length == 2 && args[0] == "--diag") {
                Diagnose(args[1]);
                return 0;
            }
            if (args.Length != 2) {
                Console.Error.WriteLine("Uso: jt2wrl <entrata.jt> <uscita.wrl>");
                return 1;
            }
            var input = args[0];
            var output = args[1];
            if (!File.Exists(input)) {
                Console.Error.WriteLine("File non trovato: " + input);
                return 1;
            }
            var temp = output + ".parziale";
            bool empty = false;
            try {
                using (var stream = new FileStream(temp, FileMode.Create, FileAccess.Write, FileShare.None, 1 << 20))
                using (var w = new StreamWriter(stream, new UTF8Encoding(false), 1 << 20)) {
                    w.NewLine = "\n";
                    w.Write("#VRML V2.0 utf8\n# Convertito da " + Path.GetFileName(input) + " con jt2wrl\n\n");
                    var conv = new Converter(w);
                    conv.ConvertFile(input, Affine.Identity, 0, true);
                    w.Flush();
                    Converter.Progress(1.0, "Conversione completata");
                    Console.Out.WriteLine("DONE " + conv.Shapes + " " + conv.Triangles + " " + conv.Skipped);
                    empty = conv.Shapes == 0;
                }
                if (empty) {
                    // Il file temporaneo si cancella dopo averlo chiuso
                    File.Delete(temp);
                    Console.Error.WriteLine(
                        "Nessun pezzo con geometria a triangoli nel file: potrebbe contenere solo la geometria " +
                        "esatta (B-rep/XT), che jt2wrl non converte.");
                    return 3;
                }
                File.Move(temp, output, true);
                return 0;
            } catch (Exception e) {
                try { if (File.Exists(temp)) File.Delete(temp); } catch { }
                var msg = e.Message.Contains("big endian")
                    ? "File JT salvato in formato big-endian: non supportato."
                    : "Conversione non riuscita: " + e.Message;
                Console.Error.WriteLine(msg);
                return 2;
            }
        }
    }
}
