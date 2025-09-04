use std::ops::Range;

use crate::color::{Premultiplied, BGRA8};

#[inline(always)]
unsafe fn blit_generic_unchecked<S: Copy, D: Copy>(
    dst: &mut [D],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    ys: Range<usize>,
    xs: Range<usize>,
    src: &[S],
    src_stride: usize,
    process: impl Fn(S, &mut D),
) {
    let width = xs.end - xs.start;
    let mut si = ys.start * src_stride + xs.start;
    let src_row_step = src_stride - width;
    let mut di = (xs.start as isize
        + dx as isize
        + (ys.start as isize + dy as isize) * dst_stride as isize) as usize;
    let dst_row_step = dst_stride - width;

    for _ in ys {
        for _ in 0..width {
            let s = *unsafe { src.get_unchecked(si) };
            let d = unsafe { dst.get_unchecked_mut(di) };

            process(s, d);

            si += 1;
            di += 1;
        }
        si += src_row_step;
        di += dst_row_step;
    }
}

// This `#[inline(never)]` significantly improves performance, presumably because LLVM
// has more inlining budget that it can spend on inlining `BGRA8::blend_over`.
// Without `#[inline(never)]` LLVM seems to not inline that function which is performance
// suicide.
// To avoid situations like this let's never inline these core blitting functions, we want
// them to be big blocks of high performance code.
#[inline(never)]
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
    blit_generic_unchecked(dst, dst_stride, dx, dy, ys, xs, src, src_stride, |s, d| {
        *d = color.mul_alpha(s).blend_over(*d).0;
    });
}

#[inline(never)]
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
    blit_generic_unchecked(dst, dst_stride, dx, dy, ys, xs, src, src_stride, |s, d| {
        // NOTE: This is actually pre-multiplied in linear space...
        //       But I think libass ignores this too.
        //       See note in color.rs
        let n = Premultiplied(s);
        *d = n.mul_alpha(alpha).blend_over(*d).0;
    });
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
    if isx >= msx || isy >= msy {
        return None;
    }

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

#[inline(never)]
pub unsafe fn copy_monochrome_float_to_mono_u8_unchecked(
    dst: &mut [u8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    xs: Range<usize>,
    ys: Range<usize>,
    src: &[f32],
    src_stride: usize,
) {
    blit_generic_unchecked(dst, dst_stride, dx, dy, ys, xs, src, src_stride, |s, d| {
        *d = (s * 255.0) as u8;
    });
}

#[inline(never)]
pub unsafe fn blit_bgra_to_mono_unchecked(
    dst: &mut [u8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    src: &[BGRA8],
    src_width: usize,
    src_height: usize,
) {
    blit_generic_unchecked(
        dst,
        dst_stride,
        dx,
        dy,
        0..src_height,
        0..src_width,
        src,
        src_width,
        |s, d| {
            *d = s.a;
        },
    );
}

#[inline(never)]
pub unsafe fn blit_mono_to_mono_unchecked(
    dst: &mut [u8],
    dst_stride: usize,
    dx: i32,
    dy: i32,
    src: &[u8],
    src_width: usize,
    src_height: usize,
) {
    blit_generic_unchecked(
        dst,
        dst_stride,
        dx,
        dy,
        0..src_height,
        0..src_width,
        src,
        src_width,
        |s, d| {
            *d = s;
        },
    );
}
