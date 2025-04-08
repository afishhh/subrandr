use std::{
    cell::{Cell, UnsafeCell},
    collections::HashMap,
    ffi::{CStr, CString},
    hash::Hash,
    mem::MaybeUninit,
    ops::RangeInclusive,
    path::Path,
    sync::Arc,
};

use crate::{
    color::BGRA8,
    math::{I16Dot16, I26Dot6},
    outline::Outline,
    rasterize::{PixelFormat, Rasterizer, Texture},
    util::fmt_from_fn,
};

use super::ft_utils::*;
use text_sys::*;

#[repr(transparent)]
struct FaceMmVar(*mut FT_MM_Var);

impl FaceMmVar {
    #[inline(always)]
    fn has(face: FT_Face) -> bool {
        unsafe { ((*face).face_flags & FT_FACE_FLAG_MULTIPLE_MASTERS as FT_Long) != 0 }
    }

    fn get(face: FT_Face) -> Option<Self> {
        unsafe {
            if Self::has(face) {
                Some(Self({
                    let mut output = MaybeUninit::uninit();
                    fttry!(FT_Get_MM_Var(face, output.as_mut_ptr()));
                    output.assume_init()
                }))
            } else {
                None
            }
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
            FT_Done_MM_Var(Library::get_or_init().ptr, self.0);
        }
    }
}

type MmCoords = [FT_Fixed; T1_MAX_MM_AXIS as usize];

