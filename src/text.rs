use std::{
    ffi::{CStr, CString},
    mem::MaybeUninit,
    os::unix::ffi::OsStrExt,
    path::Path,
    sync::{Mutex, OnceLock},
};

use text_sys::*;

macro_rules! fttry {
    ($expr: expr) => {
        let code = $expr;
        #[allow(unused_unsafe)]
        if code != 0 {
            panic!("ft error: 0x{code:X}")
        }
    };
}

struct Library {
    ptr: FT_Library,
    // [Since 2.5.6] In multi-threaded applications it is easiest to use one FT_Library object per thread. In case this is too cumbersome, a single FT_Library object across threads is possible also, as long as a mutex lock is used around FT_New_Face and FT_Done_Face.
    face_mutation_mutex: Mutex<()>,
}

static FT_LIBRARY: OnceLock<Library> = OnceLock::new();

impl Library {
    fn get_or_init() -> &'static Library {
        FT_LIBRARY.get_or_init(|| unsafe {
            let mut ft = std::ptr::null_mut();
            fttry!(FT_Init_FreeType(&mut ft));
            Library {
                ptr: ft,
                face_mutation_mutex: Mutex::default(),
            }
        })
    }
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}

fn f32_to_fractional_points(value: f32) -> FT_F26Dot6 {
    (value * 26.6).round() as i64
}

fn f32_to_fixed_point(value: f32) -> FT_Fixed {
    (value * 65536.0).round() as i64
}

#[repr(transparent)]
struct FaceMmVar(*mut FT_MM_Var);

impl FaceMmVar {
    fn get(face: FT_Face) -> Option<FaceMmVar> {
        unsafe {
            if ((*face).face_flags & FT_FACE_FLAG_MULTIPLE_MASTERS as i64) != 0 {
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
        unsafe { std::slice::from_raw_parts((*self.0).axis, (*self.0).num_axis as usize) }
    }

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

pub struct Face {
    face: FT_Face,
}

const fn create_freetype_tag(text: [u8; 4]) -> u64 {
    (((text[0] as u32) << 24)
        + ((text[1] as u32) << 16)
        + ((text[2] as u32) << 8)
        + (text[3] as u32)) as u64
}

const WEIGHT_AXIS_TAG: u64 = create_freetype_tag(*b"wght");

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>) -> Self {
        let library = Library::get_or_init();
        let _guard = library.face_mutation_mutex.lock().unwrap();
        let cstr = CString::new(path.as_ref().as_os_str().as_bytes()).unwrap();

        let mut face = std::ptr::null_mut();
        unsafe {
            fttry!(FT_New_Face(library.ptr, cstr.as_ptr(), 0, &mut face));
        }

        Self { face }
    }

    #[inline(always)]
    pub fn with_size(&self, point_size: f32, dpi: u32) -> Font {
        self.with_size_and_weight(point_size, dpi, 400.)
    }

    pub fn with_size_and_weight(&self, point_size: f32, dpi: u32, weight: f32) -> Font {
        Font {
            ft_face: self.face,
            hb_font: unsafe { hb_ft_font_create_referenced(self.face) },
            frac_point_size: f32_to_fractional_points(point_size * 8.0),
            dpi,
            fixed_point_weight: f32_to_fixed_point(weight),
        }
    }
}

pub struct Font {
    // owned by hb_font
    ft_face: FT_Face,
    hb_font: *mut hb_font_t,
    frac_point_size: FT_F26Dot6,
    dpi: u32,
    fixed_point_weight: FT_Fixed,
}

impl Font {
    fn with_applied_size(&self) -> FT_Face {
        unsafe {
            fttry!(FT_Set_Char_Size(
                self.ft_face,
                self.frac_point_size,
                self.frac_point_size,
                self.dpi,
                self.dpi
            ));
        }
        if let Some(mm) = FaceMmVar::get(self.ft_face) {
            let mut coords = [FT_Fixed::default(); T1_MAX_MM_AXIS as usize];
            for (i, axis) in mm.axes().iter().enumerate() {
                if axis.tag == WEIGHT_AXIS_TAG {
                    coords[i] = self.fixed_point_weight.clamp(axis.minimum, axis.maximum);
                } else {
                    coords[i] = axis.def;
                }
            }

            unsafe {
                fttry!(FT_Set_Var_Design_Coordinates(
                    self.ft_face,
                    mm.axes().len() as u32,
                    coords.as_mut_ptr()
                ));
            }
        }
        self.ft_face
    }

