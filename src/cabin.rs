//! Cabina di verniciatura generata dalle misure: grigliato, pareti modulari, testate con le
//! aperture di ingresso e uscita, soffitto.
//!
//! Riferimento (coordinate veicolo, millimetri): X lungo la cabina dall'ingresso (X = 0) verso
//! l'uscita, Y = 0 sull'asse, Z = 0 sul piano del grigliato, Z verso l'alto. La forma ricalca le
//! cabine dei file di riferimento: montanti 100×100 ogni 2 m, pannelli tra i montanti, zoccolo
//! scuro sotto il grigliato, fascia vetrata, grigliato a barre piatte diviso in moduli.
//!
//! La cabina diventa un file VRML 2.0 tenuto in memoria: nell'albero, nell'export e nella
//! finestra Posizione si comporta come un file aperto.

use std::fmt::Write;

/// Misure della cabina, in millimetri.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CabinParams {
    /// Lunghezza interna (lungo X)
    pub length: f32,
    /// Larghezza interna (lungo Y)
    pub width: f32,
    /// Altezza dal grigliato al soffitto
    pub height: f32,
    /// Profondità del fondo sotto il grigliato (le pareti scendono fin lì)
    pub depth: f32,
    /// Passo dei montanti delle pareti laterali
    pub module: f32,
    /// Lato dei montanti (sezione quadrata)
    pub post: f32,
    /// Spessore dei pannelli, del soffitto e delle testate
    pub panel: f32,
    /// Fascia vetrata nelle pareti laterali
    pub glass: bool,
    /// Quota inferiore e superiore della fascia vetrata (dal grigliato)
    pub glass_from: f32,
    pub glass_to: f32,
    /// Passo delle barre del grigliato (nei due versi)
    pub bar_pitch: f32,
    /// Larghezza e spessore delle barre piatte
    pub bar_width: f32,
    pub bar_thickness: f32,
    /// Moduli del grigliato: lunghezza lungo X e larghezza lungo Y
    pub grid_module_x: f32,
    pub grid_module_y: f32,
    /// Apertura di ingresso (testata a X = 0): larghezza e altezza dal grigliato
    pub entry_width: f32,
    pub entry_height: f32,
    /// Apertura di uscita (testata a X = lunghezza)
    pub exit_width: f32,
    pub exit_height: f32,
}

impl Default for CabinParams {
    /// Misure ricavate dalle cabine dei file di riferimento.
    fn default() -> Self {
        Self {
            length: 20000.0,
            width: 5700.0,
            height: 3100.0,
            depth: 1250.0,
            module: 2000.0,
            post: 100.0,
            panel: 50.0,
            glass: true,
            glass_from: 2200.0,
            glass_to: 3000.0,
            bar_pitch: 500.0,
            bar_width: 40.0,
            bar_thickness: 10.0,
            grid_module_x: 6000.0,
            grid_module_y: 2000.0,
            entry_width: 3000.0,
            entry_height: 2500.0,
            exit_width: 3000.0,
            exit_height: 2500.0,
        }
    }
}

impl CabinParams {
    /// Valori in ordine, per le preferenze.
    pub fn to_text(self) -> String {
        let mut v: Vec<String> = self.values().iter().map(|x| x.to_string()).collect();
        v.push(u8::from(self.glass).to_string());
        v.join(" ")
    }

    /// Dal testo di [`to_text`](Self::to_text); `None` se non torna.
    pub fn from_text(text: &str) -> Option<Self> {
        let mut v: Vec<f32> = text.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let glass = v.pop()? != 0.0;
        let mut p = Self { glass, ..Self::default() };
        let fields = p.fields_mut();
        if v.len() != fields.len() {
            return None;
        }
        for (f, x) in fields.into_iter().zip(v) {
            *f = x;
        }
        Some(p)
    }

    fn values(&self) -> Vec<f32> {
        let mut copy = *self;
        copy.fields_mut().into_iter().map(|f| *f).collect()
    }

    fn fields_mut(&mut self) -> Vec<&mut f32> {
        vec![
            &mut self.length,
            &mut self.width,
            &mut self.height,
            &mut self.depth,
            &mut self.module,
            &mut self.post,
            &mut self.panel,
            &mut self.glass_from,
            &mut self.glass_to,
            &mut self.bar_pitch,
            &mut self.bar_width,
            &mut self.bar_thickness,
            &mut self.grid_module_x,
            &mut self.grid_module_y,
            &mut self.entry_width,
            &mut self.entry_height,
            &mut self.exit_width,
            &mut self.exit_height,
        ]
    }