struct SharedFaceData {
    axes: Vec<Axis>,
    glyph_cache: GlyphCache,
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

const fn create_freetype_tag(text: [u8; 4]) -> u32 {
    ((text[0] as u32) << 24) + ((text[1] as u32) << 16) + ((text[2] as u32) << 8) + (text[3] as u32)
}

pub const WEIGHT_AXIS: u32 = create_freetype_tag(*b"wght");
#[expect(dead_code)]
pub const WIDTH_AXIS: u32 = create_freetype_tag(*b"wdth");
pub const ITALIC_AXIS: u32 = create_freetype_tag(*b"ital");

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>) -> Self {
        let library = Library::get_or_init();
        let _guard = library.face_mutation_mutex.lock().unwrap();
        let cstr = CString::new(path.as_ref().as_os_str().as_encoded_bytes()).unwrap();

        let mut face = std::ptr::null_mut();
        unsafe {
            fttry!(FT_New_Face(library.ptr, cstr.as_ptr(), 0, &mut face));
        }

        Self::adopt_ft(face, None)
    }

    pub fn load_from_bytes(bytes: Arc<[u8]>) -> Self {
        let library = Library::get_or_init();
        let _guard = library.face_mutation_mutex.lock().unwrap();

        let mut face = std::ptr::null_mut();
        unsafe {
            fttry!(FT_New_Memory_Face(
                library.ptr,
                bytes.as_ptr(),
                bytes.len() as FT_Long,
                0,
                &mut face
            ));
        }

        Self::adopt_ft(face, Some(bytes))
    }

    fn adopt_ft(face: FT_Face, memory: Option<Arc<[u8]>>) -> Self {
        let mut axes = Vec::new();
        let mut default_coords = MmCoords::default();

        if let Some(mm) = FaceMmVar::get(face) {
            for (index, ft_axis) in mm.axes().iter().enumerate() {
                axes.push(Axis {
                    // FT_ULong may be u64, but this tag always fits in u32
                    #[allow(clippy::unnecessary_cast)]
                    tag: ft_axis.tag as u32,
                    index,
                    minimum: I16Dot16::from_ft(ft_axis.minimum),
                    maximum: I16Dot16::from_ft(ft_axis.maximum),
                });
                default_coords[index] = ft_axis.def;
            }
        }

        unsafe {
            (*face).generic.data = Box::into_raw(Box::new(SharedFaceData {
                axes,
                glyph_cache: GlyphCache::new(),
                memory,
            })) as *mut std::ffi::c_void;
            (*face).generic.finalizer = Some(SharedFaceData::finalize);
        }

        Self {
            face,
            coords: default_coords,
        }
    }

    #[inline(always)]
    pub fn with_size(&self, point_size: f32, dpi: u32) -> Font {
        Font::create(self.face, self.coords, I26Dot6::from_f32(point_size), dpi)
    }

    pub fn with_size_from(&self, other: &Font) -> Font {
        Font::create(self.face, self.coords, other.point_size, other.dpi)
    }

    pub fn family_name(&self) -> &str {
        // NOTE: FreeType says this is *always* an ASCII string.
        unsafe { CStr::from_ptr((*self.face).family_name).to_str().unwrap() }
    }

    fn shared_data(&self) -> &SharedFaceData {
        SharedFaceData::get_ref(self.face)
    }

    pub(super) fn glyph_cache(&self) -> &GlyphCache {
        &SharedFaceData::get_ref(self.face).glyph_cache
    }

    pub fn axes(&self) -> &[Axis] {
        &self.shared_data().axes
    }

    pub fn axis(&self, tag: u32) -> Option<Axis> {
        self.axes().iter().find(|x| x.tag == tag).copied()
    }

    pub fn set_axis(&mut self, index: usize, value: I16Dot16) {
        assert!(self.shared_data().axes[index].is_value_in_range(value));
        self.coords[index] = value.into_ft();
    }

    fn os2_weight(&self) -> Option<u16> {
        unsafe {
            let table = FT_Get_Sfnt_Table(self.face, FT_SFNT_OS2) as *const TT_OS2;
            table.as_ref().map(|os2| os2.usWeightClass)
        }
    }

    pub fn weight(&self) -> I16Dot16 {
        SharedFaceData::get_ref(self.face)
            .axes
            .iter()
            .find_map(|x| (x.tag == WEIGHT_AXIS).then_some(x.index))
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

    pub fn italic(&self) -> bool {
        SharedFaceData::get_ref(self.face)
            .axes
            .iter()
            .find_map(|x| (x.tag == ITALIC_AXIS).then_some(x.index))
            .map_or_else(
                || unsafe { (*self.face).style_flags & (FT_STYLE_FLAG_ITALIC as FT_Long) != 0 },
                |idx| I16Dot16::from_ft(self.coords[idx]) > I16Dot16::HALF,
            )
    }
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Face({:?}@{:?}, ", self.family_name(), self.face,)?;

        let s = unsafe { (*self.face).style_flags };
        if (s & FT_STYLE_FLAG_ITALIC as FT_Long) != 0 {
            write!(f, "italic, ")?;
        }
        if (s & FT_STYLE_FLAG_BOLD as FT_Long) != 0 {
            write!(f, "bold, ")?;
        }

        f.debug_map()
            .entries(
                self.axes()
                    .iter()
                    .enumerate()
                    .map(|(i, axis)| (debug_tag(axis.tag), I16Dot16::from_ft(self.coords[i]))),
            )
            .finish()?;
        write!(f, ")")
    }
}

#[derive(Clone, Copy)]
pub struct Axis {
    pub tag: u32,
    pub index: usize,
    minimum: I16Dot16,
    maximum: I16Dot16,
}

fn debug_tag(tag: u32) -> impl std::fmt::Debug {
    fmt_from_fn(move |fmt| {
        let bytes = tag.to_be_bytes();
        let end = 'f: {
            for (i, b) in bytes.iter().enumerate() {
                if *b != 0 {
                    break 'f i;
                }
            }
            bytes.len()
        };
        let bytes = &bytes[end..];
        if let Ok(s) = std::str::from_utf8(bytes) {
            write!(fmt, "{s:?}")
        } else {
            write!(fmt, "{:?}", bytes)
        }
    })
}

impl Axis {
    // TODO: Switch these APIs to I16Dot16 instead.
    #[inline(always)]
    pub fn minimum(&self) -> I16Dot16 {
        self.minimum
    }

    #[inline(always)]
    pub fn maximum(&self) -> I16Dot16 {
        self.maximum
    }

    #[inline(always)]
    pub fn range(&self) -> RangeInclusive<I16Dot16> {
        self.minimum..=self.maximum
    }

    #[inline(always)]
    pub fn is_value_in_range(&self, value: I16Dot16) -> bool {
        self.range().contains(&value)
    }
}

