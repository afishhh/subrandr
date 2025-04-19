use std::{
    fmt::{Debug, Display},
    num::NonZero,
    sync::Mutex,
};

use once_cell::sync::OnceCell;
use text_sys::*;

#[derive(Debug)]
pub struct FreeTypeError(NonZero<FT_Error>);

impl FreeTypeError {
    pub(super) fn from_ft(code: NonZero<FT_Error>) -> Self {
        Self(code)
    }

    pub(super) fn result_from_ft(code: FT_Error) -> Result<(), Self> {
        if let Some(error) = std::num::NonZero::new(code) {
            Err(Self::from_ft(error))
        } else {
            Ok(())
        }
    }
}

impl Display for FreeTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match text_sys::FREETYPE_ERRORS
            .iter()
            .find_map(|&(c, msg)| (c == self.0.get()).then_some(msg))
        {
            Some(msg) => f.write_str(msg),
            None => write!(f, "FreeType error {:#X}", self.0),
        }
    }
}

impl std::error::Error for FreeTypeError {}

macro_rules! fttry {
    ($expr: expr) => {
        $crate::text::ft_utils::FreeTypeError::result_from_ft($expr)
    };
}

pub(crate) use fttry;

pub struct Library {
    pub ptr: FT_Library,
    // [Since 2.5.6] In multi-threaded applications it is easiest to use one FT_Library object per thread. In case this is too cumbersome, a single FT_Library object across threads is possible also, as long as a mutex lock is used around FT_New_Face and FT_Done_Face.
    pub face_mutation_mutex: Mutex<()>,
}

static FT_LIBRARY: OnceCell<Library> = OnceCell::new();

impl Library {
    pub fn get_or_init() -> Result<&'static Self, FreeTypeError> {
        FT_LIBRARY.get_or_try_init(|| unsafe {
            let mut ft = std::ptr::null_mut();
            fttry!(FT_Init_FreeType(&mut ft))?;
            Ok(Self {
                ptr: ft,
                face_mutation_mutex: Mutex::default(),
            })
        })
    }
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}
