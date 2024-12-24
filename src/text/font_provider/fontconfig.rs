use std::{
    ffi::{c_int, CStr, CString, OsString},
    mem::MaybeUninit,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
};

use text_sys::unix::*;
use thiserror::Error;

use crate::{
    text::font_select::{FontInfo, FontProvider, FontRequest, FontSource, FontWeight},
    util::{AnyError, Sealed},
};

// TODO: FcFini
#[derive(Debug)]
pub struct FontconfigFontProvider {
    config: *mut FcConfig,
}

#[derive(Error, Debug)]
#[error("Failed to initialize fontconfig")]
pub struct NewError(());

#[derive(Error, Debug)]
pub enum LoadError {
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
}

impl FontconfigFontProvider {
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

impl Drop for FontconfigFontProvider {
    fn drop(&mut self) {
        unsafe {
            FcConfigDestroy(self.config);
            FcFini()
        };
    }
}

const FONT_WEIGHTS: &[(f32, c_int)] = &[
    (400.0, FC_WEIGHT_REGULAR as c_int),
    (700.0, FC_WEIGHT_BOLD as c_int),
];

impl FontProvider for FontconfigFontProvider {
    fn query(
        &mut self,
        req: &FontRequest,
    ) -> Result<Vec<crate::text::font_select::FontInfo>, AnyError> {
        assert!(req.weight.0.is_normal() && req.weight.0.is_sign_positive());

        // TODO: Free on error
        let pattern = unsafe { FcPatternCreate() };
        if pattern.is_null() {
            return Err(LoadError::PatternCreate.into());
        }

        macro_rules! pattern_add {
            ($cfun: ident, $err: ident, $key: ident, $value: expr, $errvalue: expr) => {
                #[allow(unused_unsafe)]
                if unsafe { $cfun(pattern, $key.as_ptr() as *const i8, $value) == 0 } {
                    return Err(LoadError::$err(
                        const { CStr::from_bytes_with_nul($key) }.unwrap(),
                        $errvalue,
                    )
                    .into());
                }
            };
        }

        // TODO: closest match
        let Ok(fc_weight) = FONT_WEIGHTS
            .binary_search_by(|x| x.0.partial_cmp(&req.weight.0).unwrap())
            .map(|idx| FONT_WEIGHTS[idx].1)
        else {
            todo!();
        };

        for family in &req.families {
            let Ok(cname) = CString::new(family.clone()) else {
                continue;
            };

            pattern_add!(
                FcPatternAddString,
                PatternAddString,
                FC_FAMILY,
                cname.as_ptr() as *const u8,
                family.to_string()
            );
        }

        pattern_add!(
            FcPatternAddInteger,
            PatternAddInteger,
            FC_WEIGHT,
            fc_weight,
            fc_weight
        );

        let slant = if req.italic {
            FC_SLANT_ITALIC as i32
        } else {
            FC_SLANT_ROMAN as i32
        };

        pattern_add!(
            FcPatternAddInteger,
            PatternAddInteger,
            FC_SLANT,
            slant,
            slant
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
                (req.codepoint.is_some() && req.families.is_empty()) as i32,
                std::ptr::null_mut(),
                result.as_mut_ptr(),
            )
        };
        let result = unsafe { result.assume_init() };
        if result == FcResultNoMatch {
            return Ok(Vec::new());
        } else if result != FcResultMatch {
            return Err(LoadError::Match(result).into());
        }

        let fonts = unsafe {
            std::slice::from_raw_parts(
                (*font_set).fonts as *const *mut FcPattern,
                (*font_set).nfont as usize,
            )
        };

        let mut results = Vec::new();

        for font in fonts.iter().copied() {
            unsafe {
                if let Some(codepoint) = req.codepoint {
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

            let family = unsafe {
                let mut family = MaybeUninit::uninit();
                if FcPatternGetString(
                    font,
                    FC_FAMILY.as_ptr() as *const i8,
                    0,
                    family.as_mut_ptr(),
                ) != FcResultMatch
                {
                    continue;
                }

                match CStr::from_ptr(family.assume_init() as *const i8).to_str() {
                    Ok(family) => family,
                    Err(_) => continue,
                }
            };

            let weight = unsafe {
                let mut wght = MaybeUninit::uninit();
                let mut range = MaybeUninit::uninit();
                if FcPatternGetInteger(font, FC_WEIGHT.as_ptr() as *const i8, 0, wght.as_mut_ptr())
                    == FcResultMatch
                {
                    let wght = wght.assume_init();

                    if let Some(weight) = FONT_WEIGHTS
                        .iter()
                        .find_map(|&(ot, fc)| (fc == wght).then_some(ot))
                        .map(FontWeight::Static)
                    {
                        weight
                    } else {
                        continue;
                    }
                } else {
                    let is_variable = if FcPatternGetRange(
                        font,
                        FC_WEIGHT.as_ptr() as *const i8,
                        0,
                        range.as_mut_ptr(),
                    ) == FcResultMatch
                    {
                        let range = range.assume_init();
                        let mut begin = MaybeUninit::uninit();
                        let mut end = MaybeUninit::uninit();
                        if FcRangeGetDouble(range, begin.as_mut_ptr(), end.as_mut_ptr()) == 0 {
                            false
                        } else {
                            let begin = begin.assume_init();
                            let end = end.assume_init();
                            begin.abs() < f64::EPSILON && (210.0 - end).abs() < f64::EPSILON
                        }
                    } else {
                        false
                    };

                    if is_variable {
                        FontWeight::Variable
                    } else {
                        continue;
                    }
                }
            };

            let italic = unsafe {
                let mut slant = 0;
                if FcPatternGetInteger(font, FC_SLANT.as_ptr() as *const i8, 0, &mut slant)
                    != FcResultMatch
                {
                    continue;
                }

                if slant == FC_SLANT_OBLIQUE as i32 {
                    continue;
                }

                slant == FC_SLANT_ITALIC as i32
            };

            let path = unsafe {
                let mut path = MaybeUninit::uninit();
                if FcPatternGetString(font, FC_FILE.as_ptr() as *const i8, 0, path.as_mut_ptr())
                    != FcResultMatch
                {
                    continue;
                }

                PathBuf::from(OsString::from_vec(
                    CStr::from_ptr(path.assume_init() as *const _)
                        .to_bytes()
                        .to_vec(),
                ))
            };

            results.push(FontInfo {
                family: family.to_owned(),
                weight,
                italic,
                source: FontSource::File(path),
            });
        }

        unsafe {
            FcPatternDestroy(pattern);
            FcFontSetDestroy(font_set);
        };

        Ok(results)
    }
}

impl Sealed for FontconfigFontProvider {}
