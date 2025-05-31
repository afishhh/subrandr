use std::{convert::Infallible, ffi::c_void, mem::MaybeUninit};

use text_sys::{
    hb_blob_get_empty, hb_bool_t, hb_codepoint_t, hb_face_create, hb_face_set_glyph_count,
    hb_font_create, hb_font_destroy, hb_font_extents_t, hb_font_funcs_create,
    hb_font_funcs_set_font_h_extents_func, hb_font_funcs_set_glyph_extents_func,
    hb_font_funcs_set_glyph_h_advance_func, hb_font_funcs_set_glyph_h_origin_func,
    hb_font_funcs_set_nominal_glyph_func, hb_font_get_user_data, hb_font_make_immutable,
    hb_font_reference, hb_font_set_funcs, hb_font_set_user_data, hb_font_t, hb_glyph_extents_t,
    hb_position_t, hb_user_data_key_t,
};

use crate::{
    layout::{FixedL, Point2L, Vec2L},
    math::{I16Dot16, Point2, Rect2, Vec2},
    rasterize::{PixelFormat, Rasterizer},
    util::slice_assume_init_mut,
    I26Dot6,
};

use super::{FaceImpl, FontImpl, FontMetrics, GlyphCache, GlyphMetrics, SingleGlyphBitmap};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Face;

impl FaceImpl for Face {
    type Font = Font;

    fn family_name(&self) -> &str {
        "Subrandr Tofu Font"
    }

    fn axes(&self) -> &[super::Axis] {
        &[]
    }

    fn set_axis(&mut self, _index: usize, _value: crate::math::I16Dot16) {
        panic!("Cannot set builtin tofu font axis values")
    }

    fn weight(&self) -> crate::math::I16Dot16 {
        I16Dot16::new(400)
    }

    fn italic(&self) -> bool {
        false
    }

    type Error = Infallible;
    fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Self::Font, Self::Error> {
        Ok(Font::create(point_size, dpi))
    }
}

static TOFU_HB_FONT_USERDATA_KEY: hb_user_data_key_t = hb_user_data_key_t { unused: 104 };

