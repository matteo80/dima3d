/// Avanza oltre spazi bianchi e commenti VRML (linee che iniziano con `#`).
/// Ritorna la nuova posizione.
pub fn skip_whitespace_comments(data: &[u8], mut pos: usize) -> usize {
    let len = data.len();
    loop {
        // Salta spazi bianchi ASCII
        while pos < len && data[pos].is_ascii_whitespace() {
            pos += 1;
        }
        // Salta commento fino a fine riga
        if pos < len && data[pos] == b'#' {
            while pos < len && data[pos] != b'\n' {
                pos += 1;
            }
        } else {
            break;
        }
    }
    pos
}

/// Estrae la prossima parola ASCII (token non-whitespace, non-delimitatore).
/// Ritorna `(word_str, next_pos)` oppure `None` se il buffer è terminato.
pub fn scan_word(data: &[u8], pos: usize) -> Option<(&str, usize)> {
    let pos = skip_whitespace_comments(data, pos);
    let len = data.len();
    if pos >= len {
        return None;
    }

    let start = pos;
    let mut end = pos;

    // Un "token" termina su: spazio bianco, commento, o delimitatori strutturali
    while end < len {
        let b = data[end];
        if b.is_ascii_whitespace() || b == b'{' || b == b'}' || b == b'[' || b == b']' || b == b',' || b == b'"' || b == b'#' {
            break;
        }
        end += 1;
    }

    if end == start {
        // Siamo su un singolo carattere delimitatore: restituiscilo come token
        let ch = &data[start..start + 1];
        let s = std::str::from_utf8(ch).ok()?;
        return Some((s, start + 1));
    }

    let word = std::str::from_utf8(&data[start..end]).ok()?;
    Some((word, end))
}

/// Estrae tutti i numeri `f32` dall'intervallo `[start, end)` del buffer.
/// Parsing veloce: gestisce segno, punto decimale, esponente (e/E).
pub fn scan_numbers_f32(data: &[u8], start: usize, end: usize) -> Vec<f32> {
    let slice = &data[start..end.min(data.len())];
    let mut result = Vec::new();
    let mut i = 0;
    let len = slice.len();

    while i < len {
        // Salta tutto ciò che non è parte di un numero
        while i < len && !is_number_start(slice[i]) {
            // Gestisci commenti anche qui
            if slice[i] == b'#' {
                while i < len && slice[i] != b'\n' {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        if i >= len {
            break;
        }

        // Parsing del numero
        if let Some((val, next)) = parse_f32(slice, i) {
            result.push(val);
            i = next;
        } else {
            i += 1;
        }
    }

    result
}

/// Estrae tutti i numeri `i32` dall'intervallo `[start, end)` del buffer.
pub fn scan_numbers_i32(data: &[u8], start: usize, end: usize) -> Vec<i32> {
    let slice = &data[start..end.min(data.len())];
    let mut result = Vec::new();
    let mut i = 0;
    let len = slice.len();

    while i < len {
        while i < len && !is_number_start(slice[i]) {
            if slice[i] == b'#' {
                while i < len && slice[i] != b'\n' {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        if i >= len {
            break;
        }

        if let Some((val, next)) = parse_i32(slice, i) {
            result.push(val);
            i = next;
        } else {
            i += 1;
        }
    }

    result
}

// ─── Helpers privati ──────────────────────────────────────────────────────────

#[inline]
fn is_number_start(b: u8) -> bool {
    b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.'
}

/// Parsing veloce di un f32 a partire da `slice[pos]`.
/// Ritorna `(valore, prossima_posizione)`.
fn parse_f32(slice: &[u8], mut pos: usize) -> Option<(f32, usize)> {
    let len = slice.len();
    let start = pos;

    // Segno
    if pos < len && (slice[pos] == b'-' || slice[pos] == b'+') {
        pos += 1;
    }

    // Parte intera
    let has_int = pos < len && slice[pos].is_ascii_digit();
    while pos < len && slice[pos].is_ascii_digit() {
        pos += 1;
    }

    // Parte decimale
    let has_frac = pos < len && slice[pos] == b'.';
    if has_frac {
        pos += 1;
        while pos < len && slice[pos].is_ascii_digit() {
            pos += 1;
        }
    }

    if !has_int && !has_frac {
        return None;
    }

    // Esponente
    if pos < len && (slice[pos] == b'e' || slice[pos] == b'E') {
        pos += 1;
        if pos < len && (slice[pos] == b'-' || slice[pos] == b'+') {
            pos += 1;
        }
        while pos < len && slice[pos].is_ascii_digit() {
            pos += 1;
        }
    }

    let s = std::str::from_utf8(&slice[start..pos]).ok()?;
    let v: f32 = s.parse().ok()?;
    Some((v, pos))
}

/// Parsing veloce di un i32.
fn parse_i32(slice: &[u8], mut pos: usize) -> Option<(i32, usize)> {
    let len = slice.len();
    let start = pos;

    if pos < len && (slice[pos] == b'-' || slice[pos] == b'+') {
        pos += 1;
    }

    if pos >= len || !slice[pos].is_ascii_digit() {
        return None;
    }

    while pos < len && slice[pos].is_ascii_digit() {
        pos += 1;
    }

    let s = std::str::from_utf8(&slice[start..pos]).ok()?;
    let v: i32 = s.parse().ok()?;
    Some((v, pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_numbers_f32() {
        let data = b"1.0 -2.5 3e10 +0.001";
        let nums = scan_numbers_f32(data, 0, data.len());
        assert_eq!(nums.len(), 4);
        assert!((nums[0] - 1.0).abs() < 1e-6);
        assert!((nums[1] - (-2.5)).abs() < 1e-6);
        assert!((nums[2] - 3e10).abs() < 1e3);
        assert!((nums[3] - 0.001).abs() < 1e-6);
    }

    #[test]
    fn test_scan_numbers_i32() {
        let data = b"1 -2 300 -1";
        let nums = scan_numbers_i32(data, 0, data.len());
        assert_eq!(nums, vec![1, -2, 300, -1]);
    }

    #[test]
    fn test_skip_comment() {
        let data = b"  # questo e un commento\n  word";
        let pos = skip_whitespace_comments(data, 0);
        assert_eq!(&data[pos..pos + 4], b"word");
    }
}
