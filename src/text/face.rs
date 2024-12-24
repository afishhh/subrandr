use std::{
    ffi::{CStr, CString},
    mem::MaybeUninit,
    path::Path,
};

use crate::{math::Fixed, util::fmt_from_fn};

use super::ft_utils::*;
use text_sys::*;

#[repr(transparent)]
struct FaceMmVar(*mut FT_MM_Var);

impl FaceMmVar {
    #[inline(always)]
    fn has(face: FT_Face) -> bool {
        unsafe { ((*face).face_flags & FT_FACE_FLAG_MULTIPLE_MASTERS as i64) != 0 }
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
pub struct Face {
    face: FT_Face,
    coords: MmCoords,
}

const fn create_freetype_tag(text: [u8; 4]) -> u64 {
    (((text[0] as u32) << 24)
        + ((text[1] as u32) << 16)
        + ((text[2] as u32) << 8)
        + (text[3] as u32)) as u64
}

pub const WEIGHT_AXIS: u64 = create_freetype_tag(*b"wght");
#[expect(dead_code)]
pub const WIDTH_AXIS: u64 = create_freetype_tag(*b"wdth");
pub const ITALIC_AXIS: u64 = create_freetype_tag(*b"ital");

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>) -> Self {
        let library = Library::get_or_init();
        let _guard = library.face_mutation_mutex.lock().unwrap();
        let cstr = CString::new(path.as_ref().as_os_str().as_encoded_bytes()).unwrap();

        let mut face = std::ptr::null_mut();
        unsafe {
            fttry!(FT_New_Face(library.ptr, cstr.as_ptr(), 0, &mut face));
        }

        let mut axes = Vec::new();
        let mut default_coords = MmCoords::default();

        if let Some(mm) = FaceMmVar::get(face) {
            for (index, ft_axis) in mm.axes().iter().enumerate() {
                axes.push(Axis {
                    tag: ft_axis.tag,
                    index,
                    minimum: ft_axis.minimum,
                    maximum: ft_axis.maximum,
                });
                default_coords[index] = ft_axis.def;
            }
        }

        unsafe {
            (*face).generic.data =
                Box::into_raw(Box::new(SharedFaceData { axes })) as *mut std::ffi::c_void;
            (*face).generic.finalizer = Some(SharedFaceData::finalize);
        }

        Self {
            face,
            coords: default_coords,
        }
    }