impl std::fmt::Debug for Axis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Axis")
            .field("tag", &debug_tag(self.tag))
            .field("index", &self.index)
            .field("minimum", &self.minimum())
            .field("maximum", &self.maximum())
            .finish()
    }
}

impl Clone for Face {
    fn clone(&self) -> Self {
        unsafe {
            fttry!(FT_Reference_Face(self.face));
        }
        Self {
            face: self.face,
            coords: self.coords,
        }
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        let _guard = Library::get_or_init().face_mutation_mutex.lock().unwrap();
        unsafe {
            FT_Done_Face(self.face);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    pub width: I26Dot6,
    pub height: I26Dot6,
    pub hori_bearing_x: I26Dot6,
    pub hori_bearing_y: I26Dot6,
    pub hori_advance: I26Dot6,
    pub vert_bearing_x: I26Dot6,
    pub vert_bearing_y: I26Dot6,
    pub vert_advance: I26Dot6,
}

#[repr(C)]
pub struct Font {
    // owned by hb_font
    ft_face: FT_Face,
    coords: MmCoords,
    hb_font: *mut hb_font_t,
    point_size: I26Dot6,
    dpi: u32,

    /// -1 = not fixed size
    fixed_size_index: i32,
    pub(super) scale: I26Dot6,
}

impl Font {
    fn create(face: FT_Face, coords: MmCoords, point_size: I26Dot6, dpi: u32) -> Self {
        let (fixed_size_index, scale) = if unsafe {
            (*face).face_flags & (FT_FACE_FLAG_FIXED_SIZES as FT_Long) == 0
        } {
            unsafe {
                fttry!(FT_Set_Char_Size(
                    face,
                    point_size.into_ft(),
                    point_size.into_ft(),
                    dpi,
                    dpi
                ));
            }

            (-1, I26Dot6::ONE)
        } else {
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
                if i64::from(size.x_ppem) > ppem && size.x_ppem < sizes[picked_size_index].x_ppem {
                    picked_size_index = i;
                }
            }

            let scale = I26Dot6::from_wide_quotient(ppem, sizes[picked_size_index].x_ppem as i64);

            unsafe {
                fttry!(FT_Select_Size(face, picked_size_index as i32));
            }

            (picked_size_index as i32, scale)
        };

        Self {
            ft_face: face,
            coords,
            hb_font: unsafe { hb_ft_font_create_referenced(face) },
            point_size,
            dpi,
            fixed_size_index,
            scale,
        }
    }

    pub(super) fn with_applied_size(&self) -> FT_Face {
        unsafe {
            if self.fixed_size_index != -1 {
                fttry!(FT_Select_Size(self.ft_face, self.fixed_size_index));

                let metrics = &mut (*(*self.ft_face).size).metrics;
                macro_rules! scale_field {
                    ($name: ident, $intermediate: ident, $final: ident) => {
                        metrics.$name = ((metrics.$name as $intermediate
                            * self.scale.into_raw() as $intermediate)
                            >> 6) as $final;
                    };
                }

                scale_field!(x_ppem, u32, u16);
                scale_field!(y_ppem, u32, u16);
                scale_field!(x_scale, i64, FT_Long);
                scale_field!(y_scale, i64, FT_Long);
                scale_field!(ascender, i64, FT_Long);
                scale_field!(descender, i64, FT_Long);
                scale_field!(height, i64, FT_Long);
                scale_field!(max_advance, i64, FT_Long);
            } else {
                fttry!(FT_Set_Char_Size(
                    self.ft_face,
                    self.point_size.into_ft(),
                    self.point_size.into_ft(),
                    self.dpi,
                    self.dpi,
                ));
            }
        }

        if FaceMmVar::has(self.ft_face) {
            unsafe {
                fttry!(FT_Set_Var_Design_Coordinates(
                    self.ft_face,
                    SharedFaceData::get_ref(self.ft_face).axes.len() as u32,
                    std::mem::transmute(self.coords.as_ptr())
                ));
            }
        }

        self.ft_face
    }

    pub(super) fn with_applied_size_and_hb(&self) -> (FT_Face, *mut hb_font_t) {
        let ft_face = self.with_applied_size();
        (ft_face, self.hb_font)
    }

    /// Gets the Outline associated with the glyph at `index`.
    ///
    /// Returns [`None`] if the glyph does not exist in this font, or it is not
    /// an outline glyph.
    pub fn glyph_outline(&self, index: u32) -> Option<Outline> {
        let face = self.with_applied_size();
        unsafe {
            // According to FreeType documentation, bitmap-only fonts ignore
            // FT_LOAD_NO_BITMAP.
            if ((*face).face_flags & FT_FACE_FLAG_SCALABLE as FT_Long) == 0 {
                return None;
            }

            // TODO: return none if the glyph does not exist in the fonot
            fttry!(FT_Load_Glyph(face, index, FT_LOAD_NO_BITMAP as i32));

            Some(Outline::from_freetype(&(*(*face).glyph).outline))
        }
    }

    pub fn metrics(&self) -> FT_Size_Metrics {
        let face = self.with_applied_size();
        unsafe { (*(*face).size).metrics }
    }

    // TODO: Make these result in Fixed<> values
    pub fn horizontal_extents(&self) -> hb_font_extents_t {
        let mut result = MaybeUninit::uninit();
        unsafe {
            assert!(hb_font_get_h_extents(self.hb_font, result.as_mut_ptr()) > 0);
            result.assume_init()
        }
    }

    pub fn vertical_extents(&self) -> hb_font_extents_t {
        let mut result = MaybeUninit::uninit();
        unsafe {
            assert!(hb_font_get_v_extents(self.hb_font, result.as_mut_ptr()) > 0);
            result.assume_init()
        }
    }

    pub fn face(&self) -> &Face {
        unsafe { std::mem::transmute(self) }
    }

    pub fn point_size(&self) -> I26Dot6 {
        self.point_size
    }

    pub fn weight(&self) -> I16Dot16 {
        self.face().weight()
    }

    pub fn italic(&self) -> bool {
        self.face().italic()
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("face", self.face())
            .field("point_size", &self.point_size())
            .field("dpi", &self.dpi)
            .finish()
    }
}

impl Clone for Font {
    fn clone(&self) -> Self {
        Self {
            ft_face: self.ft_face,
            hb_font: { unsafe { hb_font_reference(self.hb_font) } },
            point_size: self.point_size,
            dpi: self.dpi,
            coords: self.coords,
            fixed_size_index: self.fixed_size_index,
            scale: self.scale,
        }
    }
}

impl Hash for Font {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.ft_face.addr());
        state.write_i32(self.point_size.into_raw());
        state.write_u32(self.dpi);
        self.coords.hash(state);
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.ft_face, other.ft_face)
            && self.point_size == other.point_size
            && self.dpi == other.dpi
            && self.coords == other.coords
    }
}

