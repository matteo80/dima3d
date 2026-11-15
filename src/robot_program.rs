//! Programmi del robot: righe del programma e punti, per disegnare la traiettoria nella vista e
//! cambiare l'ordine dei movimenti.
//!
//! Due modi di scrivere gli stessi programmi:
//! * `.TIS` + `.TID`: `LIN( AP_1)`, argomenti separati da spazi, commenti `--`; punti
//!   `CONST POINT AP_1 = Object x y z a b c …`.
//! * `.tip` + `.tid`: `LIN(ap_1)`, argomenti separati da virgole, commenti `//`; punti
//!   `ap_1 : POINT := (Object, x, y, z, a, b, c, …)`.
//!
//! I punti `Object` sono nelle coordinate dell'oggetto: con la matematica messa con il riferimento
//! oggetto nello zero della scena si disegnano così come sono (mm). I punti `World` (coordinate del
//! robot) e `Axis` (assi) non si possono disegnare senza il robot: si contano e si saltano.
//!
//! I file si leggono e si scrivono byte per carattere (Latin-1): salvando, i caratteri che il
//! programma non tocca restano identici.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use glam::Vec3;

/// Riferimento delle coordinate di un punto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    Object,
    World,
    Axis,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeachPoint {
    pub frame: Frame,
    /// Object/World: X Y Z A B C e gli eventuali valori in più; Axis: gli assi
    pub values: Vec<f64>,
}

impl TeachPoint {
    /// Posizione nella scena (mm), solo per i punti in coordinate dell'oggetto.
    pub fn position(&self) -> Option<Vec3> {
        match (self.frame, self.values.as_slice()) {
            (Frame::Object, [x, y, z, ..]) => Some(Vec3::new(*x as f32, *y as f32, *z as f32)),
            _ => None,
        }
    }
}

/// Una riga del programma, com'è nel file (senza il fine riga).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line(pub String);

/// Movimento verso un punto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Lin,
    Ptp,
}

impl Line {
    /// Rientro (spazi all'inizio, tabulazione = 2).
    pub fn indent(&self) -> usize {
        self.0.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 2 } else { 1 }).sum()
    }

    pub fn command(&self) -> &str {
        self.0.trim()
    }

    pub fn is_comment(&self) -> bool {
        let c = self.command();
        c.starts_with("--") || c.starts_with("//")
    }

    /// Nome del comando e argomenti: `SETBRUSH( Gun1 6 P1 TR1)` → (`SETBRUSH`, [Gun1, 6, P1, TR1]).
    pub fn call(&self) -> Option<(&str, Vec<&str>)> {
        if self.is_comment() {
            return None;
        }
        let c = self.command();
        let open = c.find('(')?;
        let close = c.rfind(')').filter(|&e| e > open)?;
        let name = c[..open].trim();
        if name.is_empty() || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return None;
        }
        let args = c[open + 1..close]
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .map(|a| a.trim_matches('"'))
            .filter(|a| !a.is_empty())
            .collect();
        Some((name, args))
    }

    /// `LIN(punto)` o `PTP(punto)`: movimento e nome del punto.
    pub fn motion(&self) -> Option<(Motion, &str)> {
        let (name, args) = self.call()?;
        let m = if name.eq_ignore_ascii_case("LIN") {
            Motion::Lin
        } else if name.eq_ignore_ascii_case("PTP") {
            Motion::Ptp
        } else {
            return None;
        };
        Some((m, args.first().copied()?))
    }
}

/// Programma letto: righe e punti (dal file dei dati e dai dati globali della cartella).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub program_path: PathBuf,
    /// File dei punti del programma (stesso nome, estensione `.tid`)
    pub data_path: Option<PathBuf>,
    pub lines: Vec<Line>,
    /// Punti per nome in maiuscolo
    pub points: HashMap<String, TeachPoint>,
    /// Fine riga del file (`\r\n` o `\n`)
    pub line_ending: &'static str,
    /// Il file finiva con un fine riga
    pub final_newline: bool,
}

/// Punto della traiettoria disegnata.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPoint {
    /// Riga del programma
    pub line: usize,
    pub name: String,
    pub motion: Motion,
    /// `None`: punto senza coordinate dell'oggetto (Axis, World o non trovato): la linea si interrompe
    pub position: Option<Vec3>,
}

/// Punto usato da un comando che non muove (es. accensione della pistola sulla traiettoria).
#[derive(Debug, Clone, PartialEq)]
pub struct EventPoint {
    pub line: usize,
    pub name: String,
    pub position: Vec3,
}

