# STATO PROGETTO — Gestore-WRL

> Applicazione desktop nativa Rust per la visualizzazione, analisi e manipolazione interattiva  
> di file VRML V2.0 (`.wrl`) di grandi dimensioni (fino a 600+ MB).  
> **Aggiornato:** 2026-11-07 · **Fatti:** visualizzazione, albero, colori, export, rendering veloce, rotazione OpenInventor (confermata dall'utente), Step 1 completo (exe provato dall'utente su un altro PC), lettura VRML 1.0, interfaccia dello Step 2, normali originali (`normalIndex`), antialiasing 4x, illuminazione solidale alla camera, selezione con contorno, viste standard in coordinate veicolo, più matematiche nella stessa scena con conversione delle unità, apertura dei file JT · cubo di orientamento, proprietà e misure, piano di sezione, export in background, rifiniture (blocco 6) · **Prossimo:** piano completato; nuove richieste dall'utente (sezione 8)

---

## 1. Architettura e Librerie (Cargo.toml)

### Dipendenze runtime

| Crate | Versione | Ruolo |
|---|---|---|
| `eframe` | 0.31 (feature `wgpu`) | Framework applicativo: event loop, finestra nativa, integrazione wgpu |
| `egui` | 0.31 | UI immediata: pannelli, albero, pulsanti, viewport callback |
| `wgpu` | 24 | Pipeline GPU 3D condivisa con eframe (device/queue riusati) |
| `glam` | 0.29 (feature `bytemuck`) | Vettori/matrici SIMD: `Vec3`, `Mat4`, operazioni transform |
| `memmap2` | 0.9 | Memory-mapped file I/O: lettura O(1) di file da 600 MB senza caricarli in RAM |
| `rayon` | 1.10 | Parallelismo data-parallel: parsing e clustering multi-core (attivato in M2/M5) |
| `rstar` | 0.12 | R*-tree per spatial index nel clustering geometrico (attivato in M5) |
| `rfd` | 0.15 | File dialog nativo OS per apertura/salvataggio WRL |
| `tracing` + `tracing-subscriber` | 0.1 / 0.3 | Logging strutturato, sostituisce TeeOutput Python |
| `bytemuck` | 1.19 (feature `derive`) | Cast zero-copy `Vertex` → `&[u8]` per upload GPU |

### Profili di compilazione

```toml
[profile.release]   # → .exe standalone, nessun runtime
opt-level = 3
lto = "fat"         # Link-Time Optimization massima
codegen-units = 1   # Un'unità: ottimizzazione inter-procedurale completa
panic = "abort"     # Elimina il runtime di unwinding (~200 KB risparmio)
strip = "symbols"   # Rimuove simboli di debug dal binario finale
```

---

## 2. Moduli e File Rust Creati

```
Gestore-WRL/
├── Cargo.toml                        ✅ Configurato e ottimizzato
├── build.rs                          ✅ Placeholder (winres futuro per icona .exe)
├── .cargo/config.toml                ✅ CRT statica (+crt-static), niente target-cpu=native
│
└── src/
    ├── main.rs                       ✅ Entry point eframe (1280×800, titolo "Gestore WRL")
    ├── app.rs                        ✅ GestoreApp + LoadState enum + eframe::App impl
    ├── settings.rs                   ✅ Preferenze persistenti (sfondo, colore predefinito, asse Z, file recenti)
    ├── project.rs                    ✅ Chiudi/apri con conferma, annulla/ripeti, titolo finestra, export dialog
    │
    ├── parser/
    │   ├── mod.rs                    ✅ Re-export ShapeInfo, parse_wrl
    │   ├── delimiter_map.rs          ✅ Single-pass O(n) su &[u8] → brace_map + bracket_map
    │   ├── lexer.rs                  ✅ skip_whitespace_comments, scan_word, scan_numbers_f32/i32
    │   ├── material.rs               ✅ Colore Material/Appearance (DEF/USE, transparency, emissive)
    │   ├── transform.rs              ✅ build_transform_matrix (T·Tc·R·Rso·S·Rso⁻¹·Tc⁻¹) via glam
    │   ├── shape_extractor.rs        ✅ parse_wrl() + extract_shapes() ricorsivo completo
    │   └── vrml1.rs                  ✅ VRML 1.0 → VRML 2.0 in memoria (stato Separator, LOD, Switch, DEF/USE)
    │
    ├── scene_graph/
    │   ├── mod.rs                    ✅ Re-export SceneGraph, SceneNode, NodeKind, NodeInfo, MacroComponent, Aabb
    │   ├── builder.rs                ✅ (M4/M5) Trie dei percorsi → albero, fusione catene anonime, auto-clustering
    │   ├── node.rs                   ✅ Arena + NodeInfo (padre/tipo/conteggio), isolate/delete/show_all, revision, pick()
    │   └── bbox.rs                   ✅ Aabb: extend_point, extend_aabb, center, diagonal, distance_to
    │
    ├── clusterer/
    │   ├── mod.rs                    ✅ cluster_flat() + ClusterConfig (tolerance=0.01, min_match=5)
    │   ├── union_find.rs             ✅ UnionFind con path compression + union by rank
    │   └── spatial.rs                ✅ SpatialGrid: grid hash 3D, insert(), query_neighbors()
    │
    ├── renderer/
    │   ├── mod.rs                    ✅ WgpuRenderer stub (upload_meshes → todo! in M3)
    │   ├── camera.rs                 ✅ ArcballCamera trackball a quaternione (rotazione libera, roll), pan/zoom/fit_to_bbox
    │   └── mesh_buffer.rs            ✅ MeshBuffer wgpu, Vertex [pos+norm+pad=32B], compute_normals()
    │
    ├── ui/
    │   ├── mod.rs                    ✅ UiState, draw_main_layout (Top+Side+Central+Bottom panel)
    │   ├── toolbar.rs                ✅ Apri WRL, Isola, Nascondi/Mostra, Elimina, Colore, Mostra tutto, Vista, Esporta
    │   ├── color_menu.rs             ✅ Menu colore condiviso (palette, selettore, trasparenza, ripristino)
    │   ├── scene_tree.rs             ✅ Albero virtualizzato (show_rows), espandi/comprimi, menu contestuale
    │   ├── viewport.rs               ✅ Viewport wgpu + ray picking + sync visibilità/selezione
    │   └── progress.rs               ✅ Status bar con progress bar e statistiche scena
    │
    └── exporter/
        ├── mod.rs                    ✅ Re-export export_visible_wrl
        └── writer.rs                 ✅ export_visible_wrl: world-space, gerarchia DEF Group, appearance originale
```

**Totale: 26 file** · **~75 KB di codice Rust** generati

---

## 3. Stato Implementativo per Area

### 3.1 Parser WRL (`parser/`)

| Funzionalità | Stato | Note |
|---|---|---|
| Delimiter map single-pass | ✅ Completo | O(n) su `&[u8]`, gestisce commenti `#`, stringhe `"..."`, escape `\"` |
| Lexer byte-level | ✅ Completo | Opera su `&[u8]` raw, nessuna allocazione `String` intermedia |
| Parsing nodi `Transform` | ✅ Completo | Stack di Mat4 con `parent_matrix * local_matrix` |
| Parsing nodi `Group` / contenitori | ✅ Completo | `Anchor`, `Collision`, `Billboard` trattati come Group; `LOD`: primo livello non vuoto; `Switch`: solo `whichChoice` |
| Parsing `Shape` → `IndexedFaceSet` | ✅ Completo | Estrae `coord Coordinate { point [...] }` e `coordIndex [...]` |
| Parsing `DEF` / `USE` | ✅ Completo | Pre-scansione globale `DefRegistry` per Shape, contenitori, Coordinate, geometria (`geometry DEF/USE`) e materiali. Nomi ridefiniti risolti sull'ultimo `DEF` prima del `USE` (`latest_before`) |
| Formato VRML 1.0 (`Separator`, `Coordinate3`) | ✅ Completo | `parser/vrml1.rs`: conversione in memoria in VRML 2.0 (superfici; linee, punti e primitive solo come avviso). Riverificato 2026-11-07 dopo l'interruzione di corrente su `Sportello_V1` e `Supporto_V1`: export e riapertura identici |
| `IndexedLineSet` (fili) | ❌ Ignorato | Le Shape con sole linee non vengono caricate (es. `__PART_WF` in Scocca_B) |
| Matrici di trasformazione | ✅ Completo | `translation`, `rotation`, `scale`, `center`, `scaleOrientation` tutti implementati via `glam::Mat4` |
| Coordinate world-space | ✅ Completo | `matrix.transform_point3()` applicato su ogni vertice al momento del parsing |
| Memory mapping | ✅ Completo | `memmap2::MmapOptions` con tipo zero-copy `SourceData::Mmap`, fallback buffer |
| Parsing parallelo (rayon) | ✅ Completo | Scansione blocchi top-level e distribuzione data-parallel multi-core con `rayon::par_iter` |

### 3.2 Gestione delle Normali (`renderer/mesh_buffer.rs`)

