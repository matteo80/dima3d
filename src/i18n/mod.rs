//! Lingua dell'interfaccia.
//!
//! La chiave di ogni testo è il testo italiano stesso (come gettext): nel codice si scrive
//! `tr("Apri")` e la traduzione si cerca in `en.txt`. Un testo senza traduzione resta in italiano.
//! Il test `every_key_is_translated` legge i sorgenti e controlla che ogni `tr(..)`, `trf!(..)` e
//! `tn!(..)` abbia la sua riga in `en.txt`.
//!
//! - `tr("…")`: testo fisso.
//! - `trf!("Annulla ({n} passi)", n = steps)`: testo con valori; i segnaposto hanno un nome perché
//!   in inglese l'ordine può cambiare. La formattazione dei numeri (decimali…) si fa prima.
//! - `tn!("…")`: segna un testo da tradurre dove non si può chiamare una funzione (costanti,
//!   tabelle); lo si traduce poi con `tr(..)` quando lo si mostra.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Lingue dell'interfaccia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    It,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::It, Lang::En];

    /// Codice salvato nelle impostazioni.
    pub fn code(self) -> &'static str {
        match self {
            Lang::It => "it",
            Lang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Nome della lingua scritto nella lingua stessa (per il menu).
    pub fn name(self) -> &'static str {
        match self {
            Lang::It => "Italiano",
            Lang::En => "English",
        }
    }

    /// Lingua di Windows: italiano se l'interfaccia di Windows è in italiano, altrimenti inglese.
    pub fn system() -> Lang {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetUserDefaultUILanguage() -> u16;
            }
            // SAFETY: funzione di sistema senza argomenti che restituisce solo un numero
            let id = unsafe { GetUserDefaultUILanguage() };
            // Lingua primaria nei 10 bit bassi: 0x10 = italiano
            if id & 0x3ff == 0x10 {
                Lang::It
            } else {
                Lang::En
            }
        }
        #[cfg(not(windows))]
        {
            Lang::It
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Lingua in uso.
pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::It,
    }
}

/// Cambia la lingua in uso: vale dal fotogramma successivo.
pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// Traduzioni inglesi, lette da `en.txt` alla prima richiesta.
fn english() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| parse(include_str!("en.txt")).into_iter().collect())
}

/// Legge un file di traduzioni: per ogni testo una riga con l'italiano e la riga successiva con
/// `= ` e la traduzione. Righe vuote e righe che iniziano con `#` si saltano. Gli a capo dentro un
/// testo si scrivono `\n`.
fn parse(text: &'static str) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    let mut key: Option<&'static str> = None;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("= ").or_else(|| (trimmed == "=").then_some("")) {
            if let Some(k) = key.take() {
                out.push((unescape(k), unescape(value)));
            }
        } else {
            key = Some(trimmed);
        }
    }
    out
}