/// Byte → testo, un carattere per byte.
pub fn decode(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Testo → byte, al contrario di `decode` (caratteri oltre 255: `?`).
pub fn encode(text: &str) -> Vec<u8> {
    text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect()
}

fn numbers<'a>(it: impl Iterator<Item = &'a str>) -> Vec<f64> {
    it.filter_map(|x| x.trim().parse().ok()).collect()
}

fn frame_of(word: &str) -> Option<Frame> {
    match word.trim().to_ascii_lowercase().as_str() {
        "object" => Some(Frame::Object),
        "world" => Some(Frame::World),
        "axis" => Some(Frame::Axis),
        _ => None,
    }
}

/// Una riga del file dei dati: `CONST POINT N = Object …` oppure `N : POINT := (Object, …)`.
pub fn parse_point_line(line: &str) -> Option<(String, TeachPoint)> {
    let line = line.trim();
    if let Some((lhs, rhs)) = line.split_once(":=") {
        let (name, typ) = lhs.split_once(':')?;
        if !typ.trim().eq_ignore_ascii_case("POINT") {
            return None;
        }
        let body = rhs.trim().trim_start_matches('(');
        let body = body.rsplit_once(')').map_or(body, |(b, _)| b);
        let mut parts = body.split(',');
        let frame = frame_of(parts.next()?)?;
        return Some((name.trim().to_string(), TeachPoint { frame, values: numbers(parts) }));
    }
    let mut words = line.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("CONST") || !words.next()?.eq_ignore_ascii_case("POINT") {
        return None;
    }
    let name = words.next()?.to_string();
    if words.next()? != "=" {
        return None;
    }
    let frame = frame_of(words.next()?)?;
    Some((name, TeachPoint { frame, values: numbers(words) }))
}

pub fn parse_points(text: &str, into: &mut HashMap<String, TeachPoint>) {
    for (name, p) in text.lines().filter_map(parse_point_line) {
        into.insert(name.to_ascii_uppercase(), p);
    }
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// File della stessa cartella con lo stesso nome e una di queste estensioni (maiuscole o minuscole).
fn sibling(path: &Path, exts: &[&str]) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.is_file() && has_ext(p, exts) && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(stem))
    })
}

/// Estensioni dei programmi e dei dati.
pub const PROGRAM_EXTS: [&str; 2] = ["tis", "tip"];
pub const DATA_EXTS: [&str; 1] = ["tid"];

/// File di un programma o dei suoi punti (per aprirlo trascinandolo sulla finestra).
pub fn is_program_file(path: &Path) -> bool {
    has_ext(path, &PROGRAM_EXTS) || has_ext(path, &DATA_EXTS)
}

impl Program {
    pub fn from_text(program_path: PathBuf, text: &str) -> Program {
        let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let final_newline = text.ends_with('\n');
        let lines = text.lines().map(|l| Line(l.trim_end_matches('\r').to_string())).collect();
        Program { program_path, data_path: None, lines, points: HashMap::new(), line_ending, final_newline }
    }

