use std::collections::HashMap;

/// Costruisce due mappe che associano ogni delimitatore di apertura
/// alla posizione del corrispondente delimitatore di chiusura.
///
/// - `brace_map`:   posizione `{` → posizione `}`
/// - `bracket_map`: posizione `[` → posizione `]`
///
/// Il single-pass gestisce correttamente:
/// - Commenti VRML: dal carattere `#` fino a fine riga
/// - Stringhe doppie virgolette: `"..."` con escape `\"`
pub fn build_delimiter_maps(text: &[u8]) -> (HashMap<usize, usize>, HashMap<usize, usize>) {
    let mut brace_map: HashMap<usize, usize> = HashMap::new();
    let mut bracket_map: HashMap<usize, usize> = HashMap::new();

    let mut brace_stack: Vec<usize> = Vec::new();
    let mut bracket_stack: Vec<usize> = Vec::new();

    let len = text.len();
    let mut i = 0;

    while i < len {
        match text[i] {
            // Commento: salta fino a fine riga
            b'#' => {
                i += 1;
                while i < len && text[i] != b'\n' {
                    i += 1;
                }
            }
            // Stringa: salta fino alla virgolette di chiusura (gestisce escape)
            b'"' => {
                i += 1;
                while i < len {
                    if text[i] == b'\\' {
                        // Carattere di escape: salta il prossimo byte
                        i += 2;
                    } else if text[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'{' => {
                brace_stack.push(i);
                i += 1;
            }
            b'}' => {
                if let Some(open) = brace_stack.pop() {
                    brace_map.insert(open, i);
                }
                i += 1;
            }
            b'[' => {
                bracket_stack.push(i);
                i += 1;
            }
            b']' => {
                if let Some(open) = bracket_stack.pop() {
                    bracket_map.insert(open, i);
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    (brace_map, bracket_map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_braces() {
        let data = b"{ { } }";
        let (bm, _) = build_delimiter_maps(data);
        assert_eq!(bm.get(&0), Some(&6));
        assert_eq!(bm.get(&2), Some(&4));
    }

    #[test]
    fn test_comment_ignored() {
        let data = b"{ # { comment\n}";
        let (bm, _) = build_delimiter_maps(data);
        // Solo la brace esterna deve essere abbinata
        assert_eq!(bm.get(&0), Some(&14));
        assert!(!bm.contains_key(&3));
    }

    #[test]
    fn test_string_ignored() {
        let data = b"{ \"{ not a brace }\" }";
        let (bm, _) = build_delimiter_maps(data);
        assert_eq!(bm.get(&0), Some(&20));
    }
}
