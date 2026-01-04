use std::{
    ffi::{CStr, CString},
    hash::Hash,
    mem::{ManuallyDrop, MaybeUninit},
    path::Path,
    rc::Rc,
    sync::Arc,
};

use rasterize::{PixelFormat, Rasterizer};
use text_sys::*;
use thiserror::Error;
use util::math::{I16Dot16, I26Dot6, Vec2};

use super::{Axis, FaceImpl, FontImpl, FontMetrics, OpenTypeTag, SingleGlyphBitmap};
use crate::text::{ft_utils::*, FontSizeCacheKey};

// Light hinting is used to ensure horizontal metrics remain unchanged by hinting.
// This is required because we currently rely on subpixel positioning while rendering
// text which would defeat the point of grid-fitting done by full hinting.
// I believe this is what web browsers do anyway, pango also switched its defaults at
// one point which did make some complain[1]. In subrandr's case hopefully there will be
// no difference since subtitles are usually large enough not to care much about hinting.
//
// [1] See https://github.com/harfbuzz/harfbuzz/issues/2394 for people complaining about
//     the lack of full hinting in newer pango versions.
//     See https://github.com/harfbuzz/harfbuzz/issues/1892 for pango developer complaining about
//     people complaining about the metrics being different (unhinted).
//
// Also this is not picked up by bindgen because it's a macro expression I think
const FT_LOAD_TARGET_LIGHT: u32 = (FT_RENDER_MODE_LIGHT & 15) << 16;

#[repr(transparent)]
struct FaceMmVar(*mut FT_MM_Var);

impl FaceMmVar {
    #[inline(always)]
    unsafe fn has(face: FT_Face) -> bool {
        unsafe { ((*face).face_flags & FT_FACE_FLAG_MULTIPLE_MASTERS as FT_Long) != 0 }
    }

    unsafe fn get(face: FT_Face) -> Result<Option<Self>, FreeTypeError> {
        unsafe {
            Ok(if Self::has(face) {
                Some(Self({
                    let mut output = MaybeUninit::uninit();
                    fttry!(FT_Get_MM_Var(face, output.as_mut_ptr()))?;
                    output.assume_init()
                }))
            } else {
                None
            })
        }
    }

    fn axes(&self) -> &[FT_Var_Axis] {
        unsafe {
            assert!((*self.0).num_axis <= T1_MAX_MM_AXIS);
            std::slice::from_raw_parts((*self.0).axis, (*self.0).num_axis as usize)
        }
    }

    #[expect(dead_code)]
    fn namedstyles(&self) -> &[FT_Var_Named_Style] {
        unsafe {
            std::slice::from_raw_parts((*self.0).namedstyle, (*self.0).num_namedstyles as usize)
        }
    }
}

impl Drop for FaceMmVar {
    fn drop(&mut self) {
        unsafe {
            FT_Done_MM_Var(Library::get_or_init().unwrap().ptr, self.0);
        }
    }
}

pub(super) type MmCoords = [FT_Fixed; T1_MAX_MM_AXIS as usize];

struct SharedFaceData {
    axes: Vec<Axis>,
    // This is only here to ensure the memory backing the font doesn't get
    // deallocated while FreeType is still using it.
    #[expect(dead_code)]
    memory: Option<Arc<[u8]>>,
}

impl SharedFaceData {
    fn get_ref(face: FT_Face) -> &'static Self {
        unsafe { &*((*face).generic.data as *const Self) }
    }

    unsafe extern "C" fn finalize(face: *mut std::ffi::c_void) {
        let face = face as FT_Face;
        drop(unsafe { Box::from_raw((*face).generic.data as *mut Self) });
    }
}