    fn with_applied_size_and_hb(&self) -> (FT_Face, *mut hb_font_t) {
        let ft_face = self.with_applied_size();
        (ft_face, self.hb_font)
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
}

/// Renders text, see example below.
///
/// # Examples
///
/// ```
/// let renderer = TextRenderer::new();
/// let face = text::Face::load_from_file("./NotoSansMono[wdth,wght].ttf");
/// let normal_64pt = face.with_size(64. /* pt */, 72);
/// let bold_32pt = face.with_size_and_weight(32. /* pt */, 72, 700.);
/// let shaped = renderer.shape_text(&normal_64pt, "hello world");
/// let extents = renderer.compute_extents(&normal_64pt, &shaped);
/// renderer.paint(
///     panic!(), /* 8-bit RGBA */
///     0, /* baseline offset */
///     0,
///     0, /* buffer dimensions */
///     0,
///     0,
///     &normal_64pt,
///     &shaped,
///     [255, 0, 0],
///     0.5,
/// );
/// ```
pub struct TextRenderer {
    ft: &'static Library,
}

pub struct ShapedText(*mut hb_buffer_t);

impl ShapedText {
    fn glyph_infos(&self) -> &[hb_glyph_info_t] {
        unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_infos(self.0, &mut nglyphs);
            std::slice::from_raw_parts(infos as *const _, nglyphs as usize)
        }
    }

    fn glyph_positions(&self) -> &[hb_glyph_position_t] {
        unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_positions(self.0, &mut nglyphs);
            std::slice::from_raw_parts(infos as *const _, nglyphs as usize)
        }
    }
}

// TODO: exact lookup table instead of this approximation?
#[inline(always)]
fn srgb_to_linear(color: u8) -> f32 {
    (color as f32 / 255.0).powf(1.0 / 2.2)
}

#[inline(always)]
fn blend_over(dst: f32, src: f32, alpha: f32) -> f32 {
    alpha * src + (1.0 - alpha) * dst
}

#[inline(always)]
fn linear_to_srgb(color: f32) -> u8 {
    (color.powf(2.2 / 1.0) * 255.0).round() as u8
}

fn direction_is_horizontal(dir: hb_direction_t) -> bool {
    dir == hb_direction_t_HB_DIRECTION_LTR || dir == hb_direction_t_HB_DIRECTION_RTL
}

#[derive(Debug, Clone, Copy)]
pub struct TextExtents {
    pub paint_height: i32,
    pub paint_width: i32,
    // TODO: with font extents for non-primary dimension
    //  logical_width: usize,
    //  logical_height: usize,
}

impl TextRenderer {
    pub fn new() -> Self {
        let library = Library::get_or_init();

        Self { ft: library }
    }

    pub fn shape_text(&self, font: &Font, text: &str) -> ShapedText {
        let (_, hb_font) = font.with_applied_size_and_hb();
        unsafe {
            let buf: *mut hb_buffer_t = hb_buffer_create();
            hb_buffer_add_utf8(buf, text.as_ptr() as *const _, text.len() as i32, 0, -1);
            hb_buffer_guess_segment_properties(buf);
            hb_shape(hb_font, buf, std::ptr::null(), 0);

            ShapedText(buf)
        }
    }

