#[derive(Debug, Clone, Copy)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Oklab {
    // https://bottosson.github.io/posts/oklab/
    #[allow(clippy::excessive_precision)]
    pub fn from_linear_srgb(r: f32, g: f32, b: f32) -> Self {
        let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
        let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
        let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();

        Self {
            l: 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
            a: 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
            b: 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
        }
    }

    pub fn from_gamma22_rgb(r: u8, g: u8, b: u8) -> Self {
        const SCALE: f32 = 255f32.recip();
        const EXPONENT: f32 = 2.2f32.recip();

        Self::from_linear_srgb(
            (r as f32 * SCALE).powf(EXPONENT),
            (g as f32 * SCALE).powf(EXPONENT),
            (b as f32 * SCALE).powf(EXPONENT),
        )
    }

    pub fn distance_sq(self, other: Self) -> f32 {
        let dl = self.l - other.l;
        let da = self.a - other.a;
        let db = self.b - other.b;
        dl * dl + da * da + db * db
    }

    pub fn closer_to_than(self, other: Self, threshold: f32) -> bool {
        self.distance_sq(other) < threshold * threshold
    }
}