| Aspetto | Stato | Dettaglio |
|---|---|---|
| Calcolo normali smooth | ✅ Implementato | `compute_normals()`: accumula normale di faccia (cross product) su ogni vertice, poi normalizza |
| Normale di fallback | ✅ Implementato | Vertici con normale degenere (length² < 1e-10) ricevono `[0, 1, 0]` |
| Triangolazione da `coordIndex` | ✅ Completo | Fan triangulation poligoni (triangoli, quad, n-gon) con gestione terminatore `-1` |
| Normali pre-esistenti nel WRL | ✅ Completo | `Normal { vector }` con `normalIndex` e `normalPerVertex` secondo la specifica (`FileNormals` in `shape_extractor.rs`); indici non validi → `compute_normals` |
| Spigoli vivi | ✅ Completo | Un vertice con normali diverse sulle sue facce viene copiato per la sola resa (`ShapeInfo.split`, `SplitNormals`); geometria, export, picking e clustering non cambiano |
| Layout Vertex GPU | ✅ Definito | `[position: [f32;3], normal: [f32;3], _pad: [f32;2]]` = **32 byte/vertice**, allineato a 16 byte |

### 3.3 Gerarchia della Scena (`scene_graph/`)

| Aspetto | Stato | Dettaglio |
|---|---|---|
| Arena allocator (Vec + indici u32) | ✅ Completo | Zero pointer chasing, cache-friendly, serializzabile |
| Nodi `Group`, `Transform`, `Shape` | ✅ Definiti | Con `visible`, `selected`, `name`, `children: Vec<u32>` |
| `set_subtree_visible()` | ✅ Implementato | Stack iterativo (no ricorsione), O(nodi nel sotto-albero) |
| `isolate()` | ✅ Implementato | Nasconde tutto → mostra solo il sotto-albero selezionato |
| `delete_node()` | ✅ Implementato | Tombstone (Group vuoto invisibile), indici esistenti non invalidati |
| `MacroComponent` | ✅ Definito | `shape_indices: Vec<u32>`, `bbox: Aabb`, `face_count`, `vertex_count` |
| Gerarchia strutturata da WRL | ✅ Completo | Ogni `ShapeInfo` porta `path: Vec<PathSeg>` (Group/Transform/USE, costruito bottom-up con `prefix_path`). `builder::build_tree` ricostruisce l'albero; le catene di contenitori anonimi vengono fuse, i wrapper anonimi di una sola Shape rimossi |
| Istanze `USE` di contenitori | ✅ Completo | Diventano nodi 🔗 distinti (chiave = offset del token USE), con il sotto-albero del DEF |
| Auto-clustering file piatti | ✅ Completo | Ogni contenitore (o la radice) con ≥ 10 Shape dirette viene clusterizzato: gruppi ≥ 2 → nodo "Componente N"; ≥ 30 pezzi isolati → "Pezzi singoli" |
| Metadati nodo (`NodeInfo`) | ✅ Completo | `parent`, `kind` (Group/Transform/Use/Cluster/Singles/Shape), `shape_count`, `deleted` — allineati a `nodes` |
| `revision` | ✅ Completo | Incrementato a ogni modifica: il renderer risincronizza i flag GPU solo quando cambia |

### 3.4 Clusterer Geometrico (`clusterer/`)

| Aspetto | Stato | Dettaglio |
|---|---|---|
| `UnionFind` (path compression + union by rank) | ✅ Completo | Identico all'algoritmo Python, O(α(n)) ammortizzato |
| `SpatialGrid` (grid hash 3D) | ✅ Completo | Celle cubiche di lato `tolerance`, query sulle 27 celle vicine (M1) |
| `cluster_flat()` | ✅ Implementato | Collega le Shape per vertici coincidenti entro `tolerance` con `min_matching_vertices` minimi |
| `cluster_subset()` parallelo | ✅ Completo | Griglia in sola lettura, ricerca vicini con `rayon` (`flat_map_iter`), Union-Find finale sequenziale. Bench: 12.000 Shape/300k vertici → 2.000 componenti in ~0,4 s (anche single-thread) |
| Upgrade a `rstar` R*-tree | ⏳ Pianificato M5 | Sostituirà il grid hash per query O(log n) invece di O(27·bucket) |

---

## 4. Prossimi Passi — TODO / Milestones Rimanenti

### ✅ M1 — Scaffolding (COMPLETATO)
- [x] Struttura moduli completa (26 file)
- [x] `Cargo.toml` ottimizzato per `.exe` standalone
- [x] Tutti i tipi dati e le API pubbliche definite
- [x] UI placeholder funzionante (toolbar + pannello laterale + viewport grigio)

---

### ✅ M2 — Integrazione memmap2 + Parser Testato (COMPLETATO)
**Obiettivo:** Il parser legge file WRL reali via memory map e popola il SceneGraph.

- [x] Integrare `memmap2::MmapOptions` in `app.rs` e `parser::loader` per l'apertura file (pulsante "Apri WRL")
- [x] Collegare `parser::parse_wrl()` al flusso di caricamento asincrono (thread separato + canale `std::sync::mpsc`)
- [x] Aggiornare la progress bar in `ui/progress.rs` con i dati del parsing continuo (non-blocking UI a 60 fps)
- [x] Testare con file WRL reali: semplici (< 10 MB) e multi-blocco
- [x] Verificare la correttezza delle matrici `Transform` annidate
- [x] Gestione `USE`: duplicare la geometria del nodo `DEF` referenziato (clonare i vertici applicando la matrice accumulata)
- [x] Implementare parsing parallelo con `rayon` sui blocchi top-level con pre-scansione `DefRegistry`

**Criterio di successo:** File parsato via memory map O(1), supporto DEF/USE completo, 21 test unitari superati con successo.

---

### ✅ M3 — Rendering 3D Base (COMPLETATO)
**Obiettivo:** Il viewport mostra la scena 3D con camera interattiva.

- [x] Creare la `wgpu::RenderPipeline` in `renderer/mod.rs` usando il device di `eframe`
- [x] Scrivere gli shader WGSL: `src/renderer/shaders/mesh.wgsl` (Blinn-Phong + two-sided lighting satinato)
- [x] Sostituire il viewport placeholder con offscreen render target wgpu + visualizzazione ad alta risoluzione
- [x] Implementare triangolazione corretta da `coordIndex` VRML in `shape_extractor.rs`:
  - Fan per ogni faccia poligonale (ogni sequenza fino a `-1`)
  - Gestione `coordIndex` con e senza `-1` finale
- [x] Leggere `Normal { vector [...] }` dal WRL se presente; altrimenti usare `compute_normals()`
- [x] Collegare gli input mouse/trackpad a `ArcballCamera` (orbit tasto sinistro, pan tasto destro/centrale, zoom rotella)
- [x] Implementare `camera.fit_to_bbox()` con calcolo bounding box scena e pulsante overlay "Centra vista"

**Criterio di successo:** Scena 3D renderizzata fluidamente a 60 fps con Z-buffer hardware e controllo telecamera completo.

---

### ✅ M4 — Scene Tree UI + Selezione (COMPLETATO)
**Obiettivo:** L'albero laterale riflette la gerarchia WRL; selezione bidirezionale albero ↔ 3D.

- [x] Parser: percorso dei contenitori (`PathSeg`) per ogni Shape, DEF/USE compresi
- [x] Ricostruzione della gerarchia nell'arena (`scene_graph/builder.rs` + `SceneGraph::from_built`)
- [x] Albero egui virtualizzato: solo le righe a schermo vengono disegnate (`ScrollArea::show_rows`)
- [x] Selezione click su albero → evidenziazione arancio nel viewport (bind group colore, gruppo 1 dello shader)
- [x] Selezione click nel viewport 3D (ray picking CPU: AABB + Möller–Trumbore, parallelo) → albero espanso e scorso fino al nodo
  - click = macro-oggetto · Ctrl+click o secondo click = singolo pezzo · click sul vuoto = deseleziona
- [x] **Bug corretto:** Isola/Nascondi/Elimina non avevano effetto nel 3D perché `WgpuRenderer::visibility` non veniva mai aggiornato dal SceneGraph → ora `sync_with_scene()` a ogni frame (costo solo se cambia `revision`/selezione)
- [x] Azioni centralizzate `app::Action` + `GestoreApp::apply()` (toolbar, albero, viewport, tastiera)
- [x] Scorciatoie: `H` nascondi/mostra · `I` isola · `Canc` elimina · `F` inquadra · `Esc` deseleziona
- [x] Doppio click (albero o 3D) → inquadra; "Centra vista" inquadra le sole parti visibili
- [x] Elimina stacca il nodo dal padre (niente righe "Group" vuote) e aggiorna i conteggi

---

### ✅ M5 — Clusterer Auto (COMPLETATO)
- [x] Rilevamento automatico: qualsiasi contenitore (o radice) con ≥ 10 Shape dirette
- [x] `clusterer::cluster_subset()` collegato al caricamento (fase "Ricostruzione gerarchia" della progress bar)
- [x] Fase di ricerca vicini parallelizzata con `rayon`
- [x] Macro-componenti come nodi `Group` sintetici (`NodeKind::Cluster`, icona 📦)
- [ ] Slider di tolleranza / `min_matching_vertices` in UI (oggi: 0.01 / 5 come lo script Python)
- [ ] Valutare `rstar` solo se la griglia hash diventa il collo di bottiglia su file > 600 MB

