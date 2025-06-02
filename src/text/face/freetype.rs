use std::{
    ffi::{c_int, c_void},
    hash::Hash,
    mem::MaybeUninit,
    num::NonZero,
    path::Path,
    rc::Rc,
    sync::Arc,
};

use text_sys::*;
use thiserror::Error;
use ttf_parser::{LineMetrics, NormalizedCoordinate};

use super::{
    Axis, FaceImpl, FontImpl, FontMetrics, GlyphCache, GlyphMetrics, SingleGlyphBitmap,
    ITALIC_AXIS, WEIGHT_AXIS,
};
use crate::{
    math::{I16Dot16, I26Dot6, Point2f, Vec2},
    outline::{Outline, OutlineBuilder, SegmentDegree},
    rasterize::Rasterizer,
    text::ft_utils::*,
    util::slice_assume_init_mut,
};

struct FaceData {
    parser: ttf_parser::Face<'static>,
    data: Arc<[u8]>,
    index: u32,

    metrics: FaceMetrics,
    axes: Vec<Axis>,
    default_coordinates: Vec<NormalizedCoordinate>,

    name: Box<str>,
    os2_weight: Option<u16>,
    is_bold: bool,
    is_italic: bool,

    glyph_cache: GlyphCache<Font>,
}

impl FaceData {
    fn parser(&self) -> &ttf_parser::Face<'_> {
        &self.parser
    }
}

// Unscaled font metrics
struct FaceMetrics {
    ascender: i32,
    descender: i32,
    height: i32,

    underline: Option<LineMetrics>,
    strikeout: Option<LineMetrics>,
}

impl FaceMetrics {
    fn get(face: &ttf_parser::Face<'_>) -> FaceMetrics {
        let ascender = i32::from(face.ascender());
        let descender = i32::from(face.descender());
        let height = ascender - descender + i32::from(face.line_gap());

        FaceMetrics {
            ascender,
            descender,
            height,
            underline: face.underline_metrics(),
            strikeout: face.strikeout_metrics(),
        }
    }
}

impl FaceData {
    pub fn new(
        data: Arc<[u8]>,
        mut index: u32,
    ) -> Result<(Self, Vec<NormalizedCoordinate>), ttf_parser::FaceParsingError> {
        // TODO: PR named instance support to ttf-parser
        index &= 0xFFFF;
        let _named_instance = index >> 16;
        let font_index = index & 0xFFFF;

        let face = ttf_parser::Face::parse(&data, font_index)?;

        let mut axes = Vec::new();
        let mut coordinates = Vec::new();
        for (index, axis) in face.variation_axes().into_iter().enumerate() {
            coordinates.push(NormalizedCoordinate::from(axis.def_value));

            axes.push(Axis {
                tag: axis.tag,
                minimum: I16Dot16::from_f32(axis.min_value),
                maximum: I16Dot16::from_f32(axis.max_value),
                index,
            });
        }

        Ok((
            Self {
                metrics: FaceMetrics::get(&face),
                axes,
                default_coordinates: coordinates.clone(),

                name: face
                    .names()
                    .into_iter()
                    .find_map(|name| name.to_string())
                    .unwrap()
                    .into_boxed_str(),
                os2_weight: face.tables().os2.map(|t| t.weight().to_number()),
                is_bold: face.is_bold(),
                is_italic: face.is_italic(),

                glyph_cache: GlyphCache::new(),

                parser: unsafe { std::mem::transmute(face) },
                data,
                index,
            },
            coordinates,
        ))
    }
}

#[repr(C)]
#[derive(Clone)]
pub(super) struct Face {
    face: Rc<FaceData>,
    coords: Vec<NormalizedCoordinate>,
}

impl PartialEq for Face {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.face, &other.face) && self.coords == other.coords
    }
}

impl Eq for Face {}

impl Hash for Face {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.face).hash(state);
        for coord in &self.coords {
            coord.get().hash(state);
        }
    }
}

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>, index: i32) -> Result<Self, FreeTypeError> {
        Ok(Self::load_from_bytes(std::fs::read(path).unwrap().into(), index).unwrap())
    }

    pub fn load_from_bytes(bytes: Arc<[u8]>, index: i32) -> Result<Self, FreeTypeError> {
        let (face, coords) = FaceData::new(bytes, index as u32).unwrap();
        Ok(Self {
            face: Rc::new(face),
            coords,
        })
    }

    pub fn glyph_cache(&self) -> &GlyphCache<Font> {
        &self.face.glyph_cache
    }
}

