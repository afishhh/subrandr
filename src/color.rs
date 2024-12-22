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

#[inline(always)]
fn srgb_to_linear(color: u8) -> f32 {
    (color as f32 / 255.0).powf(1.0 / 2.2)
}

#[inline(always)]
fn blend_over(dst: f32, src: f32, alpha: f32) -> f32 {
    alpha * src + (1.0 - alpha) * dst
}

#[inline(always)]
fn linear_to_srgb(color: f32) -> u8 {
    (color.powf(2.2 / 1.0) * 255.0).round() as u8
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
    /// out = in
    None,
    /// out = over(out, in)
    Over,
}

impl BlendMode {
    pub fn blend_with_parts(self, b: &mut BGRA8, ac: [u8; 3], aa: f32) {
        match self {
            Self::None => *b = BGRA8::new(ac[0], ac[1], ac[2], (aa * 255.0) as u8),
            Self::Over => {
                let ([bb, bg, br], ba) = color_to_linear(*b);
                let [ab, ag, ar] = ac.map(srgb_to_linear);
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

    pub fn blend(self, b: &mut BGRA8, a: BGRA8) {
        self.blend_with_parts(b, [a.b, a.g, a.r], a.a as f32 / 255.0);
    }
}