/// `\n` → a capo, `\\` → `\`. Le righe senza sequenze restano in prestito dal file.
fn unescape(s: &'static str) -> &'static str {
    if !s.contains('\\') {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    // Tabella letta una volta sola per tutta la durata del programma
    Box::leak(out.into_boxed_str())
}

/// Traduzione di `key` nella lingua indicata (il testo italiano se manca).
pub fn tr_in(lang: Lang, key: &'static str) -> &'static str {
    match lang {
        Lang::It => key,
        Lang::En => english().get(key).copied().unwrap_or(key),
    }
}

/// Traduzione di `key` nella lingua in uso.
pub fn tr(key: &'static str) -> &'static str {
    tr_in(current(), key)
}

/// Sostituisce i segnaposto `{nome}` con i valori. `{{` e `}}` restano graffe.
pub fn fill(template: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('{') {
            if let Some(end) = tail.find('}') {
                let name = &tail[1..end];
                if let Some((_, v)) = args.iter().find(|(n, _)| *n == name) {
                    use std::fmt::Write;
                    let _ = write!(out, "{v}");
                    rest = &tail[end + 1..];
                    continue;
                }
            }
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// Testo tradotto con valori: `trf!("Annulla ({n} passi)", n = steps)`.
macro_rules! trf {
    ($key:literal $(, $name:ident = $val:expr)* $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::tr($key),
            &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),*],
        )
    };
}

/// Segna un testo da tradurre senza tradurlo (per costanti e tabelle): resta il testo italiano,
/// da passare a `tr(..)` quando lo si mostra.
#[allow(unused_macros)]
macro_rules! tn {
    ($key:literal) => {
        $key
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_fallback() {
        assert_eq!(tr_in(Lang::It, "Apri"), "Apri");
        assert_eq!(tr_in(Lang::En, "Apri"), "Open");
        assert_eq!(tr_in(Lang::En, "testo senza traduzione"), "testo senza traduzione");
    }

    #[test]
    fn fill_placeholders() {
        let n = 3;
        assert_eq!(fill("Annulla ({n} passi)", &[("n", &n)]), "Annulla (3 passi)");
        assert_eq!(fill("{b} prima di {a}", &[("a", &"A"), ("b", &"B")]), "B prima di A");
        assert_eq!(fill("{{graffe}} e {ignoto}", &[]), "{graffe} e {ignoto}");
    }

    #[test]
    fn file_format() {
        let t = parse("# commento\nUno\n= One\n\nDue\\nrighe\n= Two\\nlines\nVuoto\n=\n");
        assert_eq!(t, vec![("Uno", "One"), ("Due\nrighe", "Two\nlines"), ("Vuoto", "")]);
    }

    #[test]
    fn codes_roundtrip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
    }

    /// Legge una stringa Rust "normale" a partire dalle virgolette: restituisce il valore e dove finisce.
    fn rust_literal(src: &str) -> Option<String> {
        let mut out = String::new();
        let mut chars = src.strip_prefix('"')?.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    '\\' => out.push('\\'),
                    '0' => out.push('\0'),
                    // Continuazione di riga: si saltano a capo e spazi iniziali della riga dopo
                    '\n' | '\r' => {
                        while chars.peek().is_some_and(|c| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    'u' => {
                        let hex: String = chars.by_ref().skip(1).take_while(|&c| c != '}').collect();
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    other => panic!("sequenza \\{other} non gestita"),
                },
                '\r' => {}
                c => out.push(c),
            }
        }
        None
    }

    /// Tutti i testi da tradurre scritti nei sorgenti, con il file dove compaiono.
    fn keys_in_sources() -> Vec<(String, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        let mut dirs = vec![root];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|e| e == "rs") && !p.ends_with("i18n/mod.rs") && !p.ends_with("i18n\\mod.rs") {
                    files.push(p);
                }
            }
        }
        let mut keys = Vec::new();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            for marker in ["tr(", "trf!(", "tn!("] {
                for (i, _) in text.match_indices(marker) {
                    // Evita `foo_tr("` o `.tr("`: deve iniziare una parola
                    let before = text[..i].chars().last();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        continue;
                    }
                    // Esempi nei commenti
                    let line_start = text[..i].rfind('\n').map_or(0, |p| p + 1);
                    if text[line_start..i].trim_start().starts_with("//") {
                        continue;
                    }
                    // Il testo può iniziare sulla riga dopo; `tr(nome)` (non una stringa) si salta
                    let after = &text[i + marker.len()..];
                    let literal = after.trim_start();
                    if !literal.starts_with('"') {
                        continue;
                    }
                    let key = rust_literal(literal).expect("stringa non chiusa");
                    keys.push((key, f.file_name().unwrap().to_string_lossy().into_owned()));
                }
            }
        }
        keys
    }

    #[test]
    fn every_key_is_translated() {
        let table = english();
        let mut missing: Vec<String> = keys_in_sources()
            .into_iter()
            .filter(|(k, _)| !table.contains_key(k.as_str()))
            .map(|(k, file)| format!("{file}: {}", k.replace('\n', "\\n")))
            .collect();
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "{} testi senza traduzione in en.txt:\n{}", missing.len(), missing.join("\n"));
    }

    #[test]
    fn translations_keep_placeholders() {
        // Ogni {nome} dell'italiano deve esserci anche nell'inglese, altrimenti il valore sparisce
        let names = |s: &str| {
            let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string())).collect();
            v.sort();
            v
        };
        let wrong: Vec<&str> = english().iter().filter(|(k, v)| names(k) != names(v)).map(|(k, _)| *k).collect();
        assert!(wrong.is_empty(), "segnaposto diversi: {wrong:?}");
    }

    #[test]
    fn no_duplicate_keys() {
        let mut keys: Vec<&str> = parse(include_str!("en.txt")).into_iter().map(|(k, _)| k).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "chiavi ripetute in en.txt");
    }
}