    /// Misure da correggere prima di costruire (vuoto se vanno bene).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let positive = [
            (self.length, "lunghezza"),
            (self.width, "larghezza"),
            (self.height, "altezza"),
            (self.module, "passo dei montanti"),
            (self.post, "montanti"),
            (self.panel, "spessore dei pannelli"),
            (self.bar_pitch, "passo delle barre"),
            (self.bar_width, "larghezza delle barre"),
            (self.bar_thickness, "spessore delle barre"),
            (self.grid_module_x, "moduli del grigliato"),
            (self.grid_module_y, "moduli del grigliato"),
        ];
        for (v, name) in positive {
            if v.is_nan() || v <= 0.0 {
                out.push(format!("{name}: deve essere maggiore di zero"));
            }
        }
        if self.depth < 0.0 {
            out.push("fondo sotto il grigliato: non può essere negativo".into());
        }
        if self.glass && !(0.0 <= self.glass_from && self.glass_from < self.glass_to && self.glass_to <= self.height) {
            out.push("fascia vetrata: deve stare tra il grigliato e il soffitto, dal basso verso l'alto".into());
        }
        for (w, h, name) in [(self.entry_width, self.entry_height, "ingresso"), (self.exit_width, self.exit_height, "uscita")] {
            if w < 0.0 || h < 0.0 || w > self.width || h > self.height {
                out.push(format!("apertura di {name}: deve stare nella testata (larghezza e altezza della cabina)"));
            }
        }
        // Troppi pezzi: misure probabilmente sbagliate (es. metri invece di millimetri)
        let bars = (self.length / self.bar_pitch.max(1.0)) * (self.width / self.bar_pitch.max(1.0));
        if bars > 1.0e6 || self.length / self.module.max(1.0) > 1000.0 {
            out.push("troppi elementi: controlla che le misure siano in millimetri".into());
        }
        out
    }
}

/// Insieme di parallelepipedi che diventano una sola `Shape`.
#[derive(Default)]
struct Boxes {
    points: Vec<[f32; 3]>,
    faces: Vec<[u32; 4]>,
}

impl Boxes {
    /// Parallelepipedo tra due angoli opposti (si ignorano quelli senza volume).
    fn add(&mut self, a: [f32; 3], b: [f32; 3]) {
        let lo = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let hi = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        if (0..3).any(|i| hi[i] - lo[i] <= 0.01) {
            return;
        }
        let base = self.points.len() as u32;
        for k in 0..8u32 {
            self.points.push([
                if k & 1 != 0 { hi[0] } else { lo[0] },
                if k & 2 != 0 { hi[1] } else { lo[1] },
                if k & 4 != 0 { hi[2] } else { lo[2] },
            ]);
        }
        // Facce viste da fuori in senso antiorario
        for f in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            self.faces.push(f.map(|i| base + i));
        }
    }

    fn write(&self, out: &mut String, name: &str, color: [f32; 3], transparency: f32) {
        if self.faces.is_empty() {
            return;
        }
        let [r, g, b] = color;
        let _ = writeln!(
            out,
            "DEF {name} Shape {{ appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} transparency {transparency} }} }}"
        );
        out.push_str("geometry IndexedFaceSet { solid TRUE creaseAngle 0.5\ncoord Coordinate { point [\n");
        for [x, y, z] in &self.points {
            let _ = writeln!(out, "{} {} {},", round(*x), round(*y), round(*z));
        }
        out.push_str("] }\ncoordIndex [\n");
        for [a, b, c, d] in &self.faces {
            let _ = writeln!(out, "{a} {b} {c} {d} -1");
        }
        out.push_str("] } }\n");
    }
}