    /// Apre un programma (`.tis`/`.tip`) o il suo file dei punti (`.tid`): legge l'altro dalla
    /// stessa cartella, più i dati globali (file `.tid` che iniziano con `_`).
    pub fn load(path: &Path) -> Result<Program, String> {
        let program_path = if has_ext(path, &DATA_EXTS) {
            sibling(path, &PROGRAM_EXTS).ok_or_else(|| format!("Nessun programma .tis o .tip accanto a {}", path.display()))?
        } else {
            path.to_path_buf()
        };
        let bytes = std::fs::read(&program_path).map_err(|e| format!("{}: {e}", program_path.display()))?;
        let mut program = Program::from_text(program_path.clone(), &decode(&bytes));
        // Prima i globali, poi quelli del programma (che hanno la precedenza)
        if let Some(dir) = program_path.parent() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut globals: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| has_ext(p, &DATA_EXTS) && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('_')))
                    .collect();
                globals.sort();
                for g in globals {
                    if let Ok(b) = std::fs::read(&g) {
                        parse_points(&decode(&b), &mut program.points);
                    }
                }
            }
        }
        program.data_path = sibling(&program_path, &DATA_EXTS);
        if let Some(data) = &program.data_path {
            let b = std::fs::read(data).map_err(|e| format!("{}: {e}", data.display()))?;
            parse_points(&decode(&b), &mut program.points);
        }
        Ok(program)
    }

    pub fn point(&self, name: &str) -> Option<&TeachPoint> {
        self.points.get(&name.to_ascii_uppercase())
    }

    /// Movimenti nell'ordine del programma.
    pub fn path(&self) -> Vec<PathPoint> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let (motion, name) = l.motion()?;
                Some(PathPoint {
                    line: i,
                    name: name.to_string(),
                    motion,
                    position: self.point(name).and_then(TeachPoint::position),
                })
            })
            .collect()
    }

    /// Punti nominati dai comandi che non muovono (es. pistola accesa o spenta in un punto).
    pub fn events(&self) -> Vec<EventPoint> {
        let mut out = Vec::new();
        for (i, l) in self.lines.iter().enumerate() {
            if l.motion().is_some() {
                continue;
            }
            let Some((_, args)) = l.call() else { continue };
            for a in args {
                if let Some(position) = self.point(a).and_then(TeachPoint::position) {
                    out.push(EventPoint { line: i, name: a.to_string(), position });
                }
            }
        }
        out
    }

    /// Primo argomento di un comando (es. `SETOBJECT`), per l'intestazione.
    pub fn argument_of(&self, command: &str) -> Option<String> {
        self.lines.iter().find_map(|l| {
            let (name, args) = l.call()?;
            name.eq_ignore_ascii_case(command).then(|| args.first().map(|s| s.to_string())).flatten()
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = self.lines.iter().map(|l| l.0.as_str()).collect::<Vec<_>>().join(self.line_ending);
        if self.final_newline {
            out.push_str(self.line_ending);
        }
        out
    }

    /// Salva il programma in `path`. Con un nome o una cartella diversi copia accanto anche il
    /// file dei punti con lo stesso nome, se non ce n'è già uno. Ritorna il file dei punti copiato.
    pub fn save_as(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        std::fs::write(path, encode(&self.to_text())).map_err(|e| format!("{}: {e}", path.display()))?;
        let Some(data) = &self.data_path else { return Ok(None) };
        let ext = data.extension().and_then(|e| e.to_str()).unwrap_or("tid");
        let target = path.with_extension(ext);
        if target == *data || sibling(path, &DATA_EXTS).is_some() {
            return Ok(None);
        }
        std::fs::copy(data, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        Ok(Some(target))
    }
}

// ── Modifiche alla sequenza ──────────────────────────────────────────────────────────────────

/// Righe in `selected` (indici in ordine crescente, senza ripetizioni).
pub fn take(lines: &[Line], selected: &[usize]) -> Vec<Line> {
    selected.iter().filter_map(|&i| lines.get(i).cloned()).collect()
}

/// Toglie le righe scelte.
pub fn remove(lines: &mut Vec<Line>, selected: &[usize]) {
    let mut k = 0;
    lines.retain(|_| {
        let keep = selected.binary_search(&k).is_err();
        k += 1;
        keep
    });
}

/// Inverte l'ordine delle righe scelte, ognuna al posto di un'altra scelta.
pub fn reverse(lines: &mut [Line], selected: &[usize]) {
    let taken = take(lines, selected);
    for (&i, l) in selected.iter().zip(taken.into_iter().rev()) {
        lines[i] = l;
    }
}

/// Inserisce `new` prima della riga `at` (`lines.len()` = in fondo). Ritorna gli indici nuovi.
pub fn insert(lines: &mut Vec<Line>, at: usize, new: Vec<Line>) -> Vec<usize> {
    let at = at.min(lines.len());
    let n = new.len();
    lines.splice(at..at, new);
    (at..at + n).collect()
}

/// Sposta le righe scelte prima della riga `before` (indice prima dello spostamento). Ritorna
/// gli indici nuovi delle righe spostate.
pub fn move_to(lines: &mut Vec<Line>, selected: &[usize], before: usize) -> Vec<usize> {
    let moved = take(lines, selected);
    let at = before - selected.iter().filter(|&&i| i < before).count();
    remove(lines, selected);
    insert(lines, at, moved)
}

/// Rientro della riga `at` (per le righe incollate): quello della riga sopra, o della sotto.
pub fn indent_like(lines: &[Line], at: usize, new: &mut [Line]) {
    let Some(reference) = at.checked_sub(1).and_then(|i| lines.get(i)).or_else(|| lines.get(at)) else { return };
    let width = reference.indent();
    // Le righe di apertura di un blocco (es. MOVE) hanno sotto un rientro in più
    let opens = reference.call().is_some_and(|(n, _)| n.eq_ignore_ascii_case("MOVE")) && at > 0;
    let width = width + if opens { 2 } else { 0 };
    let min = new.iter().map(Line::indent).min().unwrap_or(0);
    for l in new {
        let extra = l.indent() - min;
        l.0 = format!("{}{}", " ".repeat(width + extra), l.command());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACES: &str = "STARTPROG( \"prova_a\")\r\n  SELECT( \"R1\")\r\n  --\r\n  SETOBJECT( PEZZO)\r\n  MOVE( \"prova_a\")\r\n    LIN( A_1)\r\n    SETBRUSH( Gun1 6 P1 TR1)\r\n    LIN( A_2)\r\n    --LIN( A_9)\r\n    PTP( CASA)\r\n  ENDMOVE( \"prova_a\")\r\nENDPROG( \"prova_a\")\r\nRETURN \r\n";
    const SPACES_DATA: &str = "--POINT\r\nCONST POINT A_1 = Object 1 2 3 -158.734 0.093 -22.369 0 0 0\r\nCONST POINT P1 = Object 4 5 6 0 0 0 0 0 0\r\nCONST POINT A_2 = Object 7.5 8 9 0 0 0 0 0 0\r\n--TRIGGERFRAME\r\nCONST TRIGGERFRAME TR1 = Object 0 0 0 0\r\n";
    const COMMAS: &str = "RSELECT(\"R1\")\n  //\n  SETOBJECT(PEZZO)\n  LIN(b_1)\n  GUN(GUN1, GunOn, PT2, TR2)\n  LIN(b_2)\nRDESELECT(\"R1\")\n";
    const COMMAS_DATA: &str = "// POINT\nb_1 : POINT := (Object, 1, 2, 3, 133.172, -33.883, 143.851, 0, 0, 0)\nb_2 : POINT := (World, 4, 5, 6, 0, 0, 0)\nPT2 : POINT := (Object, 7, 8, 9, 0, 0, 0)\nTR2 : TRIGGERFRAME := (Object, 0, 0, 0, 0)\n";
    const GLOBAL: &str = "--POINT\r\nCONST POINT CASA = Axis -6.7 49.6 -91 0 0 0\r\n";

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dima3d_programma_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn points_in_both_forms() {
        let (n, p) = parse_point_line("CONST POINT A_1 = Object 1 2 3 -158.734 0.093 -22.369 0 0 0").unwrap();
        assert_eq!((n.as_str(), p.frame, p.values.len()), ("A_1", Frame::Object, 9));
        assert_eq!(p.position(), Some(Vec3::new(1.0, 2.0, 3.0)));
        let (n, p) = parse_point_line("b_2 : POINT := (World, 4, 5, 6, 0, 0, 0)").unwrap();
        assert_eq!((n.as_str(), p.frame), ("b_2", Frame::World));
        assert_eq!(p.position(), None, "World: coordinate del robot");
        assert!(parse_point_line("CONST TRIGGERFRAME TR1 = Object 0 0 0 0").is_none());
        assert!(parse_point_line("TR2 : TRIGGERFRAME := (Object, 0, 0, 0, 0)").is_none());
        assert!(parse_point_line("NUM CONTATORE").is_none());
    }

    #[test]
    fn commands_and_motions() {
        assert_eq!(Line("    LIN( A_1)".into()).motion(), Some((Motion::Lin, "A_1")));
        assert_eq!(Line("  LIN(b_1)".into()).motion(), Some((Motion::Lin, "b_1")));
        assert_eq!(Line("PTP( CASA)".into()).motion(), Some((Motion::Ptp, "CASA")));
        assert_eq!(Line("    --LIN( A_9)".into()).motion(), None, "commentata");
        assert_eq!(Line("  //LIN(x)".into()).motion(), None);
        assert_eq!(Line("CALL\"RHOME\"".into()).call(), None);
        let l = Line("  GUN(GUN1, GunOn, PT2, TR2)".into());
        assert_eq!(l.call(), Some(("GUN", vec!["GUN1", "GunOn", "PT2", "TR2"])));
        assert_eq!(Line("  SELECT( \"R1\")".into()).call(), Some(("SELECT", vec!["R1"])));
        assert_eq!(Line("    LIN( A_1)".into()).indent(), 4);
    }

    #[test]
    fn loads_program_with_spaces_and_global_points() {
        let d = dir("spazi");
        std::fs::write(d.join("PROVA.TIS"), SPACES).unwrap();
        std::fs::write(d.join("PROVA.TID"), SPACES_DATA).unwrap();
        std::fs::write(d.join("_GLOBAL.TID"), GLOBAL).unwrap();
        // Anche scegliendo il file dei punti si apre il programma
        let p = Program::load(&d.join("PROVA.TID")).unwrap();
        assert_eq!(p.program_path, d.join("PROVA.TIS"));
        assert_eq!(p.line_ending, "\r\n");
        assert_eq!(p.argument_of("SETOBJECT").as_deref(), Some("PEZZO"));
        assert_eq!(p.argument_of("SELECT").as_deref(), Some("R1"));
        let path = p.path();
        assert_eq!(path.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["A_1", "A_2", "CASA"]);
        assert_eq!(path[1].position, Some(Vec3::new(7.5, 8.0, 9.0)));
        assert_eq!(path[2].position, None, "Axis, dai dati globali");
        assert!(p.point("casa").is_some(), "nomi senza maiuscole e minuscole");
        let events = p.events();
        assert_eq!(events.len(), 1, "P1 sì, TR1 (TRIGGERFRAME) no");
        assert_eq!((events[0].name.as_str(), events[0].line), ("P1", 6));
        assert_eq!(p.to_text(), SPACES, "si riscrive identico");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn loads_program_with_commas_and_saves_a_copy() {
        let d = dir("virgole");
        std::fs::write(d.join("prova_b.tip"), COMMAS).unwrap();
        std::fs::write(d.join("prova_b.tid"), COMMAS_DATA).unwrap();
        let mut p = Program::load(&d.join("prova_b.tip")).unwrap();
        assert_eq!(p.line_ending, "\n");
        let path = p.path();
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].position, Some(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(path[1].position, None, "World");
        assert_eq!(p.events()[0].name, "PT2");
        // Inverte i due LIN e salva con un altro nome: il file dei punti si copia accanto
        reverse(&mut p.lines, &[3, 5]);
        let copy = p.save_as(&d.join("nuovo.tip")).unwrap();
        assert_eq!(copy, Some(d.join("nuovo.tid")));
        let again = Program::load(&d.join("nuovo.tip")).unwrap();
        assert_eq!(again.path().iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["b_2", "b_1"]);
        // Salvato sopra l'originale: il file dei punti non si tocca
        assert_eq!(p.save_as(&d.join("prova_b.tip")).unwrap(), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn non_ascii_bytes_survive() {
        let bytes = b"  // perch\xe8 \xc3\xa8\n  LIN(a)\n".to_vec();
        assert_eq!(encode(&decode(&bytes)), bytes);
    }

    fn names(lines: &[Line]) -> Vec<&str> {
        lines.iter().map(Line::command).collect()
    }

    fn lines(names: &[&str]) -> Vec<Line> {
        names.iter().map(|n| Line(format!("  {n}"))).collect()
    }

    #[test]
    fn sequence_edits() {
        let mut l = lines(&["a", "b", "c", "d", "e"]);
        reverse(&mut l, &[1, 2, 3]);
        assert_eq!(names(&l), ["a", "d", "c", "b", "e"]);
        reverse(&mut l, &[0, 4]);
        assert_eq!(names(&l), ["e", "d", "c", "b", "a"]);

        let mut l = lines(&["a", "b", "c", "d", "e"]);
        assert_eq!(move_to(&mut l, &[3], 1), vec![1]);
        assert_eq!(names(&l), ["a", "d", "b", "c", "e"]);
        assert_eq!(move_to(&mut l, &[0, 1], 4), vec![2, 3], "in giù: prima di «e»");
        assert_eq!(names(&l), ["b", "c", "a", "d", "e"]);
        assert_eq!(move_to(&mut l, &[1, 3], 5), vec![3, 4], "in fondo");
        assert_eq!(names(&l), ["b", "a", "e", "c", "d"]);

        let mut l = lines(&["a", "b", "c"]);
        let cut = take(&l, &[0, 2]);
        remove(&mut l, &[0, 2]);
        assert_eq!(names(&l), ["b"]);
        assert_eq!(insert(&mut l, 1, cut), vec![1, 2]);
        assert_eq!(names(&l), ["b", "a", "c"]);
    }

    #[test]
    fn pasted_lines_take_the_indent_of_the_place() {
        let l = vec![Line("  MOVE( \"x\")".into()), Line("    LIN( A)".into())];
        let mut new = vec![Line("LIN( B)".into()), Line("  GUN( C)".into())];
        indent_like(&l, 2, &mut new);
        assert_eq!(new[0].0, "    LIN( B)");
        assert_eq!(new[1].0, "      GUN( C)");
        let mut new = vec![Line("LIN( B)".into())];
        indent_like(&l, 1, &mut new);
        assert_eq!(new[0].0, "    LIN( B)", "subito dopo MOVE: dentro il blocco");
    }
}