    pub fn compute_extents(&self, font: &Font, text: &ShapedText) -> TextExtents {
        unsafe {
            let infos = text.glyph_infos();
            let positions = text.glyph_positions();
            let (_, hb_font) = font.with_applied_size_and_hb();

            let direction = hb_buffer_get_direction(text.0);

            let mut results = TextExtents {
                paint_height: 0,
                paint_width: 0,
            };

            let mut iterator = infos.iter().zip(positions.iter()).enumerate();

            if let Some((_, (info, _))) = iterator.next_back() {
                let mut extents = MaybeUninit::uninit();
                assert!(
                    hb_font_get_glyph_extents(hb_font, info.codepoint, extents.as_mut_ptr(),) > 0
                );
                let extents = extents.assume_init();
                results.paint_height += extents.height.abs();
                results.paint_width += extents.width;
            }

            for (_, (info, position)) in iterator {
                let mut extents = MaybeUninit::uninit();
                assert!(
                    hb_font_get_glyph_extents(hb_font, info.codepoint, extents.as_mut_ptr()) > 0
                );
                let extents = extents.assume_init();
                if direction_is_horizontal(direction) {
                    results.paint_height = results.paint_height.max(extents.height.abs());
                    results.paint_width += position.x_advance;
                } else {
                    results.paint_width = results.paint_width.max(extents.width.abs());
                    results.paint_height += position.y_advance;
                }
            }

            results.paint_height /= 64;
            results.paint_width /= 64;

            results
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        buffer: &mut [u8],
        baseline_x: usize,
        baseline_y: usize,
        width: usize,
        height: usize,
        stride: usize,
        font: &Font,
        text: &ShapedText,
        color: [u8; 3],
        alpha: f32,
    ) {
        unsafe {
            let infos = text.glyph_infos();
            let positions = text.glyph_positions();
            let face = font.with_applied_size();

            assert_eq!(infos.len(), positions.len());

            let mut x = baseline_x as u32;
            let mut y = baseline_y as u32;
            for (info, position) in infos.iter().zip(positions.iter()) {
                fttry!(FT_Load_Glyph(face, info.codepoint, FT_LOAD_COLOR as i32));
                let glyph = (*face).glyph;
                fttry!(FT_Render_Glyph(
                    glyph,
                    FT_Render_Mode__FT_RENDER_MODE_NORMAL
                ));

                let (ox, oy) = (
                    (*glyph).bitmap_left + position.x_offset / 64,
                    -(*glyph).bitmap_top + position.y_offset / 64,
                );
                let bitmap = &(*glyph).bitmap;

                // dbg!(bitmap.width, bitmap.rows);
                // dbg!((*glyph).bitmap_left, (*glyph).bitmap_top);

                #[expect(non_upper_case_globals)]
                let pixel_width = match bitmap.pixel_mode.into() {
                    FT_Pixel_Mode__FT_PIXEL_MODE_GRAY => 1,
                    _ => todo!("ft pixel mode {:?}", bitmap.pixel_mode),
                };

                for biy in 0..bitmap.rows {
                    for bix in 0..(bitmap.width / pixel_width) {
                        let fx = x as i32 + ox + bix as i32;
                        let fy = y as i32 + oy + biy as i32;

                        if fx < 0 || fy < 0 {
                            continue;
                        }

                        let fx = fx as usize;
                        let fy = fy as usize;
                        if fx >= width || fy >= height {
                            continue;
                        }

                        let bpos = (biy as i32 * bitmap.pitch) + (bix * pixel_width) as i32;
                        let bslice = std::slice::from_raw_parts(
                            bitmap.buffer.offset(bpos as isize),
                            pixel_width as usize,
                        );
                        #[expect(non_upper_case_globals)]
                        let (colors, alpha) = match bitmap.pixel_mode.into() {
                            FT_Pixel_Mode__FT_PIXEL_MODE_GRAY => (
                                [color[0], color[1], color[2]],
                                (bslice[0] as f32 / 255.0) * alpha,
                            ),
                            _ => todo!("ft pixel mode {:?}", bitmap.pixel_mode),
                        };

                        let i = fy * stride + fx * 4;
                        buffer[i] = linear_to_srgb(blend_over(
                            srgb_to_linear(buffer[i]),
                            srgb_to_linear(colors[0]),
                            alpha,
                        ));
                        buffer[i + 1] = linear_to_srgb(blend_over(
                            srgb_to_linear(buffer[i + 1]),
                            srgb_to_linear(colors[1]),
                            alpha,
                        ));
                        buffer[i + 2] = linear_to_srgb(blend_over(
                            srgb_to_linear(buffer[i + 2]),
                            srgb_to_linear(colors[2]),
                            alpha,
                        ));
                        buffer[i + 3] = ((alpha + (buffer[i + 3] as f32 / 255.0) * (1.0 - alpha))
                            * 255.0) as u8;
                        // eprintln!(
                        //     "{fx} {fy} = [{i}] = {colors:?} =over= {:?}",
                        //     &buffer[i..i + 4]
                        // );
                    }
                }

                // eprintln!("advance: {} {}", (position.x_advance as f32) / 64., (position.y_advance as f32) / 64.);
                x = x.checked_add_signed(position.x_advance / 64).unwrap();
                y = y.checked_add_signed(position.y_advance / 64).unwrap();
            }
        }
    }
}
