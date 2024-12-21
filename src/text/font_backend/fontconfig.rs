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
    #[error("Failed to add codepoint {0:?} to FcCharSet")]
    AddChar(u32),
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
        name: Option<&str>,
        weight: f32,
        italic: bool,
        codepoint: Option<u32>,
    ) -> Result<Option<Face>, AnyError> {
        assert!(weight.is_normal());

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

        let Ok(fc_weight) = FONT_WEIGHTS
            .binary_search_by(|x| x.0.partial_cmp(&weight).unwrap())
            .map(|idx| FONT_WEIGHTS[idx].1)
        else {
            return Ok(None);
        };

        if let Some(name) = name {
            let cname = CString::new(name).map_err(|_| AnyError::from(LoadError::NullInName))?;
            pattern_add!(
                FcPatternAddString,
                PatternAddString,
                FC_FAMILY,
                cname.as_ptr() as *const u8,
                name.to_string()
            );
        }

        pattern_add!(
            FcPatternAddInteger,
            PatternAddInteger,
            FC_WEIGHT,
            fc_weight,
            fc_weight
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

        let mut result = MaybeUninit::uninit();
        let font_set = unsafe {
            FcFontSort(
                self.config,
                pattern,
                (codepoint.is_some() && name.is_none()) as i32,
                std::ptr::null_mut(),
                result.as_mut_ptr(),
            )
        };
        let result = unsafe { result.assume_init() };
        if result == FcResultNoMatch {
            return Ok(None);
        } else if result != FcResultMatch {
            return Err(LoadError::Match(result).into());
        }

        let fonts = unsafe {
            std::slice::from_raw_parts(
                (*font_set).fonts as *const *mut FcPattern,
                (*font_set).nfont as usize,
            )
        };

        struct Found {
            pattern: *mut FcPattern,
            family: &'static CStr,
            is_variable: bool,
        }
        let mut found = None::<Found>;

        for font in fonts.iter().copied() {
            unsafe {
                if let Some(codepoint) = codepoint {
                    let mut charset = std::ptr::null_mut();
                    if FcPatternGetCharSet(font, FC_CHARSET.as_ptr() as *const i8, 0, &mut charset)
                        != FcResultMatch
                    {
                        continue;
                    }

                    if FcCharSetHasChar(charset, codepoint) == 0 {
                        continue;
                    }
                }
            }

            let mut family = MaybeUninit::uninit();
            if unsafe {
                FcPatternGetString(
                    font,
                    FC_FAMILY.as_ptr() as *const i8,
                    0,
                    family.as_mut_ptr(),
                ) != FcResultMatch
            } {
                return Err(LoadError::NoFamily.into());
            }
            let family = unsafe { CStr::from_ptr(family.assume_init() as *const i8) };

            let is_variable = {
                let mut out = 0;
                unsafe {
                    FcPatternGetBool(font, FC_VARIABLE.as_ptr() as *const i8, 0, &mut out)
                        == FcResultMatch
                        && out > 0
                }
            };

            if found
                .as_ref()
                .is_none_or(|x| !x.is_variable && is_variable && family == x.family)
            {
                found = Some(Found {
                    pattern: font,
                    family,
                    is_variable,
                });

                if is_variable {
                    break;
                }
            }
        }

        let result = if let Some(Found { pattern, .. }) = found {
            let mut path = MaybeUninit::uninit();
            if unsafe {
                FcPatternGetString(pattern, FC_FILE.as_ptr() as *const i8, 0, path.as_mut_ptr())
                    != FcResultMatch
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

            let face = Face::load_from_file(owned_path);
            println!("font found for query name={name:?} weight={weight} italic={italic} codepoint={codepoint:?}: {face:?}");
            Some(face)
        } else {
            None
        };

        unsafe {
            FcPatternDestroy(pattern);
            FcFontSetDestroy(font_set);
        };

        Ok(result)
    }
}

impl FontBackend for FontconfigFontBackend {
    fn load_fallback(&mut self, weight: f32, italic: bool) -> Result<Option<Face>, AnyError> {
        self.load("sans-serif", weight, italic)
    }

    fn load(
        &mut self,
        name: &str,
        weight: f32,
        italic: bool,
    ) -> Result<Option<crate::text::Face>, AnyError> {
        self.load_internal(Some(name), weight, italic, None)
    }

    fn load_glyph_fallback(
        &mut self,
        weight: f32,
        italic: bool,
        codepoint: u32,
    ) -> Result<Option<Face>, AnyError> {
        // NOTE: Passing a family here does not seem to make substitutions work better
        self.load_internal(None, weight, italic, Some(codepoint))
    }
}

impl Sealed for FontconfigFontBackend {}
