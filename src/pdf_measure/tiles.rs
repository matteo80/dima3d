//! Griglia delle tile: la pagina, a una data scala in pixel fisici, è divisa in quadrati di
//! `TILE_PX` pixel. Logica pura, senza egui né pdfium.

pub const TILE_PX: u32 = 512;

/// Identifica una tile renderizzata: documento (generazione), pagina, scala e posizione.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileKey {
    pub doc: u64,
    pub page: usize,
    /// Pixel fisici per unità PDF, come bit di `f32` (chiave esatta, niente arrotondamenti).
    pub scale_bits: u32,
    pub col: u32,
    pub row: u32,
}

impl TileKey {
    pub fn scale(&self) -> f32 {
        f32::from_bits(self.scale_bits)
    }

    /// Rettangolo della tile in pixel della pagina: x, y, larghezza, altezza.
    pub fn px_rect(&self, page_size: [f32; 2]) -> [u32; 4] {
        tile_px_rect(self.col, self.row, page_px(page_size, self.scale()))
    }
}

/// Dimensione della pagina in pixel alla scala data.
pub fn page_px(page_size: [f32; 2], scale: f32) -> [u32; 2] {
    [
        ((page_size[0] * scale).ceil() as u32).max(1),
        ((page_size[1] * scale).ceil() as u32).max(1),
    ]
}

pub fn tile_px_rect(col: u32, row: u32, page_px: [u32; 2]) -> [u32; 4] {
    let x0 = col * TILE_PX;
    let y0 = row * TILE_PX;
    let w = page_px[0].saturating_sub(x0).min(TILE_PX);
    let h = page_px[1].saturating_sub(y0).min(TILE_PX);
    [x0, y0, w, h]
}

/// Colonne e righe (estremi inclusi) che coprono la zona visibile `view` = [x0, y0, x1, y1]
/// in pixel della pagina, allargate di `margin` tile per lato. `None` se la zona non tocca la pagina.
pub fn tile_range(view: [f32; 4], page_px: [u32; 2], margin: u32) -> Option<[u32; 4]> {
    let x0 = view[0].max(0.0);
    let y0 = view[1].max(0.0);
    let x1 = view[2].min(page_px[0] as f32);
    let y1 = view[3].min(page_px[1] as f32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let t = TILE_PX as f32;
    let last_col = (page_px[0] - 1) / TILE_PX;
    let last_row = (page_px[1] - 1) / TILE_PX;
    let c0 = ((x0 / t) as u32).saturating_sub(margin);
    let r0 = ((y0 / t) as u32).saturating_sub(margin);
    let c1 = (((x1 - 0.001) / t) as u32 + margin).min(last_col);
    let r1 = (((y1 - 0.001) / t) as u32 + margin).min(last_row);
    Some([c0, r0, c1, r1])
}

/// Tile da mostrare, in ordine di priorità: prima quelle visibili dal centro verso i bordi,
/// poi la corona di precaricamento.
pub fn prioritized(view: [f32; 4], page_px: [u32; 2], preload: u32) -> Vec<(u32, u32)> {
    let Some(visible) = tile_range(view, page_px, 0) else {
        return Vec::new();
    };
    let all = tile_range(view, page_px, preload).unwrap_or(visible);
    let t = TILE_PX as f32;
    let cx = (view[0] + view[2]) * 0.5 / t;
    let cy = (view[1] + view[3]) * 0.5 / t;
    let mut tiles: Vec<(bool, f32, u32, u32)> = Vec::new();
    for row in all[1]..=all[3] {
        for col in all[0]..=all[2] {
            let inside =
                (visible[0]..=visible[2]).contains(&col) && (visible[1]..=visible[3]).contains(&row);
            let dx = col as f32 + 0.5 - cx;
            let dy = row as f32 + 0.5 - cy;
            tiles.push((!inside, dx * dx + dy * dy, col, row));
        }
    }
    tiles.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    tiles.into_iter().map(|(_, _, c, r)| (c, r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_tile_is_clipped() {
        let px = page_px([1190.55, 841.89], 1.0); // A3 orizzontale
        assert_eq!(px, [1191, 842]);
        assert_eq!(tile_px_rect(2, 1, px), [1024, 512, 167, 330]);
        assert_eq!(tile_px_rect(0, 0, px), [0, 0, 512, 512]);
    }

    #[test]
    fn range_clamped_to_page() {
        let px = [2000, 1000];
        assert_eq!(tile_range([-100.0, -100.0, 600.0, 400.0], px, 0), Some([0, 0, 1, 0]));
        assert_eq!(tile_range([-100.0, -100.0, 600.0, 400.0], px, 1), Some([0, 0, 2, 1]));
        assert_eq!(tile_range([0.0, 0.0, 512.0, 512.0], px, 0), Some([0, 0, 0, 0]));
        assert_eq!(tile_range([1500.0, 0.0, 9000.0, 9000.0], px, 5), Some([0, 0, 3, 1]));
        assert_eq!(tile_range([3000.0, 0.0, 4000.0, 100.0], px, 1), None);
    }

    #[test]
    fn visible_tiles_first_center_first() {
        let px = [4096, 4096];
        let order = prioritized([1024.0, 1024.0, 2048.0, 2048.0], px, 1);
        assert_eq!(order.len(), 16);
        // Le 4 visibili (colonne/righe 2..=3) vengono prima della corona.
        for &(c, r) in &order[..4] {
            assert!((2..=3).contains(&c) && (2..=3).contains(&r), "{c},{r}");
        }
        for &(c, r) in &order[4..] {
            assert!(!((2..=3).contains(&c) && (2..=3).contains(&r)));
        }
    }
}