impl FaceImpl for Face {
    type Font = Font;

    fn family_name(&self) -> &str {
        &self.face.name
    }

    fn axes(&self) -> &[Axis] {
        &self.face.axes
    }

    fn set_axis(&mut self, index: usize, value: I16Dot16) {
        assert!(self.face.axes[index].is_value_in_range(value));
        self.coords[index] = NormalizedCoordinate::from(value.into_raw() as i16 >> 2);
    }

    fn weight(&self) -> I16Dot16 {
        self.face
            .axes
            .iter()
            .find_map(|x| (x.tag == WEIGHT_AXIS).then_some(x.index))
            .map_or_else(
                || {
                    if let Some(weight) = self.face.os2_weight {
                        I16Dot16::new(weight as i32)
                    } else {
                        let has_bold_flag = self.face.is_bold;

                        I16Dot16::new(300 + 400 * has_bold_flag as i32)
                    }
                },
                |idx| I16Dot16::from_raw(i32::from(self.coords[idx].get()) << 2),
            )
    }

    fn italic(&self) -> bool {
        self.face
            .axes
            .iter()
            .find_map(|x| (x.tag == ITALIC_AXIS).then_some(x.index))
            .map_or_else(
                || self.face.is_italic,
                |idx| I16Dot16::from_raw(i32::from(self.coords[idx].get()) << 2) > I16Dot16::HALF,
            )
    }

    type Error = FreeTypeError;
    fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Font, FreeTypeError> {
        Font::create(self.face.clone(), self.coords.clone(), point_size, dpi)
    }
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Face({:?}@{:?}, ",
            self.family_name(),
            Rc::as_ptr(&self.face)
        )?;

        if self.italic() {
            write!(f, "italic, ")?;
        }
        let weight = self.weight();
        if weight != I16Dot16::new(400) {
            write!(f, "w{weight}, ")?;
        }

        f.debug_map()
            .entries(self.axes().iter().enumerate().map(|(i, axis)| {
                (
                    axis.tag,
                    I16Dot16::from_raw(i32::from(self.coords[i].get()) << 2),
                )
            }))
            .finish()?;
        write!(f, ")")
    }
}

struct FontScale {
    point_size: I26Dot6,
    dpi: u32,
    ppem: I26Dot6,
    units_per_em: u16,
}

impl FontScale {
    fn new(point_size: I26Dot6, dpi: u32, units_per_em: u16) -> Self {
        let ppem = point_size * 96 / dpi as i32;
        Self {
            point_size,
            dpi,
            ppem,
            units_per_em,
        }
    }

    fn to_pixels(&self, value: impl Into<i32>) -> I26Dot6 {
        self.ppem * value.into() / i32::from(self.units_per_em)
    }

    fn default_decoration_thickness(&self) -> I26Dot6 {
        // magic number
        self.point_size * self.dpi as i32 / 13824
    }
}

struct FontData {
    face: Face,
    scale: FontScale,
    metrics: FontMetrics,
    harfbuzz: *mut hb_font_t,
}

unsafe impl Sync for FontData {}
unsafe impl Send for FontData {}

impl FaceMetrics {
    fn scale(&self, scale: &FontScale) -> FontMetrics {
        let default_decoration_thickness = scale.default_decoration_thickness();
        let ascender = scale.to_pixels(self.ascender);
        let descender = scale.to_pixels(self.descender);
        let height = scale.to_pixels(self.height);

        let (strikeout_top_offset, strikeout_thickness) = self.strikeout.map_or_else(
            || {
                (
                    (ascender - descender) / 2 - ascender - default_decoration_thickness / 2,
                    default_decoration_thickness,
                )
            },
            |line| {
                (
                    scale.to_pixels(-line.position),
                    scale.to_pixels(line.thickness),
                )
            },
        );

        let (underline_top_offset, underline_thickness) = self.underline.map_or_else(
            || {
                (
                    (descender - default_decoration_thickness) / 2,
                    default_decoration_thickness,
                )
            },
            |line| {
                (
                    scale.to_pixels(-line.position),
                    scale.to_pixels(line.thickness),
                )
            },
        );

        FontMetrics {
            ascender,
            descender,
            height,
            strikeout_top_offset,
            strikeout_thickness,
            underline_top_offset,
            underline_thickness,
        }
    }
}

