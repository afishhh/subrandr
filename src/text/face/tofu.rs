use std::{convert::Infallible, ffi::c_void, mem::MaybeUninit};

use rasterize::{
    sw::{StripRasterizer, Strips},
    PixelFormat, Rasterizer,
};
use text_sys::*;
use util::{
    make_static_outline,
    math::{I16Dot16, I26Dot6, Outline, OutlineIterExt as _, Point2, Rect2, StaticOutline, Vec2},
};

use super::{FaceImpl, FontImpl, FontMetrics, GlyphMetrics, SingleGlyphBitmap};
use crate::{
    layout::{FixedL, Vec2L},
    text::FontSizeCacheKey,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Face;

impl FaceImpl for Face {
    type Font = Font;

    fn family_name(&self) -> &str {
        "Subrandr Tofu Font"
    }

    fn addr(&self) -> usize {
        // This is an unaligned address that should never occur
        // in any actually real font backends.
        1
    }

    fn axes(&self) -> &[super::Axis] {
        &[]
    }

    fn set_axis(&mut self, _index: usize, _value: I16Dot16) {
        panic!("Cannot set builtin tofu font axis values")
    }

    fn weight(&self) -> I16Dot16 {
        I16Dot16::new(400)
    }

    fn italic(&self) -> bool {
        false
    }

    fn contains_codepoint(&self, _codepoint: u32) -> bool {
        true
    }

    type Error = Infallible;
    fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Self::Font, Self::Error> {
        Ok(Font::create(point_size, dpi))
    }
}

static TOFU_HB_FONT_USERDATA_KEY: hb_user_data_key_t = hb_user_data_key_t { unused: 104 };

struct FontShared {
    point_size: I26Dot6,
    dpi: u32,
    glyph_metrics: GlyphMetrics,
    font_metrics: FontMetrics,
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
            let point_height = point_size * dpi_scale;
            let pixel_height = point_height + point_height / 3;
            let pixel_width = pixel_height * 2 / 3;
            let ascender = pixel_height * 5 / 6;
            let descender = ascender - pixel_height;
            FontShared {
                point_size,
                dpi,
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
                Font::hb_font_h_extents_func(extents: *mut hb_font_extents_t) -> hb_bool_t
            );

            set!(
                hb_font_funcs_set_nominal_glyph_func,
                Font::hb_nominal_glyph_func(unicode: hb_codepoint_t, glyph: *mut hb_codepoint_t) -> hb_bool_t
            );

            set!(
                hb_font_funcs_set_variation_glyph_func,
                Font::hb_variation_glyph_func(
                    unicode: hb_codepoint_t,
                    variation_selector: hb_codepoint_t,
                    glyph: *mut hb_codepoint_t
                ) -> hb_bool_t
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
            hb_face_destroy(face);

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
        1
    }

    unsafe fn hb_nominal_glyph_func(
        _shared: &FontShared,
        unicode: hb_codepoint_t,
        glyph: *mut hb_codepoint_t,
    ) -> i32 {
        // NOTE: A zero glyph is treated as not found so we convert NUL into u32::MAX
        //       which should be outside the unicode range anyway.
        glyph.write(if unicode == 0 { u32::MAX } else { unicode });
        1
    }

    unsafe fn hb_variation_glyph_func(
        shared: &FontShared,
        unicode: hb_codepoint_t,
        _variation_selector: hb_codepoint_t,
        glyph: *mut hb_codepoint_t,
    ) -> i32 {
        Self::hb_nominal_glyph_func(shared, unicode, glyph)
    }

    unsafe fn hb_glyph_h_advance_func(
        shared: &FontShared,
        _glyph: hb_codepoint_t,
    ) -> hb_position_t {
        shared.glyph_metrics.hori_advance.into_raw()
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
        out.width = shared.glyph_metrics.width.into_raw();
        out.height = shared.glyph_metrics.height.into_raw();
        out.x_bearing = shared.glyph_metrics.hori_bearing_x.into_raw();
        out.y_bearing = shared.glyph_metrics.hori_bearing_y.into_raw();
        1
    }

    fn shared(&self) -> &FontShared {
        unsafe {
            &*hb_font_get_user_data(self.hb, (&raw const TOFU_HB_FONT_USERDATA_KEY).cast_mut())
                .cast::<FontShared>()
        }
    }

    pub fn as_harfbuzz_font(&self) -> *mut hb_font_t {
        self.hb
    }
}

pub struct TofuGlyph {
    texture_offset: Vec2<i32>,
    texture_size: Vec2<u32>,
    strips: Strips,
}

impl Font {
    fn draw_glyph(&self, index: u32, offset: Vec2L, rasterizer: &mut StripRasterizer) -> TofuGlyph {
        let shared = self.shared();
        let pixel_width = shared.glyph_metrics.width;
        let pixel_height = shared.glyph_metrics.height;
        let outline_width = pixel_height / 40;
        let base_spacing = pixel_height / 12;
        let margin = base_spacing;
        let inner_offset = offset + Vec2::new(margin, margin);
        let fract_offset = Vec2::new(inner_offset.x.fract(), inner_offset.y.fract());

        let outline_size = Vec2::new(pixel_width - margin * 2, pixel_height - margin * 2);
        let texture_size = Vec2::new(
            (outline_size.x + fract_offset.x).ceil_to_inner() as u32,
            (outline_size.y + fract_offset.y).ceil_to_inner() as u32,
        );

        {
            let outline_outer = Rect2::from_min_size(fract_offset.to_point(), outline_size);

            rasterizer.add_polyline(&Rect2::to_float(outline_outer).to_points());

            let mut outline_inner = outline_outer;
            outline_inner.expand(-outline_width, -outline_width);
            let mut inner_points = Rect2::to_float(outline_inner).to_points();
            inner_points.reverse();

            rasterizer.add_polyline(&inner_points);
        }

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

        let char_space_x = (content_size.x - (min_cell_spacing_x * i32::from(max_cols - 1)))
            / i32::from(max_cols + 1);
        let char_space_y =
            (content_size.y - min_cell_spacing_y * (i32::from(rows - 1))) / i32::from(rows + 1);

        let (cell_size_x, cell_size_y) = if char_space_x / 2 < char_space_y / 3 {
            (char_space_x, char_space_x / 2 * 3)
        } else {
            (char_space_y * 2 / 3, char_space_y)
        };

        let mut draw_digit = |offset: Vec2L, size: Vec2L, digit: u8| {
            rasterizer.add_outline(&mut GLYPHS[usize::from(digit)].iter().map_points(|p| {
                Point2::new(
                    offset.x.into_f32() + p.x * size.x.into_f32() / 200.,
                    offset.y.into_f32() + p.y * size.y.into_f32() / 400.,
                )
            }));
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
                let justify_spacing_x =
                    (content_size.x - cell_size_x * i32::from(row)) / i32::from(row + 1);

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

        TofuGlyph {
            texture_offset: Vec2::new(
                inner_offset.x.floor_to_inner(),
                inner_offset.y.floor_to_inner(),
            ),
            texture_size,
            strips: rasterizer.rasterize(),
        }
    }
}

impl FontImpl for Font {
    type Face = Face;

    fn face(&self) -> &Self::Face {
        &Face
    }

    fn metrics(&self) -> &FontMetrics {
        &self.shared().font_metrics
    }

    fn point_size(&self) -> I26Dot6 {
        self.shared().point_size
    }

    fn harfbuzz_scale_factor_for(&self, _glyph: u32) -> I26Dot6 {
        I26Dot6::ONE
    }

    fn size_cache_key(&self) -> FontSizeCacheKey {
        FontSizeCacheKey::new(
            self.point_size(),
            self.shared().dpi,
            [I16Dot16::ZERO; text_sys::T1_MAX_MM_AXIS as usize],
        )
    }

    type RenderError = Infallible;
    fn render_glyph_uncached(
        &self,
        rasterizer: &mut dyn Rasterizer,
        index: u32,
        offset: Vec2L,
    ) -> Result<SingleGlyphBitmap, Self::RenderError> {
        let TofuGlyph {
            texture_offset,
            texture_size,
            strips,
        } = self.draw_glyph(index, offset, &mut StripRasterizer::new());
        let texture = unsafe {
            rasterizer.create_packed_texture_mapped(
                texture_size,
                PixelFormat::Mono,
                Box::new(|mut target| {
                    target.buffer_mut().fill(MaybeUninit::new(0));

                    strips.blend_to(target, |out, value| {
                        out.write(value);
                    });
                }),
            )
        };

        Ok(SingleGlyphBitmap {
            offset: texture_offset,
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

// All glyphs that make up our mini tofu font on a 200x400 canvas.
// This includes all hex digits and a special question mark glyph.
// TODO: Investigate using curves in this font. Would require some non-trivial manual stroking...
static GLYPHS: [StaticOutline<f32>; 17] = [
    // 0
    make_static_outline! {
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (40, 40);
        line_to (40, 360);
        line_to (160, 360);
        line_to (160, 40);

        #move_to (80, 170);
        line_to (80, 230);
        line_to (120, 230);
        line_to (120, 170);
    },
    // 1
    make_static_outline! {
        #move_to (40, 0);
        line_to (120, 0);
        line_to (120, 360);

        line_to (180, 360);
        line_to (180, 400);
        line_to (20, 400);
        line_to (20, 360);

        line_to (90, 360);
        line_to (90, 40);
        line_to (30, 40);
    },
    // 2
    make_static_outline! {
        #move_to (30, 40);
        line_to (30, 70);
        line_to (0, 70);
        line_to (0, 0);
        line_to (200, 0);
        line_to (200, 220);
        line_to (40, 220);
        line_to (40, 360);
        line_to (200, 360);
        line_to (200, 400);
        line_to (0, 400);
        line_to (0, 180);
        line_to (160, 180);
        line_to (160, 40);
    },
    // 3
    make_static_outline! {
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);

        line_to (0, 400);
        line_to (0, 360);
        line_to (160, 360);

        line_to (160, 220);
        line_to (0, 220);
        line_to (0, 180);
        line_to (160, 180);

        line_to (160, 40);
        line_to (0, 40);
    },
    // 4
    make_static_outline! {
        #move_to (40, 0);
        line_to (40, 180);
        line_to (160, 180);
        line_to (160, 0);
        line_to (200, 0);
        line_to (200, 400);

        line_to (160, 400);
        line_to (160, 360);
        line_to (160, 220);
        line_to (0, 220);
        line_to (0, 0);
    },
    // 5 (pretty much a flipped 2)
    make_static_outline! {
        #move_to (200, 40);
        line_to (200, 0);
        line_to (0, 0);
        line_to (0, 220);
        line_to (160, 220);
        line_to (160, 360);
        line_to (0, 360);
        line_to (0, 400);
        line_to (200, 400);
        line_to (200, 180);
        line_to (40, 180);
        line_to (40, 40);
    },
    // 6
    make_static_outline! {
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 40);
        line_to (40, 40);

        line_to (40, 180);
        line_to (200, 180);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (40, 220);
        line_to (40, 360);
        line_to (160, 360);
        line_to (160, 220);
    },
    // 7
    make_static_outline! {
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 40);
        line_to (80, 400);
        line_to (20, 400);
        line_to (160, 40);
        line_to (0, 40);
    },
    // 8
    make_static_outline! {
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (40, 40);
        line_to (40, 180);
        line_to (160, 180);
        line_to (160, 40);

        #move_to (40, 220);
        line_to (40, 360);
        line_to (160, 360);
        line_to (160, 220);
    },
    // 9 (xy flipped 6)
    make_static_outline! {
        #move_to (200, 400);
        line_to (0, 400);
        line_to (0, 360);
        line_to (160, 360);

        line_to (160, 220);
        line_to (0, 220);
        line_to (0, 0);
        line_to (200, 0);

        #move_to (160, 180);
        line_to (160, 40);
        line_to (40, 40);
        line_to (40, 180);
    },
    // A
    make_static_outline![
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (40, 40);
        line_to (40, 180);
        line_to (160, 180);
        line_to (160, 40);

        #move_to (40, 220);
        line_to (40, 400);
        line_to (160, 400);
        line_to (160, 220);
    ],
    // B (a slightly different 8)
    make_static_outline![
        #move_to (0, 0);
        line_to (180, 0);
        line_to (180, 180);
        line_to (200, 180);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (40, 40);
        line_to (40, 180);
        line_to (140, 180);
        line_to (140, 40);

        #move_to (40, 220);
        line_to (40, 360);
        line_to (160, 360);
        line_to (160, 220);
    ],
    // C
    make_static_outline![
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (200, 360);
        line_to (200, 40);
        line_to (40, 40);
        line_to (40, 360);
    ],
    // D
    make_static_outline![
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (160, 360);
        line_to (160, 40);
        line_to (40, 40);
        line_to (40, 360);

        #move_to (200, 0);
        line_to (160, 0);
        line_to (160, 40);
        line_to (200, 40);

        #move_to (200, 360);
        line_to (160, 360);
        line_to (160, 400);
        line_to (200, 400);
    ],
    // E
    make_static_outline![
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (200, 180);
        line_to (200, 40);
        line_to (40, 40);
        line_to (40, 180);

        #move_to (200, 360);
        line_to (200, 220);
        line_to (40, 220);
        line_to (40, 360);

        #move_to (200, 220);
        line_to (200, 180);
        line_to (160, 180);
        line_to (160, 220);
    ],
    // F
    make_static_outline![
        #move_to (0, 0);
        line_to (200, 0);
        line_to (200, 400);
        line_to (0, 400);

        #move_to (200, 180);
        line_to (200, 40);
        line_to (40, 40);
        line_to (40, 180);

        #move_to (200, 400);
        line_to (200, 220);
        line_to (40, 220);
        line_to (40, 400);

        #move_to (200, 220);
        line_to (200, 180);
        line_to (160, 180);
        line_to (160, 220);
    ],
    // ? (drawn in full content box if not enough space is available for hex)
    make_static_outline![
        #move_to (10, 120);
        line_to (50, 120);
        line_to (50, 80);
        line_to (10, 80);

        #move_to (50, 40);
        line_to (50, 80);
        line_to (150, 80);
        line_to (150, 40);

        #move_to (150, 160);
        line_to (190, 160);
        line_to (190, 80);
        line_to (150, 80);

        #move_to (150, 160);
        line_to (80, 220);
        line_to (80, 280);
        line_to (120, 280);
        line_to (120, 220);
        line_to (190, 160);

        #move_to (80, 320);
        line_to (80, 360);
        line_to (120, 360);
        line_to (120, 320);
    ],
];
