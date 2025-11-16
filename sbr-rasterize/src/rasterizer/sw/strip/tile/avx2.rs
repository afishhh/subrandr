use std::{arch::x86_64::*, mem::MaybeUninit};

use util::math::{I16Dot16, Point2};

use super::{to_op_fixed, Tile};

pub struct Avx2TileRasterizer {
    coverage_scratch_buffer: Vec<AlignedM256>,
    current_winding: __m256i,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, align(32))]
struct AlignedM256(__m256i);

impl Avx2TileRasterizer {
    #[target_feature(enable = "avx2")]
    pub fn new() -> Self {
        Self {
            coverage_scratch_buffer: Vec::new(),
            current_winding: _mm256_setzero_si256(),
        }
    }
}

impl super::TileRasterizer for Avx2TileRasterizer {
    fn reset(&mut self) {
        self.current_winding = unsafe { _mm256_setzero_si256() };
    }

    fn fill_alpha(&self) -> [u8; 4] {
        unsafe { (mm256_coverage_to_alpha(self.current_winding) as u32).to_ne_bytes() }
    }

    unsafe fn rasterize(&mut self, strip_x: u16, tiles: &[Tile], buffer: *mut [MaybeUninit<u8>]) {
        unsafe { self.rasterize_impl(strip_x, tiles, buffer) }
    }
}

struct DebugCells(__m256i);

impl std::fmt::Debug for DebugCells {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            f.debug_list()
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<0>(self.0)))
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<1>(self.0)))
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<2>(self.0)))
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<3>(self.0)))
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<4>(self.0)))
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<5>(self.0)))
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<6>(self.0)))
                .entry(&I16Dot16::from_raw(_mm256_extract_epi32::<7>(self.0)))
                .finish()
        }
    }
}

struct DebugCells16(__m256i);

impl std::fmt::Debug for DebugCells16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            f.debug_list()
                .entry(&_mm256_extract_epi16::<0>(self.0))
                .entry(&_mm256_extract_epi16::<1>(self.0))
                .entry(&_mm256_extract_epi16::<2>(self.0))
                .entry(&_mm256_extract_epi16::<3>(self.0))
                .entry(&_mm256_extract_epi16::<4>(self.0))
                .entry(&_mm256_extract_epi16::<5>(self.0))
                .entry(&_mm256_extract_epi16::<6>(self.0))
                .entry(&_mm256_extract_epi16::<7>(self.0))
                .entry(&_mm256_extract_epi16::<8>(self.0))
                .entry(&_mm256_extract_epi16::<9>(self.0))
                .entry(&_mm256_extract_epi16::<10>(self.0))
                .entry(&_mm256_extract_epi16::<11>(self.0))
                .entry(&_mm256_extract_epi16::<12>(self.0))
                .entry(&_mm256_extract_epi16::<13>(self.0))
                .entry(&_mm256_extract_epi16::<14>(self.0))
                .entry(&_mm256_extract_epi16::<15>(self.0))
                .finish()
        }
    }
}

struct DebugByteVector256(__m256i);

