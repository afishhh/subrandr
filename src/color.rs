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
    pub const ZERO: Self = Self::new(0, 0, 0, 0);

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

pub trait BGRA8Slice {
    fn as_bytes(&self) -> &[u8];
    fn as_bytes_mut(&mut self) -> &mut [u8];
}

impl BGRA8Slice for [BGRA8] {
    fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self.as_ptr() as *const u8, std::mem::size_of_val(self))
        }
    }

    fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(
                self.as_mut_ptr() as *mut u8,
                std::mem::size_of_val(self),
            )
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
        let a = self.a as f32 / 255.0;
        Premultiplied(Self {
            b: linear_to_srgb(srgb_to_linear(self.b) * a),
            g: linear_to_srgb(srgb_to_linear(self.g) * a),
            r: linear_to_srgb(srgb_to_linear(self.r) * a),
            a: self.a,
        })
    }
}

mod lut;

#[inline(always)]
pub fn srgb_to_linear(color: u8) -> f32 {
    lut::SRGB_TO_LINEAR_LUT[color as usize]
}

#[inline(always)]
fn blend_over(dst: f32, src: f32, alpha: f32) -> f32 {
    src + (1.0 - alpha) * dst
}

#[inline(always)]
// TODO: This can be improved
pub fn linear_to_srgb(color: f32) -> u8 {
    // An approximation of (color.powf(2.2) * 255.0) as u8
    // see: https://www.shadertoy.com/view/WlG3zG
    (color * color * crate::math::fast_mul_add(color, 63.75, 191.25)) as u8
}

fn color_to_linear(color: BGRA8) -> ([f32; 3], f32) {
    (
        color.to_bgr_bytes().map(srgb_to_linear),
        color.a as f32 / 255.0,
    )
}

fn linear_to_color([b, g, r]: [f32; 3], a: f32) -> BGRA8 {
    BGRA8 {
        b: linear_to_srgb(b),
        g: linear_to_srgb(g),
        r: linear_to_srgb(r),
        a: (a * 255.0) as u8,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// out = over(out, in)
    Over,
}

impl BlendMode {
    pub fn blend_with_linear_parts(self, b: &mut BGRA8, [ab, ag, ar]: [f32; 3], aa: f32) {
        match self {
            Self::Over => {
                let ([bb, bg, br], ba) = color_to_linear(*b);
                *b = linear_to_color(
                    [
                        blend_over(bb, ab, aa),
                        blend_over(bg, ag, aa),
                        blend_over(br, ar, aa),
                    ],
                    aa + ba * (1.0 - aa),
                );
            }
        }
    }

    pub fn blend_with_parts(self, b: &mut BGRA8, ac: [u8; 3], aa: f32) {
        self.blend_with_linear_parts(b, ac.map(srgb_to_linear), aa);
    }

    // FIXME: b should also be Premultiplied<BGRA8> but for **legacy reasons**
    //        &(mut?) [BGRA8] buffers are implicitly treated as premultiplied
    //        (read: I don't want to change all that code again)
    pub fn blend(self, b: &mut BGRA8, Premultiplied(a): Premultiplied<BGRA8>) {
        self.blend_with_parts(b, [a.b, a.g, a.r], a.a as f32 / 255.0);
    }
}