---

### ✅ M5b — Colori, sfondo, rinomina, controlli camera (COMPLETATO 2026-11-05)
- [x] `parser/material.rs`: colore originale di ogni Shape da `appearance → Material` (`diffuseColor`,
      `transparency`, fallback `emissiveColor` se il diffuse è nero, default VRML 0.8). `DEF`/`USE` di
      `Material` e `Appearance` risolti da una pre-scansione byte-level (`prescan_materials`) → `ShapeInfo.color`
- [x] Renderer: storage buffer con un colore per mesh, indicizzato da `instance_index` (draw con
      `first_instance = mesh`): niente bind group per pezzo. Colori sRGB → lineari. Selezione = arancio.
- [x] Trasparenze: secondo passaggio (pipeline con blending, senza scrittura depth), ordinato dal più lontano
- [x] Colore utente per nodo (`NodeInfo.color`), ereditato dal sotto-albero; impostarlo su un padre azzera
      quelli dei figli. Menu "🎨 Colore" in toolbar e nel menu contestuale: palette, selettore, trasparenza,
      "Colore originale"; "Ripristina tutti i colori originali" nel menu Vista. Quadratino colore nell'albero.
- [x] Menu "🖼 Vista": colore di sfondo (preset + selettore) e colore delle parti senza materiale,
      salvati in `%APPDATA%\Gestore-WRL\impostazioni.txt` (`settings.rs`)
- [x] Rinomina: F2 o menu contestuale → campo di testo sulla riga (Invio/click fuori conferma, Esc annulla);
      stringa vuota = nome automatico. Il nome va nell'export come `DEF` (sanificato)
- [x] Export: colore utente > appearance originale autonoma > colore letto (es. via USE) > neutro
- [x] Zoom con tasto sinistro + rotella premuti: mouse su = avvicina, giù = allontana (`camera.dolly`)
- [x] File di prova `esempi/test_colori.wrl` (colori, DEF/USE materiali, trasparenza, emissive, senza materiale)

---

### ✅ M5c — Gestione progetto e navigazione (COMPLETATO 2026-11-05, `project.rs`)
- [x] **✖ Chiudi** progetto; se ci sono modifiche non esportate compare una finestra
      "Esporta… / Continua senza esportare / Annulla". Stessa conferma aprendo un altro file e chiudendo la finestra
- [x] Modifiche non esportate: `scene.revision != saved_revision` → "● modificato" in toolbar e `*` nel titolo finestra
- [x] **Annulla/Ripeti** (↶ ↷, Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z) per nascondi/mostra/isola/elimina/colori/nomi:
      snapshot dello stato modificabile (`SceneGraph::snapshot` / `swap_snapshot`, senza geometria), max 30 passi;
      le modifiche colore consecutive sullo stesso nodo entro 1,5 s sono un solo passo
- [x] **Viste standard** (pulsanti sulla viewport e menu Vista): Fronte/Retro/Destra/Sinistra/Alto/Basso/Iso,
      tasti 1/3/7/0 (Ctrl = vista opposta), **5 = raddrizza** (annulla il rollio). Opzione asse verticale Y o Z (CAD)
- [x] **Ricerca per nome** nell'albero (Ctrl+F): mostra le corrispondenze con i loro antenati, risultato in cache
      per (testo, revisione); click su un risultato espande il percorso
- [x] **Drag & drop** di un .wrl sulla finestra, **file recenti** (menu ▾ accanto ad Apri, max 8), Ctrl+O apri,
      Ctrl+S esporta; la finestra di export propone `<nome>_export.wrl` nella cartella del file aperto

---

### ⏳ M6 — Esportatore + Build Finale
**Obiettivo:** Export WRL pulito dei soli elementi visibili; binario `.exe` distribuibile.

- [x] `exporter/writer.rs` riscritto:
  - solo Shape visibili; contenitori senza parti visibili omessi
  - coordinate world-space (`point`) + `coordIndex` triangolato
  - gerarchia mantenuta come `DEF Nome Group { children [...] }` (nomi resi identificatori VRML validi e univoci)
  - `appearance` originale copiata verbatim se autonoma (senza DEF/USE), altrimenti materiale neutro
  - test di round-trip: il file esportato si riapre con la stessa gerarchia
- [x] File dialog "Salva come" via `rfd::FileDialog::save_file()`
- [ ] Aggiungere icona applicazione (`winres` in `build.rs`) per Windows
- [ ] Aggiungere manifest Windows per DPI-awareness e stile visivo moderno
- [x] Export: poligoni originali e campi del file copiati (`exporter/ifs.rs`): `coordIndex`, `normal`/`normalIndex`,
      `solid`, `creaseAngle`, `convex`, `color`, `texCoord`. Con matrice identità anche `coord` è copiato (cifre intatte);
      altrimenti `coord`/`normal` in world-space e `ccw` invertito se la matrice è speculare. `DEF` rimossi, `USE`
      sostituiti dal nodo. Colore utente → colori per vertice omessi. Senza `coordIndex` valido: triangoli del parser
- [ ] Profiling con `cargo flamegraph` su file da 600 MB
- [x] `cargo build --release` → `.exe` standalone: 8,2 MB, CRT statica (da provare su un altro PC)

**Criterio di successo:** `.exe` standalone che apre, visualizza, manipola ed esporta WRL da 600 MB.

---

## 5. Installazione Rust (Prerequisito)

> [!IMPORTANT]
> Rust **non è installato** sul sistema. Prima di compilare:

```powershell
# 1. Scarica e installa rustup (installer ufficiale)
winget install Rustlang.Rustup
# oppure da https://rustup.rs

# 2. Aggiungi il target Windows (di solito già default)
rustup target add x86_64-pc-windows-msvc

# 3. Verifica
cargo --version   # atteso: cargo 1.83+
rustc --version   # atteso: rustc 1.83+
```

```powershell
# Dopo l'installazione, prima build (scarica dipendenze ~2-5 min)
cd <cartella del progetto>
cargo build
```

---

## 6. Comandi Utili

```powershell
# Build debug (veloce, con debug info)
cargo build

# Build release (ottimizzato, .exe standalone)
cargo build --release

# Esegui in modalità dev
cargo run

# Verifica che il codice compili (senza produrre binario)
cargo check

# Esegui i test
cargo test

# Analisi delle dipendenze
cargo tree

# Profiling release
cargo install flamegraph
cargo flamegraph --bin gestore-wrl -- path\to\file.wrl
```

---

*La roadmap aggiornata è nella sezione 8.*

---

## 7. Note di verifica (sessione 2026-11-05)

- **Prova dell'utente (2026-11-06):** rotazione "precisa, molto meglio"; la matematica si apre correttamente dopo la
  correzione dei campi Transform.
- **Prova dell'utente (2026-11-07):** Sottoscocca_A, selezionata tutta la matematica ed esportata: il file si apre correttamente.
- **Scala e visualizzatore aziendale (2026-11-07):** l'utente converte da STEP a WRL; alla prima apertura il visualizzatore aziendale chiede se mettere il file in
  scala e, dopo il salvataggio, non lo chiede più. L'origine dei `scale` nei file è un'ipotesi, non verificata. Gestore WRL non
  cambia le dimensioni: applica i `Transform` e all'export scrive le coordinate risultanti senza nodi `Transform`.

- **VRML 1.0 (2026-11-07):** `parser/vrml1.rs` converte in memoria il testo in VRML 2.0; albero, rendering ed export sono quelli
  già provati e l'export è un VRML 2.0. Nell'albero solo i contenitori con `DEF` e superfici. Verificato sui file reali con uno
  script indipendente: `Sportello_V1` 2 superfici, 52.950 vertici, 17.650 triangoli; `Supporto_V1` 2 superfici, 40.962 vertici
  (solo il primo livello dei `LOD`). Export e riapertura identici. Le linee (rami `_WF`) non si caricano: avviso nella barra di stato.
  Trasformazioni, `USE`, `Switch`, coordinate condivise: nessun file reale li usa, coperti solo da test.
- **Prova dell'utente su un altro PC (2026-11-07):** l'exe si avvia e carica perfettamente; secondo l'utente lettura velocissima e
  navigazione fluida, meglio di FreeCAD e del visualizzatore usato in azienda.