impl FaceData {
    fn data_hb_blob(&self) -> *mut hb_blob_t {
        unsafe {
            unsafe extern "C" fn destroy(user_data: *mut c_void) {
                Arc::decrement_strong_count(Arc::as_ptr(&(*user_data.cast::<FaceData>()).data))
            }

            hb_blob_create(
                self.data.as_ptr() as *const i8,
                self.data.len() as u32,
                HB_MEMORY_MODE_READONLY,
                self as *const _ as *mut c_void,
                Some(destroy),
            )
        }
    }
}

#[repr(C)]
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Font {
    data: Rc<FontData>,
}

impl Font {
    fn create(
        face: Rc<FaceData>,
        coords: Vec<NormalizedCoordinate>,
        point_size: I26Dot6,
        dpi: u32,
    ) -> Result<Self, FreeTypeError> {
        let scale = FontScale::new(point_size, dpi, face.parser().units_per_em());
        let metrics = face.metrics.scale(&scale);

        let blob = face.data_hb_blob();
        let hb_face = unsafe { hb_face_create(blob, face.index) };
        let hb_font = unsafe { hb_font_create(hb_face) };
        unsafe {
            hb_font_set_ptem(hb_font, point_size.into_f32());
            hb_font_set_ppem(
                hb_font,
                scale.ppem.into_raw() as u32,
                scale.ppem.into_raw() as u32,
            );
            hb_face_destroy(hb_face);
        }

        Ok(Self {
            data: Rc::new(FontData {
                face: Face { face, coords },
                scale,
                metrics,
                harfbuzz: hb_font,
            }),
        })
    }

    pub fn as_harfbuzz_font(&self) -> *mut hb_font_t {
        self.data.harfbuzz
    }

    /// Gets the Outline associated with the glyph at `index`.
    ///
    /// Returns [`None`] if the glyph does not exist in this font, or it is not
    /// an outline glyph.
    #[expect(dead_code)]
    pub fn glyph_outline(&self, index: u32) -> Result<Option<Outline>, FreeTypeError> {
        let mut builder = OutlineBuilder::new();
        Ok(self
            .data
            .face
            .face
            .parser
            .outline_glyph(ttf_parser::GlyphId(index as u16), &mut builder)
            .map(|_| builder.build()))
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("face", self.face())
            .field("point_size", &self.point_size())
            .field("dpi", &self.data.scale.dpi)
            .finish()
    }
}

impl Hash for FontData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.face.hash(state);
        self.scale.point_size.hash(state);
        self.scale.dpi.hash(state);
    }
}

impl PartialEq for FontData {
    fn eq(&self, other: &Self) -> bool {
        self.face == other.face
            && self.scale.point_size == other.scale.point_size
            && self.scale.dpi == other.scale.dpi
    }
}

impl Eq for FontData {}

#[derive(Debug, Error)]
pub enum GlyphRenderError {
    #[error(transparent)]
    FreeType(#[from] FreeTypeError),
    #[error("Unsupported glyph format {0} after conversion")]
    ConversionToBitmapFailed(FT_Glyph_Format),
    #[error("Unsupported pixel mode {0}")]
    UnsupportedBitmapFormat(std::ffi::c_uchar),
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) struct SizeInfo {
    coords: Vec<i16>,
    point_size: I26Dot6,
    dpi: u32,
}

impl FontImpl for Font {
    type Face = Face;

    fn face(&self) -> &Self::Face {
        &self.data.face
    }

    fn metrics(&self) -> &FontMetrics {
        &self.data.metrics
    }

    fn point_size(&self) -> I26Dot6 {
        self.data.scale.point_size
    }

    type FontSizeKey = SizeInfo;
    fn font_size_key(&self) -> Self::FontSizeKey {
        Self::FontSizeKey {
            coords: self.data.face.coords.iter().map(|x| x.get()).collect(),
            point_size: self.point_size(),
            dpi: self.data.scale.dpi,
        }
    }