struct FontShared {
    point_size: I26Dot6,
    pixel_height: I26Dot6,
    pixel_width: I26Dot6,
    glyph_metrics: GlyphMetrics,
    font_metrics: FontMetrics,
    cache: GlyphCache<Font>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Font {
    hb: *mut hb_font_t,
}

impl Font {
    fn create(point_size: I26Dot6, dpi: u32) -> Self {
        let shared = Box::into_raw(Box::new({
            let dpi_scale = I16Dot16::from_quotient(dpi as i32, 72);
            let dpi_scale6 = I26Dot6::from_raw(dpi_scale.into_raw() >> 10);
            let decoration_thickness = point_size * dpi_scale6 / 192;
            let pixel_height = point_size * 96 / dpi as i32;
            let pixel_width = pixel_height * 2 / 3;
            let ascender = pixel_height * 2 / 3;
            let descender = ascender - pixel_height;
            FontShared {
                point_size,
                pixel_height,
                pixel_width,
                glyph_metrics: GlyphMetrics {
                    width: pixel_width,
                    height: pixel_height,
                    hori_bearing_x: pixel_height / 12,
                    hori_bearing_y: ascender,
                    hori_advance: pixel_width,
                    vert_bearing_x: I26Dot6::ZERO,
                    vert_bearing_y: I26Dot6::ZERO,
                    vert_advance: pixel_height,
                },
                font_metrics: {
                    FontMetrics {
                        ascender,
                        descender,
                        height: ascender - descender,
                        max_advance: pixel_width,
                        underline_top_offset: (descender - decoration_thickness) / 2,
                        underline_thickness: decoration_thickness,
                        strikeout_top_offset: (ascender - descender) / 2
                            - ascender
                            - decoration_thickness / 2,
                        strikeout_thickness: decoration_thickness,
                    }
                },
                cache: GlyphCache::new(),
            }
        }));

        unsafe extern "C" fn free_userdata(userdata: *mut c_void) {
            drop(unsafe { Box::from_raw(userdata.cast::<FontShared>()) });
        }

        let hb = unsafe {
            let funcs = hb_font_funcs_create();
            assert!(
                !funcs.is_null(),
                "hb_font_funcs_create failed to allocate font funcs"
            );

            macro_rules! set {
                (
                    $hb_setter: ident,
                    $Self: ident :: $name: ident($($param: ident : $param_ty: ty),*) -> $ret: ty
                ) => {{
                    unsafe extern "C" fn wrapper(font: *mut hb_font_t, fontdata: *mut c_void $(, $param: $param_ty)*, userdata: *mut c_void) -> $ret {
                        _ = font;
                        debug_assert!(userdata.is_null());
                        $Self::$name(&*fontdata.cast::<FontShared>() $(, $param)*)
                    }

                    $hb_setter(
                        funcs,
                        Some(wrapper),
                        std::ptr::null_mut(),
                        None,
                    );
                }};
            }

            set!(
                hb_font_funcs_set_font_h_extents_func,
                Font::hb_font_h_extents_func(extents: *mut hb_font_extents_t) -> i32
            );

            set!(
                hb_font_funcs_set_nominal_glyph_func,
                Font::hb_nominal_glyph_func(unicode: hb_codepoint_t, glyph: *mut hb_codepoint_t) -> i32
            );

            set!(
                hb_font_funcs_set_glyph_h_advance_func,
                Font::hb_glyph_h_advance_func(glyph: hb_codepoint_t) -> hb_position_t
            );

            set!(
                hb_font_funcs_set_glyph_h_origin_func,
                Font::hb_glyph_h_origin_func(
                    glyph: hb_codepoint_t,
                    x: *mut hb_position_t,
                    y: *mut hb_position_t
                ) -> hb_bool_t
            );

            set!(
                hb_font_funcs_set_glyph_extents_func,
                Font::hb_glyphs_extents_func(glyph: hb_codepoint_t, extents: *mut hb_glyph_extents_t) -> i32
            );

            let face = hb_face_create(hb_blob_get_empty(), 0);
            assert!(!face.is_null(), "hb_face_create failed to allocate face");

            // HarfBuzz understandably assumes the font is useless if the glyph count is zero
            hb_face_set_glyph_count(face, u32::MAX);

            let font = hb_font_create(face);
            assert!(!font.is_null(), "hb_font_create failed to allocate font");

            let set = hb_font_set_user_data(
                font,
                (&raw const TOFU_HB_FONT_USERDATA_KEY).cast_mut(),
                shared.cast::<c_void>(),
                Some(free_userdata),
                1,
            );
            assert_eq!(set, 1);
            hb_font_set_funcs(font, funcs, shared.cast::<c_void>(), None);
            hb_font_make_immutable(font);
            font
        };

        Self { hb }
    }

    unsafe fn hb_font_h_extents_func(shared: &FontShared, extents: *mut hb_font_extents_t) -> i32 {
        let out = &mut *extents;
        out.ascender = shared.font_metrics.ascender.into_raw();
        out.descender = shared.font_metrics.descender.into_raw();
        out.line_gap = (shared.font_metrics.height - shared.font_metrics.ascender
            + shared.font_metrics.descender)
            .into_raw();

        0
    }

    unsafe fn hb_nominal_glyph_func(
        _shared: &FontShared,
        unicode: hb_codepoint_t,
        glyph: *mut hb_codepoint_t,
    ) -> i32 {
        // NOTE: A zero glyph is treated as not found so we convert NUL into u32::MAX
        //       which should be outside the unicode range anyway.
        glyph.write(if unicode == 0 { u32::MAX } else { unicode });
        0
    }

    unsafe fn hb_glyph_h_advance_func(
        shared: &FontShared,
        _glyph: hb_codepoint_t,
    ) -> hb_position_t {
        shared.pixel_width.into_raw()
    }

    unsafe fn hb_glyph_h_origin_func(
        shared: &FontShared,
        _glyph: hb_codepoint_t,
        x: *mut hb_position_t,
        y: *mut hb_position_t,
    ) -> hb_bool_t {
        *x = 0;
        *y = shared.glyph_metrics.hori_bearing_y.into_raw();
        1
    }

    unsafe fn hb_glyphs_extents_func(
        shared: &FontShared,
        _glyph: hb_codepoint_t,
        extents: *mut hb_glyph_extents_t,
    ) -> i32 {
        let out = &mut *extents;
        out.width = shared.pixel_width.into_raw();
        out.height = shared.pixel_height.into_raw();
        out.x_bearing = shared.glyph_metrics.hori_bearing_x.into_raw();
        out.y_bearing = shared.glyph_metrics.hori_bearing_y.into_raw();

        0
    }