- **Normali dei file VRML 2.0 (corretto 2026-11-07):** il parser ora legge `normalIndex` e `normalPerVertex`. Un vertice con normali
  diverse sulle sue facce viene copiato solo sulla GPU (`ShapeInfo.split`): geometria, export, selezione e raggruppamento invariati.
  - File HOOPS (Scocca_A, Sottoscocca_A, Scocca_C, Scocca_B): tutti con `normal` + `normalIndex`, `normalPerVertex TRUE`, `creaseAngle -0.017453`.
    Superfici con copie: Sottoscocca_A 798/958 (1,24 M copie su 2,46 M vertici), Scocca_B 1531/1533 (738 mila su 2,14 M), Scocca_C 1410/15138.
  - File del fornitore 2 (Paraurti_D, Paraurti_E, Frontale_F): né normali né `creaseAngle`; restano le normali lisce calcolate (pannelli piatti, una Shape per
    faccia). Per la specifica senza `creaseAngle` la resa sarebbe sfaccettata: non cambiato.
  - Export e riapertura: normali di ogni angolo identiche su Sottoscocca_A, Scocca_C, Scocca_B, Paraurti_D, Sportello_V1, Supporto_V1.
  - Effetto a video (Scocca_B intero e sportello isolato, `GESTORE_NORMALI_RICALCOLATE` nel test `tempo_rendering_file_reale` dà
    l'immagine di prima): circa un terzo dei pixel del pezzo cambia, quasi tutti di poche sfumature; oltre 40 livelli su 255 solo
    219 pixel su Scocca_B e 172 sullo sportello. Le normali del file differiscono spesso di poco: gli spigoli davvero vivi sono pochi.
  - Tempi: parsing di Scocca_B 5,9 s contro 5,8 s di prima (release). Memoria in più: `split.indices` (4 B per angolo) e le copie.
  - VRML 1.0: ora tradotti anche `normalIndex` diverso da `coordIndex`, `PER_FACE`, `PER_FACE_INDEXED`, `PER_VERTEX`.
    `OVERALL` resta ricalcolato. I file V1 non ne hanno bisogno (un vertice per angolo, nessuna copia).
- **Conteggi spiegati (2026-11-07):** Sottoscocca_A ha 960 `IndexedFaceSet` e 958 pezzi: 2 hanno `point [ ]` vuoto. Scocca_B ha 4
  `IndexedLineSet` VRML 2.0 che non si caricano e, diversamente dal VRML 1.0, non danno avviso.
- **Interfaccia (2026-11-07):** `ui/theme.rs` (temi, Segoe UI se presente, icone Phosphor), tema salvato in `tema`, icona e manifest
  da `build.rs` (`assets/`, icona generata da `assets/genera_icona.py`), niente console nell'exe finale, file aperto da riga di comando
  o "Apri con". Rotazione e sensibilità spostate nel menu Vista, comportamento invariato. Avvisi: pulsante nella barra di stato.
  Provato sull'exe vero con clic reali e screenshot: file vuoto, Sportello_V1, Sottoscocca_A, tema chiaro, menu Vista, avvisi, selezione
  3D, finestra da 800 px. **Non provati:** evidenziazione durante il trascinamento di un file, monitor con scala DPI diversa da 100%.
- **Campi Transform dopo `children` (corretto 2026-11-06):** i file HOOPS scrivono `translation/rotation/scale` in fondo al
  nodo e il parser si fermava a `children`. I pezzi con `scale 0.001` restavano in millimetri in un modello in metri: 1000 volte
  troppo grandi e fuori posto. Verificato: Sottoscocca_A (prima 33 pezzi oltre 400 unità, ora distanza massima 2,8), Scocca_B,
  Sottoscocca_A2, Scocca_A, Scocca_C senza pezzi fuori ingombro. `main_visible_bounding_box` resta come rete di sicurezza, ma i "pezzi
  lontani" di Scocca_B descritti sotto erano questo bug. Scocca_B è una scocca di circa 5 m, non un tappo.
- **Navigazione OpenInventor (2026-11-06):** trackball su sfera 0,8 (`orbit_trackball`), come `NavigationStyle` di FreeCAD
  verificato sul sorgente: perno sul pezzo sotto il mouse, altrimenti sul piano focale nello stesso punto; la vista non si
  ricentra. Tolta la "rotazione libera" sugli assi dello schermo; resta "Asse fisso". Sensibilità 0,2-3 salvata in
  `sensibilita_rotazione`. Non replicati l'inerzia dopo il rilascio e la scalatura della sensibilità sulla sfera di FreeCAD.
- **Selezione multipla nell'albero (2026-11-06):** `selected_node` è la riga corrente, `selected_extra` gli altri. Shift+click
  intervallo, Ctrl+click aggiunge/toglie, frecce su/giù (con Shift estendono), sinistra/destra chiudono/aprono o vanno a
  padre/figlio. Nascondi, isola, elimina, colore, inquadra valgono per tutta la selezione con un solo Ctrl+Z (`Action::Batch`).
  Testato simulando mouse e tastiera su `draw_main_layout` (`selection_tests`). La rinomina resta su un solo elemento.
- Test: 91 unitari più 4 manuali (nuovo `diagnosi_pezzi_lontani`: elenca i pezzi lontani con percorso e matrice).
- **Bug parser trovati provando file reali (2026-11-06):**
  - `children Nodo { ... }` senza parentesi quadre (HOOPS Exchange): contenuto saltato, file aperto con 0 Shape.
  - Costo esponenziale con i `DEF` annidati (ogni DEF analizzato due volte): ora il template si crea solo per i
    nomi citati da un `USE` (`DefRegistry.used`).
  - Istanze `USE` di un Transform: matrice del DEF applicata due volte (pezzo nel punto sbagliato).
- **Step 1, affidabilità dei dati (2026-11-07):**
  - **`LOD` su Scocca_B:** `SPORTELLO_CARBURANTE` (lo sportello carburante, 17.816 triangoli) sta in `LOD { level [...] }` e
    non veniva caricato. Ora Scocca_B ha 1533 pezzi invece di 1532; verificato con un'immagine del pezzo isolato. Tra i file della
    cartella di prova e Scocca_B, i `LOD` VRML 2.0 stanno solo in Scocca_B. `Switch`: solo la scelta attiva; se nessuna è attiva,
    avviso persistente nella barra di stato, perché quel contenuto non si carica né si esporta. Nessuno `Switch` nei file reali.
  - **Normali nei file in metri:** `compute_normals` aveva una soglia assoluta (`1e-10`) sul quadrato della lunghezza: con triangoli
    sotto circa 3 millimetri di lato il vertice prendeva la normale di ripiego (0,1,0). Colpiva i pezzi `scale 0.001` riaperti
    dopo l'export e qualsiasi file scritto in metri senza normali. Trovato estendendo `file_reale_round_trip` alle normali.
  - **Export e riapertura:** geometria identica al bit su Sottoscocca_A (958 pezzi), Scocca_A (673), Scocca_B (1533), Paraurti_D (18051),
    Paraurti_E (12004). Normali identiche su Sottoscocca_A e Scocca_A. Restano diverse per l'arrotondamento a `f32` in world-space,
    su triangoli piccolissimi dei pezzi con matrice: Scocca_B 456/2,14 M, Paraurti_D 1228/304 mila, Paraurti_E 1084/211 mila; zero
    non spiegate (coincidono col ricalcolo dai vertici world originali). Il test ora fallisce solo sulle non spiegate.
  - `geometry DEF/USE` e nomi ridefiniti (`latest_before`): nessun file reale li usa, coperti solo da test. Corretto anche il
    costo esponenziale con i `DEF` riusati annidati (`has_shape_def`).
  - Avviso persistente (`UiState.load_warnings`, barra di stato) anche per i pezzi oltre 256 MB; errore chiaro per VRML 1.0.
    **Non provati nella finestra vera:** l'aspetto di questi avvisi; il parser e i conteggi sì, con i test.
  - Scocca_B ora è in un'altra cartella (spostato dall'utente). Test manuali: `GESTORE_WRL_NOME` elenca pezzi per nome in
    `diagnosi_pezzi_lontani`; `GESTORE_WRL_ISOLA` in `tempo_rendering_file_reale` disegna un solo pezzo.
- **Prova su file reale:** `GESTORE_WRL_FILE=percorso cargo test file_reale -- --ignored --nocapture`
  (parsing → export → rilettura; confronta geometria al bit). Esito 2026-11-06, profilo dev (`opt-level 1`):
  `Scocca_B.wrl` 542 MB, 1532 Shape: parsing 5,3 s, export 9,1 s · `Paraurti_D.wrl` 27 MB, 18051 Shape:
  1,2 s / 1,6 s · `Paraurti_E.wrl` 19 MB, 12004 Shape: 0,8 s / 1,1 s. Nessuna differenza dopo la rilettura.
  Non provati nella finestra vera: rendering e uso interattivo del file da 542 MB.
- **Precisione:** le coordinate sono copiate intatte solo con matrice identità (tutte le Shape di Scocca_B, solo 7 del Paraurti_D);
  con una matrice diversa passano da `f32` (circa 7 cifre significative).