impl std::fmt::Debug for DebugByteVector256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            write!(f, "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                    _mm256_extract_epi8::<0>(self.0),
                    _mm256_extract_epi8::<1>(self.0),
                    _mm256_extract_epi8::<2>(self.0),
                    _mm256_extract_epi8::<3>(self.0),
                    _mm256_extract_epi8::<4>(self.0),
                    _mm256_extract_epi8::<5>(self.0),
                    _mm256_extract_epi8::<6>(self.0),
                    _mm256_extract_epi8::<7>(self.0),
                    _mm256_extract_epi8::<8>(self.0),
                    _mm256_extract_epi8::<9>(self.0),
                    _mm256_extract_epi8::<10>(self.0),
                    _mm256_extract_epi8::<11>(self.0),
                    _mm256_extract_epi8::<12>(self.0),
                    _mm256_extract_epi8::<13>(self.0),
                    _mm256_extract_epi8::<14>(self.0),
                    _mm256_extract_epi8::<15>(self.0),
                    _mm256_extract_epi8::<16>(self.0),
                    _mm256_extract_epi8::<17>(self.0),
                    _mm256_extract_epi8::<18>(self.0),
                    _mm256_extract_epi8::<19>(self.0),
                    _mm256_extract_epi8::<20>(self.0),
                    _mm256_extract_epi8::<21>(self.0),
                    _mm256_extract_epi8::<22>(self.0),
                    _mm256_extract_epi8::<23>(self.0),
                    _mm256_extract_epi8::<24>(self.0),
                    _mm256_extract_epi8::<25>(self.0),
                    _mm256_extract_epi8::<26>(self.0),
                    _mm256_extract_epi8::<27>(self.0),
                    _mm256_extract_epi8::<28>(self.0),
                    _mm256_extract_epi8::<29>(self.0),
                    _mm256_extract_epi8::<30>(self.0),
                    _mm256_extract_epi8::<31>(self.0),
            )
        }
    }
}

#[target_feature(enable = "avx2")]
#[inline]
fn mm256_coverage_to_alpha(coverage: __m256i) -> u64 {
    let abs = _mm256_abs_epi32(coverage);
    let partial_mask = _mm256_cmpeq_epi16(abs, _mm256_setzero_si256());
    let overfilled_mask = _mm256_andnot_si256(partial_mask, _mm256_set1_epi16(u8::MAX.into()));
    // correctly rounded result for each partially filled pixel present in the 3rd byte of each i32
    let partial_rounded16hi = _mm256_add_epi32(
        _mm256_sub_epi32(_mm256_slli_epi32::<8>(abs), abs),
        _mm256_set1_epi32((i32::from(u16::MAX) + 1) / 2),
    );

    let rounded16hi = _mm256_or_si256(
        _mm256_and_si256(partial_mask, partial_rounded16hi),
        overfilled_mask,
    );
    // TODO: potentially try shuffle instead?
    let rounded8 = _mm256_packus_epi16(
        _mm256_srli_epi16::<8>(_mm256_packus_epi16(rounded16hi, _mm256_setzero_si256())),
        _mm256_setzero_si256(),
    );
    let lo = _mm256_extract_epi32::<0>(rounded8) as u32 as u64;
    let hi = _mm256_extract_epi32::<4>(rounded8) as u32 as u64;

    lo | (hi << 32)
}

#[test]
fn experiment() {
    unsafe {
        eprintln!(
            "result={:08X?}",
            mm256_coverage_to_alpha(_mm256_set_epi32(
                I16Dot16::from_f32(0.5).into_raw(),
                I16Dot16::from_f32(0.75).into_raw(),
                I16Dot16::from_f32(12.0).into_raw(),
                I16Dot16::from_f32(0.1).into_raw(),
                I16Dot16::from_f32(0.8).into_raw(),
                I16Dot16::from_f32(0.3).into_raw(),
                I16Dot16::from_f32(11.0).into_raw(),
                I16Dot16::from_f32(-15.0).into_raw(),
            ))
        );

        panic!()
    }
}

