/// Axis-Aligned Bounding Box in coordinate world-space.
#[derive(Debug, Clone, Copy, Default)]
pub struct Aabb {
    pub min: glam::Vec3,
    pub max: glam::Vec3,
}

impl Aabb {
    /// AABB "vuota": min = +∞, max = -∞.
    /// Usata come punto di partenza per accumulo di punti.
    pub fn empty() -> Self {
        Self {
            min: glam::Vec3::splat(f32::INFINITY),
            max: glam::Vec3::splat(f32::NEG_INFINITY),
        }
    }

    /// Estende l'AABB per includere il punto `p`.
    pub fn extend_point(&mut self, p: glam::Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    /// Estende l'AABB per includere un'altra AABB.
    pub fn extend_aabb(&mut self, other: &Aabb) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    /// Centro dell'AABB.
    pub fn center(&self) -> glam::Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Dimensioni (larghezza, altezza, profondità).
    pub fn size(&self) -> glam::Vec3 {
        self.max - self.min
    }

    /// Lunghezza della diagonale principale.
    pub fn diagonal(&self) -> f32 {
        self.size().length()
    }

    /// Distanza euclidea minima tra due AABB.
    /// Ritorna 0.0 se le AABB si sovrappongono o si toccano.
    pub fn distance_to(&self, other: &Aabb) -> f32 {
        // Per ogni asse calcola la distanza firmata tra i due intervalli
        let dx = (self.min.x - other.max.x).max(other.min.x - self.max.x).max(0.0);
        let dy = (self.min.y - other.max.y).max(other.min.y - self.max.y).max(0.0);
        let dz = (self.min.z - other.max.z).max(other.min.z - self.max.z).max(0.0);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Verifica se l'AABB è valida (non vuota).
    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extend_point() {
        let mut bbox = Aabb::empty();
        bbox.extend_point(glam::Vec3::new(1.0, 2.0, 3.0));
        bbox.extend_point(glam::Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(bbox.min, glam::Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(bbox.max, glam::Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn test_distance_zero_overlapping() {
        let a = Aabb { min: glam::Vec3::ZERO, max: glam::Vec3::ONE };
        let b = Aabb { min: glam::Vec3::new(0.5, 0.5, 0.5), max: glam::Vec3::new(1.5, 1.5, 1.5) };
        assert!((a.distance_to(&b) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_distance_separated() {
        let a = Aabb { min: glam::Vec3::ZERO, max: glam::Vec3::ONE };
        let b = Aabb { min: glam::Vec3::new(2.0, 0.0, 0.0), max: glam::Vec3::new(3.0, 1.0, 1.0) };
        assert!((a.distance_to(&b) - 1.0).abs() < 1e-6);
    }
}
