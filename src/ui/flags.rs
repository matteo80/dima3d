//! Bandierine delle lingue, disegnate via codice: egui non mostra le emoji delle bandiere.
//!
//! Ogni bandiera è una piccola immagine calcolata pixel per pixel (con sottocampionamento per
//! smussare le diagonali) e caricata una volta come texture.

use crate::i18n::Lang;

/// Pixel dell'immagine (3:2): abbastanza per restare nitida anche con lo schermo ingrandito.
const W: usize = 48;
const H: usize = 32;
/// Dimensione mostrata, in punti.
pub const SIZE: egui::Vec2 = egui::vec2(18.0, 12.0);

type Rgb = [u8; 3];

const WHITE: Rgb = [255, 255, 255];

/// Paese della bandiera di ogni lingua (l'inglese con il Regno Unito).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Francia e Spagna sono pronte per quando si aggiungeranno le lingue
#[allow(dead_code)]
pub enum Flag {
    Italy,
    UnitedKingdom,
    France,
    Spain,
}

impl Flag {
    pub fn of(lang: Lang) -> Flag {
        match lang {
            Lang::It => Flag::Italy,
            Lang::En => Flag::UnitedKingdom,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Flag::Italy => "it",
            Flag::UnitedKingdom => "gb",
            Flag::France => "fr",
            Flag::Spain => "es",
        }
    }

    /// Colore nel punto (x, y), con x e y da 0 a 1 (da sinistra e dall'alto).
    fn color(self, x: f32, y: f32) -> Rgb {
        let thirds = |a: Rgb, b: Rgb, c: Rgb| if x < 1.0 / 3.0 { a } else if x < 2.0 / 3.0 { b } else { c };
        match self {
            Flag::Italy => thirds([0, 146, 70], WHITE, [206, 43, 55]),
            Flag::France => thirds([0, 85, 164], WHITE, [239, 65, 53]),
            // Rosso, giallo largo il doppio, rosso (senza stemma: a questa misura non si vedrebbe)
            Flag::Spain => {
                if (0.25..0.75).contains(&y) {
                    [241, 191, 0]
                } else {
                    [170, 21, 27]
                }
            }
            Flag::UnitedKingdom => union_jack(x, y),
        }
    }
}

/// Union Jack in proporzione 3:2, con le misure della bandiera vera riportate a 60 × 40.
fn union_jack(x: f32, y: f32) -> Rgb {
    const BLUE: Rgb = [1, 33, 105];
    const RED: Rgb = [200, 16, 46];
    let (px, py) = ((x - 0.5) * 60.0, (y - 0.5) * 40.0);
    // Distanza dalle due diagonali (da angolo ad angolo)
    let len = (60.0_f32 * 60.0 + 40.0 * 40.0).sqrt();
    let d1 = (px * 40.0 - py * 60.0).abs() / len;
    let d2 = (px * 40.0 + py * 60.0).abs() / len;
    let diagonal = d1.min(d2);
    if px.abs() < 3.0 || py.abs() < 3.0 {
        RED
    } else if px.abs() < 5.0 || py.abs() < 5.0 {
        WHITE
    } else if diagonal < 1.3 {
        RED
    } else if diagonal < 3.6 {
        WHITE
    } else {
        BLUE
    }
}

/// Immagine della bandiera: ogni pixel è la media di 4 × 4 campioni; bordino grigio sottile.
pub fn image(flag: Flag) -> egui::ColorImage {
    const S: usize = 4;
    let mut pixels = Vec::with_capacity(W * H);
    for j in 0..H {
        for i in 0..W {
            let mut sum = [0u32; 3];
            for sj in 0..S {
                for si in 0..S {
                    let x = (i as f32 + (si as f32 + 0.5) / S as f32) / W as f32;
                    let y = (j as f32 + (sj as f32 + 0.5) / S as f32) / H as f32;
                    let c = flag.color(x, y);
                    for k in 0..3 {
                        sum[k] += u32::from(c[k]);
                    }
                }
            }
            let n = (S * S) as u32;
            let mut c = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8];
            // Bordo: si stacca anche il bianco sullo sfondo chiaro
            if i == 0 || j == 0 || i == W - 1 || j == H - 1 {
                c = c.map(|v| ((u32::from(v) + 110) / 2) as u8);
            }
            pixels.push(egui::Color32::from_rgb(c[0], c[1], c[2]));
        }
    }
    egui::ColorImage { size: [W, H], pixels }
}

/// Bandierina della lingua, pronta da mettere in un pulsante.
pub fn flag(ctx: &egui::Context, lang: Lang) -> egui::Image<'static> {
    let flag = Flag::of(lang);
    let key = egui::Id::new("bandierina").with(flag.id());
    let texture = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)).unwrap_or_else(|| {
        let t = ctx.load_texture(format!("bandierina_{}", flag.id()), image(flag), egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(key, t.clone()));
        t
    });
    egui::Image::new(egui::load::SizedTexture::new(texture.id(), SIZE)).corner_radius(2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(img: &egui::ColorImage, x: f32, y: f32) -> egui::Color32 {
        let (i, j) = ((x * W as f32) as usize, (y * H as f32) as usize);
        img.pixels[j * W + i]
    }

    #[test]
    fn flags_have_the_right_colors() {
        let it = image(Flag::Italy);
        assert_eq!(it.size, [W, H]);
        assert_eq!(it.pixels.len(), W * H);
        assert_eq!(at(&it, 0.15, 0.5), egui::Color32::from_rgb(0, 146, 70), "verde a sinistra");
        assert_eq!(at(&it, 0.5, 0.5), egui::Color32::WHITE);
        assert_eq!(at(&it, 0.85, 0.5), egui::Color32::from_rgb(206, 43, 55), "rosso a destra");

        let gb = image(Flag::UnitedKingdom);
        assert_eq!(at(&gb, 0.5, 0.5), egui::Color32::from_rgb(200, 16, 46), "croce rossa al centro");
        assert_eq!(at(&gb, 0.3, 0.12), egui::Color32::from_rgb(1, 33, 105), "blu tra croce e diagonale");

        let es = image(Flag::Spain);
        assert_eq!(at(&es, 0.5, 0.1), egui::Color32::from_rgb(170, 21, 27));
        assert_eq!(at(&es, 0.5, 0.5), egui::Color32::from_rgb(241, 191, 0));
        let fr = image(Flag::France);
        assert_eq!(at(&fr, 0.15, 0.5), egui::Color32::from_rgb(0, 85, 164));
    }
}
