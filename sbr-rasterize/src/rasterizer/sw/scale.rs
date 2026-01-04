// NOTE: These scaling operations are based on the Vulkan specification at
//       https://docs.vulkan.org/spec/latest/chapters/textures.html#textures-sample-operations.

use std::mem::MaybeUninit;

use util::math::{I16Dot16, Vec2};

use crate::color::{Premultiplied, BGRA8};

trait WrapMode {
    fn transform(i: i32, size: u32) -> usize;
}

struct ClampToEdge;
impl WrapMode for ClampToEdge {
    fn transform(i: i32, size: u32) -> usize {
        match u32::try_from(i) {
            Ok(v) if v >= size => (size - 1) as usize,
            Ok(v) => v as usize,
            Err(_) => 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LinearSamples<S> {
    samples: [[S; 2]; 2],
    weights: Vec2<I16Dot16>,
}

impl<S: Copy> LinearSamples<S> {
    fn to_samples_and_weights(self) -> [(S, I16Dot16); 4] {
        let wx0 = I16Dot16::ONE - self.weights.x;
        let wx1 = self.weights.x;
        let wy0 = I16Dot16::ONE - self.weights.y;
        let wy1 = self.weights.y;

        [
            (self.samples[0][0], wy0 * wx0),
            (self.samples[0][1], wy0 * wx1),
            (self.samples[1][0], wy1 * wx0),
            (self.samples[1][1], wy1 * wx1),
        ]
    }
}

// A utility struct that aids in precise 32-bit averaging of interpolated samples.
#[repr(transparent)]
struct A32(u32);

impl A32 {
    const INITIAL: Self = Self(32768);

    fn add_mono_mul16(&mut self, value: u8, scale: I16Dot16) {
        self.0 += value as u32 * scale.into_raw() as u32;
    }

    fn add_bgra_mul16(this: &mut [A32; 4], value: BGRA8, scale: I16Dot16) {
        this[0].add_mono_mul16(value.b, scale);
        this[1].add_mono_mul16(value.g, scale);
        this[2].add_mono_mul16(value.r, scale);
        this[3].add_mono_mul16(value.a, scale);
    }

    fn into_mono(self) -> u8 {
        (self.0 >> 16) as u8
    }

    fn into_bgra(this: [A32; 4]) -> BGRA8 {
        BGRA8::from_bytes(this.map(Self::into_mono))
    }
}

#[inline(always)]
unsafe fn scale_generic<W: WrapMode, S: Copy, D: Copy>(
    mut dst: *mut D,
    dst_stride: usize,
    dst_width: usize,
    dst_height: usize,
    src: *const S,
    src_stride: usize,
    src_width: u32,
    src_height: u32,
    src_off: Vec2<i32>,
    src_size: Vec2<i32>,
    process: impl Fn(LinearSamples<S>, &mut D),
) {
    let dst_end = dst.add(dst_stride * dst_height);
    let dst_row_step = dst_stride - dst_width;
    let dx = I16Dot16::from_quotient(src_size.x, dst_width as i32);
    let dy = I16Dot16::from_quotient(src_size.y, dst_height as i32);

    let src_initial_x = I16Dot16::new(src_off.x);
    let mut src_y = I16Dot16::new(src_off.y);
    while dst < dst_end {
        let mut src_pos = Vec2::new(src_initial_x, src_y);
        for _ in 0..dst_width {
            let x0 = src_pos.x.floor_to_inner();
            let x1 = x0 + 1;
            let y0 = src_pos.y.floor_to_inner();
            let y1 = y0 + 1;
            let cx0 = W::transform(x0, src_width);
            let cx1 = W::transform(x1, src_width);
            let cy0 = W::transform(y0, src_height);
            let cy1 = W::transform(y1, src_height);

            let r0 = src.add(cy0 * src_stride);
            let r1 = src.add(cy1 * src_stride);

            let weights = Vec2::new(src_pos.x.fract(), src_pos.y.fract());
            let samples = [(r0, [cx0, cx1]), (r1, [cx0, cx1])].map(|(r, xs)| xs.map(|x| *r.add(x)));
            let d = unsafe { &mut *dst };

            process(LinearSamples { samples, weights }, d);

            src_pos.x += dx;
            dst = dst.add(1);
        }
        src_y += dy;
        dst = dst.add(dst_row_step);
    }
}

#[inline(never)]
pub fn scale_bgra(
    target: super::RenderTargetView<MaybeUninit<Premultiplied<BGRA8>>>,
    src: &[Premultiplied<BGRA8>],
    src_stride: usize,
    src_width: u32,
    src_height: u32,
    src_off: Vec2<i32>,
    src_size: Vec2<i32>,
) {
    assert!(src_stride
        .checked_mul(src_height as usize)
        .is_some_and(|required| required <= src.len()));

    unsafe {
        scale_generic::<ClampToEdge, _, _>(
            target.buffer.as_mut_ptr(),
            target.stride as usize,
            target.width as usize,
            target.height as usize,
            src.as_ptr(),
            src_stride,
            src_width,
            src_height,
            src_off,
            src_size,
            |s, d| {
                let mut result = [A32::INITIAL; 4];
                for (sample, weight) in s.to_samples_and_weights() {
                    A32::add_bgra_mul16(&mut result, sample.0, weight);
                }
                d.write(Premultiplied(A32::into_bgra(result)));
            },
        )
    };
}

#[inline(never)]
pub fn scale_mono(
    target: super::RenderTargetView<MaybeUninit<u8>>,
    src: &[u8],
    src_stride: usize,
    src_width: u32,
    src_height: u32,
    src_off: Vec2<i32>,
    src_size: Vec2<i32>,
) {
    assert!(src_stride
        .checked_mul(src_height as usize)
        .is_some_and(|required| required <= src.len()));

    unsafe {
        scale_generic::<ClampToEdge, _, _>(
            target.buffer.as_mut_ptr(),
            target.stride as usize,
            target.width as usize,
            target.height as usize,
            src.as_ptr(),
            src_stride,
            src_width,
            src_height,
            src_off,
            src_size,
            |s, d| {
                let mut result = A32::INITIAL;
                for (sample, weight) in s.to_samples_and_weights() {
                    result.add_mono_mul16(sample, weight);
                }
                d.write(result.into_mono());
            },
        )
    };
}
