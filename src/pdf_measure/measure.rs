//! Calibrazione della scala e misure tra due punti, in unità PDF (punti, origine in alto a sinistra).
//!
//! Nessuna dipendenza da egui: logica pura, testabile.

/// Punto sulla pagina in unità PDF (1/72 di pollice), origine in alto a sinistra.
pub type PdfPoint = [f64; 2];

pub fn distance(p1: PdfPoint, p2: PdfPoint) -> f64 {
    (p2[0] - p1[0]).hypot(p2[1] - p1[1])
}

/// Scala calibrata su una pagina: vale solo per quella pagina.
#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub page: usize,
    pub p1: PdfPoint,
    pub p2: PdfPoint,
    pub mm_per_unit: f64,
}

impl Calibration {
    /// `None` se i punti coincidono o la distanza reale non è positiva.
    pub fn new(page: usize, p1: PdfPoint, p2: PdfPoint, real_mm: f64) -> Option<Self> {
        let d = distance(p1, p2);
        if d <= 0.0 || !real_mm.is_finite() || real_mm <= 0.0 {
            return None;
        }
        Some(Self { page, p1, p2, mm_per_unit: real_mm / d })
    }

    pub fn measure(&self, p1: PdfPoint, p2: PdfPoint) -> Measurement {
        let dx = (p2[0] - p1[0]).abs() * self.mm_per_unit;
        let dy = (p2[1] - p1[1]).abs() * self.mm_per_unit;
        Measurement { p1, p2, dx_mm: dx, dy_mm: dy, d_mm: dx.hypot(dy) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    pub p1: PdfPoint,
    pub p2: PdfPoint,
    pub dx_mm: f64,
    pub dy_mm: f64,
    pub d_mm: f64,
}

/// Numero all'italiana: migliaia con il punto, decimali con la virgola.
pub fn format_number(value: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, value.abs());
    let (int_part, dec_part) = match s.split_once('.') {
        Some((i, d)) => (i, Some(d)),
        None => (s.as_str(), None),
    };
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    let negative = value < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0');
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&grouped);
    if let Some(d) = dec_part {
        out.push(',');
        out.push_str(d);
    }
    out
}

pub fn format_mm(value: f64) -> String {
    format!("{} mm", format_number(value, 1))
}

/// Legge un numero scritto all'italiana o all'inglese: "1234,5", "1.234,5", "1234.5", "1 234,5".
pub fn parse_number(text: &str) -> Option<f64> {
    let t: String = text.chars().filter(|c| !c.is_whitespace() && *c != '\'').collect();
    if t.is_empty() {
        return None;
    }
    let normalized = if t.contains(',') {
        // La virgola è il separatore decimale; i punti sono migliaia.
        t.replace('.', "").replace(',', ".")
    } else if t.matches('.').count() > 1 {
        // "1.234.567": solo migliaia.
        t.replace('.', "")
    } else {
        t
    };
    normalized.parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_and_measure() {
        let cal = Calibration::new(0, [0.0, 0.0], [100.0, 0.0], 5000.0).unwrap();
        assert!((cal.mm_per_unit - 50.0).abs() < 1e-12);
        let m = cal.measure([10.0, 10.0], [40.0, 50.0]);
        assert!((m.dx_mm - 1500.0).abs() < 1e-9);
        assert!((m.dy_mm - 2000.0).abs() < 1e-9);
        assert!((m.d_mm - 2500.0).abs() < 1e-9);
        // Il verso dei punti non conta.
        let back = cal.measure([40.0, 50.0], [10.0, 10.0]);
        assert_eq!(back.d_mm, m.d_mm);
    }

    #[test]
    fn invalid_calibration() {
        assert!(Calibration::new(0, [1.0, 1.0], [1.0, 1.0], 10.0).is_none());
        assert!(Calibration::new(0, [0.0, 0.0], [1.0, 0.0], 0.0).is_none());
        assert!(Calibration::new(0, [0.0, 0.0], [1.0, 0.0], f64::NAN).is_none());
    }

    #[test]
    fn italian_format() {
        assert_eq!(format_number(1234567.891, 1), "1.234.567,9");
        assert_eq!(format_number(999.95, 1), "1.000,0");
        assert_eq!(format_number(12.0, 3), "12,000");
        assert_eq!(format_number(0.0, 1), "0,0");
        assert_eq!(format_number(-1500.0, 1), "-1.500,0");
        assert_eq!(format_number(-0.01, 1), "0,0");
        assert_eq!(format_mm(2500.0), "2.500,0 mm");
    }

    #[test]
    fn parse_both_styles() {
        assert_eq!(parse_number("1234,5"), Some(1234.5));
        assert_eq!(parse_number("1.234,5"), Some(1234.5));
        assert_eq!(parse_number("1234.5"), Some(1234.5));
        assert_eq!(parse_number(" 1 234,5 "), Some(1234.5));
        assert_eq!(parse_number("1.234.567"), Some(1234567.0));
        assert_eq!(parse_number("5000"), Some(5000.0));
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("abc"), None);
    }
}