#[repr(C)]
#[derive(PartialEq, Eq, Hash)]
pub struct Face {
    face: FT_Face,
    coords: MmCoords,
}

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>, index: i32) -> Result<Self, FreeTypeError> {
        let library = Library::get_or_init()?;
        let _guard = library.face_mutation_mutex.lock().unwrap();
        let cstr = CString::new(path.as_ref().as_os_str().as_encoded_bytes()).unwrap();

        let mut face = std::ptr::null_mut();
        unsafe {
            #[allow(clippy::unnecessary_cast)]
            fttry!(FT_New_Face(
                library.ptr,
                cstr.as_ptr(),
                index as FT_Long,
                &mut face
            ))?;
        }

        unsafe { Self::adopt_ft(face, None) }
    }

    pub fn load_from_bytes(bytes: Arc<[u8]>, index: i32) -> Result<Self, FreeTypeError> {
        unsafe {
            Self::adopt_ft(
                Self::ft_from_memory(
                    std::ptr::slice_from_raw_parts(bytes.as_ptr(), bytes.len()),
                    index,
                )?,
                Some(bytes),
            )
        }
    }

    #[cfg(all(test, feature = "_layout_tests"))]
    pub fn load_from_static_bytes(bytes: &'static [u8], index: i32) -> Result<Self, FreeTypeError> {
        unsafe { Self::adopt_ft(Self::ft_from_memory(bytes, index)?, None) }
    }

    fn ft_from_memory(bytes: *const [u8], index: i32) -> Result<FT_Face, FreeTypeError> {
        let library = Library::get_or_init()?;
        let _guard = library.face_mutation_mutex.lock().unwrap();

        unsafe {
            let mut face = MaybeUninit::uninit();

            #[allow(clippy::unnecessary_cast)]
            fttry!(FT_New_Memory_Face(
                library.ptr,
                bytes.cast(),
                bytes.len() as FT_Long,
                index as FT_Long,
                face.as_mut_ptr()
            ))?;

            Ok(face.assume_init())
        }
    }

    unsafe fn adopt_ft(face: FT_Face, memory: Option<Arc<[u8]>>) -> Result<Self, FreeTypeError> {
        let mut axes = Vec::new();
        let mut default_coords = MmCoords::default();

        if let Some(mm) = unsafe { FaceMmVar::get(face)? } {
            for (index, ft_axis) in mm.axes().iter().enumerate() {
                axes.push(Axis {
                    // FT_ULong may be u64, but this tag always fits in u32
                    #[allow(clippy::unnecessary_cast)]
                    tag: OpenTypeTag(ft_axis.tag as u32),
                    index,
                    minimum: I16Dot16::from_ft(ft_axis.minimum),
                    maximum: I16Dot16::from_ft(ft_axis.maximum),
                });
                default_coords[index] = ft_axis.def;
            }
        }

        unsafe {
            (*face).generic.data =
                Box::into_raw(Box::new(SharedFaceData { axes, memory })) as *mut std::ffi::c_void;
            (*face).generic.finalizer = Some(SharedFaceData::finalize);
        }

        Ok(Self {
            face,
            coords: default_coords,
        })
    }

    fn shared_data(&self) -> &SharedFaceData {
        SharedFaceData::get_ref(self.face)
    }

    fn os2_weight(&self) -> Option<u16> {
        unsafe {
            let table = FT_Get_Sfnt_Table(self.face, FT_SFNT_OS2) as *const TT_OS2;
            table.as_ref().map(|os2| os2.usWeightClass)
        }
    }
}

impl FaceImpl for Face {
    type Font = Font;

    fn family_name(&self) -> &str {
        // NOTE: FreeType says this is *always* an ASCII string.
        unsafe { CStr::from_ptr((*self.face).family_name).to_str().unwrap() }
    }

    fn addr(&self) -> usize {
        self.face.addr()
    }

    fn axes(&self) -> &[Axis] {
        &self.shared_data().axes
    }

    fn set_axis(&mut self, index: usize, value: I16Dot16) {
        assert!(self.shared_data().axes[index].is_value_in_range(value));
        self.coords[index] = value.into_ft();
    }

    fn weight(&self) -> I16Dot16 {
        SharedFaceData::get_ref(self.face)
            .axes
            .iter()
            .find_map(|x| (x.tag == OpenTypeTag::AXIS_WEIGHT).then_some(x.index))
            .map_or_else(
                || {
                    if let Some(weight) = self.os2_weight() {
                        I16Dot16::new(weight as i32)
                    } else {
                        let has_bold_flag = unsafe {
                            (*self.face).style_flags & (FT_STYLE_FLAG_BOLD as FT_Long) != 0
                        };

                        I16Dot16::new(300 + 400 * has_bold_flag as i32)
                    }
                },
                |idx| I16Dot16::from_ft(self.coords[idx]),
            )
    }