impl Avx2TileRasterizer {
    #[target_feature(enable = "avx2")]
    #[inline(never)]
    fn rasterize_impl(&mut self, strip_x: u16, tiles: &[Tile], buffer: *mut [MaybeUninit<u8>]) {
        let width = buffer.len() / 4;
        unsafe { std::hint::assert_unchecked(width.is_multiple_of(4)) };

        self.coverage_scratch_buffer.clear();
        self.coverage_scratch_buffer
            .resize(width, AlignedM256(self.current_winding));

        for tile in tiles {
            self.rasterize_line(
                width,
                I16Dot16::new(4 * i32::from(tile.pos.x - strip_x)),
                tile,
            );

            // let coverage = unsafe {
            //     std::slice::from_raw_parts(
            //         self.coverage_scratch_buffer.as_ptr().cast::<I16Dot16>(),
            //         buffer.len(),
            //     )
            // };

            // for y in (0..4).rev() {
            //     for x in 0..width {
            //         eprint!("{:>6.2} ", coverage[x * 4 + y]);
            //     }
            //     eprintln!();
            // }
        }

        let coverage = unsafe {
            std::slice::from_raw_parts(
                self.coverage_scratch_buffer.as_ptr().cast::<I16Dot16>(),
                buffer.len(),
            )
        };

        let input = coverage.as_ptr().cast::<i32>();
        let offsets = _mm256_setr_epi32(0, 4, 8, 12, 16, 20, 24, 28);
        for y in 0..4 {
            unsafe {
                let mut input = input.add(y);
                let mut output = buffer.cast::<u8>().add(y * width).cast::<u64>();
                for _ in 0..width.div_ceil(8) {
                    output.write(mm256_coverage_to_alpha(_mm256_i32gather_epi32::<4>(
                        input, offsets,
                    )));
                    input = input.add(32);
                    output = output.add(1);
                }
            }
        }

        // for y in (0..4).rev() {
        //     for x in 0..width {
        //         eprint!("{:02X} ", buffer[y * width + x]);
        //     }
        //     eprintln!();
        // }
    }