impl Eq for Font {}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe { hb_font_destroy(self.hb_font) };
    }
}

#[derive(PartialEq, Eq, Hash)]
struct SizeInfo {
    coords: MmCoords,
    point_size: I26Dot6,
    dpi: u32,
}

struct CacheSlot {
    generation: u64,
    metrics: Option<GlyphMetrics>,
    bitmap: Option<SingleGlyphBitmap>,
}

impl CacheSlot {
    fn new() -> Self {
        Self {
            generation: 0,
            metrics: None,
            bitmap: None,
        }
    }
}

pub(super) struct GlyphCache {
    generation: Cell<u64>,
    glyphs: UnsafeCell<HashMap<(u32, SizeInfo), CacheSlot>>,
}

impl GlyphCache {
    fn new() -> Self {
        Self {
            generation: Cell::new(0),
            glyphs: UnsafeCell::new(HashMap::new()),
        }
    }

    pub(in crate::text) fn advance_generation(&self) {
        let glyphs = unsafe { &mut *self.glyphs.get() };

        let keep_after = self.generation.get().saturating_sub(2);
        // TODO: A scan-resistant LRU?
        if glyphs.len() > 200 {
            glyphs.retain(|_, slot| slot.generation > keep_after);
        }
        self.generation.set(self.generation.get() + 1);
    }

