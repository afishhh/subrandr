use std::ops::Range;

use crate::color::{Premultiplied, Premultiply, BGRA8};

#[inline(always)]
unsafe fn blit_generic_unchecked<S: Copy, D: Copy>(
    mut dst: *mut D,
    dst_stride: usize,
    mut src: *const S,
    src_stride: usize,
    width: usize,
    height: usize,
    process: impl Fn(S, &mut D),
) {
    let src_end = src.add(src_stride * height);
    let dst_row_step = dst_stride - width;
    let src_row_step = src_stride - width;

    while src < src_end {
        for _ in 0..width {
            let s = unsafe { *src };
            let d = unsafe { &mut *dst };

            process(s, d);

            src = src.add(1);
            dst = dst.add(1);
        }
        src = src.add(src_row_step);
        dst = dst.add(dst_row_step);
    }
}

// This `#[inline(never)]` significantly improves performance, presumably because LLVM
// has more inlining budget that it can spend on inlining `BGRA8::blend_over`.
// Without `#[inline(never)]` LLVM seems to not inline that function which is performance
// suicide.
// To avoid situations like this let's never inline these core blitting functions, we want
// them to be big blocks of high performance code.
#[inline(never)]
unsafe fn blit_mono_unchecked(
    dst: *mut BGRA8,
    dst_stride: usize,
    src: *const u8,
    src_stride: usize,
    width: usize,
    height: usize,
    color: BGRA8,
) {
    let pre = color.premultiply();
    blit_generic_unchecked(dst, dst_stride, src, src_stride, width, height, |s, d| {
        *d = pre.mul_alpha(s).blend_over(*d).0;
    });
}

#[inline(never)]
unsafe fn blit_bgra_unchecked(
    dst: *mut BGRA8,
    dst_stride: usize,
    src: *const BGRA8,
    src_stride: usize,
    width: usize,
    height: usize,
    alpha: u8,
) {
    blit_generic_unchecked(dst, dst_stride, src, src_stride, width, height, |s, d| {
        // NOTE: This is actually pre-multiplied in linear space...
        //       See note in color.rs
        let n = Premultiplied(s);
        *d = n.mul_alpha(alpha).blend_over(*d).0;
    });
}

fn calculate_blit_rectangle(
    x: isize,
    y: isize,
    target_width: usize,
    target_height: usize,
    source_width: usize,
    source_height: usize,
) -> Option<(Range<usize>, Range<usize>)> {
    let isx = if x < 0 { (-x) as usize } else { 0 };
    let isy = if y < 0 { (-y) as usize } else { 0 };
    let msx = (source_width as isize).min(target_width as isize - x);
    let msy = (source_height as isize).min(target_height as isize - y);
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

#[inline(never)]
pub unsafe fn blit_xxxa_to_bgra_unchecked(
    dst: *mut BGRA8,
    dst_stride: usize,
    src: *const BGRA8,
    src_stride: usize,
    width: usize,
    height: usize,
    color: BGRA8,
) {
    let pre = color.premultiply();
    blit_generic_unchecked(dst, dst_stride, src, src_stride, width, height, |s, d| {
        *d = pre.mul_alpha(s.a).blend_over(*d).0;
    });
}

#[inline(never)]
pub unsafe fn copy_mono_to_float_unchecked(
    dst: *mut f32,
    dst_stride: usize,
    src: *const u8,
    src_stride: usize,
    width: usize,
    height: usize,
) {
    blit_generic_unchecked(dst, dst_stride, src, src_stride, width, height, |s, d| {
        *d = s as f32 / 255.;
    });
}

#[inline(never)]
pub unsafe fn copy_bgra_to_float_unchecked(
    dst: *mut f32,
    dst_stride: usize,
    src: *const BGRA8,
    src_stride: usize,
    width: usize,
    height: usize,
) {
    blit_generic_unchecked(dst, dst_stride, src, src_stride, width, height, |s, d| {
        *d = s.a as f32 / 255.;
    });
}

#[inline(never)]
pub unsafe fn copy_float_to_mono_unchecked(
    dst: *mut u8,
    dst_stride: usize,
    src: *const f32,
    src_stride: usize,
    width: usize,
    height: usize,
) {
    blit_generic_unchecked(dst, dst_stride, src, src_stride, width, height, |s, d| {
        *d = (s * 255.0) as u8;
    });
}

macro_rules! make_checked_blitter {
    (
        $name: ident via $unchecked_name: ident,
        $src_type: ty [over] $dst_type: ty $(, $extra_name: ident: $extra_ty: ty)*
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
            dx: isize,
            dy: isize,
            $($extra_name: $extra_ty),*
        ) {
            assert!(dst_stride * dst_height <= dst.len());
            assert!(src_stride * src_height <= src.len());

            let Some((xs, ys)) = calculate_blit_rectangle(
                dx, dy,
                dst_width, dst_height,
                src_width, src_height
            ) else {
                return;
            };

            let width = xs.len();
            assert!(width <= dst_width);
            assert!(width <= src_width);

            unsafe {
                let dst_ptr = dst.as_mut_ptr().add(
                    ys.start.wrapping_add_signed(dy) * dst_stride
                    + xs.start.wrapping_add_signed(dx)
                );
                let src_ptr = src.as_ptr().add(ys.start * src_stride + xs.start);

                $unchecked_name(
                    dst_ptr,
                    dst_stride,
                    src_ptr,
                    src_stride,
                    width,
                    ys.len(),
                    $($extra_name),*
                );
            }
        }
    };
}

make_checked_blitter!(
    blit_mono via blit_mono_unchecked,
    u8 [over] BGRA8, color: BGRA8
);

make_checked_blitter!(
    blit_bgra via blit_bgra_unchecked,
    BGRA8 [over] BGRA8, alpha: u8
);

make_checked_blitter!(
    blit_xxxa_to_bgra via blit_xxxa_to_bgra_unchecked,
    BGRA8 [over] BGRA8, color: BGRA8
);

make_checked_blitter!(copy_mono_to_float via copy_mono_to_float_unchecked, u8 [over] f32);
make_checked_blitter!(copy_bgra_to_float via copy_bgra_to_float_unchecked, BGRA8 [over] f32);
make_checked_blitter!(copy_float_to_mono via copy_float_to_mono_unchecked, f32 [over] u8);
