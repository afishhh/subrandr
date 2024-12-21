use std::sync::{Mutex, OnceLock};

use text_sys::*;

macro_rules! fttry {
    ($expr: expr) => {
        let code = $expr;
        #[allow(unused_unsafe)]
        if code != 0 {
            panic!(
                "ft error: 0x{code:X} {:?}",
                text_sys::FREETYPE_ERRORS
                    .iter()
                    .find_map(|&(c, msg)| (c == code).then_some(msg))
            )
        }
    };
}

pub(crate) use fttry;

pub struct Library {
    pub ptr: FT_Library,
    // [Since 2.5.6] In multi-threaded applications it is easiest to use one FT_Library object per thread. In case this is too cumbersome, a single FT_Library object across threads is possible also, as long as a mutex lock is used around FT_New_Face and FT_Done_Face.
    pub face_mutation_mutex: Mutex<()>,
}

static FT_LIBRARY: OnceLock<Library> = OnceLock::new();

impl Library {
    pub fn get_or_init() -> &'static Self {
        FT_LIBRARY.get_or_init(|| unsafe {
            let mut ft = std::ptr::null_mut();
            fttry!(FT_Init_FreeType(&mut ft));
            Self {
                ptr: ft,
                face_mutation_mutex: Mutex::default(),
            }
        })
    }
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}

pub fn f32_to_fractional_points(value: f32) -> FT_F26Dot6 {
    (value * 64.0).round() as FT_F26Dot6
}

pub fn fractional_points_to_f32(value: FT_F26Dot6) -> f32 {
    value as f32 / 64.0
}

pub fn f32_to_fixed_point(value: f32) -> FT_Fixed {
    (value * 65536.0).round() as FT_Fixed
}

pub fn fixed_point_to_f32(value: FT_Fixed) -> f32 {
    value as f32 / 65536.0
}