    #[allow(clippy::mut_from_ref)] // This is why it's unsafe
    unsafe fn slot(&self, font: &Font, index: u32) -> &mut CacheSlot {
        let glyphs = unsafe { &mut *self.glyphs.get() };
        let size_info = SizeInfo {
            coords: font.coords,
            point_size: font.point_size,
            dpi: font.dpi,
        };

        let slot = glyphs
            .entry((index, size_info))
            .or_insert_with(CacheSlot::new);
        slot.generation = self.generation.get();
        slot
    }

    pub fn get_or_measure(&self, font: &Font, index: u32) -> &GlyphMetrics {
        unsafe { self.slot(font, index) }
            .metrics
            .get_or_insert_with(|| font.glyph_extents_uncached(index))
    }

    pub fn get_or_render(
        &self,
        rasterizer: &mut dyn Rasterizer,
        font: &Font,
        index: u32,
    ) -> &SingleGlyphBitmap {
        unsafe { self.slot(font, index) }
            .bitmap
            .get_or_insert_with(|| font.render_glyph_uncached(rasterizer, index))
    }
}

#[derive(Clone)]
pub struct SingleGlyphBitmap {
    pub offset: (I26Dot6, I26Dot6),
    pub texture: Texture<'static>,
}

pub enum BufferData {
    Monochrome(Vec<u8>),
    Color(Vec<BGRA8>),
}

impl Font {
    fn glyph_extents_uncached(&self, index: u32) -> GlyphMetrics {
        let face = self.with_applied_size();
        let mut metrics = unsafe {
            fttry!(FT_Load_Glyph(face, index, FT_LOAD_COLOR as i32));
            (*(*face).glyph).metrics
        };

        if let Some(scale) = Some(self.scale).filter(|s| *s != 1) {
            macro_rules! scale_field {
                ($name: ident) => {
                    metrics.$name = (metrics.$name * scale.into_raw() as FT_Long) >> 6;
                };
            }

            scale_field!(width);
            scale_field!(height);
            scale_field!(horiBearingX);
            scale_field!(horiBearingY);
            scale_field!(horiAdvance);
            scale_field!(vertBearingX);
            scale_field!(vertBearingY);
            scale_field!(vertAdvance);
        }

        GlyphMetrics {
            width: I26Dot6::from_raw(metrics.width as i32),
            height: I26Dot6::from_raw(metrics.height as i32),
            hori_bearing_x: I26Dot6::from_raw(metrics.horiBearingX as i32),
            hori_bearing_y: I26Dot6::from_raw(metrics.horiBearingY as i32),
            hori_advance: I26Dot6::from_raw(metrics.horiAdvance as i32),
            vert_bearing_x: I26Dot6::from_raw(metrics.vertBearingX as i32),
            vert_bearing_y: I26Dot6::from_raw(metrics.vertBearingY as i32),
            vert_advance: I26Dot6::from_raw(metrics.vertAdvance as i32),
        }
    }

    pub fn glyph_extents(&self, index: u32) -> GlyphMetrics {
        *self.face().glyph_cache().get_or_measure(self, index)
    }

