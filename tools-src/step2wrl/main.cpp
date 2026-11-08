// step2wrl — convertitore STEP → VRML 2.0 per il Gestore WRL.
//
// Legge lo STEP con OpenCASCADE (assieme, nomi e colori tramite XCAF), tassella la geometria esatta
// con le tolleranze scelte (come le "Opzioni di mesh" Standard di FreeCAD: deviazione di
// superficie in mm, deviazione angolare in gradi, eventualmente relativa) e scrive un WRL con un
// Group per ogni assieme e uno Shape per ogni pezzo, con le coordinate già trasformate.
//
// Uso: step2wrl <entrata.stp> <uscita.wrl> [--deviazione 0.1] [--angolo 30] [--relativa]
// Stdout: righe "PROGRESS <0..1> <testo>", alla fine "DONE <pezzi> <triangoli> <saltati>".
// Stderr: avvisi ("Avviso: ...") ed errori. Uscita: 0 riuscito, 1 argomenti, 2 errore di lettura,
// 3 nessun pezzo con triangoli, 4 errore di scrittura.

// Prima di OpenCASCADE, che include una versione ridotta di windows.h
#ifdef _WIN32
#define NOMINMAX
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Poly_Triangulation.hxx>
#include <Quantity_ColorRGBA.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <TDF_ChildIterator.hxx>
#include <NCollection_Sequence.hxx>
#include <TDF_Label.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <map>
#include <set>
#include <string>
#include <vector>

namespace {

using Labels = NCollection_Sequence<TDF_Label>;

struct Rgba {
    float r = 0.75f, g = 0.78f, b = 0.84f, a = 1.0f;
    bool operator<(const Rgba& o) const {
        if (r != o.r) return r < o.r;
        if (g != o.g) return g < o.g;
        if (b != o.b) return b < o.b;
        return a < o.a;
    }
};

struct Options {
    double deviation = 0.1;  // mm (o frazione, se relativa)
    double angle_deg = 30.0;
    bool relative = false;
};

void progress(double value, const std::string& text) {
    std::printf("PROGRESS %.3f %s\n", std::clamp(value, 0.0, 1.0), text.c_str());
    std::fflush(stdout);
}

void warn(const std::string& text) {
    std::fprintf(stderr, "Avviso: %s\n", text.c_str());
}

/// Coordinata con 7 cifre significative, senza zeri inutili (come jt2wrl).
void put_coord(std::string& s, double v) {
    if (v == 0.0 || std::fabs(v) < 1e-12) {
        s += '0';
        return;
    }
    char buf[32];
    std::snprintf(buf, sizeof buf, "%.7g", v);
    s += buf;
}

/// Componente di una normale con 3 decimali.
void put_normal(std::string& s, double v) {
    char buf[16];
    std::snprintf(buf, sizeof buf, "%.3f", v);
    // toglie gli zeri finali: "0.500" → "0.5", "1.000" → "1"
    std::string t = buf;
    while (!t.empty() && t.back() == '0') t.pop_back();
    if (!t.empty() && t.back() == '.') t.pop_back();
    if (t == "-0" || t.empty()) t = "0";
    s += t;
}

class Converter {
public:
    Converter(const Options& opt, FILE* out) : opt_(opt), out_(out) {}

    int run(const std::string& input) {
        progress(0.0, "Lettura STEP...");
        Handle(TDocStd_Document) doc;
        XCAFApp_Application::GetApplication()->NewDocument("MDTV-XCAF", doc);
        // Unità del documento: millimetri (lo STEP viene convertito se è in altre unità)
        XCAFDoc_DocumentTool::SetLengthUnit(doc, 1.0, UnitsMethods_LengthUnit_Millimeter);

        STEPCAFControl_Reader reader;
        reader.SetColorMode(true);
        reader.SetNameMode(true);
        reader.SetLayerMode(false);
        reader.SetPropsMode(false);
        reader.SetGDTMode(false);
        reader.SetMatMode(false);
        reader.SetViewMode(false);
        if (reader.ReadFile(input.c_str()) != IFSelect_RetDone) {
            std::fprintf(stderr, "File STEP non leggibile o danneggiato.\n");
            return 2;
        }
        progress(0.15, "Costruzione della geometria...");
        if (!reader.Transfer(doc)) {
            std::fprintf(stderr, "Nessuna geometria trasferita dal file STEP.\n");
            return 2;
        }
        shapes_ = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
        colors_ = XCAFDoc_DocumentTool::ColorTool(doc->Main());

        Labels roots;
        shapes_->GetFreeShapes(roots);
        if (roots.IsEmpty()) {
            std::fprintf(stderr, "Il file STEP non contiene forme.\n");
            return 3;
        }

        // Pezzi distinti da tassellare (le istanze ripetute condividono la tassellazione)
        for (Labels::Iterator it(roots); it.More(); it.Next()) collect_parts(it.Value(), 0);
        mesh_parts();

        progress(0.9, "Scrittura WRL...");
        std::fputs("#VRML V2.0 utf8\n# Convertito da STEP da step2wrl (OpenCASCADE)\n", out_);
        for (Labels::Iterator it(roots); it.More(); it.Next())
            visit(it.Value(), TopLoc_Location(), Rgba(), false, 0);
        if (std::ferror(out_)) {
            std::fprintf(stderr, "Scrittura del WRL non riuscita.\n");
            return 4;
        }
        progress(1.0, "Fatto");
        std::printf("DONE %d %lld %d\n", shape_count_, static_cast<long long>(triangle_count_), skipped_);
        if (shape_count_ == 0) {
            std::fprintf(stderr, "Nessun pezzo con superfici nel file STEP.\n");
            return 3;
        }
        return 0;
    }

private:
    const Options opt_;
    FILE* out_;
    Handle(XCAFDoc_ShapeTool) shapes_;
    Handle(XCAFDoc_ColorTool) colors_;
    std::vector<TopoDS_Shape> parts_;
    std::set<const void*> part_seen_;
    std::map<std::string, int> names_;
    int shape_count_ = 0;
    long long triangle_count_ = 0;
    int skipped_ = 0;