    // FIXME: This is a giant mess.
    // FIXME: This still seems to have a tiny bit more innacuraccy than the generic one?
    #[target_feature(enable = "avx2")]
    fn rasterize_line(&mut self, width: usize, x: I16Dot16, tile: &Tile) {
        if tile.line.bottom_y == tile.line.top_y {
            return;
        }

        let top = Point2::new(tile.line.top_x + x, to_op_fixed(tile.line.top_y));
        let bottom = Point2::new(tile.line.bottom_x + x, to_op_fixed(tile.line.bottom_y));
        let sign = tile.winding as i32;
        // eprintln!("{bottom:?} -> {top:?} {:?}", tile.winding);

        let start_row = tile.line.bottom_y.floor_to_inner();
        let end_row = (top.y.ceil_to_inner() - 1) as u16;
        let right_height = {
            let mut result = [I16Dot16::ZERO; 4];
            let mut current_row = start_row;

            if end_row == current_row {
                result[usize::from(current_row)] = top.y - bottom.y;
            } else {
                result[usize::from(current_row)] = I16Dot16::ONE - bottom.y.fract();
                current_row += 1;

                while current_row < end_row {
                    result[usize::from(current_row)] = I16Dot16::ONE;
                    current_row += 1;
                }

                result[usize::from(current_row)] = top.y - I16Dot16::new(current_row.into());
            }

            result
        };

        let (left, right) = if bottom.x < top.x {
            (bottom, top)
        } else {
            (top, bottom)
        };
        let mut start_px = left.x.floor_to_inner() as u16;
        let coverage_buffer = self.coverage_scratch_buffer.as_mut_ptr().cast::<__m128i>();
        let output_end = unsafe { coverage_buffer.add(width) };
        let mut output = unsafe { coverage_buffer.add(usize::from(start_px)) };
        let vsigned_right_height;

        if top.x == bottom.x {
            let v128signed_right_height = _mm_set_epi32(
                right_height[3].into_raw() * sign,
                right_height[2].into_raw() * sign,
                right_height[1].into_raw() * sign,
                right_height[0].into_raw() * sign,
            );
            let initial_width = I16Dot16::ONE - left.x.fract();
            if initial_width != I16Dot16::ONE {
                let vresult = _mm_srai_epi32::<16>(_mm_mullo_epi32(
                    _mm_set1_epi32(initial_width.into_raw()),
                    v128signed_right_height,
                ));

                unsafe {
                    _mm_store_si128(output, _mm_add_epi32(_mm_load_si128(output), vresult));
                    output = output.add(1);
                }
            }

            if !output.cast::<__m256i>().is_aligned() {
                unsafe {
                    _mm_store_si128(
                        output,
                        _mm_add_epi32(_mm_load_si128(output), v128signed_right_height),
                    )
                };
                output = unsafe { output.add(1) };
            }

            vsigned_right_height = _mm256_broadcastsi128_si256(v128signed_right_height);
        } else {
            let dy = (right.y - left.y) / (right.x - left.x);
            let dh = dy.abs();
            let dx = (top.x - bottom.x) / (top.y - bottom.y);

            let (left_x, right_x, bottom_y, top_y) = {
                let mut left_x = [I16Dot16::ZERO; 4];
                let mut right_x = [I16Dot16::ZERO; 4];
                let mut bottom_y = [I16Dot16::ZERO; 4];
                let mut top_y = [I16Dot16::ZERO; 4];
                let mut current_row = start_row;

                if dy > 0 {
                    bottom_y[usize::from(current_row)] = bottom.y / 2;
                    top_y[usize::from(end_row)] = top.y / 2;
                    if current_row != end_row {
                        for i in current_row + 1..end_row + 1 {
                            bottom_y[usize::from(i)] = I16Dot16::new(i32::from(i)) / 2;
                        }
                    }
                    for i in current_row..end_row {
                        top_y[usize::from(i)] = I16Dot16::new(i32::from(i + 1)) / 2;
                    }
                } else {
                    // TODO: is this even correct??
                    //       ^^^ seems like it
                    bottom_y[usize::from(end_row)] = bottom.y / 2;
                    top_y[usize::from(current_row)] = top.y / 2;
                    for i in current_row..end_row + 1 {
                        bottom_y[usize::from(i)] =
                            I16Dot16::new(i32::from(end_row + start_row - i)) / 2;
                    }
                    if current_row != end_row {
                        for i in current_row..end_row + 1 {
                            top_y[usize::from(i)] =
                                I16Dot16::new(i32::from(end_row + start_row - i + 1)) / 2;
                        }
                    }
                }

                if end_row == current_row {
                    left_x[usize::from(current_row)] = left.x;
                    right_x[usize::from(current_row)] = right.x;
                } else {
                    let mut current_right = bottom.x;
                    current_right += dx * (I16Dot16::ONE - bottom.y.fract());
                    if dx > 0 {
                        left_x[usize::from(current_row)] = bottom.x;
                        right_x[usize::from(current_row)] = current_right;
                    } else {
                        left_x[usize::from(current_row)] = current_right;
                        right_x[usize::from(current_row)] = bottom.x;
                    }
                    current_row += 1;

                    while current_row < end_row {
                        if dx > 0 {
                            left_x[usize::from(current_row)] = current_right;
                            current_right += dx;
                            right_x[usize::from(current_row)] = current_right;
                        } else {
                            right_x[usize::from(current_row)] = current_right;
                            current_right += dx;
                            left_x[usize::from(current_row)] = current_right;
                        }
                        current_row += 1;
                    }

                    if dx > 0 {
                        left_x[usize::from(current_row)] = current_right;
                        right_x[usize::from(current_row)] = top.x;
                    } else {
                        left_x[usize::from(current_row)] = top.x;
                        right_x[usize::from(current_row)] = current_right;
                    }
                }

                (
                    left_x.map(I16Dot16::into_raw),
                    right_x.map(I16Dot16::into_raw),
                    bottom_y.map(I16Dot16::into_raw),
                    top_y.map(I16Dot16::into_raw),
                )
            };

            let dhhalf = dh / 2;
            let vfones = _mm256_set1_epi32(I16Dot16::ONE.into_raw());
            let vsign = _mm256_set1_epi32(tile.winding as i32);
            let vdhhalf = _mm256_set1_epi32(dhhalf.into_raw());

            let first_y = if start_px & 1 != 0 {
                start_px -= 1;
                output = unsafe { output.sub(1) };
                bottom.y / 2 - dhhalf
            } else {
                bottom.y / 2
            };
            let second_y = first_y + (I16Dot16::ONE - left.x.fract()) * dhhalf;
            let third_y = second_y + dhhalf;

            let mut vcurrent_px = _mm256_set_epi32(
                i32::from(start_px + 1) << 16,
                i32::from(start_px + 1) << 16,
                i32::from(start_px + 1) << 16,
                i32::from(start_px + 1) << 16,
                i32::from(start_px) << 16,
                i32::from(start_px) << 16,
                i32::from(start_px) << 16,
                i32::from(start_px) << 16,
            );
            let mut vcurrent_y = _mm256_set_epi32(
                second_y.into_raw(),
                second_y.into_raw(),
                second_y.into_raw(),
                second_y.into_raw(),
                first_y.into_raw(),
                first_y.into_raw(),
                first_y.into_raw(),
                first_y.into_raw(),
            );
            let mut vnext_y = _mm256_set_epi32(
                third_y.into_raw(),
                third_y.into_raw(),
                third_y.into_raw(),
                third_y.into_raw(),
                second_y.into_raw(),
                second_y.into_raw(),
                second_y.into_raw(),
                second_y.into_raw(),
            );

            let vbottom_y = _mm256_set_epi32(
                bottom_y[3],
                bottom_y[2],
                bottom_y[1],
                bottom_y[0],
                bottom_y[3],
                bottom_y[2],
                bottom_y[1],
                bottom_y[0],
            );
            let vtop_y = _mm256_set_epi32(
                top_y[3], top_y[2], top_y[1], top_y[0], top_y[3], top_y[2], top_y[1], top_y[0],
            );
            let vleft_x = _mm256_set_epi32(
                left_x[3], left_x[2], left_x[1], left_x[0], left_x[3], left_x[2], left_x[1],
                left_x[0],
            );
            let vright_x = _mm256_set_epi32(
                right_x[3], right_x[2], right_x[1], right_x[0], right_x[3], right_x[2], right_x[1],
                right_x[0],
            );

            let vright_height = _mm256_set_epi32(
                right_height[3].into_raw(),
                right_height[2].into_raw(),
                right_height[1].into_raw(),
                right_height[0].into_raw(),
                right_height[3].into_raw(),
                right_height[2].into_raw(),
                right_height[1].into_raw(),
                right_height[0].into_raw(),
            );

            // TODO: This still seems to have a tiny bit more innacuraccy than the generic one?

            // eprintln!("vbottom_y={:?}", DebugCells(vbottom_y));
            // eprintln!("vtop_y={:?}", DebugCells(vtop_y));
            // eprintln!("vleft_x={:?}", DebugCells(vleft_x));
            // eprintln!("vright_x={:?}", DebugCells(vright_x));

            // eprintln!();

            while output < output_end {
                unsafe {
                    // ymm0 - vcurrent_px
                    // ymm2 - vcurrent_y
                    // ymm3 - vnext_y
                    //
                    // ymm15 - vfones
                    // ymm14 - vdhhalf
                    // ymm8 - vsign
                    //
                    // ymm13 - vright_x
                    // ymm12 - vleft_x
                    // ymm11 - vtop_y
                    // ymm10 - vbottom_y
                    // ymm9 - vright_height
                    std::arch::asm! {
                        "vpaddd ymm1, ymm0, ymm15",
                        // ymm1 - vnext_px

                        "vpminsd ymm6, ymm1, ymm13",
                        // ymm6 = min(vnext_px, vright_x)
                        "vpmaxsd ymm7, ymm0, ymm12",
                        // ymm7 = max(vcurrent_px, vleft_x)
                        "vpsubd ymm5, ymm6, ymm7",

                        "vpxor ymm0, ymm0, ymm0",
                        // ymm0 = vzeroes
                        "vpmaxsd ymm5, ymm5, ymm0",
                        // ymm5 = vwidth

                        "vpmaxsd ymm4, ymm2, ymm10",
                        // ymm4 = vinner_bottom

                        "vpminsd ymm6, ymm3, ymm11",
                        "vpsubd ymm6, ymm6, ymm4",
                        "vpmaxsd ymm7, ymm6, ymm0",
                        // ymm7 = vtriangle_height

                        "vpsubd ymm2, ymm4, ymm10",
                        // ymm2 = vbottom_height

                        // loaded here so the CPU has time to finish loading
                        // it before the end of the loop
                        "vmovdqa ymm4, [{output}]",
                        // ymm4 = *output

                        "vpslld ymm2, ymm2, 1",
                        // TODO: swap these two instructions?
                        "vpminsd ymm2, ymm2, ymm15",
                        "vpaddd ymm7, ymm7, ymm2",
                        // ymm7 = vleft_height

                        "vpsrlq ymm6, ymm7, 32",
                        "vpmuldq ymm7, ymm7, ymm5",
                        "vpsrlq ymm5, ymm5, 32",
                        "vpsrlq ymm7, ymm7, 16",
                        // ymm7 = vleft_area lomul
                        "vpmuldq ymm6, ymm6, ymm5",
                        "vpsllq ymm6, ymm6, 16",
                        // ymm6 = vleft_area himul
                        "vpblendd ymm6, ymm7, ymm6, 0xAA",
                        // ymm6 = vleft_area

                        // ymm2 = scratch 3
                        "vpsubd ymm2, ymm1, ymm13",
                        "vpminsd ymm2, ymm2, ymm15",
                        "vpmaxsd ymm2, ymm2, ymm0",
                        // ymm2 = vright_width

                        "vpmuldq ymm5, ymm2, ymm9",
                        "vpsrlq ymm2, ymm2, 32",
                        "vpsrlq ymm7, ymm9, 32",
                        "vpmuldq ymm7, ymm7, ymm2",
                        "vpsrlq ymm5, ymm5, 16",
                        // ymm5 = vright_area lomul
                        "vpsllq ymm7, ymm7, 16",
                        // ymm7 = vright_area himul
                        "vpblendd ymm5, ymm5, ymm7, 0xAA",
                        // ymm5 = vright_area

                        "vpaddd ymm6, ymm6, ymm5",
                        "vpmulld ymm6, ymm6, ymm8",

                        "vpaddd ymm7, ymm6, ymm4",
                        "vmovdqa [{output}], ymm7",

                        "vpaddd ymm2, ymm3, ymm14",
                        "vpaddd ymm3, ymm2, ymm14",
                        "vpaddd ymm0, ymm1, ymm15",
                        "add {output}, 32",
                        output = inout(reg) output,
                        inout("ymm0") vcurrent_px,
                        inout("ymm2") vcurrent_y,
                        inout("ymm3") vnext_y,

                        in("ymm15") vfones,
                        in("ymm14") vdhhalf,
                        in("ymm8") vsign,

                        in("ymm13") vright_x,
                        in("ymm12") vleft_x,
                        in("ymm11") vtop_y,
                        in("ymm10") vbottom_y,
                        in("ymm9") vright_height,

                        out("ymm1") _,
                        out("ymm4") _,
                        out("ymm5") _,
                        out("ymm6") _,
                        out("ymm7") _,

                        options(nostack)
                    }
                }
            }

            vsigned_right_height = _mm256_mul_epi32(vright_height, vsign)
        }

        while output < output_end {
            unsafe {
                _mm256_store_si256(
                    output.cast(),
                    _mm256_add_epi32(_mm256_load_si256(output.cast()), vsigned_right_height),
                )
            };

            output = unsafe { output.add(2) };
        }
        self.current_winding = _mm256_add_epi32(self.current_winding, vsigned_right_height);
    }
}
