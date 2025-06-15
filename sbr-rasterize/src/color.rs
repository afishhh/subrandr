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
            a: ((self.a as u16 * other as u16) / 255) as u8,
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
        let a = self.a as u16;
        Premultiplied(Self {
            b: ((self.b as u16 * a) / 255) as u8,
            g: ((self.g as u16 * a) / 255) as u8,
            r: ((self.r as u16 * a) / 255) as u8,
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
        let inva = (255 - a.a) as u16;
        let one = |a, b| a + (inva * b as u16 / 255) as u8;
        Premultiplied(BGRA8 {
            b: one(a.b, b.b),
            g: one(a.g, b.g),
            r: one(a.r, b.r),
            a: one(a.a, b.a),
        })
    }

    pub const fn mul_alpha(self, other: u8) -> Self {
        Self(BGRA8 {
            b: ((self.0.b as u16 * other as u16) / 255) as u8,
            g: ((self.0.g as u16 * other as u16) / 255) as u8,
            r: ((self.0.r as u16 * other as u16) / 255) as u8,
            a: ((self.0.a as u16 * other as u16) / 255) as u8,
        })
    }
}