- Test unitari: 86 (uno disegna davvero sulla GPU, se presente), più 3 manuali sui file reali (`file_reale_round_trip`,
  `tempo_picking_file_reale`, `tempo_rendering_file_reale`; con `GESTORE_RENDER_DUMP` salva/confronta l'immagine).
- **Rendering (2026-11-06):** geometria in buffer condivisi da max 256 MB (`mesh_buffer::build_chunks`), `mesh_id` nel
  vertice al posto di `instance_index`: mesh opache adiacenti = una draw call (`opaque_runs`). Immagini identiche al pixel
  al renderer precedente su Scocca_B, Paraurti_D, Paraurti_E. GTX 1060, 1600x900: Paraurti_D 30 → 2178 fps, Paraurti_E 47 → 2157, Scocca_B 350 → 619.
  Frame saltato se camera, sfondo, dimensioni e scena non cambiano. Trasparenti ancora una draw call per mesh.
  Una singola mesh oltre 256 MB (8 M di vertici) non viene disegnata: solo avviso nel log.
- **Profondità invertita (2026-11-06):** `Greater`, clear a 0: su Scocca_B i pixel instabili al variare dei piani di taglio
  scendono da 3155 a 50.
- **Antialiasing (2026-11-07):** MSAA 4x (`MSAA_SAMPLES`): colore e depth a 4 campioni, risolti nella texture mostrata da
  egui (i campioni non vengono salvati, `StoreOp::Discard`). Su Scocca_B cambiano solo i bordi: 19.690 pixel su 1,44 M; i
  puntini di z-fighting tra superfici sovrapposte restano (non dipendono dall'antialiasing). GTX 1060, 1600x900: Scocca_B
  619 → 395 fps (2,5 ms a frame), sportello isolato 2664 fps. Memoria video in più: 32 byte per pixel (circa 46 MB a
  1600x900, 265 MB a 4K).
- **Illuminazione (2026-11-07):** luci solidali alla camera (`CameraUniform::from_camera`): principale in alto a destra
  davanti, schiarita a sinistra, ambiente a emisfero (chiaro verso l'alto dello schermo), riflesso Blinn-Phong contenuto,
  leggero scurimento delle superfici viste di taglio. Prima le luci erano fisse nel mondo: guardando il pezzo da sotto
  o da dietro restava in ombra. La normale viene girata verso chi guarda (riflessa sul piano di vista, continua sui
  contorni) invece di usare `front_facing`: funziona anche con facce di verso misto. Verificato a occhio su Scocca_B e
  sullo sportello isolato (le facce delle clip ora si distinguono); velocità invariata (Scocca_B 409 fps).
- **Selezione con contorno (2026-11-07):** il pezzo selezionato mantiene il suo colore tinto al 45% di verde
  (`SELECTION_TINT`, `COLOR_SELECTED` ora verde come FreeCAD: l'arancio spariva su Scocca_B, che è arancio) e riceve un
  contorno verde di 2 pixel. Le mesh selezionate disegnano la sagoma in una maschera R8 senza depth; un triangolo a
  tutto schermo (`shaders/outline.wgsl`) colora i pixel esterni entro 2 pixel, sopra a tutto: il contorno si vede
  anche quando il pezzo è coperto (es. sportello carburante visto attraverso il finestrino). Scocca_B con selezione:
  circa 340 fps contro 409. Test GPU: tinta e contorno appena fuori dal bordo; test sul colore lineare dello shader.
- **Più matematiche insieme (2026-11-07, blocco 1 del piano):**
  - Ogni file letto diventa un nodo radice di tipo `File` (`SceneGraph::into_file`, nel thread di caricamento) con il
    suo testo in `sources` (`SourceFile`: nome, percorso, testo, intervallo di mesh, nodo, scala). Anche un file solo
    ha ora il suo nodo nell'albero.
  - `SceneGraph::append` unisce una scena in coda: nodi e mesh esistenti mantengono gli indici, l'arena resta in
    pre-order. Gli stati di Annulla/Ripeti ricevono i nodi del file aggiunto (`EditSnapshot::extend_from`): annullare
    non toglie il file. Aggiungere non rende la scena "modificata".
  - GPU: `append_meshes` crea solo i buffer delle mesh nuove (`build_chunks` con `first_mesh`).
  - Interfaccia: pulsante "Aggiungi" (più file insieme); trascinando file con una scena aperta si aggiungono (senza
    scena il primo si apre); caricati uno alla volta (`add_queue`). Tasto destro sul file: "Esporta questo file…",
    "Unità di misura", "Chiudi file" (= elimina il nodo, annullabile; la memoria si libera solo chiudendo tutto).
    Titolo "primo.wrl + N file".
  - Export: con più file un `Group` per file (nome senza estensione); con un file solo il suo nodo non viene scritto,
    quindi l'export di un file singolo è identico a prima. I `USE` si risolvono nel testo del proprio file.
  - **Unità di misura:** Scocca_A è in metri, i file V1 (VRML 1.0) in millimetri: aggiunti così finivano a 3 km. Alla
    aggiunta `suggest_unit_scale` confronta il file con il primo aperto e propone ÷1000 o ×1000 in una finestra
    ("Converti tutti" / "Lascia così"). La scala entra nella matrice delle Shape: l'export scrive le coordinate
    convertite (in `f32`). Verificato: Sportello_V1 convertito cade sul parafango posteriore sinistro di Scocca_A.
    Da segnalare all'utente: Supporto_V1 (convertito) finisce vicino al montante anteriore e "sportellino carburante
    int" (già in metri) a Y −1,3/−1,7 m, fuori dalla fiancata: posizioni scritte nei file, forse in un altro riferimento.
  - Prove reali (`piu_file_reali`, `GESTORE_WRL_FILES` separati da `|`): Scocca_A + Scocca_B + Sportello_V1 + Supporto_V1 =
    2210 shape, 8,6 M triangoli, lettura e unione 18 s (unione < 2 ms per file), export dell'insieme 8 s; riaperto,
    geometria identica al bit 2210/2210; export di ogni file da solo identico al file aperto da solo. Memoria massima
    del processo di prova 4 GB (comprende la riapertura dell'export). Disegno: Scocca_A + sportellino 344 fps.
  - Test nuovi: unione, chiusura di un file, Annulla dopo un'aggiunta, export dell'insieme e del singolo file con DEF
    uguali nei due file, GPU (file aggiunto disegnato senza riscrivere i buffer), unità (mm→m, m→mm, nessuna proposta
    con unità uguali), interfaccia (albero, finestra delle unità, click su "Converti"). I test GPU ora non girano mai
    in parallelo (un crash del driver Vulkan su ~20 esecuzioni con due istanze insieme).
- **Import JT (2026-11-07):** convertitore separato `tools\jt2wrl.exe` (`tools-src/jt2wrl`, C# .NET 8 NativeAOT,
  11 MB, senza .NET installato) basato su JTReader (MIT, commit 9845868, sorgenti compilati insieme, non nel
  repository: si scaricano con `git clone`) e SharpCompress (MIT). Il Gestore lo avvia senza finestra
  (`parser/jt.rs`): avanzamento da stdout nella prima metà della barra, poi il WRL prodotto (in
  `%TEMP%\gestore-wrl-jt`, ripulita all'avvio) si legge con `load_blocking` mostrando il `.jt` come file
  (nome, recenti, "Ricarica" riconverte). Errori chiari: convertitore mancante, big-endian, solo B-rep.
  - Convertitore: albero JT con gruppi/parti/istanze, LOD (il più dettagliato), Switch (scelta attiva), partizioni
    esterne (assiemi in più file `.jt`), trasformazioni accumulate e coordinate in world-space, colore diffuso del
    materiale e trasparenza, normali con `normalIndex` (spigoli vivi), nomi da `JT_PROP_NAME`. Istanze ripetute
    riscritte per ogni posizione.
  - Corretto un problema di JTReader senza modificarlo: nei JT 10 i segmenti di proprietà letti prima del grafo
    riusano gli identificativi dei nodi e l'albero puntava a una proprietà di testo (0 pezzi). Ora gli elementi del
    segmento del grafo hanno la precedenza (`JtFix.ReadGraph`). `jt2wrl --diag file.jt` stampa l'albero per i casi
    nuovi.
  - Prove con i file pubblici di PyOpenJt: 10/10 convertiti (blocco JT 8.1/9.5/10.3 = 12 triangoli in tutti e tre,
    piastre JT 8.0/9.5, CoffeeMaker 64 pezzi, fishing_reel 17 pezzi da 12 file esterni montati correttamente,
    Warehouse 1084, SampleJT_Server 850, san2 850 pezzi e 1,5 M triangoli in 5–10 s). WRL della versione NativeAOT
    identici al byte a quelli della versione JIT. Nel Gestore vero: fishing_reel.jt aperto in 2,2 s.
  - Colori dei JT 9 corretti (2026-11-07): con il materiale in versione 2 JTReader leggeva ogni colore come un solo
    grigio invece che RGBA, i valori slittavano e il diffuso finiva quasi sempre a 0.2 (pezzi neri). Il convertitore
    registra un suo lettore (`JtMaterial`) al posto di `MaterialAttributeData` per i JT 9 (bit "colore comune" dei
    Data Flags come nei JT 8, riflettività in versione 2); JT 8 e 10 restano a JTReader. Verifica: blocco e piastra
    JT 9.5 ora hanno gli stessi colori delle versioni JT 8/10, SampleJT_Server uguale a san2, stessi pezzi e triangoli
    di prima; `--diag` stampa i valori dei materiali e segnala byte non letti. Provato anche su due JT reali
    dell'utente (JT 9.5, prima neri, ora colorati).
  - Non ancora provato su JT reali da Robcad/Process Simulate. Non coperti: export in JT, geometria esatta B-rep/XT,
    file big-endian, mesh compresse non topologiche di JT 10 (`TopoMeshCompressedLODData.GetIndices` non
    implementato in JTReader: il pezzo viene saltato con avviso).
  - Test: righe di avanzamento ed errori, percorso completo con un convertitore finto (`.cmd`): avanzamento, avvisi,
    nome del `.jt` nella scena, conversione fallita con messaggio.
- **Rifiniture (2026-11-07, blocco 6 del piano):**
  - Sfondo sfumato: casella "Sfumato" nel menu Vista (chiave `sfondo_sfumato`). In alto il colore scelto schiarito
    del 30% verso il bianco, in basso scurito al 55% (`gradient_colors`); triangolo a tutto schermo disegnato per
    primo nel passaggio principale (colori in `bg_top`/`bg_bottom` della uniform). Scocca_B: 395 fps, come prima.
  - Raggruppamento automatico: tolleranza (0,0001–10, unità del file, predefinita 0,01) e vertici in comune (1–100,
    predefiniti 5) nel menu Vista, salvati nelle impostazioni (`raggruppamento_tolleranza`, `raggruppamento_vertici`)
    e usati a ogni apertura (`parse_wrl_with_config`, `load_wrl_async_with`). "Ricarica i file aperti" li riapre
    nello stesso ordine (conferma se ci sono modifiche non esportate; conversioni di unità da rifare, la proposta
    ricompare). Nota: la tolleranza è assoluta, quindi 0,01 vale 0,01 mm nei file in millimetri e 1 cm in quelli in
    metri.
  - Colori per vertice/faccia: nessun file reale ha nodi `Color`, `colorIndex` o `colorPerVertex`; i
    `MaterialBinding` dei file V1 sono tutti `OVERALL`. Non implementati.
  - Test: tolleranza e vertici in comune cambiano davvero i componenti, ricarica nell'ordine (e conferma se
    modificato), sfondo sfumato sulla GPU (alto più chiaro, pezzi invariati), impostazioni salvate e rilette.
- **File enormi (2026-11-07, blocco 5 del piano):**
  - `Sottoscocca_A2.wrl` (796 MB, il più grande disponibile), nell'exe: apertura 11,4 s, memoria 1,5 GB di cui 992 MB
    privati (il resto è il file mappato). Fuori dall'interfaccia: lettura 8,7 s, export circa 5,5 s (150 MB/s; i
    "14,9 s" di `file_reale_round_trip` comprendono la rilettura del file esportato), disegno 341 fps, geometria e
    normali identiche dopo l'export. Profilo dell'export: 5,4 s nella scrittura della geometria, aspetto e scrittura
    su disco trascurabili.
  - Annulla: un passo costa circa 3 MB e 1,4 ms con 18.063 nodi (Paraurti_D), 0,9 MB con 4929 nodi (Sottoscocca_A2).
    Trasparenze: Paraurti_D tutto semitrasparente 247 fps (4 ms a frame), opaco 1649 fps. Nessun intervento necessario.
  - **Export in un thread** (`start_export` / `poll_export`): la scena passa al thread e torna con il risultato;
    riquadro "Esportazione in corso" con i secondi; comandi, trascinamenti e chiusura della finestra bloccati fino
    alla fine (un file troncato non resta mai su disco per un'uscita). "Esporta…" nella conferma di chiusura o
    apertura esegue l'operazione solo a export riuscito. Prima la finestra restava ferma 5–8 s sui file grandi
    (Windows la segna "Non risponde" dopo 5 s). Verificato nell'exe: export di Sottoscocca_A2 con 0 campioni "non
    risponde". Test: scena restituita intatta, un export alla volta, operazione in attesa eseguita dopo.
- **Piano di sezione (2026-11-07, blocco 4 del piano):** pulsante "Sezione" nella vista; riquadro con asse X/Y/Z,
  cursore della posizione sull'ingombro del gruppo principale, "Inverti" (tiene l'altra metà) e chiusura. Si parte
  da X a metà ingombro (taglio trasversale della scocca). Il taglio è nello shader (`clip_plane` nella uniform della
  camera, `discard`): nessun costo sulla CPU, anche la maschera della selezione lo rispetta. Clic, perno di rotazione,
  zoom e misura attraversano la parte tagliata (`pick_triangle_kept`). Contorno giallo del piano sull'ingombro.
  Nessuna chiusura della sezione (cap): si vede l'interno delle lamiere. Verificato nell'exe su Scocca_B (taglio a
  X = 2,802 m). Test: lato tenuto e inversione, picking oltre il taglio, GPU (metà giusta sparisce, anche invertita).
- **Proprietà e misure (2026-11-07, blocco 3 del piano):**
  - `ui/properties.rs`: riquadro in fondo al pannello Scena con la selezione (anche multipla, sommata senza contare
    due volte i pezzi): nome, tipo, file, pezzi, triangoli, vertici, dimensioni e min/max. Valori lunghi troncati
    (nome completo nel suggerimento): un nome CAD lungo allargava il pannello a 711 px (test di regressione).
  - `ui/measure.rs`: pulsante "Misura" nella vista o tasto M; Esc o M per uscire. Il clic prende il punto del pezzo
    sotto il mouse; entro 8 punti sullo schermo da un vertice del triangolo colpito si aggancia al vertice (quadrato
    giallo; punto libero = cerchio). Segmento con quota, riepilogo in basso: distanza e ΔX ΔY ΔZ. Il terzo clic inizia
    una nuova misura. Decimali secondo la grandezza della scena (2 per scene da centinaia di unità, 4 in metri).
    `SceneGraph::pick_triangle` dà anche il triangolo colpito.
  - Verificato nell'exe su Scocca_B: sportello carburante 0,2750 × 0,0923 × 0,2208 m, 17.816 triangoli; misura cofano →
    montante posteriore 3,4417 m (ΔX 3,3508 ΔY −0,7111 ΔZ 0,3345), entrambi i punti agganciati a vertici.
- **Cubo di orientamento (2026-11-07, blocco 2 del piano):** `ui/view_cube.rs`, in alto a destra nella vista, ruota
  con il modello. Facce con i nomi delle viste del sistema di assi scelto (in Veicolo: Fronte = -X, Destra = +Y,
  Alto = +Z); il testo si riduce per stare nella faccia. Ogni faccia è divisa 3×3 (bordo = 40% esterno): centro →
  vista standard, bordi → spigoli, angoli → vertici, 26 direzioni; zona evidenziata sotto il mouse con il nome
  (es. "Fronte + Sinistra"). Passaggio animato di 0,3 s (slerp con partenza e arrivo morbidi,
  `ArcballCamera::animate_to`), usato anche da tasti e pulsanti delle viste; un trascinamento lo interrompe.
  Per le direzioni libere il "su" è l'asse verticale del modello (dall'alto/basso quello della vista Alto/Basso).
  Verificato nell'exe vero: clic su "Fronte" del cubo → scocca vista dal muso. Test: nomi delle facce per i tre
  sistemi di assi, centri delle facce, angolo al centro del cubo in iso, spigoli, faccia cliccata rivolta a chi guarda.
- **Viste standard (2026-11-07):** l'utente segnalava che "Fronte" mostrava la scocca dall'alto. Causa: predefinito
  "Y in alto", mentre le scocche sono in coordinate veicolo (Scocca_B: X da 0,44 a 5,17 m lungo la vettura, Y da -0,97
  a 0,97, Z da 0,44 a 1,87). Nuovo `camera::Axes` con tre sistemi, scelti nel menu Vista: **Veicolo** (predefinito: X
  verso il retro, Y a destra, Z in alto; Fronte = davanti al muso, Destra = lato destro con il muso a destra, Alto
  con il muso in alto sullo schermo, Iso = tre quarti anteriore sinistro), **Z in alto** (come FreeCAD), **Y in alto**.
  All'apertura di un file la vista è l'isometrica del sistema scelto. Chiave `assi` nelle impostazioni; la vecchia
  `z_alto` viene ignorata, così chi l'aveva salvata riparte da Veicolo. Verificato con immagini di Scocca_B in Fronte,
  Destra, Alto, Iso (`GESTORE_VISTA` e `GESTORE_WRL_SELEZIONA` nel test `tempo_rendering_file_reale`).
- **Inquadratura (2026-11-06):** `main_visible_bounding_box` esclude i pezzi oltre 20 volte la distanza del 90% dei pezzi
  dalla mediana (da 20 pezzi in su), con avviso nella barra di stato; riguarda solo "Centra vista" e l'apertura. Scocca_B: 26 pezzi
  esclusi su 1532, a migliaia di unità dagli altri. Paraurti_D, Paraurti_E, Scocca_C: nessuno.
- **Exe distribuibile (2026-11-06):** tolto `target-cpu=native`, CRT statica (`+crt-static`). Nell'exe non compaiono
  VCRUNTIME140, MSVCP140 né api-ms-win-crt. Non ancora provato su un altro PC.
- **Navigazione (2026-11-06):** l'utente è abituato al visualizzatore usato in azienda, di cui non si è trovata documentazione pubblica:
  i comandi non ne sono una copia. Ora rotazione, rollio, spostamento e zoom usano il punto del pezzo sotto il mouse
  (sul vuoto: centro vista per il trascinamento, piano del target per la rotella). Spostamento esatto col mouse;
  rotella circa 8% a scatto (prima 2,5% circa); piani di clipping ricalcolati a ogni frame da `scene_bounds`; terna XYZ
  e marcatore del centro di rotazione; modalità "Rotazione libera" (predefinita) / "Asse fisso" salvata nelle
  impostazioni (`rotazione`). Picking su Scocca_B (3,7 M triangoli): 6 ms nel caso peggiore su 40 raggi.

- **Bug corretto (2026-11-05):** il pannello Scena si allargava di ~9 px a ogni frame (campo di ricerca con
  larghezza "disponibile − 28" più larga dello spazio reale) fino a coprire la vista 3D. Ora layout da destra +
  `desired_width(INFINITY)`, `max_width` del pannello = 50% finestra; test `scene_panel_width_is_stable` simula 60 frame.
- Simboli: i font di egui non hanno ↶ ↷ ▾ ▼ ⟂ ● 🗂 ✓ ✗ (apparivano come quadratini). Sostituiti; il test
  `ui_glyphs_exist_in_default_fonts` verifica tutti i simboli usati.
- Test unitari: 59 (parser, materiali, gerarchia, clustering, isola/nascondi/elimina, colori/rinomina,
  picking, export round-trip con colori e nomi, impostazioni, camera e viste, annulla/ripeti, chiusura, ricerca).
  La parte non-GPU è stata compilata e testata; **UI egui e renderer wgpu vanno verificati con `cargo build`** sulla macchina di sviluppo.
- **Camera (2026-11-05):** sostituita la camera yaw/pitch (up fisso +Y, pitch bloccato a ±89°, impossibile
  ruotare oltre i poli) con un trackball a quaternione: trascinamento sinistro = rotazione libera attorno agli
  assi dello schermo (l'oggetto segue il mouse), Shift + trascinamento sinistro = rollio.

- **WRL leggeri e riduzione delle facce (2026-11-08, Step 6):**
  - Export: opzioni "Formato compatto" e "Scrivi le normali" (menu accanto a Esporta, salvate nelle impostazioni come
    `export_compatto` / `export_normali`, predefinite sì/sì). Compatto = scrittura dai dati letti (`write_geometry` in
    `exporter/writer.rs`): niente rientri né virgole, triangoli, coordinate con passo di un milionesimo dell'ingombro
    (`coordinate_decimals`: 6 decimali per una scocca in metri, 3 in millimetri), normali a 3 decimali (~0,05°),
    `normalIndex` dai vertici di resa. Senza normali: `creaseAngle 0.5`. Senza compatto: copia fedele come prima.
    I pezzi con `ShapeInfo::rewritten` (ridotti) si scrivono comunque dai dati. `estimate_size` conta i byte dello
    stesso codice di scrittura senza scrivere il file (stima = dimensione vera, verificato nei test).
  - Scocca_B (529 MB, 1533 pezzi, 3,7 M triangoli, in metri): copia fedele 536 MB, compatto 238 MB, compatto senza
    normali 121 MB.
  - Riduzione (`reduce.rs`): saldatura dei vertici uguali, poi meshoptimizer 1.3 (crate `meshopt` 0.7, MIT) con errore
    assoluto unico per tutta la scena, così i piani perdono molte facce e i raccordi poche (come l'ordinamento globale
    di MeshLab). Per un numero di facce si cerca l'errore per secanti in scala log con correzione "Illinois" (3–5
    passate, entro il 2%). Le normali del file pesano nella semplificazione (`NORMAL_WEIGHT` = 0,03 × ingombro):
    senza, sul cofano lucido i riflessi si spezzavano in macchie triangolari. Peso scelto confrontando le immagini
    con l'originale (pixel diversi: 2985 senza peso, 1019 con 0,01, 799 con 0,03, 1801 con 0,1). I vertici tenuti
    ricevono le normali che avevano nel file (scelte per faccia, così gli spigoli vivi restano distinti).
  - Scocca_B ridotta a 1 milione di facce: 3,3 s, errore 0,43 mm; WRL 62 MB con normali, 30 MB senza (l'utente
    otteneva 60–70 MB con MeshLab). Riaperto: 1533 pezzi, stessi triangoli. A 500.000 facce: 31 / 15 MB. Con
    "Blocca i bordi aperti" la scocca non scende sotto ~550.000 facce (i bordi di 1533 pezzi restano tutti).
  - `reduction.rs`: la scena va nel thread di calcolo (come l'export) con un riquadro di avanzamento; i pezzi ridotti
    prendono il posto degli originali in `shapes_data` (vista, selezione, misure ed export usano il risultato) e gli
    originali restano da parte (`stash`); ogni calcolo riparte dagli originali; "Originale" li rimette. La conversione
    delle unità scala anche gli originali. Aprire o chiudere file azzera tutto.
  - jt2wrl: scrittura compatta (7 cifre significative per le coordinate, 3 decimali per le normali, niente rientri):
    WRL al 40–62% di prima sui 10 JT di esempio (SampleJT_Server 182 → 74 MB, san2 292 → 126 MB). Confronto con
    l'uscita precedente (`confronto_due_file`): stessi pezzi e triangoli, vertici entro 5·10⁻⁷ relativo, normali entro
    0,05°.
  - Licenza di meshoptimizer in `licenses/meshoptimizer-MIT.txt`.
  - Verifica: test automatici (riduzione, ricerca dell'obiettivo, normali trasferite, export compatto e stima, pannello
    con clic su "Calcola anteprima"); immagini della scocca prima/dopo dal test di rendering con `GESTORE_RENDER_DUMP`.
    Non provato a mano sull'exe (in questa sessione non c'era il controllo del desktop).

- **STL (2026-11-08, Step 5):**
  - Apertura (`parser/stl.rs`): riconosciuto dal contenuto (binario dalla lunghezza 84 + 50·n, perché molti binari
    iniziano anch'essi con "solid"; ASCII da `solid` + `facet`), convertito in VRML 2.0 come i VRML 1.0
    (`parse_wrl_with_config`): una Shape per `solid` col suo nome, vertici uguali uniti, normali ricalcolate a
    spigolo vivo (`reduce::crease_normals`, 0,5 rad) perché quelle del file sono di faccia e spesso nulle. Coordinate
    col passo di un milionesimo dell'ingombro. File rotto: avviso con il numero di riga. Filtri delle finestre
    Apri/Aggiungi con `.stl`.
  - Export (`exporter/stl.rs`): parti visibili in world-space, binario o ASCII (opzione "In formato testo" nel menu
    accanto a Esporta, chiave `export_stl_testo`), con triangoli girati per le matrici speculari (normali verso
    l'esterno). Colori e albero non esistono nel formato. Il pannello "Riduci" mostra anche la dimensione dell'STL
    binario.
  - Scocca_B: STL binario 186 MB scritto in 0,2 s, ASCII 750 MB in 7 s; riaperti con tutti i triangoli tranne 2.868
    degeneri (area nulla, scartati apposta), l'ASCII con i 1533 pezzi. Riapertura del binario ~10 s (lettura 0,07 s,
    saldatura 1 s, normali 1,6 s, testo intermedio 3 s, parsing 4,4 s): possibile miglioramento costruendo la scena
    senza passare dal testo.

---

## 8. Roadmap (aggiornata 2026-11-07)

Ordine scelto: prima ciò che può dare un risultato sbagliato, poi la grafica, poi le funzioni nuove, che così nascono già
nell'aspetto definitivo.

### Step 1 — Affidabilità dei dati
- [x] Export sui file corretti: esportare e riaprire Sottoscocca_A (pezzi con `scale 0.001`) e confrontare la geometria
- [x] Normali sbagliate nei file in metri (trovato con la prova di export)
- [x] Parser: `geometry DEF Nome IndexedFaceSet` e `geometry USE Nome`
- [x] Parser: contenuto di `LOD` (`level`) e `Switch` (`choice`)
- [x] Parser: `DEF` ridefinito, da risolvere sulla definizione precedente al `USE`
- [x] Avviso visibile se una mesh oltre 256 MB non viene disegnata
- [x] Errore chiaro all'apertura dei file VRML 1.0
- [x] Lettura dei file VRML 1.0 (`Sportello_V1`, `Supporto_V1`)
- [x] Prova dell'exe su un altro PC: funziona (utente, 2026-11-07)

### Step 2 — Grafica professionale
- [x] Icona dell'applicazione e manifest Windows (DPI, stile moderno)
- [x] Tema curato, chiaro e scuro, con colori e spaziature coerenti al posto del tema standard di egui
- [x] Icone vere e uniformi al posto delle emoji nella toolbar e nell'albero
- [x] Toolbar a gruppi; vista 3D più pulita: comandi e aiuti raccolti, statistiche nella barra di stato
- [x] Normali originali dei file VRML 2.0: `normalIndex`, `normalPerVertex`, spigoli vivi (vedi note 2026-11-07)
- [x] Resa 3D: antialiasing MSAA 4x (2026-11-07)
- [x] Resa 3D: illuminazione rivista (2026-11-07)
- [x] Resa 3D: selezione con contorno (2026-11-07)
- [x] Viste standard in coordinate veicolo (2026-11-07, segnalato dall'utente: "Fronte" mostrava la scocca dall'alto)
- [x] Resa 3D: sfondo sfumato opzionale (2026-11-07)
- [x] Cubo di orientamento cliccabile (2026-11-07; la terna XYZ resta in basso a sinistra)

### Step 3 — Funzioni di analisi (proposte, da confermare con l'utente)
- [x] **Più matematiche nella stessa scena (2026-11-07, richiesta dell'utente):** "Aggiungi", trascinamento, un nodo per
  file, chiudi/esporta un file, conversione delle unità (vedi note 2026-11-07)
- [x] Pannello proprietà del pezzo selezionato: nome, tipo, file, pezzi, triangoli, vertici, dimensioni, min/max (2026-11-07)
- [x] Misure: distanza tra due punti con ΔX ΔY ΔZ e aggancio ai vertici (2026-11-07)
- [x] Piano di sezione X/Y/Z con cursore e "Inverti" (2026-11-07)
- [x] Cursore tolleranza del raggruppamento automatico (2026-11-07)
- [x] Nodi `Color` per vertice/faccia: nessun file reale li usa (verificato 2026-11-07), non implementati

### Step 4 — Rifinitura per file enormi e rilascio
- [x] Profiling su file da 800-900 MB: tempi di apertura e memoria (2026-11-07, vedi note)
- [x] Memoria di Annulla: misurata, 3 MB e 1,4 ms a passo sul file con più nodi (Paraurti_D): nessuno storico a comandi necessario
- [x] Trasparenze: misurate, caso peggiore (18.051 pezzi tutti semitrasparenti) 247 fps: nessuna modifica necessaria
- [ ] Exe finale: cartella pronta da copiare (`gestore-wrl.exe`, `tools\` con i convertitori, `licenses\`), zip

### Step 5 — Formati e movimentazione (richieste dell'utente 2026-11-07)

Scelta sui convertitori: restano **programmi separati** in `tools\` (come `jt2wrl.exe`), non DLL caricate nel Gestore.
Un file rotto o un errore della libreria chiude solo il convertitore, non il Gestore con il lavoro aperto; avanzamento ed
errori passano già da stdout/stderr (`parser/jt.rs`), e ogni convertitore si aggiorna senza ricompilare il Gestore.

- [x] **STL, import ed export** (2026-11-08, vedi note): binario e ASCII in apertura (un pezzo per `solid`); export
      binario (predefinito) o ASCII con un `solid` per pezzo, scegliendo `.stl` nella finestra Salva
- [ ] **STEP, import** (convertitore separato `step2wrl.exe` con OpenCASCADE, LGPL): tassellazione della geometria
      esatta con tolleranza regolabile, nomi e colori dell'assieme (XCAF), trasformazioni. Da valutare peso e tempi su
      una matematica reale.
- [ ] **STEP, export**: dalla mesh si può scrivere solo geometria a facce piane (B-rep "sfaccettato" o tassellato
      AP242), non superfici CAD vere; utile per scambio/visualizzazione, non per riprogettare. Da confermare se serve
      così.
- [ ] **Rotazione di un pezzo attorno a un asse** (es. baule sulle due cerniere): selezione dei pezzi da muovere, asse
      definito da due punti cliccati con aggancio ai vertici (come le Misure) — o centro di un foro/perno da tre punti
      sul bordo — oppure un punto + asse X/Y/Z; angolo con cursore e valore numerico, anteprima dal vivo, Annulla.
      In seguito anche la traslazione. Nell'export il pezzo esce nella posizione ruotata (`Transform` attorno ai suoi
      figli). Da decidere con l'utente: punti sulle cerniere o centro del perno; salvare più posizioni (aperto/chiuso).

### Step 6 — WRL più leggeri (richiesta dell'utente 2026-11-07: JT da 70 MB → WRL da 600 MB)

Misura sui WRL prodotti da jt2wrl (Warehouse, san2, CoffeeMaker): coordinate 19–47%, normali 31–33%, `coordIndex` e
`normalIndex` 10–24% ciascuno, spazi di rientro 20–45% del file. Dimensione finale rispetto a oggi:

| Tecnica | Risultato | Perdita |
|---|---|---|
| Niente rientri, niente virgole, numeri a 3 decimali in mm (0,001 mm) | 39–61% | nessuna visibile |
| + normali omesse (ricalcolate con `creaseAngle`) | 19–41% | spigoli calcolati invece che quelli del CAD |

gzip escluso: i programmi dell'utente non leggono i WRL compressi (risposta dell'utente 2026-11-07).

- [x] Scrittura compatta, sia in jt2wrl sia nell'export del Gestore (2026-11-08, vedi note)
- [x] **Riduzione delle facce nel Gestore** (2026-11-08; prima l'utente usava MeshLab, "Quadric Edge Collapse
      Decimation", provando più percentuali): pulsante "Riduci", obiettivo in facce o errore massimo, tutti i pezzi
      visibili o la selezione, anteprima nella vista, dimensione prevista con e senza normali, "Originale" per tornare
      indietro, "Esporta WRL…" (vedi note)
- [x] Normali dopo la riduzione: quelle del file originale nei vertici tenuti; opzione "normali sì/no" nell'export
- [ ] Pezzi ripetuti (viti, clip) scritti una volta con `DEF` e richiamati con `USE` dentro un `Transform`
- [ ] Unione dei vertici doppi dentro ogni pezzo
- [ ] Per i JT: usare anche un LOD più leggero già presente nel file

### Step 7 — Export Open Inventor e HSF (valutazione 2026-11-07)

- [ ] **IV ASCII** (`#Inventor V2.1 ascii`): fattibile, basso rischio. Il VRML 1.0, che il Gestore legge già, deriva da
      Inventor: `Separator`, `Material`, `Coordinate3`, `Normal`/`NormalBinding`, `IndexedFaceSet`, `Transform`.
- [ ] **IV binario** (`#Inventor V2.1 binary`): fattibile, rischio medio. Formato non documentato in una specifica
      pubblica: riferimento al codice di Coin3D (BSD-3, github.com/coin3d/coin), che sa anche rileggerlo per la verifica.
      Più compatto del testo e più veloce da aprire.
- [ ] **HSF** (HOOPS Stream File, Tech Soft 3D): fattibilità incerta. Formato proprietario senza specifica pubblica,
      con il toolkit commerciale. Unica strada senza licenza HOOPS: il toolkit HOOPS Stream incluso nel DWF Toolkit 7.7
      di Autodesk (`w3dtk`, copia su GitHub), in un convertitore separato. Da verificare: la licenza Autodesk, se il file
      è un `.hsf` puro e se il visualizzatore di destinazione lo apre. Prima di investirci, l'utente deve dire dove
      serve l'HSF e procurare un file `.hsf` di esempio.

### Piano di lavoro in autonomia (proposto 2026-11-07, in attesa di approvazione)

Ordine proposto: prima la richiesta ripetuta dall'utente, poi l'orientamento, poi le funzioni di analisi, infine la
rifinitura. Ogni blocco termina con test automatici, verifica su file reali (immagini o confronti), aggiornamento di
questo file ed exe ricompilato.

| # | Blocco | Contenuto | Verifica |
|---|--------|-----------|----------|
| 1 ✅ | Più matematiche | "Aggiungi file…" accanto ad "Apri"; un nodo radice per file nell'albero; nascondi/isola/colora per file; chiudi un singolo file; inquadratura sull'insieme | Sportello_V1 + Supporto_V1 + Scocca_B insieme; tempi e memoria; export e riapertura |
| 2 ✅ | Cubo di orientamento | Cubo cliccabile in alto a destra con facce Fronte/Retro/Destra/Sinistra/Alto/Basso in coordinate veicolo; spigoli e vertici per le viste intermedie; passaggio animato breve | Test: ogni faccia dà la stessa vista dei tasti 1/3/7; immagini |
| 3 ✅ | Proprietà e misure | Pannello del pezzo selezionato (nome, file, dimensioni X/Y/Z, triangoli); misura distanza tra due punti cliccati, con ΔX ΔY ΔZ | Test sul picking; misure note su pezzi di Scocca_B |
| 4 ✅ | Piano di sezione | Piano X/Y/Z con cursore di posizione, lato da tenere invertibile | Immagini su Scocca_B |
| 5 ✅ | File enormi | Profiling su file da 800–900 MB (apertura, memoria, export); storico Annulla più leggero; trasparenze raggruppate | Tempi prima/dopo nelle note |
| 6 ✅ | Rifiniture | Sfondo sfumato opzionale; cursore tolleranza del raggruppamento; nodi `Color` solo se presenti nei file reali | — |

Regole: rotazione OpenInventor e convenzione veicolo invariate; i file dell'utente solo in lettura (export di prova
nella cartella temporanea); scelte che cambiano il modo di lavorare vanno chieste prima.

Decisioni chieste all'utente prima di partire (con la proposta predefinita):
- Più matematiche: "Apri" sostituisce, "Aggiungi file…" aggiunge; il trascinamento con una scena aperta aggiunge.
- Export dell'insieme: un unico WRL con tutti i file visibili, più l'export di un singolo file dal suo nodo.
- Isometrica: oggi tre quarti anteriore sinistro; da confermare rispetto al visualizzatore usato in azienda.
- Colore di selezione verde: confermare o scegliere.
