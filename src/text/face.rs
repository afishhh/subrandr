use std::{ffi::CString, mem::MaybeUninit, path::Path};

use crate::util::fmt_from_fn;

use super::ft_utils::*;
use text_sys::*;

#[repr(transparent)]
struct FaceMmVar(*mut FT_MM_Var);

impl FaceMmVar {
    #[inline(always)]
    fn has(face: FT_Face) -> bool {
        unsafe { ((*face).face_flags & FT_FACE_FLAG_MULTIPLE_MASTERS as i64) != 0 }
    }

    fn get(face: FT_Face) -> Option<FaceMmVar> {
        unsafe {
            if Self::has(face) {
                Some(FaceMmVar({
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
    default_coords: MmCoords,
}

impl SharedFaceData {
    fn get_ref(face: FT_Face) -> &'static SharedFaceData {
        unsafe { &*((*face).generic.data as *const SharedFaceData) }
    }
}

pub struct Face {
    face: FT_Face,
}

const fn create_freetype_tag(text: [u8; 4]) -> u64 {
    (((text[0] as u32) << 24)
        + ((text[1] as u32) << 16)
        + ((text[2] as u32) << 8)
        + (text[3] as u32)) as u64
}

pub const WEIGHT_AXIS: u64 = create_freetype_tag(*b"wght");
pub const ITALIC_AXIS: u64 = create_freetype_tag(*b"ital");

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>) -> Self {
        let library = Library::get_or_init();
        let _guard = library.face_mutation_mutex.lock().unwrap();
        let cstr = CString::new(path.as_ref().as_os_str().as_encoded_bytes()).unwrap();

        let mut face = std::ptr::null_mut();
        unsafe {
            fttry!(FT_New_Face(library.ptr, cstr.as_ptr(), 0, &mut face));

            let mut axes = Vec::new();
            let mut default_coords = MmCoords::default();
            if let Some(mm) = FaceMmVar::get(face) {
                for (index, ft_axis) in mm.axes().into_iter().enumerate() {
                    axes.push(Axis {
                        tag: ft_axis.tag,
                        index,
                        minimum: ft_axis.minimum,
                        maximum: ft_axis.maximum,
                    });
                    default_coords[index] = ft_axis.def;
                }
            }

            (*face).generic.data = Box::into_raw(Box::new(SharedFaceData {
                axes,
                default_coords,
            })) as *mut std::ffi::c_void;
        }

        Self { face }
    }

    #[inline(always)]
    #[expect(dead_code)]
    pub fn with_size(&self, point_size: f32, dpi: u32) -> Font {
        Font {
            ft_face: self.face,
            hb_font: unsafe { hb_ft_font_create_referenced(self.face) },
            frac_point_size: f32_to_fractional_points(point_size * 2.0),
            dpi,
            coords: self.shared_data().default_coords,
        }
    }

    #[inline(always)]
    pub fn builder_with_size(&self, point_size: f32, dpi: u32) -> FontBuilder {
        FontBuilder {
            font: self.with_size(point_size, dpi),
        }
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
}

pub struct FontBuilder {
    font: Font,
}

impl FontBuilder {
    #[inline(always)]
    pub fn set_axis(mut self, axis: &Axis, value: f32) -> Self {
        assert!(axis.is_value_in_range(value));
        self.font.coords[axis.index] = f32_to_fixed_point(value);
        self
    }

    #[inline(always)]
    pub fn build(self) -> Font {
        self.font
    }
}

#[derive(Clone, Copy)]
pub struct Axis {
    tag: u64,
    index: usize,
    minimum: FT_Fixed,
    maximum: FT_Fixed,
}

impl Axis {
    #[inline(always)]
    pub fn tag(&self) -> u64 {
        self.tag
    }

    #[inline(always)]
    pub fn minimum(&self) -> f32 {
        fixed_point_to_f32(self.minimum)
    }

    #[inline(always)]
    pub fn maximum(&self) -> f32 {
        fixed_point_to_f32(self.maximum)
    }

    #[inline(always)]
    fn is_fixed_value_in_range(&self, fixed: i64) -> bool {
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
            .field(
                "tag",
                &fmt_from_fn(|fmt| {
                    let bytes = self.tag.to_be_bytes();
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
                }),
            )
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
        Self { face: self.face }
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

pub struct Font {
    // owned by hb_font
    ft_face: FT_Face,
    hb_font: *mut hb_font_t,
    frac_point_size: FT_F26Dot6,
    dpi: u32,
    coords: [FT_Fixed; T1_MAX_MM_AXIS as usize],
}

impl Font {
    pub(super) fn with_applied_size(&self) -> FT_Face {
        unsafe {
            fttry!(FT_Set_Char_Size(
                self.ft_face,
                self.frac_point_size,
                self.frac_point_size,
                self.dpi,
                self.dpi,
            ));
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

    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    pub fn glyph_extents(&self, codepoint: u32) -> FT_Glyph_Metrics_ {
        let face = self.with_applied_size();
        unsafe {
            fttry!(FT_Load_Glyph(face, codepoint, FT_LOAD_COLOR as i32));
            (*(*face).glyph).metrics
        }
    }

    pub fn horizontal_extents(&self) -> hb_font_extents_t {
        let mut result = MaybeUninit::uninit();
        unsafe {
            assert!(hb_font_get_h_extents(self.hb_font, result.as_mut_ptr()) > 0);
            result.assume_init()
        }
    }

    #[expect(dead_code)]
    pub fn vertical_extents(&self) -> hb_font_extents_t {
        let mut result = MaybeUninit::uninit();
        unsafe {
            assert!(hb_font_get_v_extents(self.hb_font, result.as_mut_ptr()) > 0);
            result.assume_init()
        }
    }
}

impl Clone for Font {
    fn clone(&self) -> Self {
        Self {
            ft_face: self.ft_face,
            hb_font: { unsafe { hb_font_reference(self.hb_font) } },
            frac_point_size: self.frac_point_size,
            dpi: self.dpi,
            coords: self.coords,
        }
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.ft_face == other.ft_face
            && self.frac_point_size == other.frac_point_size
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