    fn glyph_cache(&self) -> &GlyphCache<Self> {
        self.face().glyph_cache()
    }

    type MeasureError = FreeTypeError;
    fn measure_glyph_uncached(&self, index: u32) -> Result<GlyphMetrics, Self::MeasureError> {
        let parser = self.data.face.face.parser();
        let glyph = ttf_parser::GlyphId(index as u16);

        dbg!(glyph, self.data.face.face.parser().number_of_glyphs());
        let rect = parser
            .glyph_bounding_box(glyph)
            .map(|r| (i32::from(r.width()), i32::from(r.height())))
            .or_else(|| {
                parser
                    .glyph_raster_image(glyph, self.data.scale.ppem.round_to_inner() as u16)
                    .map(|i| (i32::from(i.width), i32::from(i.height)))
            })
            .unwrap_or((0, 0));
        let hori_advance = parser.glyph_hor_advance(glyph).unwrap();
        let vert_advance = parser.glyph_ver_advance(glyph).unwrap_or(0);
        let hori_bearing_x = parser.glyph_hor_side_bearing(glyph).unwrap();
        let vert_bearing_x = parser.glyph_ver_side_bearing(glyph).unwrap_or(0);
        let bearing_y = parser.glyph_y_origin(glyph).unwrap_or(0);

        Ok(GlyphMetrics {
            width: I26Dot6::new(rect.0),
            height: I26Dot6::new(rect.1),
            hori_bearing_x: self.data.scale.to_pixels(hori_bearing_x),
            hori_bearing_y: self.data.scale.to_pixels(bearing_y),
            hori_advance: self.data.scale.to_pixels(hori_advance),
            vert_bearing_x: self.data.scale.to_pixels(vert_bearing_x),
            vert_bearing_y: self.data.scale.to_pixels(bearing_y),
            vert_advance: self.data.scale.to_pixels(vert_advance),
        })
    }