    #[inline(always)]
    pub fn with_size(&self, point_size: f32, dpi: u32) -> Font {
        let point_size = f32_to_fractional_points(point_size);
        Font::create(self.face, self.coords, point_size, dpi)
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

    pub fn axes(&self) -> &[Axis] {
        &self.shared_data().axes
    }

    pub fn axis(&self, tag: u64) -> Option<Axis> {
        self.axes().iter().find(|x| x.tag == tag).copied()
    }

    pub fn set_axis(&mut self, index: usize, value: f32) {
        assert!(self.shared_data().axes[index].is_value_in_range(value));
        self.coords[index] = f32_to_fixed_point(value);
    }

    fn os2_weight(&self) -> Option<u16> {
        unsafe {
            let table = FT_Get_Sfnt_Table(self.face, FT_SFNT_OS2) as *const TT_OS2;
            table.as_ref().map(|os2| os2.usWeightClass)
        }
    }

    pub fn weight(&self) -> f32 {
        SharedFaceData::get_ref(self.face)
            .axes
            .iter()
            .find_map(|x| (x.tag == WEIGHT_AXIS).then_some(x.index))
            .map_or_else(
                || {
                    if let Some(weight) = self.os2_weight() {
                        weight as f32
                    } else {
                        let has_bold_flag = unsafe {
                            (*self.face).style_flags & (FT_STYLE_FLAG_BOLD as FT_Long) != 0
                        };

                        (300 + 400 * has_bold_flag as i32) as f32
                    }
                },
                |idx| fixed_point_to_f32(self.coords[idx]),
            )
    }

    pub fn italic(&self) -> bool {
        SharedFaceData::get_ref(self.face)
            .axes
            .iter()
            .find_map(|x| (x.tag == ITALIC_AXIS).then_some(x.index))
            .map_or_else(
                || unsafe { (*self.face).style_flags & (FT_STYLE_FLAG_ITALIC as FT_Long) != 0 },
                |idx| fixed_point_to_f32(self.coords[idx]) > 0.5,
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
                    .map(|(i, axis)| (debug_tag(axis.tag), fixed_point_to_f32(self.coords[i]))),
            )
            .finish()?;
        write!(f, ")")
    }
}

#[derive(Clone, Copy)]
pub struct Axis {
    pub tag: u64,
    pub index: usize,
    minimum: FT_Fixed,
    maximum: FT_Fixed,
}

fn debug_tag(tag: u64) -> impl std::fmt::Debug {
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
    #[inline(always)]
    pub fn minimum(&self) -> f32 {
        fixed_point_to_f32(self.minimum)
    }

    #[inline(always)]
    pub fn maximum(&self) -> f32 {
        fixed_point_to_f32(self.maximum)
    }

    #[inline(always)]
    const fn is_fixed_value_in_range(&self, fixed: i64) -> bool {
        self.minimum <= fixed && fixed <= self.maximum
    }

    #[inline(always)]
    pub fn is_value_in_range(&self, value: f32) -> bool {
        self.is_fixed_value_in_range(f32_to_fixed_point(value))
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

#[repr(C)]
pub struct Font {
    // owned by hb_font
    ft_face: FT_Face,
    coords: MmCoords,
    hb_font: *mut hb_font_t,
    point_size: FT_F26Dot6,
    dpi: u32,

    /// -1 = not fixed size
    fixed_size_index: i32,
    pub(super) scale: Fixed<6>,
}

impl Font {
    fn create(face: FT_Face, coords: MmCoords, point_size: FT_F26Dot6, dpi: u32) -> Self {
        let (fixed_size_index, scale) = if unsafe {
            (*face).face_flags & (FT_FACE_FLAG_FIXED_SIZES as FT_Long) == 0
        } {
            unsafe {
                fttry!(FT_Set_Char_Size(face, point_size, point_size, dpi, dpi));
            }

            (-1, Fixed::ONE)
        } else {
            let sizes = unsafe {
                std::slice::from_raw_parts_mut(
                    (*face).available_sizes,
                    (*face).num_fixed_sizes as usize,
                )
            };

            // 3f3e3de freetype/include/freetype/internal/ftobjs.h:653
            let map_to_ppem = |dimension: i64, resolution: i64| (dimension * resolution + 36) / 72;
            let ppem = map_to_ppem(point_size, dpi.into());

            // First size larger than requested, or the largest size if not found
            let mut picked_size_index = 0usize;
            for (i, size) in sizes.iter().enumerate() {
                if size.x_ppem > ppem && size.x_ppem < sizes[picked_size_index].x_ppem {
                    picked_size_index = i;
                }
            }

            let scale = Fixed::<6>::from_quotient64(ppem, sizes[picked_size_index].x_ppem);

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
                scale_field!(x_scale, i64, i64);
                scale_field!(y_scale, i64, i64);
                scale_field!(ascender, i64, i64);
                scale_field!(descender, i64, i64);
                scale_field!(height, i64, i64);
                scale_field!(max_advance, i64, i64);
            } else {
                fttry!(FT_Set_Char_Size(
                    self.ft_face,
                    self.point_size,
                    self.point_size,
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

    pub fn glyph_extents(&self, index: u32) -> FT_Glyph_Metrics_ {
        let face = self.with_applied_size();
        let mut metrics = unsafe {
            fttry!(FT_Load_Glyph(face, index, FT_LOAD_COLOR as i32));
            (*(*face).glyph).metrics
        };

        if let Some(scale) = Some(self.scale).filter(|s| *s != 1) {
            macro_rules! scale_field {
                ($name: ident) => {
                    metrics.$name = (metrics.$name * scale.into_raw() as i64) >> 6;
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

        metrics
    }

    pub fn metrics(&self) -> FT_Size_Metrics {
        let face = self.with_applied_size();
        unsafe { (*(*face).size).metrics }
    }

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

    pub fn weight(&self) -> f32 {
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
            .field("point_size", &Fixed::<6>::from_raw(self.point_size as i32))
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

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.ft_face == other.ft_face
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