    fn italic(&self) -> bool {
        SharedFaceData::get_ref(self.face)
            .axes
            .iter()
            .find_map(|x| (x.tag == OpenTypeTag::AXIS_ITALIC).then_some(x.index))
            .map_or_else(
                || unsafe { (*self.face).style_flags & (FT_STYLE_FLAG_ITALIC as FT_Long) != 0 },
                |idx| I16Dot16::from_ft(self.coords[idx]) > I16Dot16::HALF,
            )
    }

    fn contains_codepoint(&self, codepoint: u32) -> bool {
        if unsafe { FT_Select_Charmap(self.face, FT_ENCODING_UNICODE) } != 0 {
            return false;
        }

        #[allow(clippy::useless_conversion)]
        let index = unsafe { FT_Get_Char_Index(self.face, codepoint as std::ffi::c_ulong) };

        index != 0
    }

    type Error = FreeTypeError;
    fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Font, FreeTypeError> {
        Font::create(self.face, self.coords, point_size, dpi)
    }
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Face({:?}@{:?}, ", self.family_name(), self.face,)?;

        if self.italic() {
            write!(f, "italic, ")?;
        }
        let weight = self.weight();
        if weight != I16Dot16::new(400) {
            write!(f, "w{weight}, ")?;
        }

        f.debug_map()
            .entries(
                self.axes()
                    .iter()
                    .enumerate()
                    .map(|(i, axis)| (axis.tag, I16Dot16::from_ft(self.coords[i]))),
            )
            .finish()?;
        write!(f, ")")
    }
}

impl Clone for Face {
    fn clone(&self) -> Self {
        unsafe {
            fttry!(FT_Reference_Face(self.face)).expect("FT_Reference_Face failed");
        }
        Self {
            face: self.face,
            coords: self.coords,
        }
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        let _guard = Library::get_or_init()
            .unwrap()
            .face_mutation_mutex
            .lock()
            .unwrap();
        unsafe {
            FT_Done_Face(self.face);
        }
    }
}

struct Size {
    ft_size: FT_Size,
    metrics: FontMetrics,
    bitmap_scale: I26Dot6,
    point_size: I26Dot6,
    dpi: u32,
}

impl Drop for Size {
    fn drop(&mut self) {
        unsafe {
            FT_Done_Size(self.ft_size);
        }
    }
}

unsafe fn get_table<T>(face: FT_Face, tag: FT_Sfnt_Tag) -> Option<&'static T> {
    unsafe { FT_Get_Sfnt_Table(face, tag).cast::<T>().as_ref() }
}