    type RenderError = GlyphRenderError;
    fn render_glyph_uncached(
        &self,
        rasterizer: &mut dyn Rasterizer,
        index: u32,
        offset: Vec2<I26Dot6>,
    ) -> Result<SingleGlyphBitmap, Self::RenderError> {
        let outline = self.glyph_outline(index)?.unwrap();
        let bbox = outline.control_box();
        let width = (bbox.max.x.ceil() - bbox.min.x.floor()).ceil() as u32;
        let height = (bbox.max.y.ceil() - bbox.min.y.floor()).ceil() as u32;
        let texture = unsafe {
            rasterizer.create_packed_texture_mapped(
                width,
                height,
                crate::rasterize::PixelFormat::Mono,
                Box::new(|data, stride| {
                    data.fill(MaybeUninit::new(0));
                    let data = unsafe { slice_assume_init_mut(data) };
                    struct Userdata {
                        ptr: *mut u8,
                        height: u32,
                        stride: usize,
                    }

                    unsafe extern "C" fn fill_span(
                        y: c_int,
                        count: c_int,
                        spans: *const FT_Span,
                        user: *mut c_void,
                    ) {
                        unsafe {
                            let ud = &mut *user.cast::<Userdata>();
                            let row_ptr = ud.ptr.add(y as usize * ud.stride);
                            for span in std::slice::from_raw_parts(spans, count as usize) {
                                row_ptr
                                    .add(span.x as usize)
                                    .write_bytes(span.coverage, usize::from(span.len));
                            }
                        }
                    }

                    let mut user = Userdata {
                        ptr: data.as_mut_ptr(),
                        height,
                        stride,
                    };
                    let mut ft_outline = outline.to_freetype();
                    let library = Library::get_or_init().unwrap();
                    fttry!(FT_Outline_Render(
                        library.ptr,
                        &mut ft_outline,
                        &mut FT_Raster_Params {
                            target: std::ptr::null(),
                            source: &ft_outline as *const _ as *const c_void,
                            flags: FT_RASTER_FLAG_AA as i32 | FT_RASTER_FLAG_DIRECT as i32,
                            gray_spans: Some(fill_span),
                            black_spans: None,
                            bit_test: None,
                            bit_set: None,
                            user: &mut user as *mut _ as *mut c_void,
                            clip_box: FT_BBox_ {
                                xMin: bbox.min.x.floor() as i64,
                                yMin: bbox.min.y.floor() as i64,
                                xMax: bbox.max.x.ceil() as i64,
                                yMax: bbox.max.x.ceil() as i64,
                            },
                        },
                    ))
                    .unwrap();
                }),
            )
        };

        // struct FtGlyph(FT_Glyph);
        // impl Drop for FtGlyph {
        //     fn drop(&mut self) {
        //         unsafe {
        //             FT_Done_Glyph(self.0);
        //         }
        //     }
        // }

        // unsafe {
        //     let face = self.with_applied_size()?;

        //     fttry!(FT_Load_Glyph(face, index, FT_LOAD_COLOR as i32))?;
        //     let is_bitmap;
        //     let glyph = {
        //         let slot = (*face).glyph;
        //         let mut glyph = {
        //             let mut glyph = MaybeUninit::uninit();
        //             fttry!(FT_Get_Glyph(slot, glyph.as_mut_ptr()))?;
        //             FtGlyph(glyph.assume_init())
        //         };

        //         is_bitmap = (*glyph.0).format == FT_GLYPH_FORMAT_BITMAP;

        //         if !is_bitmap {
        //             fttry!(FT_Glyph_To_Bitmap(
        //                 &mut glyph.0,
        //                 FT_RENDER_MODE_NORMAL,
        //                 &FT_Vector {
        //                     x: offset.x.into_ft(),
        //                     y: offset.y.into_ft()
        //                 },
        //                 1
        //             ))?;
        //         }

        //         glyph
        //     };

        //     let scale = if is_bitmap {
        //         self.size.bitmap_scale
        //     } else {
        //         I26Dot6::ONE
        //     };
        //     let scale6 = scale.into_raw();

        //     // I don't think this can happen but let's be safe
        //     if (*glyph.0).format != FT_GLYPH_FORMAT_BITMAP {
        //         return Err(GlyphRenderError::ConversionToBitmapFailed(
        //             (*glyph.0).format,
        //         ));
        //     }

        //     let bitmap_glyph = glyph.0.cast::<FT_BitmapGlyphRec>();
        //     let (ox, oy) = (
        //         I26Dot6::from_raw((*bitmap_glyph).left * scale6),
        //         I26Dot6::from_raw(-(*bitmap_glyph).top * scale6),
        //     );

        //     let bitmap = &(*bitmap_glyph).bitmap;

        //     let scaled_width = (bitmap.width * scale6 as u32) >> 6;
        //     let scaled_height = (bitmap.rows * scale6 as u32) >> 6;

        //     let pixel_mode = match bitmap.pixel_mode.into() {
        //         FT_PIXEL_MODE_GRAY => CopyPixelMode::Mono8,
        //         FT_PIXEL_MODE_BGRA => CopyPixelMode::Bgra32,
        //         _ => return Err(GlyphRenderError::UnsupportedBitmapFormat(bitmap.pixel_mode)),
        //     };

        //     let texture = rasterizer.create_packed_texture_mapped(
        //         scaled_width,
        //         scaled_height,
        //         if matches!(pixel_mode, CopyPixelMode::Bgra32) {
        //             PixelFormat::Bgra
        //         } else {
        //             PixelFormat::Mono
        //         },
        //         Box::new(|buffer_data, stride| {
        //             macro_rules! copy_font_bitmap_with {
        //                 ($pixel_mode: expr) => {
        //                     copy_font_bitmap::<$pixel_mode>(
        //                         bitmap.buffer.cast_const(),
        //                         bitmap.pitch as isize,
        //                         bitmap.width as u32,
        //                         bitmap.rows as u32,
        //                         buffer_data,
        //                         stride,
        //                         scale,
        //                         scaled_width,
        //                         scaled_height,
        //                     )
        //                 };
        //             }

        //             match pixel_mode {
        //                 CopyPixelMode::Mono8 => copy_font_bitmap_with!(COPY_PIXEL_MODE_MONO8),
        //                 CopyPixelMode::Bgra32 => copy_font_bitmap_with!(COPY_PIXEL_MODE_BGRA32),
        //             }
        //         }),
        //     );

        Ok(SingleGlyphBitmap {
            offset: Vec2::new(I26Dot6::ZERO, I26Dot6::ZERO),
            texture,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum CopyPixelMode {
    Mono8 = 0,
    Bgra32 = 1,
}

const COPY_PIXEL_MODE_MONO8: u8 = CopyPixelMode::Mono8 as u8;
const COPY_PIXEL_MODE_BGRA32: u8 = CopyPixelMode::Bgra32 as u8;

fn copy_font_bitmap<const PIXEL_MODE: u8>(
    input_data: *const u8,
    input_stride: isize,
    input_width: u32,
    input_height: u32,
    out_data: &mut [MaybeUninit<u8>],
    out_stride: usize,
    scale: I26Dot6,
    out_width: u32,
    out_height: u32,
) {
    const { assert!(PIXEL_MODE < 2) };

    let pixel_width: u8 = match PIXEL_MODE {
        COPY_PIXEL_MODE_MONO8 => 1,
        COPY_PIXEL_MODE_BGRA32 => 4,
        _ => unreachable!(),
    };

    let scale6 = scale.into_raw() as u32;
    for biy in 0..out_height {
        for bix in 0..out_width {
            // TODO: replace with a macro?
            let get_pixel_values = |x: u32, y: u32| -> [u8; 4] {
                match PIXEL_MODE {
                    COPY_PIXEL_MODE_MONO8 => [
                        unsafe {
                            input_data
                                .offset(y as isize * input_stride + x as isize)
                                .read()
                        },
                        0,
                        0,
                        0,
                    ],
                    COPY_PIXEL_MODE_BGRA32 => unsafe {
                        input_data
                            .offset(y as isize * input_stride + (x as isize) * 4)
                            .cast::<[u8; 4]>()
                            .read()
                    },
                    _ => unreachable!(),
                }
            };

            let interpolate_pixel_values = |a: [u8; 4], fa: u32, b: [u8; 4], fb: u32| {
                let mut r = [0; 4];
                for i in 0..pixel_width as usize {
                    r[i] = (((a[i] as u32 * fa) + (b[i] as u32 * fb)) >> 6) as u8;
                }
                r
            };

            let pixel_data = if scale6 == 64 {
                get_pixel_values(bix, biy)
            } else {
                // bilinear scaling
                let source_pixel_x6 = (bix << 12) / scale6;
                let source_pixel_y6 = (biy << 12) / scale6;

                let floor_x = source_pixel_x6 >> 6;
                let floor_y = source_pixel_y6 >> 6;
                let next_x = floor_x + 1;
                let next_y = floor_y + 1;

                let factor_floor_x = 64 - (source_pixel_x6 & 0x3F);
                let factor_next_x = source_pixel_x6 & 0x3F;
                let factor_floor_y = 64 - (source_pixel_y6 & 0x3F);
                let factor_next_y = source_pixel_y6 & 0x3F;

                if next_x >= input_width {
                    if next_y >= input_height {
                        get_pixel_values(floor_x, floor_y)
                    } else {
                        let a = get_pixel_values(floor_x, floor_y);
                        let b = get_pixel_values(floor_x, next_y);
                        interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                    }
                } else if next_y >= input_height {
                    let a = get_pixel_values(floor_x, floor_y);
                    let b = get_pixel_values(next_x, floor_y);
                    interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                } else {
                    let a = {
                        let a = get_pixel_values(floor_x, floor_y);
                        let b = get_pixel_values(next_x, floor_y);
                        interpolate_pixel_values(a, factor_floor_x, b, factor_next_x)
                    };
                    let b = {
                        let a = get_pixel_values(floor_x, next_y);
                        let b = get_pixel_values(next_x, next_y);
                        interpolate_pixel_values(a, factor_floor_x, b, factor_next_x)
                    };
                    interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                }
            };

            let i = bix as usize * pixel_width as usize + biy as usize * out_stride;
            out_data[i..i + pixel_width as usize].copy_from_slice(unsafe {
                std::mem::transmute(&pixel_data[..pixel_width as usize])
            });
        }
    }
}
