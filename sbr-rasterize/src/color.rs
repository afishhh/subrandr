use std::fmt::Debug;

#[allow(clippy::upper_case_acronyms)]
#[repr(C, align(4))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// BGRA8888 in memory
// ARGB32 value on little-endian
// BGRA32 value on big-endian
pub struct BGRA8 {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl BGRA8 {
    pub const WHITE: Self = Self::new(255, 255, 255, 255);
    pub const BLACK: Self = Self::new(0, 0, 0, 255);

    pub const RED: Self = Self::new(255, 0, 0, 255);
    pub const GREEN: Self = Self::new(0, 255, 0, 255);
    pub const BLUE: Self = Self::new(0, 0, 255, 255);

    pub const LIME: Self = Self::GREEN;
    pub const CYAN: Self = Self::new(0, 255, 255, 255);
    pub const GOLD: Self = Self::new(255, 255, 0, 255);
    pub const YELLOW: Self = Self::new(255, 255, 0, 255);
    pub const MAGENTA: Self = Self::new(255, 0, 255, 255);

    pub const ZERO: Self = Self::new(0, 0, 0, 0);

    pub const ORANGERED: Self = Self::new(0xFF, 0x45, 0x00, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_bytes(value: [u8; 4]) -> Self {
        unsafe { std::mem::transmute(value) }
    }

    pub const fn to_bgr_bytes(self) -> [u8; 3] {
        unsafe { std::mem::transmute_copy(&self) }
    }

    pub const fn from_ne_u32(value: u32) -> Self {
        unsafe { std::mem::transmute(value) }
    }

    pub const fn to_ne_u32(self) -> u32 {
        unsafe { std::mem::transmute(self) }
    }

    pub const fn from_argb32(value: u32) -> Self {
        Self::from_ne_u32(value.to_le())
    }

    pub const fn to_argb32(self) -> u32 {
        self.to_ne_u32().to_le()
    }

    pub const fn from_rgba32(value: u32) -> Self {
        Self::from_argb32(value.rotate_right(8))
    }

    pub const fn to_rgba32(self) -> u32 {
        let argb = self.to_argb32();
        argb.rotate_left(8)
    }

    pub const fn mul_alpha(self, other: u8) -> Self {
        Self {
            a: mul_rgb(self.a, other),
            ..self
        }
    }
}

pub trait Premultiply: Debug + Clone + Copy {
    fn premultiply(self) -> Premultiplied<Self>;
}

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Premultiplied<T: Premultiply>(pub T);

impl Premultiply for BGRA8 {
    fn premultiply(self) -> Premultiplied<Self> {
        Premultiplied(Self {
            b: mul_rgb(self.b, self.a),
            g: mul_rgb(self.g, self.a),
            r: mul_rgb(self.r, self.a),
            a: self.a,
        })
    }
}

// FIXME: RANT: The alpha compositing mess.
//     Alpha compositing is ideally done in linear space, as suggested
//     by FreeType docs. This allows more physically realistic blending
//     of colors as opposed to blending gamma-encoded sRGB.
//     Naturally, this is not what everyone does and thus to remain compatible
//     we have to do it this way.
//     See `ba3312f` for a commit that still has linear blending code if it
//     ever needs to be brought back.

// TODO: blend_over_mul_alpha

impl BGRA8 {
    pub fn blend_over(self, b: BGRA8) -> Premultiplied<BGRA8> {
        self.premultiply().blend_over(b)
    }
}

impl Premultiplied<BGRA8> {
    pub fn blend_over(
        self,
        b: /* TODO: Premultiplied< */ BGRA8, /* > */
    ) -> Premultiplied<BGRA8> {
        let a = self.0;
        let inva = 255 - a.a;
        let one = |a, b| a + mul_rgb(inva, b);
        Premultiplied(BGRA8 {
            b: one(a.b, b.b),
            g: one(a.g, b.g),
            r: one(a.r, b.r),
            a: one(a.a, b.a),
        })
    }

    pub const fn mul_alpha(self, other: u8) -> Self {
        Self(BGRA8 {
            b: mul_rgb(self.0.b, other),
            g: mul_rgb(self.0.g, other),
            r: mul_rgb(self.0.r, other),
            a: mul_rgb(self.0.a, other),
        })
    }
}

/// Calculates `(a * b + 127) / 255` but without a division.
pub(crate) const fn mul_rgb(a: u8, b: u8) -> u8 {
    let c = a as u16 * b as u16 + 128;
    ((c + (c >> 8)) >> 8) as u8
}

#[cfg(test)]
mod test {
    use super::mul_rgb;

    #[test]
    fn test_mul_rgb() {
        assert_eq!(mul_rgb(255, 1), 1);
        assert_eq!(mul_rgb(255, 255), 255);

        for a in 0..=255 {
            for b in 0..=255 {
                assert_eq!(
                    mul_rgb(a, b),
                    ((a as u16 * b as u16 + 127) / 255) as u8,
                    "{a} * {b} yielded incorrect result"
                );
            }
        }
    }
}
