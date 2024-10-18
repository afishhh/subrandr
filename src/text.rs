use std::{
    ffi::CString,
    mem::{ManuallyDrop, MaybeUninit},
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
    #[expect(dead_code)]
    pub fn with_size(&self, point_size: f32, dpi: u32) -> Font {
        self.with_size_and_weight(point_size, dpi, 400.)
    }

    pub fn with_size_and_weight(&self, point_size: f32, dpi: u32, weight: f32) -> Font {
        Font {
            ft_face: self.face,
            hb_font: unsafe { hb_ft_font_create_referenced(self.face) },
            frac_point_size: f32_to_fractional_points(point_size * 2.0),
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

    #[expect(dead_code)]
    pub fn vertical_extents(&self) -> hb_font_extents_t {
        let mut result = MaybeUninit::uninit();
        unsafe {
            assert!(hb_font_get_v_extents(self.hb_font, result.as_mut_ptr()) > 0);
            result.assume_init()
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe { hb_font_destroy(self.hb_font) };
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
pub struct TextRenderer {}

pub struct Glyphs {
    // NOTE: These are not 'static, just self referential
    infos: &'static mut [hb_glyph_info_t],
    positions: &'static mut [hb_glyph_position_t],
    buffer: *mut hb_buffer_t,
}

macro_rules! define_glyph_accessors {
    () => {
        #[inline(always)]
        #[allow(dead_code)]
        pub fn codepoint(&self) -> u32 {
            self.info.codepoint
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn x_advance(&self) -> i32 {
            self.position.x_advance
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn y_advance(&self) -> i32 {
            self.position.y_advance
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn x_offset(&self) -> i32 {
            self.position.x_offset
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn y_offset(&self) -> i32 {
            self.position.y_offset
        }
    };
}

#[derive(Clone, Copy)]
pub struct Glyph<'a> {
    info: &'a hb_glyph_info_t,
    position: &'a hb_glyph_position_t,
}

impl Glyph<'_> {
    define_glyph_accessors!();
}

pub struct GlyphMut<'a> {
    info: &'a mut hb_glyph_info_t,
    position: &'a mut hb_glyph_position_t,
}

impl GlyphMut<'_> {
    define_glyph_accessors!();
}

macro_rules! define_glyph_fmt {
    () => {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Glyph")
                .field("codepoint", &self.codepoint())
                .field("x_advance", &self.x_advance())
                .field("y_advance", &self.y_advance())
                .field("x_offset", &self.x_offset())
                .field("y_offset", &self.y_offset())
                .finish()
        }
    };
}

impl std::fmt::Debug for Glyph<'_> {
    define_glyph_fmt!();
}

impl std::fmt::Debug for GlyphMut<'_> {
    define_glyph_fmt!();
}

impl Glyphs {
    unsafe fn from_shaped_buffer(buffer: *mut hb_buffer_t) -> Self {
        let infos = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_infos(buffer, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        let positions = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_positions(buffer, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        assert_eq!(infos.len(), positions.len());

        Self {
            infos,
            positions,
            buffer,
        }
    }

    #[expect(dead_code)]
    pub fn get(&self, index: usize) -> Option<Glyph> {
        self.infos.get(index).map(|info| unsafe {
            let position = self.positions.get_unchecked(index);
            Glyph { info, position }
        })
    }

    #[expect(dead_code)]
    pub fn get_mut(&mut self, index: usize) -> Option<GlyphMut> {
        self.infos.get_mut(index).map(|info| unsafe {
            let position = self.positions.get_unchecked_mut(index);
            GlyphMut { info, position }
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = Glyph> + ExactSizeIterator + DoubleEndedIterator {
        (0..self.infos.len()).into_iter().map(|i| Glyph {
            info: &self.infos[i],
            position: &self.positions[i],
        })
    }

    pub fn compute_extents(&self, font: &Font) -> TextExtents {
        unsafe {
            let (_, hb_font) = font.with_applied_size_and_hb();

            let direction = hb_buffer_get_direction(self.buffer);

            let mut results = TextExtents {
                paint_height: 0,
                paint_width: 0,
            };

            let mut iterator = self.iter();

            if let Some(glyph) = iterator.next_back() {
                let mut extents = MaybeUninit::uninit();
                assert!(
                    hb_font_get_glyph_extents(hb_font, glyph.codepoint(), extents.as_mut_ptr(),)
                        > 0
                );
                let extents = extents.assume_init();
                results.paint_height += extents.height.abs();
                results.paint_width += extents.width;
            }

            for glyph in iterator {
                let mut extents = MaybeUninit::uninit();
                assert!(
                    hb_font_get_glyph_extents(hb_font, glyph.codepoint(), extents.as_mut_ptr()) > 0
                );
                let extents = extents.assume_init();
                if direction_is_horizontal(direction) {
                    results.paint_height = results.paint_height.max(extents.height.abs());
                    results.paint_width += glyph.x_advance();
                } else {
                    results.paint_width = results.paint_width.max(extents.width.abs());
                    results.paint_height += glyph.y_advance();
                }
            }

            results.paint_height /= 64;
            results.paint_width /= 64;

            results
        }
    }
}

impl Drop for Glyphs {
    fn drop(&mut self) {
        unsafe {
            hb_buffer_destroy(self.buffer);
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

#[inline(always)]
fn direction_is_horizontal(dir: hb_direction_t) -> bool {
    dir == hb_direction_t_HB_DIRECTION_LTR || dir == hb_direction_t_HB_DIRECTION_RTL
}

pub struct ShapingBuffer {
    buffer: *mut hb_buffer_t,
}

impl ShapingBuffer {
    pub fn new() -> Self {
        Self {
            buffer: unsafe { hb_buffer_create() },
        }
    }

    pub fn add(&mut self, text: &str) -> usize {
        unsafe {
            hb_buffer_add_utf8(
                self.buffer,
                text.as_ptr() as *const _,
                text.len() as i32,
                0,
                -1,
            );
            hb_buffer_get_length(self.buffer) as usize
        }
    }

    pub fn shape(self, font: &Font) -> Glyphs {
        let (_, hb_font) = font.with_applied_size_and_hb();

        unsafe {
            hb_buffer_guess_segment_properties(self.buffer);
            hb_shape(hb_font, self.buffer, std::ptr::null(), 0);

            Glyphs::from_shaped_buffer(ManuallyDrop::new(self).buffer)
        }
    }
}

impl Drop for ShapingBuffer {
    fn drop(&mut self) {
        unsafe {
            hb_buffer_destroy(self.buffer);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TextExtents {
    pub paint_height: i32,
    pub paint_width: i32,
}

impl TextRenderer {
    pub fn new() -> Self {
        Self {}
    }

    pub fn shape_text(&self, font: &Font, text: &str) -> Glyphs {
        let mut buffer = ShapingBuffer::new();
        buffer.add(text);
        buffer.shape(font)
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
        glyphs: &Glyphs,
        // In desired output buffer order, i.e. if the output buffer is supposed to be RGBA then this should also be RGBA
        color: [u8; 3],
        alpha: f32,
    ) {
        unsafe {
            let face = font.with_applied_size();

            let mut x = baseline_x as u32;
            let mut y = baseline_y as u32;
            for Glyph { info, position } in glyphs.iter() {
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