/// Decimo di millimetro, senza zeri inutili.
fn round(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

const DARK: [f32; 3] = [0.4, 0.4, 0.4];
const PANEL: [f32; 3] = [0.75, 0.75, 0.75];
const GLASS: [f32; 3] = [0.1, 0.2, 0.3];
const CEILING: [f32; 3] = [0.8, 0.8, 0.8];

/// Posizioni regolari da `0` a `end` con passo `step` (l'ultima è sempre `end`).
fn stations(end: f32, step: f32) -> Vec<f32> {
    let n = (end / step).ceil().max(1.0) as usize;
    (0..=n).map(|k| (k as f32 * step).min(end)).collect::<Vec<_>>().into_iter().fold(Vec::new(), |mut v, x| {
        if v.last().is_none_or(|&l: &f32| x - l > 0.01) {
            v.push(x);
        }
        v
    })
}

/// Testo VRML 2.0 della cabina.
pub fn vrml(p: &CabinParams) -> String {
    let mut out = String::from("#VRML V2.0 utf8\n# Cabina generata da Dima 3D (millimetri)\n");
    let (l, half, h, d, t) = (p.length, p.width / 2.0, p.height, p.depth, p.panel);

    // Grigliato: barre piatte con il piano superiore a Z = 0, un pezzo per modulo
    out.push_str("DEF Grigliato Group { children [\n");
    let xs = stations(l, p.grid_module_x);
    let ys = stations(p.width, p.grid_module_y);
    for (i, x) in xs.windows(2).enumerate() {
        for (j, y) in ys.windows(2).enumerate() {
            let (x0, x1, y0, y1) = (x[0], x[1], y[0] - half, y[1] - half);
            let mut bars = Boxes::default();
            let (bw, bt) = (p.bar_width / 2.0, p.bar_thickness);
            // Barre lungo X e barre lungo Y, sui bordi del modulo e ogni `bar_pitch`
            for y in stations(y1 - y0, p.bar_pitch) {
                let y = (y0 + y).clamp(y0 + bw, y1 - bw);
                bars.add([x0, y - bw, -bt], [x1, y + bw, 0.0]);
            }
            for x in stations(x1 - x0, p.bar_pitch) {
                let x = (x0 + x).clamp(x0 + bw, x1 - bw);
                bars.add([x - bw, y0, -bt * 2.0], [x + bw, y1, -bt]);
            }
            bars.write(&mut out, &format!("Modulo_{}_{}", i + 1, j + 1), DARK, 0.0);
        }
    }
    out.push_str("] }\n");

    // Pareti laterali: montanti, zoccolo sotto il grigliato, pannelli, vetrata
    for (side, sign) in [("sinistra", 1.0f32), ("destra", -1.0)] {
        let y_in = sign * half;
        let y_out = sign * (half + t);
        let post_out = sign * (half + p.post);
        let mut posts = Boxes::default();
        let mut base = Boxes::default();
        let mut panels = Boxes::default();
        let mut glass = Boxes::default();
        let xs = stations(l, p.module);
        for &x in &xs {
            let x = x.clamp(p.post / 2.0, l - p.post / 2.0);
            posts.add([x - p.post / 2.0, y_in, -d], [x + p.post / 2.0, post_out, h]);
        }
        base.add([0.0, y_in, -d], [l, y_out, 0.0]);
        if p.glass {
            panels.add([0.0, y_in, 0.0], [l, y_out, p.glass_from]);
            glass.add([0.0, y_in, p.glass_from], [l, y_out, p.glass_to]);
            panels.add([0.0, y_in, p.glass_to], [l, y_out, h]);
        } else {
            panels.add([0.0, y_in, 0.0], [l, y_out, h]);
        }
        let _ = writeln!(out, "DEF Parete_{side} Group {{ children [");
        posts.write(&mut out, &format!("Montanti_{side}"), DARK, 0.0);
        base.write(&mut out, &format!("Zoccolo_{side}"), DARK, 0.0);
        panels.write(&mut out, &format!("Pannelli_{side}"), PANEL, 0.0);
        glass.write(&mut out, &format!("Vetrata_{side}"), GLASS, 0.5);
        out.push_str("] }\n");
    }

    // Testate: parete piena tranne l'apertura, centrata sull'asse e appoggiata al grigliato
    for (name, x0, x1, ow, oh) in [
        ("ingresso", -t, 0.0, p.entry_width, p.entry_height),
        ("uscita", l, l + t, p.exit_width, p.exit_height),
    ] {
        let mut wall = Boxes::default();
        let (outer, ow) = (half + t, ow / 2.0);
        wall.add([x0, -outer, -d], [x1, outer, 0.0]);
        wall.add([x0, ow, 0.0], [x1, outer, h]);
        wall.add([x0, -outer, 0.0], [x1, -ow, h]);
        wall.add([x0, -ow, oh], [x1, ow, h]);
        wall.write(&mut out, &format!("Testata_{name}"), PANEL, 0.0);
    }

    // Soffitto: semitrasparente, per vedere dentro dall'alto
    let mut ceiling = Boxes::default();
    ceiling.add([-t, -half - t, h], [l + t, half + t, h + t]);
    ceiling.write(&mut out, "Soffitto", CEILING, 0.6);
    out
}

/// Scena della cabina, pronta da aprire o aggiungere come file «Cabina».
pub fn scene(p: &CabinParams) -> crate::scene_graph::SceneGraph {
    let text = vrml(p);
    let mut scene = crate::parser::parse_wrl(text.as_bytes());
    // Le misure sono già in millimetri: niente proposte di cambio di unità
    scene.units_mm = true;
    scene.into_file("Cabina".to_string(), None);
    scene
}

impl crate::app::GestoreApp {
    /// Nodo radice della cabina generata, se è ancora aperta.
    pub fn cabin_node(&self) -> Option<u32> {
        let scene = self.scene_graph.as_ref()?;
        let node = self.ui_state.cabin_node?;
        scene
            .open_files()
            .any(|f| f.node == node && f.path.is_none() && f.copy_of.is_none() && f.name == "Cabina")
            .then_some(node)
    }

    /// Costruisce la cabina con le misure date: sostituisce quella generata prima, se c'è.
    pub fn build_cabin(&mut self, params: CabinParams) {
        let problems = params.problems();
        if !problems.is_empty() {
            self.ui_state.status_message = format!("Cabina non costruita: {}", problems.join("; "));
            return;
        }
        if self.ui_state.is_loading || self.project.busy() {
            return;
        }
        let mut settings = self.settings.clone();
        settings.cabin = params;
        self.apply_settings(settings);
        let replaced = self.cabin_node();
        if let Some(old) = replaced {
            self.apply(crate::app::Action::Delete(old));
        }
        let scene = scene(&params);
        if self.scene_graph.is_some() {
            self.add_loaded_file(scene);
            self.ui_state.cabin_node = self.scene_graph.as_ref().and_then(|s| s.sources.last()).map(|s| s.node);
        } else {
            // Scena vuota: la cabina si apre come un file, con lo stesso percorso del caricamento
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = tx.send(crate::parser::LoadEvent::Finished(scene));
            self.project.loading_path = None;
            self.project.loading_adds = false;
            self.ui_state.is_loading = true;
            self.load_rx = Some(rx);
            self.ui_state.cabin_node = Some(0);
        }
        self.ui_state.status_message = format!(
            "Cabina {}: {} × {} × {} mm",
            if replaced.is_some() { "aggiornata" } else { "costruita" },
            params.length,
            params.width,
            params.height
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(scene: &crate::scene_graph::SceneGraph) -> (glam::Vec3, glam::Vec3) {
        let b = scene.compute_bounding_box();
        (b.min, b.max)
    }

    #[test]
    fn cabin_has_the_declared_size_and_parts() {
        let p = CabinParams::default();
        assert!(p.problems().is_empty(), "{:?}", p.problems());
        let scene = scene(&p);
        assert!(scene.load_warnings.is_empty(), "{:?}", scene.load_warnings);
        let (min, max) = bbox(&scene);
        // Ingombro: testate e pareti fuori dalle misure interne, fondo sotto il grigliato
        assert_eq!(min, glam::Vec3::new(-p.panel, -p.width / 2.0 - p.post, -p.depth));
        assert_eq!(max, glam::Vec3::new(p.length + p.panel, p.width / 2.0 + p.post, p.height + p.panel));
        let names: Vec<String> = (0..scene.nodes.len() as u32).map(|n| scene.display_name(n)).collect();
        for part in ["Grigliato", "Parete_sinistra", "Vetrata_destra", "Testata_ingresso", "Soffitto", "Modulo_1_1"] {
            assert!(names.iter().any(|n| n == part), "manca {part}: {names:?}");
        }
        // Grigliato: 20 m / 6 m → 4 moduli lungo X, 5,7 m / 2 m → 3 lungo Y
        assert!(names.iter().any(|n| n == "Modulo_4_3") && !names.iter().any(|n| n == "Modulo_5_1"));
        assert_eq!(scene.sources[0].name, "Cabina");
    }

    /// L'apertura di ingresso è vuota: un raggio lungo X sull'asse a metà altezza
    /// dell'apertura non tocca la testata.
    #[test]
    fn openings_are_empty() {
        let p = CabinParams { glass: false, ..CabinParams::default() };
        let scene = scene(&p);
        let visible = vec![true; scene.shapes_data.len()];
        // Raggio lungo X da fuori dell'ingresso: cosa colpisce per primo
        let inside = |y: f32, z: f32| {
            let hit = scene.pick(glam::Vec3::new(-1000.0, y, z), glam::Vec3::X, &visible);
            hit.and_then(|m| scene.shapes_data[m as usize].name.clone()).as_deref() == Some("Testata_ingresso")
        };
        assert!(!inside(0.0, p.entry_height / 2.0), "apertura libera");
        assert!(inside(0.0, p.entry_height + 100.0), "sopra l'apertura c'è la parete");
        assert!(inside(p.width / 2.0 - 10.0, 1000.0), "ai lati c'è la parete");
    }

    /// Dal programma: con la scena vuota la cabina si apre come file; «Aggiorna» la sostituisce
    /// (sempre una sola); l'export la scrive.
    #[test]
    fn build_and_update_from_the_app() {
        let mut app = crate::app::GestoreApp::new_headless();
        let ctx = egui::Context::default();
        app.build_cabin(CabinParams::default());
        app.poll_loading(&ctx);
        let cabins = |app: &crate::app::GestoreApp| {
            app.scene_graph.as_ref().map_or(0, |s| s.open_files().filter(|f| f.name == "Cabina").count())
        };
        assert_eq!(cabins(&app), 1, "{}", app.ui_state.status_message);
        assert!(app.cabin_node().is_some());

        app.build_cabin(CabinParams { length: 12000.0, ..CabinParams::default() });
        assert_eq!(cabins(&app), 1, "sostituita, non aggiunta");
        assert!(app.ui_state.status_message.contains("aggiornata"), "{}", app.ui_state.status_message);
        let scene = app.scene_graph.as_ref().unwrap();
        let node = app.cabin_node().unwrap();
        let length = scene.subtree_bounding_box(node);
        assert_eq!(length.max.x - length.min.x, 12000.0 + 2.0 * CabinParams::default().panel);
        assert_eq!(app.settings.cabin.length, 12000.0, "misure ricordate");

        let path = std::env::temp_dir().join("dima3d_cabina_export.wrl");
        crate::exporter::export_with(scene, &crate::exporter::Scope::Visible, &path, Default::default()).unwrap();
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        let _ = std::fs::remove_file(&path);
        // Solo la cabina aperta: quella sostituita non si esporta
        assert_eq!(again.shapes_data.len(), scene.info[node as usize].shape_count as usize);

        // Misure sbagliate: niente cabina nuova, il motivo nella barra di stato
        app.build_cabin(CabinParams { bar_pitch: 0.0, ..CabinParams::default() });
        assert!(app.ui_state.status_message.starts_with("Cabina non costruita"));
        assert_eq!(cabins(&app), 1);
    }

    /// Salva il VRML della cabina con le misure predefinite:
    /// `GESTORE_CABINA_OUT=cabina.wrl cargo test anteprima_cabina -- --ignored`
    #[test]
    #[ignore]
    fn anteprima_cabina() {
        std::fs::write(std::env::var("GESTORE_CABINA_OUT").expect("GESTORE_CABINA_OUT"), vrml(&CabinParams::default())).unwrap();
    }

    #[test]
    fn params_roundtrip_and_checks() {
        let p = CabinParams { length: 12345.0, glass_to: 2900.0, ..CabinParams::default() };
        assert_eq!(CabinParams::from_text(&p.to_text()), Some(p));
        let no_glass = CabinParams { glass: false, ..p };
        assert_eq!(CabinParams::from_text(&no_glass.to_text()), Some(no_glass));
        assert_eq!(CabinParams::from_text("1 2 3"), None);
        let wrong = CabinParams { length: 20.0, width: 5.7, bar_pitch: 0.0, entry_width: 9000.0, ..CabinParams::default() };
        let problems = wrong.problems();
        assert!(problems.iter().any(|p| p.contains("passo delle barre")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("ingresso")), "{problems:?}");
    }
}