unsafe fn build_font_metrics(
    face: FT_Face,
    metrics: &FT_Size_Metrics,
    scale: I26Dot6,
    dpi_scale: I26Dot6,
) -> FontMetrics {
    macro_rules! scale {
        ($value: expr) => {{
            I26Dot6::from_ft((($value as i64 * i64::from(scale.into_raw())) >> 6) as FT_Long)
        }};
    }

    let scalable = ((*face).face_flags & FT_FACE_FLAG_SCALABLE as FT_Long) != 0;
    let units_per_em = (*face).units_per_EM;
    let y_ppem = metrics.y_ppem;

    // NOTE: Do not use when `scalable` is false, no idea how to make these metrics make sense in that case.
    macro_rules! scale_font_units {
        ($value: expr) => {
            I26Dot6::from_wide_quotient($value as i64 * y_ppem as i64, units_per_em as i64)
        };
    }

    let max_advance = scale!(metrics.max_advance);

    struct TypoMetrics {
        ascender: I26Dot6,
        descender: I26Dot6,
        height: I26Dot6,
    }

    let mut strikeout_metrics = None;
    let mut typo_metrics = None;

    if let Some(os2) = unsafe { get_table::<TT_OS2>(face, FT_SFNT_OS2).filter(|_| scalable) } {
        const USE_TYPO_METRICS: FT_UShort = 1 << 7;

        strikeout_metrics = Some((
            scale_font_units!(-os2.yStrikeoutPosition),
            scale_font_units!(os2.yStrikeoutSize),
        ));

        if os2.fsSelection & USE_TYPO_METRICS != 0 {
            let ascender = scale_font_units!(os2.sTypoAscender);
            let descender = scale_font_units!(os2.sTypoDescender);
            typo_metrics = Some(TypoMetrics {
                ascender,
                descender,
                height: ascender - descender + scale_font_units!(os2.sTypoLineGap),
            })
        } else {
            // Fallback to metrics from the hhea table
        }
    }

    if typo_metrics.is_none() {
        if let Some(hhea) =
            unsafe { get_table::<TT_HoriHeader>(face, FT_SFNT_HHEA).filter(|_| scalable) }
        {
            let ascender = scale_font_units!(hhea.Ascender);
            let descender = scale_font_units!(hhea.Descender);
            typo_metrics = Some(TypoMetrics {
                ascender,
                descender,
                height: ascender - descender + scale_font_units!(hhea.Line_Gap),
            })
        }
    }

    // TODO: Use OS/2 metrics if hhea metrics resulted in zero and OS/2 table exists
    //       Note that this is basically reimplementing what FreeType already does
    //       but whatever, maybe it'll be useful if we ever switch to a Rust based
    //       ttf parser and glyph rasterizer.

    let TypoMetrics {
        ascender,
        descender,
        height,
    } = typo_metrics.unwrap_or_else(|| TypoMetrics {
        ascender: scale!(metrics.ascender),
        descender: scale!(metrics.descender),
        height: scale!(metrics.height),
    });
    let (strikeout_top_offset, strikeout_thickness) = strikeout_metrics
        .unwrap_or_else(|| ((ascender - descender) / 2 - ascender - scale / 2, scale));

    let underline_top_offset;
    let underline_thickness;

    if let Some(postscript) =
        unsafe { get_table::<TT_Postscript>(face, FT_SFNT_POST).filter(|_| scalable) }
    {
        underline_top_offset = scale_font_units!(-postscript.underlinePosition);
        underline_thickness = scale_font_units!(postscript.underlineThickness);
    } else {
        underline_top_offset = (descender - dpi_scale) / 2;
        underline_thickness = dpi_scale;
    };

    FontMetrics {
        ascender,
        descender,
        height,
        max_advance,
        underline_top_offset,
        underline_thickness,
        strikeout_top_offset,
        strikeout_thickness,
    }
}

#[repr(C)]
pub struct Font {
    // owned by hb_font
    ft_face: FT_Face,
    coords: MmCoords,
    hb_font: *mut hb_font_t,
    size: ManuallyDrop<Rc<Size>>,
}

impl Font {
    fn create(
        face: FT_Face,
        coords: MmCoords,
        point_size: I26Dot6,
        dpi: u32,
    ) -> Result<Self, FreeTypeError> {
        let size = unsafe {
            let mut size = MaybeUninit::uninit();
            fttry!(FT_New_Size(face, size.as_mut_ptr()))?;
            size.assume_init()
        };

        unsafe {
            fttry!(FT_Activate_Size(size))?;
        }

        let dpi_scale = I26Dot6::from_quotient(dpi as i32, 72);

        let is_scalable = unsafe { (*face).face_flags & (FT_FACE_FLAG_SCALABLE as FT_Long) != 0 };
        let has_bitmaps =
            unsafe { (*face).face_flags & (FT_FACE_FLAG_FIXED_SIZES as FT_Long) != 0 };
        let mut bitmap_scale = I26Dot6::ONE;

        if has_bitmaps {
            let sizes = unsafe {
                std::slice::from_raw_parts_mut(
                    (*face).available_sizes,
                    (*face).num_fixed_sizes as usize,
                )
            };

            // 3f3e3de freetype/include/freetype/internal/ftobjs.h:653
            let map_to_ppem = |dimension: i64, resolution: i64| (dimension * resolution + 36) / 72;
            let ppem = map_to_ppem(point_size.into_raw().into(), dpi.into());

            // First size larger than requested, or the largest size if not found
            let mut picked_size_index = 0usize;
            for (i, size) in sizes.iter().enumerate() {
                #[allow(clippy::useless_conversion)] // c_ulong conversion
                if (i64::from(sizes[picked_size_index].x_ppem) < ppem
                    && size.x_ppem > sizes[picked_size_index].x_ppem)
                    || (i64::from(size.x_ppem) > ppem
                        && size.x_ppem < sizes[picked_size_index].x_ppem)
                {
                    picked_size_index = i;
                }
            }

            #[allow(clippy::unnecessary_cast)]
            let new_scale =
                I26Dot6::from_wide_quotient(ppem, sizes[picked_size_index].x_ppem as i64);
            bitmap_scale = new_scale;

            unsafe {
                fttry!(FT_Select_Size(face, picked_size_index as i32))?;
            }
        }

        if is_scalable {
            unsafe {
                fttry!(FT_Set_Char_Size(
                    face,
                    point_size.into_ft(),
                    point_size.into_ft(),
                    dpi,
                    dpi
                ))?;
            }
        };

        let metrics = unsafe {
            build_font_metrics(
                face,
                &(*size).metrics,
                if is_scalable {
                    I26Dot6::ONE
                } else {
                    bitmap_scale
                },
                dpi_scale,
            )
        };

        Ok(Self {
            ft_face: face,
            coords,
            size: ManuallyDrop::new(Rc::new(Size {
                ft_size: size,
                metrics,
                bitmap_scale,
                point_size,
                dpi,
            })),
            hb_font: unsafe { hb_ft_font_create_referenced(face) },
        })
    }

