use std::{
    ffi::{c_double, c_int, CStr, CString, OsStr},
    mem::MaybeUninit,
    ops::RangeInclusive,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
};

use text_sys::fontconfig::*;
use thiserror::Error;
use util::math::I16Dot16;

use super::PlatformFontProvider;
use crate::{
    log::info,
    text::{
        font_db::{FaceInfo, FontSource},
        FontAxisValues, FontFallbackRequest,
    },
};

mod pattern;
mod weight;

use pattern::{Pattern, PatternAddError, PatternGetError, PatternRef, Value};
use weight::{map_fontconfig_weight_to_opentype, map_opentype_weight_to_fontconfig};

#[derive(Debug)]
pub struct FontconfigFontProvider {
    config: *mut FcConfig,
    fonts: Vec<FaceInfo>,
}

#[derive(Error, Debug)]
pub enum NewError {
    #[error("Failed to initialize fontconfig")]
    Init,
    #[error("Fontconfig is too old (older than 2.10.91)")]
    Outdated,
    #[error(transparent)]
    Update(#[from] UpdateError),
}

#[derive(Error, Debug)]
pub enum UpdateError {
    #[error("Failed to reload configuration")]
    BringUpToDate,
    #[error("Failed to list fonts")]
    List,
}

#[derive(Error, Debug)]
pub enum SubstituteError {
    #[error("Family contained null byte")]
    NulInFamily,
    #[error("Language contained null byte")]
    NulInLang,
    #[error(transparent)]
    PatternAdd(#[from] PatternAddError),
    #[error("Failed to get value from pattern: {0}")]
    PatternGet(#[from] PatternGetError),
    #[error("Failed execute substitutions")]
    Substitute,
}

#[derive(Error, Debug)]
pub enum FallbackError {
    #[error("OpenType weight out of range")]
    WeightOutOfRange,
    #[error(transparent)]
    PatternAdd(#[from] PatternAddError),
    #[error("Failed execute substitutions")]
    Substitute,
    #[error("Failed to sort fonts for fallback")]
    Sort,
}

impl FontconfigFontProvider {
    pub fn new() -> Result<Self, NewError> {
        unsafe {
            // Before this version fontconfig was not thread-safe, we share a single
            // global PlatformFontProvider so we want it to be thread safe.
            // This version was released 12 years ago so it's really just a sanity check.
            if FcGetVersion() < 21091 {
                return Err(NewError::Outdated);
            }

            let config = FcConfigGetCurrent();
            if config.is_null() {
                return Err(NewError::Init);
            }

            Ok({
                let mut result = Self {
                    config,
                    fonts: Vec::new(),
                };
                result.update_font_list()?;
                result
            })
        }
    }
}

impl Drop for FontconfigFontProvider {
    fn drop(&mut self) {
        unsafe {
            FcConfigDestroy(self.config);
            FcFini();
        };
    }
}

unsafe fn pattern_get_axis_values(
    pattern: &Pattern,
    name: &CStr,
) -> Result<FontAxisValues, PatternGetError> {
    match pattern.get::<c_int>(name, 0) {
        #[allow(clippy::unnecessary_cast)]
        Ok(value) => Ok(FontAxisValues::Fixed(I16Dot16::new(value as i32))),
        Err(PatternGetError::TypeMismatch) => {
            let range = pattern.get::<RangeInclusive<c_double>>(name, 0)?;
            Ok(FontAxisValues::Range(
                I16Dot16::from_f32(*range.start() as f32),
                I16Dot16::from_f32(*range.end() as f32),
            ))
        }
        Err(error) => Err(error),
    }
}

fn map_fontconfig_weight_axis_to_opentype(axis: FontAxisValues) -> Option<FontAxisValues> {
    Some(match axis {
        FontAxisValues::Fixed(value) => {
            FontAxisValues::Fixed(map_fontconfig_weight_to_opentype(value)?)
        }
        FontAxisValues::Range(start, end) => FontAxisValues::Range(
            map_fontconfig_weight_to_opentype(start)?,
            map_fontconfig_weight_to_opentype(end)?,
        ),
    })
}

unsafe fn font_info_from_pattern(pattern: &Pattern) -> Option<FaceInfo> {
    let mut family_names = Vec::new();
    for i in 0.. {
        match pattern.get::<&CStr>(c"family", i) {
            Ok(family) => {
                if let Ok(family) = family.to_str() {
                    family_names.push(family.into());
                }
            }
            Err(PatternGetError::NoId) => break,
            Err(_) => continue,
        }
    }

    if family_names.is_empty() {
        return None;
    }

    let width = pattern_get_axis_values(pattern, c"width").ok()?;

    let weight =
        map_fontconfig_weight_axis_to_opentype(pattern_get_axis_values(pattern, c"weight").ok()?)?;

    let italic = {
        let slant = pattern.get::<c_int>(c"slant", 0).ok()?;

        if slant == FC_SLANT_OBLIQUE as i32 {
            return None;
        }

        slant == FC_SLANT_ITALIC as i32
    };

    let path = PathBuf::from(OsStr::from_bytes(
        pattern.get::<&CStr>(c"file", 0).ok()?.to_bytes(),
    ));

    #[allow(clippy::unnecessary_cast)]
    let index = pattern.get::<c_int>(c"index", 0).ok()? as i32;

    Some(FaceInfo {
        family_names: family_names.into(),
        width,
        weight,
        italic,
        source: FontSource::File { path, index },
    })
}

impl FontconfigFontProvider {
    fn update_font_list(&mut self) -> Result<(), UpdateError> {
        let pattern = Pattern::new();

        let font_set =
            unsafe { FcFontList(self.config, pattern.as_mut_ptr(), std::ptr::null_mut()) };
        if font_set.is_null() {
            return Err(UpdateError::List);
        }

        let fonts = unsafe {
            std::slice::from_raw_parts(
                (*font_set).fonts as *const *mut FcPattern,
                (*font_set).nfont as usize,
            )
        };

        self.fonts.clear();

        for font in fonts.iter().copied() {
            let font = PatternRef::from_raw(font);

            let Some(info) = (unsafe { font_info_from_pattern(&font) }) else {
                continue;
            };

            self.fonts.push(info);
        }

        unsafe {
            FcFontSetDestroy(font_set);
        };

        Ok(())
    }
}

impl PlatformFontProvider for FontconfigFontProvider {
    fn update_if_changed(&mut self, sbr: &crate::Subrandr) -> Result<bool, super::UpdateError> {
        if unsafe { FcInitBringUptoDate() } == FcFalse as FcBool {
            return Err(UpdateError::BringUpToDate.into());
        }

        let current = unsafe { FcConfigGetCurrent() };
        Ok(if current != self.config {
            info!(sbr, "Fontconfig configuration updated, reloading font list");
            self.config = current;
            self.update_font_list()?;
            true
        } else {
            false
        })
    }

    fn substitute(
        &self,
        _sbr: &crate::Subrandr,
        request: &mut super::FaceRequest,
    ) -> Result<(), super::SubstituteError> {
        let mut pattern = Pattern::new();

        for family in &request.families {
            let c_family =
                CString::new(family.as_bytes()).map_err(|_| SubstituteError::NulInFamily)?;
            pattern
                .add(c"family", pattern::Value::String(&c_family), true)
                .map_err(SubstituteError::from)?;
        }

        if let Some(lang) = &request.language {
            let c_lang = CString::new(lang.as_bytes()).map_err(|_| SubstituteError::NulInLang)?;
            pattern
                .add(c"lang", pattern::Value::String(&c_lang), true)
                .map_err(SubstituteError::from)?;
        }

        if unsafe { FcConfigSubstitute(self.config, pattern.as_mut_ptr(), FcMatchPattern) }
            == FcFalse as FcBool
        {
            return Err(SubstituteError::Substitute.into());
        }

        request.families.clear();

        for i in 0.. {
            match pattern.get_with_binding(c"family", i) {
                // Treat weak bindings as fallback fonts we don't want
                #[expect(non_upper_case_globals)]
                Ok((_, FcValueBindingWeak)) => {}
                Ok((Value::String(family), _)) => {
                    if let Ok(family) = family.to_str() {
                        request.families.push(family.into());
                    }
                }
                Ok((_, _)) => {
                    return Err(SubstituteError::PatternGet(PatternGetError::TypeMismatch).into())
                }
                Err(PatternGetError::NoId) => break,
                Err(error) => return Err(SubstituteError::from(error).into()),
            }
        }

        Ok(())
    }

    fn fonts(&self) -> &[FaceInfo] {
        &self.fonts
    }

    fn fallback(
        &self,
        request: &FontFallbackRequest,
    ) -> Result<Option<FaceInfo>, super::FallbackError> {
        let mut pattern = Pattern::new();

        for family in &request.families {
            let Ok(c_family) = CString::new(family.clone().into_string()) else {
                continue;
            };

            pattern
                .add(c"family", Value::String(&c_family), true)
                .map_err(FallbackError::from)?
        }

        let Some(fc_weight) = map_opentype_weight_to_fontconfig(request.style.weight) else {
            return Err(FallbackError::WeightOutOfRange.into());
        };

        pattern
            .add(c"weight", Value::Integer(fc_weight.round_to_inner()), false)
            .map_err(FallbackError::from)?;

        let slant = if request.style.italic {
            FC_SLANT_ITALIC as c_int
        } else {
            FC_SLANT_ROMAN as c_int
        };

        pattern
            .add(c"slant", Value::Integer(slant), false)
            .map_err(FallbackError::from)?;

        if unsafe { FcConfigSubstitute(self.config, pattern.as_mut_ptr(), FcMatchPattern) == 0 } {
            return Err(FallbackError::Substitute.into());
        }

        unsafe { FcDefaultSubstitute(pattern.as_mut_ptr()) };

        let mut result = MaybeUninit::uninit();
        let font_set = unsafe {
            FcFontSort(
                self.config,
                pattern.as_mut_ptr(),
                FcFalse as FcBool,
                std::ptr::null_mut(),
                result.as_mut_ptr(),
            )
        };
        let result = unsafe { result.assume_init() };
        if result == FcResultNoMatch {
            return Ok(None);
        } else if result != FcResultMatch {
            return Err(FallbackError::Sort.into());
        }

        let fonts = unsafe {
            std::slice::from_raw_parts(
                (*font_set).fonts as *const *mut FcPattern,
                (*font_set).nfont as usize,
            )
        };

        let mut result = None;

        for font in fonts.iter().copied() {
            let font = PatternRef::from_raw(font);

            unsafe {
                let Ok(charset) = font.get::<*mut FcCharSet>(c"charset", 0) else {
                    continue;
                };

                if FcCharSetHasChar(charset, request.codepoint) == 0 {
                    continue;
                }
            }

            let Some(info) = (unsafe { font_info_from_pattern(&font) }) else {
                continue;
            };

            result = Some(info);
            break;
        }

        unsafe {
            FcFontSetDestroy(font_set);
        };

        Ok(result)
    }
}

unsafe impl Send for FontconfigFontProvider {}
unsafe impl Sync for FontconfigFontProvider {}