    void collect_parts(const TDF_Label& label, int depth) {
        if (depth > 200) return;
        TDF_Label ref = label;
        if (shapes_->IsReference(label)) shapes_->GetReferredShape(label, ref);
        if (shapes_->IsAssembly(ref)) {
            Labels comps;
            shapes_->GetComponents(ref, comps);
            for (Labels::Iterator it(comps); it.More(); it.Next()) collect_parts(it.Value(), depth + 1);
            return;
        }
        TopoDS_Shape shape = shapes_->GetShape(ref);
        if (shape.IsNull()) return;
        if (part_seen_.insert(shape.TShape().get()).second) parts_.push_back(shape);
    }

    void mesh_parts() {
        const double angle = opt_.angle_deg * M_PI / 180.0;
        const size_t n = parts_.size();
        for (size_t i = 0; i < n; ++i) {
            if (i % 8 == 0 || i + 1 == n)
                progress(0.3 + 0.6 * double(i) / double(n),
                         "Tassellazione: pezzo " + std::to_string(i + 1) + " di " + std::to_string(n));
            try {
                BRepMesh_IncrementalMesh mesher(parts_[i], opt_.deviation, opt_.relative, angle, true);
            } catch (const Standard_Failure& e) {
                warn(std::string("tassellazione non riuscita su un pezzo: ") + e.what());
            }
        }
    }

    std::string label_name(const TDF_Label& label) const {
        Handle(TDataStd_Name) attr;
        if (!label.FindAttribute(TDataStd_Name::GetID(), attr)) return {};
        TCollection_AsciiString s(attr->Get());  // UTF-8
        std::string name = s.ToCString();
        // Nomi automatici di OpenCASCADE ("=>[0:1:1:2]") o vuoti: non servono
        if (name.rfind("=>", 0) == 0) return {};
        return name;
    }

    bool label_color(const TDF_Label& label, Rgba& c) const {
        Quantity_ColorRGBA q;
        if (colors_->GetColor(label, XCAFDoc_ColorSurf, q) || colors_->GetColor(label, XCAFDoc_ColorGen, q)) {
            set(c, q);
            return true;
        }
        return false;
    }

    bool shape_color(const TopoDS_Shape& shape, Rgba& c) const {
        Quantity_ColorRGBA q;
        if (colors_->GetColor(shape, XCAFDoc_ColorSurf, q) || colors_->GetColor(shape, XCAFDoc_ColorGen, q)) {
            set(c, q);
            return true;
        }
        return false;
    }

    static void set(Rgba& c, const Quantity_ColorRGBA& q) {
        // Valori sRGB come nel file STEP (OpenCASCADE li tiene in RGB lineare)
        double r, g, b;
        q.GetRGB().Values(r, g, b, Quantity_TOC_sRGB);
        c = {float(r), float(g), float(b), q.Alpha()};
    }

    /// Identificatore VRML valido e unico nel file (come `sanitize_identifier` del Gestore).
    std::string unique(const std::string& label) {
        std::string s;
        for (unsigned char ch : label) {
            bool ok = (ch < 128 && (std::isalnum(ch) || ch == '_' || ch == '-'));
            s += ok ? char(ch) : '_';
        }
        if (s.empty()) s = "_";
        if (std::isdigit(static_cast<unsigned char>(s[0])) || s[0] == '-' || s[0] == '.' || s[0] == '+') s = "_" + s;
        static const char* reserved[] = {"DEF", "USE", "PROTO", "EXTERNPROTO", "IS", "ROUTE", "TO", "TRUE", "FALSE", "NULL"};
        for (auto r : reserved)
            if (s == r) s = "_" + s;
        int& n = names_[s];
        ++n;
        return n == 1 ? s : s + "_" + std::to_string(n);
    }