    pub(super) fn with_applied_size(&self) -> Result<FT_Face, FreeTypeError> {
        unsafe {
            fttry!(FT_Activate_Size(self.size.ft_size))?;
        }

        if unsafe { FaceMmVar::has(self.ft_face) } {
            unsafe {
                fttry!(FT_Set_Var_Design_Coordinates(
                    self.ft_face,
                    SharedFaceData::get_ref(self.ft_face).axes.len() as u32,
                    self.coords.as_ptr().cast_mut()
                ))?;
            }
        }

        Ok(self.ft_face)
    }

    pub(super) fn with_applied_size_and_hb(
        &self,
    ) -> Result<(FT_Face, *mut hb_font_t), FreeTypeError> {
        Ok((self.with_applied_size()?, self.hb_font))
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("face", self.face())
            .field("point_size", &self.point_size())
            .field("dpi", &self.size.dpi)
            .finish()
    }
}

impl Clone for Font {
    fn clone(&self) -> Self {
        Self {
            ft_face: self.ft_face,
            coords: self.coords,
            hb_font: { unsafe { hb_font_reference(self.hb_font) } },
            size: self.size.clone(),
        }
    }
}

impl Hash for Font {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.ft_face.addr());
        self.coords.hash(state);
        state.write_i32(self.size.point_size.into_raw());
        state.write_u32(self.size.dpi);
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.ft_face, other.ft_face)
            && self.size.point_size == other.size.point_size
            && self.size.dpi == other.size.dpi
            && self.coords == other.coords
    }
}

impl Eq for Font {}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            _ = ManuallyDrop::take(&mut self.size);
            hb_font_destroy(self.hb_font);
        }
    }
}

#[derive(Debug, Error)]
pub enum GlyphRenderError {
    #[error(transparent)]
    FreeType(#[from] FreeTypeError),
    #[error("Unsupported glyph format {0} after conversion")]
    ConversionToBitmapFailed(FT_Glyph_Format),
    #[error("Unsupported pixel mode {0}")]
    UnsupportedBitmapFormat(std::ffi::c_uchar),
}

impl FontImpl for Font {
    type Face = Face;

    fn face(&self) -> &Self::Face {
        unsafe { std::mem::transmute(self) }
    }

    fn metrics(&self) -> &FontMetrics {
        &self.size.metrics
    }

    fn point_size(&self) -> I26Dot6 {
        self.size.point_size
    }

    // FIXME: This is not really correct since it should also scale bitmaps in scalable
    //        fonts but doing that with FreeType is a pain. Since fonts rarely mix
    //        outlines and bitmaps let's just ignore that for now.
    fn harfbuzz_scale_factor_for(&self, _glyph: u32) -> I26Dot6 {
        let is_scalable =
            unsafe { (*self.ft_face).face_flags & (FT_FACE_FLAG_SCALABLE as FT_Long) != 0 };
        if is_scalable {
            I26Dot6::ONE
        } else {
            self.size.bitmap_scale
        }
    }

    fn size_cache_key(&self) -> FontSizeCacheKey {
        FontSizeCacheKey::new(
            self.point_size(),
            self.size.dpi,
            self.coords.map(I16Dot16::from_ft),
        )
    }

