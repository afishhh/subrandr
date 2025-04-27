use std::{
    ffi::{c_char, c_int, CStr, CString, OsStr},
    mem::MaybeUninit,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
};

use text_sys::fontconfig::*;
use thiserror::Error;

use crate::{
    math::I16Dot16,
    text::{
        font_db::{FaceInfo, FontFallbackRequest, FontProvider, FontSource},
        FontAxisValues,
    },
    util::AnyError,
};

#[derive(Debug)]
pub struct FontconfigFontProvider {
    config: *mut FcConfig,
}

#[derive(Error, Debug)]
#[error("Failed to initialize fontconfig")]
pub struct NewError;

#[derive(Error, Debug)]
pub enum LoadError {
    #[error("Invalid weight")]
    InvalidWeight,

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

    #[error("Failed to create object set")]
    ObjectSetBuild,
    #[error("Failed to list fonts")]
    FontList,
}

impl FontconfigFontProvider {
    pub fn new() -> Result<Self, NewError> {
        unsafe {
            let config = FcInitLoadConfigAndFonts();
            if config.is_null() {
                return Err(NewError);
            }
            Ok(Self { config })
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

// https://gitlab.freedesktop.org/fontconfig/fontconfig/-/blob/main/src/fcweight.c
#[rustfmt::skip]
const WEIGHT_MAP: &[(I16Dot16, I16Dot16)] = &[
    (I16Dot16::new(FC_WEIGHT_THIN as i32), I16Dot16::new(100)),
    (I16Dot16::new(FC_WEIGHT_EXTRALIGHT as i32), I16Dot16::new(200)),
    (I16Dot16::new(FC_WEIGHT_LIGHT as i32), I16Dot16::new(300)),
    (I16Dot16::new(FC_WEIGHT_DEMILIGHT as i32), I16Dot16::new(350)),
    (I16Dot16::new(FC_WEIGHT_BOOK as i32), I16Dot16::new(380)),
    (I16Dot16::new(FC_WEIGHT_REGULAR as i32), I16Dot16::new(400)),
    (I16Dot16::new(FC_WEIGHT_MEDIUM as i32), I16Dot16::new(500)),
    (I16Dot16::new(FC_WEIGHT_SEMIBOLD as i32), I16Dot16::new(600)),
    (I16Dot16::new(FC_WEIGHT_BOLD as i32), I16Dot16::new(700)),
    (I16Dot16::new(FC_WEIGHT_EXTRABOLD as i32), I16Dot16::new(800)),
    (I16Dot16::new(FC_WEIGHT_BLACK as i32), I16Dot16::new(900)),
    (I16Dot16::new(FC_WEIGHT_EXTRABLACK as i32), I16Dot16::new(1000)),
];

fn map_fontconfig_weight_to_opentype(fc_weight: I16Dot16) -> Option<I16Dot16> {
    if fc_weight < 0 || fc_weight > I16Dot16::new(FC_WEIGHT_EXTRABLACK as i32) {
        return None;
    }

    let i = WEIGHT_MAP
        .binary_search_by(|x| x.0.partial_cmp(&fc_weight).unwrap())
        .map_or_else(std::convert::identity, std::convert::identity);

    if WEIGHT_MAP[i].0 == fc_weight {
        return Some(WEIGHT_MAP[i].1);
    }

    return Some({
        let fc_start = WEIGHT_MAP[i - 1].0;
        let fc_end = WEIGHT_MAP[i].0;
        let ot_start = WEIGHT_MAP[i - 1].1;
        let ot_end = WEIGHT_MAP[i].1;
        let fc_diff = fc_end - fc_start;
        let ot_diff = ot_end - ot_start;
        ot_start + (fc_weight - fc_start) * ot_diff / fc_diff
    });
}

fn map_opentype_weight_to_fontconfig(ot_weight: I16Dot16) -> Option<I16Dot16> {
    if ot_weight < 0 || ot_weight > 1000 {
        return None;
    }

    if ot_weight <= 100 {
        return Some(I16Dot16::new(FC_WEIGHT_THIN as i32));
    }

    let i = WEIGHT_MAP
        .binary_search_by(|x| x.1.partial_cmp(&ot_weight).unwrap())
        .map_or_else(std::convert::identity, std::convert::identity);

    if WEIGHT_MAP[i].1 == ot_weight {
        return Some(WEIGHT_MAP[i].0);
    }

    return Some({
        let fc_start = WEIGHT_MAP[i - 1].0;
        let fc_end = WEIGHT_MAP[i].0;
        let ot_start = WEIGHT_MAP[i - 1].1;
        let ot_end = WEIGHT_MAP[i].1;
        let fc_diff = fc_end - fc_start;
        let ot_diff = ot_end - ot_start;
        fc_start + (ot_weight - ot_start) * fc_diff / ot_diff
    });
}

#[cfg(test)]
mod test {
    use std::ops::Range;

    use super::*;

    #[test]
    fn test_fontconfig_to_opentype_weight_mapping() {
        for &(fc, ot) in &WEIGHT_MAP[1..] {
            assert_eq!(map_fontconfig_weight_to_opentype(fc), Some(ot));
        }

        assert_eq!(map_fontconfig_weight_to_opentype(I16Dot16::new(-1)), None);
        assert_eq!(map_fontconfig_weight_to_opentype(I16Dot16::new(300)), None);

        const LERP_CASES: &[(i32, Range<i32>)] = &[
            (30, 100..200),
            (60, 350..380),
            (213, 900..1000),
            (203, 700..800),
        ];

        for (fc, ot_range) in LERP_CASES.iter().map(|&(fc, Range { start, end })| {
            (I16Dot16::new(fc), I16Dot16::new(start)..I16Dot16::new(end))
        }) {
            println!("mapping {fc} to opentype, expecting a result in {ot_range:?}");
            let result = map_fontconfig_weight_to_opentype(fc).unwrap();
            println!("got: {result}");
            assert!(ot_range.contains(&result));
        }
    }

    #[test]
    fn test_opentype_to_fontconfig_weight_mapping() {
        for &(fc, ot) in WEIGHT_MAP {
            assert_eq!(map_opentype_weight_to_fontconfig(ot), Some(fc));
        }

        assert_eq!(map_opentype_weight_to_fontconfig(I16Dot16::new(-1)), None);
        assert_eq!(map_opentype_weight_to_fontconfig(I16Dot16::new(1100)), None);

        const LERP_CASES: &[(i32, Range<i32>)] = &[
            (150, FC_WEIGHT_THIN as i32..FC_WEIGHT_EXTRALIGHT as i32),
            (250, FC_WEIGHT_EXTRALIGHT as i32..FC_WEIGHT_LIGHT as i32),
            (375, FC_WEIGHT_DEMILIGHT as i32..FC_WEIGHT_BOOK as i32),
            (750, FC_WEIGHT_BOLD as i32..FC_WEIGHT_EXTRABOLD as i32),
            (950, FC_WEIGHT_BLACK as i32..FC_WEIGHT_EXTRABLACK as i32),
        ];

        for (fc, ot_range) in LERP_CASES.iter().map(|&(fc, Range { start, end })| {
            (I16Dot16::new(fc), I16Dot16::new(start)..I16Dot16::new(end))
        }) {
            println!("mapping {fc} to fontconfig, expecting a result in {ot_range:?}");
            let result = map_opentype_weight_to_fontconfig(fc).unwrap();
            println!("got: {result}");
            assert!(ot_range.contains(&result));
        }
    }
}

unsafe fn pattern_get_axis_values(
    pattern: *mut FcPattern,
    object: *const c_char,
) -> Option<FontAxisValues> {
    let mut wght = MaybeUninit::uninit();
    let mut range = MaybeUninit::uninit();
    if FcPatternGetInteger(pattern, object, 0, wght.as_mut_ptr()) == FcResultMatch {
        Some(FontAxisValues::Fixed(I16Dot16::new(wght.assume_init())))
    } else {
        if FcPatternGetRange(pattern, object, 0, range.as_mut_ptr()) == FcResultMatch {
            let range = range.assume_init();
            let mut begin = MaybeUninit::uninit();
            let mut end = MaybeUninit::uninit();
            if FcRangeGetDouble(range, begin.as_mut_ptr(), end.as_mut_ptr()) == 0 {
                None
            } else {
                let begin = begin.assume_init() as f32;
                let end = end.assume_init() as f32;
                Some(FontAxisValues::Range(
                    I16Dot16::from_f32(begin),
                    I16Dot16::from_f32(end),
                ))
            }
        } else {
            None
        }
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

unsafe fn font_info_from_pattern(pattern: *mut FcPattern) -> Option<FaceInfo> {
    let family = unsafe {
        let mut family = MaybeUninit::uninit();
        if FcPatternGetString(pattern, FC_FAMILY.as_ptr().cast(), 0, family.as_mut_ptr())
            != FcResultMatch
        {
            return None;
        }

        match CStr::from_ptr(family.assume_init().cast()).to_str() {
            Ok(family) => family,
            Err(_) => return None,
        }
    };

    let width = unsafe { pattern_get_axis_values(pattern, FC_WIDTH.as_ptr().cast())? };

    let weight = unsafe {
        pattern_get_axis_values(pattern, FC_WEIGHT.as_ptr().cast())
            .and_then(map_fontconfig_weight_axis_to_opentype)?
    };

    let italic = unsafe {
        let mut slant = 0;
        if FcPatternGetInteger(pattern, FC_SLANT.as_ptr().cast(), 0, &mut slant) != FcResultMatch {
            return None;
        }

        if slant == FC_SLANT_OBLIQUE as i32 {
            return None;
        }

        slant == FC_SLANT_ITALIC as i32
    };

    let path = unsafe {
        let mut path = MaybeUninit::uninit();
        if FcPatternGetString(pattern, FC_FILE.as_ptr().cast(), 0, path.as_mut_ptr())
            != FcResultMatch
        {
            return None;
        }

        PathBuf::from(OsStr::from_bytes(
            CStr::from_ptr(path.assume_init().cast()).to_bytes(),
        ))
    };

    let index = unsafe {
        let mut index = MaybeUninit::uninit();
        if FcPatternGetInteger(pattern, FC_INDEX.as_ptr().cast(), 0, index.as_mut_ptr())
            == FcResultMatch
        {
            #[allow(clippy::unnecessary_cast)]
            {
                index.assume_init() as i32
            }
        } else {
            0
        }
    };

    Some(FaceInfo {
        family: family.into(),
        width,
        weight,
        italic,
        source: FontSource::File { path, index },
    })
}

impl FontconfigFontProvider {
    fn query_by_param(
        &mut self,
        param_name: *const c_char,
        value: &CStr,
    ) -> Result<Vec<FaceInfo>, AnyError> {
        let pattern = unsafe { FcPatternCreate() };
        if pattern.is_null() {
            return Err(LoadError::PatternCreate.into());
        }

        unsafe { FcPatternAddString(pattern, param_name, value.as_ptr().cast()) };

        let object_set = unsafe {
            FcObjectSetBuild(
                FC_FAMILY.as_ptr().cast(),
                FC_SLANT,
                FC_WIDTH,
                FC_WEIGHT,
                FC_FILE,
            )
        };

        if object_set.is_null() {
            unsafe { FcPatternDestroy(pattern) };
            return Err(LoadError::ObjectSetBuild.into());
        }

        let font_set = unsafe { FcFontList(self.config, pattern, object_set) };

        if font_set.is_null() {
            unsafe {
                FcPatternDestroy(pattern);
                FcObjectSetDestroy(object_set)
            };
            return Err(LoadError::FontList.into());
        }

        if unsafe { (*font_set).nfont } == 0 {
            unsafe {
                FcPatternDestroy(pattern);
                FcFontSetDestroy(font_set);
                FcObjectSetDestroy(object_set)
            };
            return Ok(Vec::new());
        }

        let fonts = unsafe {
            std::slice::from_raw_parts(
                (*font_set).fonts as *const *mut FcPattern,
                (*font_set).nfont as usize,
            )
        };

        let mut results = Vec::new();

        for font in fonts.iter().copied() {
            let Some(info) = (unsafe { font_info_from_pattern(font) }) else {
                continue;
            };

            results.push(info);
        }

        unsafe {
            FcPatternDestroy(pattern);
            FcFontSetDestroy(font_set);
            FcObjectSetDestroy(object_set);
        };

        Ok(results)
    }
}

impl FontProvider for FontconfigFontProvider {
    fn query_fallback(
        &mut self,
        req: &FontFallbackRequest,
    ) -> Result<Vec<crate::text::font_db::FaceInfo>, AnyError> {
        // TODO: Free on error
        let pattern = unsafe { FcPatternCreate() };
        if pattern.is_null() {
            return Err(LoadError::PatternCreate.into());
        }

        macro_rules! pattern_add {
            ($cfun: ident, $err: ident, $key: ident, $value: expr, $errvalue: expr) => {
                #[allow(unused_unsafe)]
                if unsafe { $cfun(pattern, $key.as_ptr().cast(), $value) == 0 } {
                    return Err(LoadError::$err(
                        const { CStr::from_bytes_with_nul($key) }.unwrap(),
                        $errvalue,
                    )
                    .into());
                }
            };
        }

        let Some(fc_weight) = map_opentype_weight_to_fontconfig(req.style.weight) else {
            return Err(LoadError::InvalidWeight.into());
        };

        for family in &req.families {
            let Ok(cname) = CString::new(family.clone().into_string()) else {
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
            fc_weight.round_to_inner(),
            fc_weight.round_to_inner()
        );

        let slant = if req.style.italic {
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
                req.families.is_empty() as i32,
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
                let mut charset = std::ptr::null_mut();
                if FcPatternGetCharSet(font, FC_CHARSET.as_ptr().cast(), 0, &mut charset)
                    != FcResultMatch
                {
                    continue;
                }

                if FcCharSetHasChar(charset, req.codepoint) == 0 {
                    continue;
                }
            }

            let Some(info) = (unsafe { font_info_from_pattern(font) }) else {
                continue;
            };

            results.push(info);
        }

        unsafe {
            FcPatternDestroy(pattern);
            FcFontSetDestroy(font_set);
        };

        Ok(results)
    }

    fn query_family(&mut self, family: &str) -> Result<Vec<FaceInfo>, AnyError> {
        let Ok(cfamily) = CString::new(family) else {
            return Ok(Vec::new());
        };

        let properties: [&[u8]; 3] = [FC_FAMILY, FC_POSTSCRIPT_NAME, FC_FULLNAME];
        for property in properties {
            let result = self.query_by_param(property.as_ptr().cast(), &cfamily)?;
            if !result.is_empty() {
                return Ok(result);
            }
        }

        Ok(Vec::new())
    }
}