    void visit(const TDF_Label& label, const TopLoc_Location& parent, Rgba color, bool has_color, int depth) {
        if (depth > 200) return;
        TDF_Label ref = label;
        TopLoc_Location loc = parent;
        std::string name = label_name(label);
        if (shapes_->IsReference(label)) {
            loc = parent * shapes_->GetLocation(label);
            shapes_->GetReferredShape(label, ref);
            // Colore dell'istanza, poi quello del pezzo
            Rgba c;
            if (label_color(label, c)) { color = c; has_color = true; }
            std::string ref_name = label_name(ref);
            if (!ref_name.empty()) name = ref_name;
        }
        if (!colors_->IsVisible(ref)) return;
        Rgba c;
        if (label_color(ref, c)) { color = c; has_color = true; }

        if (shapes_->IsAssembly(ref)) {
            Labels comps;
            shapes_->GetComponents(ref, comps);
            std::fprintf(out_, "DEF %s Group { children [\n", unique(name.empty() ? "Assieme" : name).c_str());
            for (Labels::Iterator it(comps); it.More(); it.Next())
                visit(it.Value(), loc, color, has_color, depth + 1);
            std::fputs("] }\n", out_);
            return;
        }
        TopoDS_Shape shape = shapes_->GetShape(ref);
        if (shape.IsNull()) return;
        // Le facce si leggono dalla forma del pezzo (i colori delle facce sono registrati lì);
        // la posizione dell'istanza si applica alle coordinate
        write_part(shape, loc, name.empty() ? "Pezzo" : name, color);
    }

    struct Mesh {
        std::vector<double> points;   // xyz
        std::vector<double> normals;  // xyz, una per punto
        std::vector<int> tris;
    };

    void write_part(const TopoDS_Shape& shape, const TopLoc_Location& loc, const std::string& name, Rgba color) {
        // Facce raggruppate per colore: di solito uno solo; se le facce hanno colori propri il pezzo
        // diventa un gruppo con uno Shape per colore.
        std::map<Rgba, Mesh> by_color;
        int faces = 0, empty = 0;
        for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next()) {
            const TopoDS_Face& face = TopoDS::Face(ex.Current());
            ++faces;
            TopLoc_Location floc;
            Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, floc);
            if (tri.IsNull() || tri->NbTriangles() == 0) {
                ++empty;
                continue;
            }
            Rgba fc = color;
            Rgba own;
            if (shape_color(face, own)) fc = own;
            Mesh& m = by_color[fc];
            if (!tri->HasNormals()) BRepLib_ToolTriangulatedShape::ComputeNormals(face, tri);
            const gp_Trsf trsf = loc.Transformation() * floc.Transformation();
            const bool reversed = face.Orientation() == TopAbs_REVERSED;
            const int base = int(m.points.size() / 3);
            for (int i = 1; i <= tri->NbNodes(); ++i) {
                gp_Pnt p = tri->Node(i).Transformed(trsf);
                m.points.insert(m.points.end(), {p.X(), p.Y(), p.Z()});
                gp_Dir d(0, 0, 1);
                if (tri->HasNormals()) {
                    gp_Vec v(tri->Normal(i));
                    if (v.SquareMagnitude() > 1e-20) d = gp_Dir(v.Transformed(trsf));
                }
                if (reversed) d.Reverse();
                m.normals.insert(m.normals.end(), {d.X(), d.Y(), d.Z()});
            }
            for (int t = 1; t <= tri->NbTriangles(); ++t) {
                int a, b, c;
                tri->Triangle(t).Get(a, b, c);
                if (reversed) std::swap(b, c);
                m.tris.insert(m.tris.end(), {base + a - 1, base + b - 1, base + c - 1});
            }
        }
        if (by_color.empty()) {
            ++skipped_;
            if (faces > 0) warn("pezzo " + name + " senza tassellazione (" + std::to_string(empty) + " facce)");
            return;
        }
        if (by_color.size() == 1) {
            write_shape(name, by_color.begin()->first, by_color.begin()->second);
        } else {
            std::fprintf(out_, "DEF %s Group { children [\n", unique(name).c_str());
            for (auto& [c, m] : by_color) write_shape(name, c, m);
            std::fputs("] }\n", out_);
        }
    }

    void write_shape(const std::string& name, const Rgba& c, const Mesh& m) {
        std::string s;
        s.reserve(64 + m.points.size() * 12 + m.tris.size() * 8);
        s += "DEF " + unique(name) + " Shape {\nappearance Appearance { material Material { diffuseColor ";
        put_coord(s, c.r); s += ' '; put_coord(s, c.g); s += ' '; put_coord(s, c.b);
        float transparency = std::clamp(1.0f - c.a, 0.0f, 1.0f);
        if (transparency > 0.0005f) { s += " transparency "; put_coord(s, transparency); }
        s += " } }\ngeometry IndexedFaceSet {\nsolid FALSE\ncoord Coordinate { point [\n";
        for (size_t i = 0; i < m.points.size(); i += 3) {
            put_coord(s, m.points[i]); s += ' ';
            put_coord(s, m.points[i + 1]); s += ' ';
            put_coord(s, m.points[i + 2]); s += '\n';
        }
        s += "] }\ncoordIndex [\n";
        std::string idx;
        for (size_t t = 0; t < m.tris.size(); t += 3) {
            idx += std::to_string(m.tris[t]); idx += ' ';
            idx += std::to_string(m.tris[t + 1]); idx += ' ';
            idx += std::to_string(m.tris[t + 2]); idx += " -1\n";
        }
        s += idx;
        s += "]\nnormal Normal { vector [\n";
        for (size_t i = 0; i < m.normals.size(); i += 3) {
            put_normal(s, m.normals[i]); s += ' ';
            put_normal(s, m.normals[i + 1]); s += ' ';
            put_normal(s, m.normals[i + 2]); s += '\n';
        }
        // Normali per vertice con gli stessi indici dei punti: spigoli vivi tra le facce
        s += "] }\nnormalIndex [\n";
        s += idx;
        s += "]\n}\n}\n";
        std::fwrite(s.data(), 1, s.size(), out_);
        ++shape_count_;
        triangle_count_ += static_cast<long long>(m.tris.size() / 3);
    }
};

