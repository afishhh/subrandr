use std::{
    ffi::{c_int, CStr, CString, OsString},
    mem::MaybeUninit,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
};

use text_sys::unix::*;
use thiserror::Error;

use crate::{
    text::{Face, FontBackend},
    util::{AnyError, Sealed},
};

// TODO: FcFini
#[derive(Debug)]
pub struct FontconfigFontBackend {
    config: *mut FcConfig,
}

#[derive(Error, Debug)]
#[error("Failed to initialize fontconfig")]
pub struct NewError(());

#[derive(Error, Debug)]
pub enum LoadError {
    #[error("Family name contains null byte")]
    NullInName,
    #[error("Failed to create pattern")]
    PatternCreate,
    #[error("Failed to set pattern key {0:?} to {1:?}")]
    PatternAddString(&'static CStr, String),
    #[error("Failed to set pattern key {0:?} to {1}")]
    PatternAddInteger(&'static CStr, c_int),
    #[error("Failed to execute substitutions")]
    Substitute,
    #[error("Failed to find matching font: {0:?}")]
    Match(FcResult),
    #[error("Match did not contain a family name")]
    NoFamily,
    #[error("Match did not contain a file path")]
    NoFile,
    // TODO: FtError or whatever we'll use in text
    #[error("Failed to load font")]
    Load(),
}

impl FontconfigFontBackend {
    pub fn new() -> Result<Self, NewError> {
        unsafe {
            let config = FcInitLoadConfigAndFonts();
            if config.is_null() {
                return Err(NewError(()));
            }
            Ok(Self { config })
        }
    }

    pub fn fallback_font(&self) -> String {
        "sans-serif".to_string()
    }
}

const FONT_WEIGHTS: &[(f32, c_int)] = &[
    (400.0, FC_WEIGHT_REGULAR as c_int),
    (700.0, FC_WEIGHT_BOLD as c_int),
];

impl FontconfigFontBackend {
    fn load_internal(
        &mut self,
        name: &str,
        weight: f32,
        italic: bool,
        check_result_name: bool,
    ) -> Result<Option<Face>, AnyError> {
        assert!(weight.is_normal());

        println!(
            "fontconfig backend load_internal({:?}, {}, {:?}, {:?})",
            name, weight, italic, check_result_name
        );

        // TODO: Free on error
        let pattern = unsafe { FcPatternCreate() };
        if pattern.is_null() {
            return Err(LoadError::PatternCreate.into());
        }

        macro_rules! pattern_add {
            ($cfun: ident, $err: ident, $key: ident, $value: expr, $errvalue: expr) => {
                if unsafe { $cfun(pattern, $key.as_ptr() as *const i8, $value) == 0 } {
                    return Err(LoadError::$err(
                        const { CStr::from_bytes_with_nul($key) }.unwrap(),
                        $errvalue,
                    )
                    .into());
                }
            };
        }

        let Ok(weight) = FONT_WEIGHTS.binary_search_by(|x| x.0.partial_cmp(&weight).unwrap())
        else {
            return Ok(None);
        };

        let cname = CString::new(name).map_err(|_| AnyError::from(LoadError::NullInName))?;
        pattern_add!(
            FcPatternAddString,
            PatternAddString,
            FC_FAMILY,
            cname.as_ptr() as *const u8,
            name.to_string()
        );

        pattern_add!(
            FcPatternAddString,
            PatternAddString,
            FC_FONTFORMAT,
            // TODO: Specify all formats freetype supports
            c"TrueType".as_ptr() as *const u8,
            "TrueType".to_string()
        );

        pattern_add!(
            FcPatternAddInteger,
            PatternAddInteger,
            FC_WEIGHT,
            weight as i32,
            weight as i32
        );

        let style = if italic { c"Italic" } else { c"Regular" };
        pattern_add!(
            FcPatternAddString,
            PatternAddString,
            FC_STYLE,
            style.as_ptr() as *const u8,
            style.to_str().unwrap().to_string()
        );

        if unsafe { FcConfigSubstitute(self.config, pattern, FcMatchPattern) == 0 } {
            return Err(LoadError::Substitute.into());
        }

        unsafe { FcDefaultSubstitute(pattern) };

        // unsafe {
        //     FcPatternPrint(pattern);
        // }

        let mut result = MaybeUninit::uninit();
        let prepared = unsafe { FcFontMatch(self.config, pattern, result.as_mut_ptr()) };
        // unsafe {
        //     FcPatternPrint(prepared);
        // }
        let result = unsafe { result.assume_init() };
        if result == FcResultNoMatch {
            return Ok(None);
        } else if result != FcResultMatch {
            return Err(LoadError::Match(result).into());
        }

        // if check_result_name {
        //     let mut out = MaybeUninit::uninit();
        //     if unsafe {
        //         FcPatternGetString(
        //             prepared,
        //             FC_FAMILY.as_ptr() as *const i8,
        //             0,
        //             out.as_mut_ptr(),
        //         ) != FcResultMatch
        //     } {
        //         return Err(LoadError::NoFamily.into());
        //     }
        //     let out = unsafe { CStr::from_ptr(out.assume_init() as *const i8) };

        //     if out.to_bytes() != name.as_bytes() {
        //         return Ok(None);
        //     }
        // }

        let mut path = MaybeUninit::uninit();
        if unsafe {
            FcPatternGetString(
                prepared,
                FC_FILE.as_ptr() as *const i8,
                0,
                path.as_mut_ptr(),
            ) != FcResultMatch
        } {
            return Err(LoadError::NoFile.into());
        }

        let owned_path = unsafe {
            PathBuf::from(OsString::from_vec(
                CStr::from_ptr(path.assume_init() as *const _)
                    .to_bytes()
                    .to_vec(),
            ))
        };

        unsafe {
            FcPatternDestroy(pattern);
            FcPatternDestroy(prepared);
        };

        Ok(Some(Face::load_from_file(owned_path)))
    }
}

impl FontBackend for FontconfigFontBackend {
    fn load_fallback(&mut self, weight: f32, italic: bool) -> Result<Option<Face>, AnyError> {
        self.load_internal("sans-serif", weight, italic, false)
    }

    fn load(
        &mut self,
        name: &str,
        weight: f32,
        italic: bool,
    ) -> Result<Option<crate::text::Face>, AnyError> {
        self.load_internal(name, weight, italic, true)
    }
}

impl Sealed for FontconfigFontBackend {}