    fn shared(&self) -> &FontShared {
        unsafe {
            &*hb_font_get_user_data(self.hb, (&raw const TOFU_HB_FONT_USERDATA_KEY).cast_mut())
                .cast::<FontShared>()
        }
    }

    pub fn glyph_metrics(&self) -> &GlyphMetrics {
        &self.shared().glyph_metrics
    }

    pub fn as_harfbuzz_font(&self) -> *mut hb_font_t {
        self.hb
    }
}

impl FontImpl for Font {
    type Face = Face;

    fn face(&self) -> &Self::Face {
        &Face
    }

    fn glyph_cache(&self) -> &super::GlyphCache<Self> {
        &self.shared().cache
    }

    fn metrics(&self) -> &FontMetrics {
        &self.shared().font_metrics
    }

    fn point_size(&self) -> I26Dot6 {
        self.shared().point_size
    }

    type FontSizeKey = I26Dot6;
    fn font_size_key(&self) -> Self::FontSizeKey {
        self.shared().pixel_height
    }

    type MeasureError = Infallible;
    fn measure_glyph_uncached(&self, _index: u32) -> Result<GlyphMetrics, Self::MeasureError> {
        unreachable!()
    }

    type RenderError = Infallible;
    fn render_glyph_uncached(
        &self,
        parent_rasterizer: &mut dyn Rasterizer,
        index: u32,
        offset: Vec2L,
    ) -> Result<SingleGlyphBitmap, Self::RenderError> {
        let shared = self.shared();
        let pixel_height = shared.pixel_height;
        let pixel_width = shared.pixel_width;
        let outline_width = pixel_height / 160;
        let base_spacing = pixel_height / 12;
        let margin = base_spacing;
        let inner_offset = offset + Vec2::new(margin, margin);
        let fract_offset = Vec2::new(inner_offset.x.fract(), inner_offset.y.fract());

        let outline_size = Vec2::new(pixel_width - margin * 2, pixel_height - margin * 2);
        let texture_size = Vec2::new(
            (outline_size.x + fract_offset.x).ceil_to_inner() as u32,
            (outline_size.y + fract_offset.y).ceil_to_inner() as u32,
        );
        let texture = unsafe {
            parent_rasterizer.create_packed_texture_mapped(
                texture_size.x,
                texture_size.y,
                PixelFormat::Mono,
                Box::new(|texture, stride| {
                    texture.fill(MaybeUninit::new(0));
                    let texture = slice_assume_init_mut(texture);

                    let mut rasterizer = crate::rasterize::sw::Rasterizer::new();
                    let mut target = crate::rasterize::sw::create_render_target_mono(
                        texture,
                        texture_size.x,
                        texture_size.y,
                        stride as u32,
                    );

                    rasterizer.fill_axis_aligned_antialias_mono_rect_set(
                        &mut target,
                        Rect2::layout_to_float(
                            Rect2 {
                                min: Point2::new(FixedL::ZERO, outline_size.y - outline_width),
                                max: Point2::new(outline_size.x, outline_size.y),
                            }
                            .translate(fract_offset),
                        ),
                        255,
                    );

                    rasterizer.fill_axis_aligned_antialias_mono_rect_set(
                        &mut target,
                        Rect2::layout_to_float(
                            Rect2 {
                                min: Point2::new(FixedL::ZERO, FixedL::ZERO),
                                max: Point2::new(outline_size.x, outline_width),
                            }
                            .translate(fract_offset),
                        ),
                        255,
                    );

                    rasterizer.fill_axis_aligned_antialias_mono_rect_set(
                        &mut target,
                        Rect2::layout_to_float(
                            Rect2 {
                                min: Point2::new(FixedL::ZERO, outline_width),
                                max: Point2::new(outline_width, outline_size.y - outline_width),
                            }
                            .translate(fract_offset),
                        ),
                        255,
                    );

                    rasterizer.fill_axis_aligned_antialias_mono_rect_set(
                        &mut target,
                        Rect2::layout_to_float(
                            Rect2 {
                                min: Point2::new(outline_size.x - outline_width, outline_width),
                                max: Point2::new(outline_size.x, outline_size.y - outline_width),
                            }
                            .translate(fract_offset),
                        ),
                        255,
                    );

                    let content_offset = fract_offset + Vec2::splat(outline_width);
                    let content_size = outline_size - Vec2::splat(outline_width * 2);
                    let min_cell_spacing_x = base_spacing / 2;
                    let min_cell_spacing_y = base_spacing;

                    let mut digits_buf = [0u8; 8];
                    let num_chars = {
                        let mut value = if index == u32::MAX { 0 } else { index };
                        let mut len = 0usize;
                        while value > 0 {
                            digits_buf[len] = (value % 16) as u8;
                            value /= 16;
                            len += 1;
                        }
                        if len == 0 {
                            len += 1;
                        }
                        digits_buf[..len].reverse();
                        len
                    };

                    let (cells_per_row, max_cols): (&[u8], u8) = match num_chars {
                        1 => (&[1], 1),
                        2 => (&[2], 2),
                        3 => (&[2, 2], 2),
                        4 => (&[2, 2], 2),
                        5 => (&[2, 2, 2], 3),
                        6 => (&[2, 2, 2], 3),
                        // These should not actually occur
                        7 => (&[3, 3, 3], 3),
                        8 => (&[3, 3, 3], 3),
                        0 | 9.. => unreachable!(),
                    };
                    let rows = cells_per_row.len() as u8;

                    let char_space_x = (content_size.x
                        - (min_cell_spacing_x * i32::from(max_cols - 1)))
                        / i32::from(max_cols + 1);
                    let char_space_y = (content_size.y
                        - min_cell_spacing_y * (i32::from(rows - 1)))
                        / i32::from(rows + 1);

                    let (cell_size_x, cell_size_y) = if char_space_x / 2 < char_space_y / 3 {
                        (char_space_x, char_space_x / 2 * 3)
                    } else {
                        (char_space_y * 2 / 3, char_space_y)
                    };

                    let mut draw_digit = |offset: Vec2L, size: Vec2L, digit: u8| {
                        let rects = GLYPHS[usize::from(digit)];
                        for rect in rects {
                            let transform =
                                |p: Point2L| Point2::new(p.x * size.x / 200, p.y * size.y / 400);
                            rasterizer.fill_axis_aligned_antialias_mono_rect_set(
                                &mut target,
                                Rect2::layout_to_float(
                                    Rect2 {
                                        min: transform(rect.min),
                                        max: transform(rect.max),
                                    }
                                    .translate(offset),
                                ),
                                255,
                            );
                        }
                    };

                    // If we don't have at least 2.5x5 pixels per glyph then our digits
                    // aren't going to be readable anyway, just draw a question mark.
                    if cell_size_x < FixedL::from_quotient(5, 2) || cell_size_y < 5 {
                        draw_digit(
                            content_offset,
                            Vec2::new(content_size.x, content_size.y),
                            16,
                        );
                    } else {
                        let mut i = 0;
                        let justify_spacing_y =
                            (content_size.y - cell_size_y * i32::from(rows)) / i32::from(rows + 1);
                        let mut y = content_offset.y + justify_spacing_y.into_f32();
                        for &row in cells_per_row {
                            let justify_spacing_x = (content_size.x - cell_size_x * i32::from(row))
                                / i32::from(row + 1);

                            let mut x = content_offset.x + justify_spacing_x.into_f32();
                            for _ in 0..row {
                                draw_digit(
                                    Vec2::new(x, y),
                                    Vec2::new(cell_size_x, cell_size_y),
                                    digits_buf[i],
                                );
                                x += cell_size_x.into_f32() + justify_spacing_x.into_f32();
                                i += 1;
                            }

                            y += cell_size_y.into_f32() + justify_spacing_y.into_f32();
                        }
                    }

                    rasterizer.flush(&mut target);
                }),
            )
        };

        Ok(SingleGlyphBitmap {
            offset: Vec2::new(inner_offset.x.floor(), inner_offset.y.floor()),
            texture,
        })
    }
}

impl Clone for Font {
    fn clone(&self) -> Self {
        Self {
            hb: unsafe { hb_font_reference(self.hb) },
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe { hb_font_destroy(self.hb) };
    }
}

macro_rules! glyph {
    [
        $([($a: literal, $b: literal), ($c: literal, $d: literal)],)*
    ] => {[
        $(Rect2 {
            min: Point2::new(FixedL::new($a), FixedL::new($b)),
            max: Point2::new(FixedL::new($c), FixedL::new($d))
        },)*
    ]};
}

// All glyphs that make up our mini tofu font on a 200x400 canvas.
// This includes all hex digits and a special question mark glyph.
const GLYPHS: &[&[Rect2<FixedL>]; 17] = &[
    // 0
    &glyph![
        [(10, 0), (190, 20)],
        [(10, 380), (190, 400)],
        [(0, 20), (10, 380)],
        [(190, 20), (200, 380)],
        [(90, 180), (110, 220)],
    ],
    // 1
    &glyph![
        [(30, 0), (95, 20)],
        [(95, 20), (105, 380)],
        [(10, 380), (190, 400)],
    ],
    // 2
    &glyph![
        [(0, 20), (10, 50)],
        [(10, 0), (190, 20)],
        [(190, 20), (200, 190)],
        [(10, 190), (190, 210)],
        [(0, 210), (10, 380)],
        [(10, 380), (200, 400)],
    ],
    // 3
    &glyph![
        [(0, 0), (190, 20)],
        [(0, 190), (190, 210)],
        [(0, 380), (190, 400)],
        [(190, 20), (200, 380)],
    ],
    // 4
    &glyph![
        [(0, 0), (10, 190)],
        [(190, 0), (200, 400)],
        [(10, 190), (190, 210)],
    ],
    // 5 (pretty much a flipped 2)
    &glyph![
        [(10, 0), (200, 20)],
        [(0, 20), (10, 190)],
        [(10, 190), (190, 210)],
        [(190, 210), (200, 380)],
        [(0, 380), (190, 400)],
    ],
    // 6
    &glyph![
        [(10, 0), (200, 20)],
        [(0, 20), (10, 400)],
        [(10, 190), (190, 210)],
        [(190, 210), (200, 380)],
        [(10, 380), (190, 400)],
    ],
    // 7
    &glyph![
        [(10, 0), (190, 20)],
        [(160, 20), (180, 80)],
        [(140, 80), (160, 160)],
        [(120, 160), (140, 200)],
        [(100, 200), (120, 260)],
        [(80, 260), (100, 320)],
        [(60, 320), (80, 380)],
        [(40, 380), (60, 400)],
    ],
    // 8
    &glyph![
        [(10, 0), (190, 20)],
        [(0, 20), (10, 190)],
        [(190, 20), (200, 190)],
        [(10, 190), (190, 210)],
        [(0, 210), (10, 380)],
        [(190, 210), (200, 380)],
        [(10, 380), (190, 400)],
    ],
    // 9
    &glyph![
        [(10, 0), (190, 20)],
        [(0, 20), (10, 190)],
        [(190, 20), (200, 380)],
        [(10, 190), (190, 210)],
        [(10, 380), (190, 400)],
    ],
    // A
    &glyph![
        [(10, 0), (190, 20)],
        [(0, 20), (10, 380)],
        [(190, 20), (200, 380)],
        [(10, 190), (190, 210)],
    ],
    // B (a slightly different 8)
    &glyph![
        [(0, 0), (10, 400)],
        [(10, 0), (170, 20)],
        [(10, 150), (190, 170)],
        [(10, 380), (190, 400)],
        [(170, 20), (180, 150)],
        [(190, 170), (200, 380)],
    ],
    // C
    &glyph![
        [(0, 20), (10, 380)],
        [(10, 0), (190, 20)],
        [(10, 380), (190, 400)],
    ],
    // D
    &glyph![
        [(0, 0), (10, 400)],
        [(10, 0), (190, 20)],
        [(20, 380), (190, 400)],
        [(190, 20), (200, 380)],
    ],
    // E
    &glyph![
        [(45, 0), (190, 20)],
        [(45, 190), (190, 210)],
        [(45, 380), (190, 400)],
        [(35, 20), (45, 380)],
    ],
    // F
    &glyph![
        [(10, 0), (190, 20)],
        [(10, 190), (130, 210)],
        [(0, 20), (10, 400)],
    ],
    // ? (drawn in full content box if not enough space is available for hex)
    &glyph![
        [(50, 100), (60, 120)],
        [(60, 80), (140, 100)],
        [(150, 100), (160, 160)],
        [(140, 160), (150, 175)],
        [(130, 175), (140, 190)],
        [(120, 190), (130, 205)],
        [(110, 205), (120, 220)],
        [(100, 220), (110, 235)],
        [(90, 235), (110, 260)],
        [(90, 320), (110, 340)],
    ],
];