#ifdef _WIN32
std::string utf8(const wchar_t* w) {
    int n = WideCharToMultiByte(CP_UTF8, 0, w, -1, nullptr, 0, nullptr, nullptr);
    std::string s(n > 0 ? n - 1 : 0, '\0');
    if (n > 1) WideCharToMultiByte(CP_UTF8, 0, w, -1, s.data(), n, nullptr, nullptr);
    return s;
}
#endif

int usage() {
    std::fprintf(stderr, "Uso: step2wrl <entrata.stp> <uscita.wrl> [--deviazione mm] [--angolo gradi] [--relativa]\n");
    return 1;
}

}  // namespace

#ifdef _WIN32
int wmain(int argc, wchar_t** wargv) {
    SetConsoleOutputCP(CP_UTF8);
    std::vector<std::string> args;
    for (int i = 1; i < argc; ++i) args.push_back(utf8(wargv[i]));
#else
int main(int argc, char** argv) {
    std::vector<std::string> args(argv + 1, argv + argc);
#endif
    if (args.size() < 2) return usage();
    Options opt;
    for (size_t i = 2; i < args.size(); ++i) {
        const std::string& a = args[i];
        try {
            if (a == "--deviazione" && i + 1 < args.size()) opt.deviation = std::stod(args[++i]);
            else if (a == "--angolo" && i + 1 < args.size()) opt.angle_deg = std::stod(args[++i]);
            else if (a == "--relativa") opt.relative = true;
            else return usage();
        } catch (...) {
            return usage();
        }
    }
    if (!(opt.deviation > 0) || !(opt.angle_deg > 0)) return usage();

    // Messaggi di OpenCASCADE: non devono finire su stdout (lo legge il Gestore)
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_Printer));

#ifdef _WIN32
    std::wstring wout;
    {
        int n = MultiByteToWideChar(CP_UTF8, 0, args[1].c_str(), -1, nullptr, 0);
        wout.resize(n > 0 ? n - 1 : 0);
        if (n > 1) MultiByteToWideChar(CP_UTF8, 0, args[1].c_str(), -1, wout.data(), n);
    }
    FILE* out = nullptr;
    if (_wfopen_s(&out, wout.c_str(), L"wb") != 0) out = nullptr;
#else
    FILE* out = std::fopen(args[1].c_str(), "wb");
#endif
    if (!out) {
        std::fprintf(stderr, "Impossibile creare il file WRL.\n");
        return 4;
    }
    std::setvbuf(out, nullptr, _IOFBF, 1 << 20);
    int code;
    try {
        Converter conv(opt, out);
        code = conv.run(args[0]);
    } catch (const Standard_Failure& e) {
        std::fprintf(stderr, "Errore di OpenCASCADE: %s\n", e.what());
        code = 2;
    } catch (const std::exception& e) {
        std::fprintf(stderr, "Errore: %s\n", e.what());
        code = 2;
    }
    if (std::fclose(out) != 0 && code == 0) code = 4;
    return code;
}