    type RenderError = GlyphRenderError;
    fn render_glyph_uncached(
        &self,
        rasterizer: &mut dyn Rasterizer,
        index: u32,
        offset: Vec2<I26Dot6>,
    ) -> Result<SingleGlyphBitmap, Self::RenderError> {
        unsafe {
            let face = self.with_applied_size()?;

            struct TransformGuard(FT_Face);
            impl TransformGuard {
                unsafe fn new(face: FT_Face, offset: Vec2<I26Dot6>) -> Self {
                    FT_Set_Transform(
                        face,
                        std::ptr::null_mut(),
                        &mut FT_Vector {
                            x: offset.x.into_ft(),
                            y: offset.y.into_ft(),
                        },
                    );
                    Self(face)
                }
            }
            impl Drop for TransformGuard {
                fn drop(&mut self) {
                    unsafe { FT_Set_Transform(self.0, std::ptr::null_mut(), std::ptr::null_mut()) };
                }
            }

            let _guard = TransformGuard::new(face, offset);
            fttry!(FT_Load_Glyph(
                face,
                index,
                (FT_LOAD_TARGET_LIGHT | FT_LOAD_COLOR) as i32
            ))?;
            let is_bitmap;
            let glyph = {
                let slot = (*face).glyph;

                is_bitmap = (*slot).format == FT_GLYPH_FORMAT_BITMAP;

                if !is_bitmap {
                    fttry!(FT_Render_Glyph(slot, FT_RENDER_MODE_NORMAL))?;
                }

                slot
            };

            let scale = if is_bitmap {
                self.size.bitmap_scale
            } else {
                I26Dot6::ONE
            };
            let scale6 = scale.into_raw();

            // I don't think this can happen but let's be safe
            if (*glyph).format != FT_GLYPH_FORMAT_BITMAP {
                return Err(GlyphRenderError::ConversionToBitmapFailed((*glyph).format));
            }

            let mut bitmap_offset = Vec2::new((*glyph).bitmap_left, -(*glyph).bitmap_top);
            let bitmap = &(*glyph).bitmap;
            let scaled_width = (bitmap.width * scale6 as u32) >> 6;
            let scaled_height = (bitmap.rows * scale6 as u32) >> 6;

            // FIXME: This isn't really fully accurate since we don't adjust
            //        our billinear scaling for the lost fractional part here.
            if scale6 != 64 {
                bitmap_offset.x *= scale6;
                bitmap_offset.x /= 64;
                bitmap_offset.y *= scale6;
                bitmap_offset.y /= 64;
            }

            let pixel_mode = match bitmap.pixel_mode.into() {
                FT_PIXEL_MODE_GRAY => CopyPixelMode::Mono8,
                FT_PIXEL_MODE_BGRA => CopyPixelMode::Bgra32,
                _ => return Err(GlyphRenderError::UnsupportedBitmapFormat(bitmap.pixel_mode)),
            };

            let texture = rasterizer.create_packed_texture_mapped(
                Vec2::new(scaled_width, scaled_height),
                if matches!(pixel_mode, CopyPixelMode::Bgra32) {
                    PixelFormat::Bgra
                } else {
                    PixelFormat::Mono
                },
                Box::new(|mut target| {
                    // TODO: The software rasterizer now supports texture scaling.
                    //       However that cannot really be used by this code because it expects
                    //       sane textures with a positive stride (and FreeType's can be negative).
                    //       I think I will defer switching to that here until we start using
                    //       skrifa for parsing fonts.
                    let stride = target.stride() as usize;
                    macro_rules! copy_font_bitmap_with {
                        ($pixel_mode: expr) => {
                            copy_font_bitmap::<$pixel_mode>(
                                bitmap.buffer.cast_const(),
                                bitmap.pitch as isize,
                                bitmap.width as u32,
                                bitmap.rows as u32,
                                target.buffer_mut(),
                                stride,
                                scale,
                                scaled_width,
                                scaled_height,
                            )
                        };
                    }

                    match pixel_mode {
                        CopyPixelMode::Mono8 => copy_font_bitmap_with!(COPY_PIXEL_MODE_MONO8),
                        CopyPixelMode::Bgra32 => copy_font_bitmap_with!(COPY_PIXEL_MODE_BGRA32),
                    }
                }),
            );

            Ok(SingleGlyphBitmap {
                offset: bitmap_offset,
                texture,
            })
        }
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
