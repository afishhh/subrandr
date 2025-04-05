use std::ops::Range;

use crate::color::{Premultiplied, BGRA8};

unsafe fn blit_monochrome_unchecked(
    dst: &mut [BGRA8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    ys: Range<usize>,
    xs: Range<usize>,
    src: &[u8],
    src_stride: usize,
    color: BGRA8,
) {
    for y in ys {
        let fy = dy + y as i32;
        for x in xs.clone() {
            let fx = dx + x as i32;

            let si = y * src_stride + x;
            let sv = *unsafe { src.get_unchecked(si) };

            let di = (fx as usize) + (fy as usize) * dst_stride;
            let d = unsafe { dst.get_unchecked_mut(di) };
            *d = color.mul_alpha(sv).blend_over(*d).0;
        }
    }
}

unsafe fn blit_bgra_unchecked(
    dst: &mut [BGRA8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    ys: Range<usize>,
    xs: Range<usize>,
    src: &[BGRA8],
    src_stride: usize,
    alpha: u8,
) {
    for y in ys {
        let fy = dy + y as i32;
        for x in xs.clone() {
            let fx = dx + x as i32;

            let si = y * src_stride + x;
            // NOTE: This is actually pre-multiplied in linear space...
            //       But I think libass ignores this too.
            //       See note in color.rs
            let n = Premultiplied(*src.get_unchecked(si));

            let di = (fx as usize) + (fy as usize) * dst_stride;
            let d = unsafe { dst.get_unchecked_mut(di) };
            *d = n.mul_alpha(alpha).blend_over(*d).0;
        }
    }
}

pub fn calculate_blit_rectangle(
    x: i32,
    y: i32,
    target_width: usize,
    target_height: usize,
    source_width: usize,
    source_height: usize,
) -> Option<(Range<usize>, Range<usize>)> {
    let isx = if x < 0 { (-x) as usize } else { 0 };
    let isy = if y < 0 { (-y) as usize } else { 0 };
    let msx = (source_width as i32).min(target_width as i32 - x);
    let msy = (source_height as i32).min(target_height as i32 - y);
    if msx <= 0 || msy <= 0 {
        return None;
    }
    let msx = msx as usize;
    let msy = msy as usize;

    Some((isx..msx, isy..msy))
}

macro_rules! make_blitter {
    (
        $name: ident via $unsafe: ident,
        $src_type: ty [over] $dst_type: ty, $($extra_name: ident: $extra_ty: ty),*
     ) => {
        pub fn $name(
            dst: &mut [$dst_type],
            dst_stride: usize,
            dst_width: usize,
            dst_height: usize,
            src: &[$src_type],
            src_stride: usize,
            src_width: usize,
            src_height: usize,
            dx: i32,
            dy: i32,
            $($extra_name: $extra_ty),*
        ) {
            let Some((xs, ys)) = calculate_blit_rectangle(
                dx, dy,
                dst_width, dst_height,
                src_width, src_height
            ) else {
                return;
            };

            unsafe {
                $unsafe(
                    dst, dst_stride, dx, dy, ys, xs, src, src_stride,
                    $($extra_name),*
                );
            }
        }
    };
}

make_blitter!(
    blit_monochrome via blit_monochrome_unchecked,
    u8 [over] BGRA8, color: BGRA8
);

make_blitter!(
    blit_bgra via blit_bgra_unchecked,
    BGRA8 [over] BGRA8, alpha: u8
);

pub unsafe fn blit_monochrome_float_unchecked(
    dst: &mut [BGRA8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    xs: Range<usize>,
    ys: Range<usize>,
    src_source: &[f32],
    src_stride: usize,
    color: [u8; 3],
) {
    for y in ys {
        let fy = dy + y as i32;
        for x in xs.clone() {
            let fx = dx + x as i32;

            let si = y * src_stride + x;
            let sv = (*unsafe { src_source.get_unchecked(si) }).clamp(0.0, 1.0);

            let di = (fx as usize) + (fy as usize) * dst_stride;
            let d = unsafe { dst.get_unchecked_mut(di) };

            let c = BGRA8::from_bytes([color[0], color[1], color[2], (sv * 255.0) as u8]);
            *d = c.blend_over(*d).0;
        }
    }
}

pub unsafe fn blit_bgra_to_mono_unchecked(
    dst: &mut [u8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    src: &[BGRA8],
    src_width: usize,
    src_height: usize,
) {
    for sy in 0..src_height {
        for sx in 0..src_width {
            let si = sy * src_width + sx;
            let di = (dst_stride as i32 * (dy + sy as i32) + (dx + sx as i32)) as usize;
            *dst.get_unchecked_mut(di) = src.get_unchecked(si).a;
        }
    }
}

pub unsafe fn blit_mono_to_mono_unchecked(
    dst: &mut [u8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    src: &[u8],
    src_width: usize,
    src_height: usize,
) {
    for sy in 0..src_height {
        for sx in 0..src_width {
            let si = sy * src_width + sx;
            let di = (dst_stride as i32 * (dy + sy as i32) + (dx + sx as i32)) as usize;
            *dst.get_unchecked_mut(di) = *src.get_unchecked(si);
        }
    }
}
