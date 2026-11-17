use std::{hash::Hash, sync::Arc};

use rasterize::scene::{Rotation, SceneBuilder, SceneColor, SceneContentBuilder, SubsceneKind};
use util::{
    make_static_outline,
    math::{I16Dot16, I26Dot6, Outline, OutlineIterExt as _, Point2, Rect2, StaticOutline, Vec2},
};

use super::FontMetrics;
use crate::{
    layout::{FixedL, Vec2L},
    text::{BaselineMetrics, FontSizeKey, GlyphSubscene},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Face;

impl Face {
    pub(super) fn family_name(&self) -> &str {
        "Subrandr Tofu"
    }

    pub(super) fn axes(&self) -> &[super::Axis] {
        &[]
    }

    pub(super) fn set_axis(&mut self, _index: usize, _value: I16Dot16) {
        panic!("Cannot set builtin tofu font axis values")
    }

    pub(super) fn weight(&self) -> I16Dot16 {
        I16Dot16::new(400)
    }

    pub(super) fn italic(&self) -> bool {
        false
    }

    pub(super) fn contains_codepoint(&self, _codepoint: u32) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::text) struct GlyphMetrics {
    pub width: I26Dot6,
    pub height: I26Dot6,
    pub hori_y_offset: I26Dot6,
    pub hori_advance: I26Dot6,
    pub vert_advance: I26Dot6,
}

impl GlyphMetrics {
    pub(in crate::text) fn hori_x_offset(&self) -> I26Dot6 {
        I26Dot6::ZERO
    }

    pub(in crate::text) fn vert_x_offset(&self) -> I26Dot6 {
        -(self.width / 2)
    }

    pub(in crate::text) fn vert_y_offset(&self) -> I26Dot6 {
        -self.height
    }
}

#[derive(Debug)]
struct FontShared {
    point_size: I26Dot6,
    dpi: u32,
    glyph_metrics: GlyphMetrics,
    font_metrics: FontMetrics,
}

#[derive(Debug, Clone)]
pub struct Font(Arc<FontShared>);

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Font {}

impl Hash for Font {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl Font {
    pub fn create(point_size: I26Dot6, dpi: u32) -> Self {
        let dpi_scale = I16Dot16::from_quotient(dpi as i32, 72);
        let dpi_scale6 = I26Dot6::from_raw(dpi_scale.into_raw() >> 10);
        let decoration_thickness = point_size * dpi_scale6 / 192;
        let point_height = point_size * dpi_scale;
        let pixel_height = (point_height + point_height * 3 / 11).ceil();
        let pixel_width = (pixel_height * 2 / 3).ceil();
        let ascender = pixel_height * 9 / 11;
        let descender = ascender - pixel_height;

        Self(Arc::new(FontShared {
            point_size,
            dpi,
            glyph_metrics: GlyphMetrics {
                width: pixel_width,
                height: pixel_height,
                hori_y_offset: ascender,
                hori_advance: pixel_width,
                vert_advance: pixel_height,
            },
            font_metrics: FontMetrics {
                alphabetic_baseline: BaselineMetrics {
                    ascender,
                    descender,
                },
                horiz_height: ascender - descender,
                central_baseline: BaselineMetrics {
                    ascender: pixel_width / 2,
                    descender: (pixel_width / 2) - pixel_width,
                },

                underline_top_offset: (descender - decoration_thickness) / 2,
                underline_thickness: decoration_thickness,
                strikeout_top_offset: (ascender - descender) / 2
                    - ascender
                    - decoration_thickness / 2,
                strikeout_thickness: decoration_thickness,
            },
        }))
    }

    pub(in crate::text) fn glyph_metrics(&self) -> &GlyphMetrics {
        &self.0.glyph_metrics
    }
}

impl Font {
    fn build_glyph_outline(&self, mut output: SceneContentBuilder, index: u32) {
        let mut outline = Vec::new();

        let pixel_width = self.0.glyph_metrics.width;
        let pixel_height = self.0.glyph_metrics.height;
        let outline_width = (pixel_height / 40).floor().max(I26Dot6::ONE);
        let base_spacing = (pixel_height / 12).round();
        let margin = base_spacing;
        let inner_offset = Vec2::new(margin, margin);
        let outline_size = Vec2::new(pixel_width - margin * 2, pixel_height - margin * 2);

        {
            let outline_outer = Rect2::from_min_size(inner_offset.to_point(), outline_size);

            outline.extend(&Rect2::to_float(outline_outer).to_outline(false));

            let mut outline_inner = outline_outer;
            outline_inner.expand(-outline_width, -outline_width);

            outline.extend(Rect2::to_float(outline_inner).to_outline(true));
        }

        let content_offset = inner_offset + Vec2::splat(outline_width);
        let content_size = outline_size - Vec2::splat(outline_width * 2);
        let min_cell_spacing_x = base_spacing / 2;
        let min_cell_spacing_y = base_spacing;

        let mut digits_buf = [0u8; 8];
        let num_chars = {
            let mut value = index;
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
            outline.extend(GLYPHS[usize::from(digit)].iter().map_points(|p| {
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

        output.filled_outline(outline.iter().copied(), SceneColor::ACTIVE);
    }

    pub(in crate::text) fn metrics(&self) -> &FontMetrics {
        &self.0.font_metrics
    }

    pub(super) fn point_size(&self) -> I26Dot6 {
        self.0.point_size
    }

    pub(super) fn size_cache_key(&self) -> FontSizeKey {
        FontSizeKey::new(
            self.point_size(),
            self.0.dpi,
            [I16Dot16::ZERO; text_sys::T1_MAX_MM_AXIS as usize],
        )
    }

    pub(super) fn glyph_subscene_uncached(
        &self,
        index: u32,
        rotation: Rotation,
        _rasterizer: &mut dyn rasterize::Rasterizer,
    ) -> GlyphSubscene {
        let mut builder = SceneBuilder::new();
        self.build_glyph_outline(
            {
                let mut root = builder.root();
                root.apply_rotation(rotation);
                root
            },
            index,
        );
        GlyphSubscene(SubsceneKind::Scene(builder.finish()))
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
