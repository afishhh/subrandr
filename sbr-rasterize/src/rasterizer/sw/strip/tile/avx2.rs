use std::{arch::x86_64::*, mem::MaybeUninit};

use util::math::{I16Dot16, Point2};

use super::{to_op_fixed, Tile};

pub struct Avx2TileRasterizer {
    coverage_scratch_buffer: Vec<AlignedM256>,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, align(32))]
struct AlignedM256(__m256i);

impl Avx2TileRasterizer {
    pub fn new() -> Self {
        Self {
            coverage_scratch_buffer: Vec::new(),
        }
    }
}

impl super::TileRasterizer for Avx2TileRasterizer {
    unsafe fn rasterize(
        &mut self,
        strip_x: u16,
        tiles: &[Tile],
        initial_winding: I16Dot16,
        buffer: *mut [MaybeUninit<u8>],
    ) {
        unsafe { self.rasterize_impl(strip_x, tiles, initial_winding, buffer) }
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

// TODO: This function is only used on values in the range [0, 1].
//       Can this operation be sped up? (or surrounding code
//       changed to allow using mulhi_epu16?)
#[target_feature(enable = "avx2")]
#[inline]
fn mm256_mul_16dot16(a: __m256i, b: __m256i) -> __m256i {
    let lomul = _mm256_srli_epi64::<16>(_mm256_mul_epi32(a, b));
    let himul = _mm256_slli_epi64::<16>(_mm256_mul_epi32(
        _mm256_srli_epi64::<32>(a),
        _mm256_srli_epi64::<32>(b),
    ));
    _mm256_blend_epi32::<0b10101010>(lomul, himul)
}

#[test]
fn experiment2() {
    unsafe {
        eprintln!(
            "{:?}",
            DebugCells(mm256_mul_16dot16(
                _mm256_setr_epi32(
                    I16Dot16::from_f32(1.0).into_raw(),
                    I16Dot16::from_f32(0.5).into_raw(),
                    I16Dot16::from_f32(0.25).into_raw(),
                    I16Dot16::from_f32(1.0).into_raw(),
                    I16Dot16::from_f32(0.5).into_raw(),
                    I16Dot16::from_f32(0.0).into_raw(),
                    I16Dot16::from_f32(1.0).into_raw(),
                    I16Dot16::from_f32(0.33).into_raw(),
                ),
                _mm256_setr_epi32(
                    I16Dot16::from_f32(1.0).into_raw(),
                    I16Dot16::from_f32(0.5).into_raw(),
                    I16Dot16::from_f32(0.75).into_raw(),
                    I16Dot16::from_f32(0.5).into_raw(),
                    I16Dot16::from_f32(1.0).into_raw(),
                    I16Dot16::from_f32(1.0).into_raw(),
                    I16Dot16::from_f32(0.0).into_raw(),
                    I16Dot16::from_f32(0.33).into_raw(),
                ),
            ))
        );

        panic!();
    }
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
    fn rasterize_impl(
        &mut self,
        strip_x: u16,
        tiles: &[Tile],
        initial_winding: I16Dot16,
        buffer: *mut [MaybeUninit<u8>],
    ) {
        let width = buffer.len() / 4;
        unsafe { std::hint::assert_unchecked(width.is_multiple_of(4)) };

        self.coverage_scratch_buffer.clear();
        self.coverage_scratch_buffer.resize(
            width,
            AlignedM256(_mm256_set1_epi32(initial_winding.into_raw())),
        );

        for tile in tiles {
            self.rasterize_line(
                width,
                I16Dot16::new(4 * i32::from(tile.pos.x - strip_x)),
                tile,
            );

            let coverage = unsafe {
                std::slice::from_raw_parts(
                    self.coverage_scratch_buffer.as_ptr().cast::<I16Dot16>(),
                    buffer.len(),
                )
            };

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

        let v128signed_right_height = _mm_set_epi32(
            right_height[3].into_raw() * sign,
            right_height[2].into_raw() * sign,
            right_height[1].into_raw() * sign,
            right_height[0].into_raw() * sign,
        );

        let (left, right) = if bottom.x < top.x {
            (bottom, top)
        } else {
            (top, bottom)
        };
        let start_px = left.x.floor_to_inner() as u16;
        let end_px = right.x.ceil_to_inner() as u16;
        let coverage_buffer = self.coverage_scratch_buffer.as_mut_ptr().cast::<__m128i>();
        let output_end = unsafe { coverage_buffer.add(width) };
        let mut output = unsafe { coverage_buffer.add(usize::from(start_px)) };

        if top.x == bottom.x {
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
            let vzeroes = _mm256_set1_epi32(I16Dot16::ZERO.into_raw());
            let vfones = _mm256_set1_epi32(I16Dot16::ONE.into_raw());
            let vsign = _mm256_set1_epi32(tile.winding as i32);
            let vdhhalf = _mm256_set1_epi32(dhhalf.into_raw());

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
            let first_y = bottom.y / 2;
            let second_y = first_y + (I16Dot16::ONE - left.x.fract()) * dhhalf;
            let third_y = second_y + dhhalf;
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

            // TODO: single pass here
            //       ^^^ what the fuck did I mean in this comment?
            // TODO: This still seems to have a tiny bit more innacuraccy than the generic one?

            // eprintln!("vbottom_y={:?}", DebugCells(vbottom_y));
            // eprintln!("vtop_y={:?}", DebugCells(vtop_y));
            // eprintln!("vleft_x={:?}", DebugCells(vleft_x));
            // eprintln!("vright_x={:?}", DebugCells(vright_x));

            // eprintln!();

            let mut current_px = start_px;
            while current_px < end_px {
                let vnext_px = _mm256_add_epi32(vcurrent_px, vfones);

                let vwidth = _mm256_max_epi32(
                    _mm256_sub_epi32(
                        _mm256_min_epi32(vnext_px, vright_x),
                        _mm256_max_epi32(vcurrent_px, vleft_x),
                    ),
                    vzeroes,
                );
                // eprintln!("vwidth={:?}", DebugCells(vwidth));
                // eprintln!("vcurrent_y={:?}", DebugCells(vcurrent_y));
                // eprintln!("vnext_y={:?}", DebugCells(vnext_y));

                let vinner_bottom = _mm256_max_epi32(vcurrent_y, vbottom_y);
                let vtriangle_height = {
                    let t = _mm256_min_epi32(vnext_y, vtop_y);
                    let h = _mm256_sub_epi32(t, vinner_bottom);
                    _mm256_max_epi32(h, vzeroes)
                };
                let vbottom_height = _mm256_sub_epi32(vinner_bottom, vbottom_y);
                // eprintln!("vtriangle_height={:?}", DebugCells(vtriangle_height));
                // eprintln!("vbottom_height={:?}", DebugCells(vbottom_height));
                let vleft_area = mm256_mul_16dot16(
                    _mm256_add_epi32(
                        vtriangle_height,
                        _mm256_min_epi32(_mm256_slli_epi32::<1>(vbottom_height), vfones),
                    ),
                    vwidth,
                );
                // eprintln!("vleft_area={:?}", DebugCells(vleft_area));

                let vright_area = {
                    let vright_width = _mm256_max_epi32(
                        _mm256_min_epi32(_mm256_sub_epi32(vnext_px, vright_x), vfones),
                        vzeroes,
                    );
                    mm256_mul_16dot16(vright_width, vright_height)
                };
                // eprintln!("vright_area={:?}", DebugCells(vright_area));

                let vresult = _mm256_mullo_epi32(_mm256_add_epi32(vleft_area, vright_area), vsign);
                // eprintln!("vresult={:?}", DebugCells(vresult));

                unsafe {
                    _mm256_storeu_si256(
                        output.cast(),
                        _mm256_add_epi32(_mm256_loadu_si256(output.cast()), vresult),
                    )
                };

                vcurrent_y = _mm256_add_epi32(vnext_y, vdhhalf);
                vnext_y = _mm256_add_epi32(vcurrent_y, vdhhalf);
                vcurrent_px = _mm256_add_epi32(vnext_px, vfones);
                current_px += 2;
                output = unsafe { output.add(2) };

                // eprintln!();
            }
        }

        Self::fill_tail(output, output_end, v128signed_right_height);
    }

    #[target_feature(enable = "avx2")]
    fn fill_tail(
        mut output: *mut __m128i,
        output_end: *mut __m128i,
        v128signed_right_height: __m128i,
    ) {
        if output < output_end {
            let vsigned_right_height =
                _mm256_set_m128i(v128signed_right_height, v128signed_right_height);

            if !output.cast::<__m256i>().is_aligned() {
                unsafe {
                    _mm_store_si128(
                        output,
                        _mm_add_epi32(_mm_load_si128(output), v128signed_right_height),
                    )
                };
                output = unsafe { output.add(1) };
            }

            while output < output_end {
                unsafe {
                    _mm256_storeu_si256(
                        output.cast(),
                        _mm256_add_epi32(_mm256_loadu_si256(output.cast()), vsigned_right_height),
                    )
                };

                output = unsafe { output.add(2) };
            }
        }
    }
}