    fn render_glyph_uncached(
        &self,
        rasterizer: &mut dyn Rasterizer,
        index: u32,
    ) -> SingleGlyphBitmap {
        unsafe {
            let face = self.with_applied_size();

            fttry!(FT_Load_Glyph(face, index, FT_LOAD_COLOR as i32));
            let slot = (*face).glyph;
            fttry!(FT_Render_Glyph(slot, FT_RENDER_MODE_NORMAL));

            let scale6 = self.scale.into_raw();

            let (ox, oy) = (
                I26Dot6::from_raw((*slot).bitmap_left * scale6),
                I26Dot6::from_raw(-(*slot).bitmap_top * scale6),
            );

            let bitmap = &(*slot).bitmap;

            let scaled_width = (bitmap.width * scale6 as u32) >> 6;
            let scaled_height = (bitmap.rows * scale6 as u32) >> 6;

            const MAX_PIXEL_WIDTH: usize = 4;

            let pixel_width = match bitmap.pixel_mode.into() {
                FT_PIXEL_MODE_GRAY => 1,
                FT_PIXEL_MODE_BGRA => 4,
                _ => todo!("ft pixel mode {:?}", bitmap.pixel_mode),
            };

            let texture = rasterizer.create_texture_mapped(
                scaled_width,
                scaled_height,
                if pixel_width == 1 {
                    PixelFormat::Mono
                } else {
                    PixelFormat::Bgra
                },
                Box::new(|buffer_data, stride| {
                    for biy in 0..scaled_height {
                        for bix in 0..scaled_width {
                            let get_pixel_values = |x: u32, y: u32| -> [u8; MAX_PIXEL_WIDTH] {
                                let bpos = (y as i32 * bitmap.pitch) + (x * pixel_width) as i32;
                                let bslice = std::slice::from_raw_parts(
                                    bitmap.buffer.offset(bpos as isize),
                                    pixel_width as usize,
                                );
                                let mut pixel_data: [u8; MAX_PIXEL_WIDTH] = [0; 4];
                                pixel_data[..pixel_width as usize].copy_from_slice(bslice);
                                pixel_data
                            };

                            let interpolate_pixel_values =
                                |a: [u8; MAX_PIXEL_WIDTH],
                                 fa: u32,
                                 b: [u8; MAX_PIXEL_WIDTH],
                                 fb: u32| {
                                    let mut r = [0; MAX_PIXEL_WIDTH];
                                    for i in 0..pixel_width as usize {
                                        r[i] =
                                            (((a[i] as u32 * fa) + (b[i] as u32 * fb)) >> 6) as u8;
                                    }
                                    r
                                };

                            let pixel_data = if scale6 == 64 {
                                get_pixel_values(bix, biy)
                            } else {
                                // bilinear scaling
                                let source_pixel_x6 = (bix << 12) / scale6 as u32;
                                let source_pixel_y6 = (biy << 12) / scale6 as u32;

                                let floor_x = source_pixel_x6 >> 6;
                                let floor_y = source_pixel_y6 >> 6;
                                let next_x = floor_x + 1;
                                let next_y = floor_y + 1;

                                let factor_floor_x = 64 - (source_pixel_x6 & 0x3F);
                                let factor_next_x = source_pixel_x6 & 0x3F;
                                let factor_floor_y = 64 - (source_pixel_y6 & 0x3F);
                                let factor_next_y = source_pixel_y6 & 0x3F;

                                if next_x >= bitmap.width {
                                    if next_y >= bitmap.rows {
                                        get_pixel_values(floor_x, floor_y)
                                    } else {
                                        let a = get_pixel_values(floor_x, floor_y);
                                        let b = get_pixel_values(floor_x, next_y);
                                        interpolate_pixel_values(
                                            a,
                                            factor_floor_y,
                                            b,
                                            factor_next_y,
                                        )
                                    }
                                } else if next_y >= bitmap.rows {
                                    let a = get_pixel_values(floor_x, floor_y);
                                    let b = get_pixel_values(next_x, floor_y);
                                    interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                                } else {
                                    let a = {
                                        let a = get_pixel_values(floor_x, floor_y);
                                        let b = get_pixel_values(next_x, floor_y);
                                        interpolate_pixel_values(
                                            a,
                                            factor_floor_x,
                                            b,
                                            factor_next_x,
                                        )
                                    };
                                    let b = {
                                        let a = get_pixel_values(floor_x, next_y);
                                        let b = get_pixel_values(next_x, next_y);
                                        interpolate_pixel_values(
                                            a,
                                            factor_floor_x,
                                            b,
                                            factor_next_x,
                                        )
                                    };
                                    interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                                }
                            };

                            let i = bix as usize * pixel_width as usize + biy as usize * stride;
                            buffer_data[i..i + pixel_width as usize].copy_from_slice(
                                std::mem::transmute(&pixel_data[..pixel_width as usize]),
                            );
                        }
                    }
                }),
            );

            SingleGlyphBitmap {
                offset: (ox, oy),
                texture,
            }
        }
    }

    pub fn render_glyph(&self, rasterizer: &mut dyn Rasterizer, index: u32) -> SingleGlyphBitmap {
        self.face()
            .glyph_cache()
            .get_or_render(rasterizer, self, index)
            .clone()
    }
}
