use std::{collections::HashMap, hash::Hash, path::PathBuf};

use thiserror::Error;

use crate::{
    math::I16Dot16,
    text, trace,
    util::{AnyError, Sealed},
    Subrandr,
};

use super::{ft_utils::FreeTypeError, Face, WEIGHT_AXIS};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontRequest {
    pub families: Vec<Box<str>>,
    pub style: FontStyle,
    pub codepoint: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontStyle {
    pub weight: I16Dot16,
    pub italic: bool,
}

impl Default for FontStyle {
    fn default() -> Self {
        Self {
            weight: I16Dot16::new(400),
            italic: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FaceInfo {
    pub family: Box<str>,
    // TODO: Implement font width support
    //       SRV3 (I think) and WebVTT-without-CSS don't need this but will be
    //       necessary in the future
    #[expect(dead_code)]
    pub width: FontAxisValues,
    pub weight: FontAxisValues,
    pub italic: bool,
    pub source: FontSource,
}

#[derive(Debug, Clone)]
pub enum FontAxisValues {
    Fixed(I16Dot16),
    Range(I16Dot16, I16Dot16),
}

impl FontAxisValues {
    pub fn minimum(&self) -> I16Dot16 {
        match self {
            &FontAxisValues::Fixed(fixed) => fixed,
            &FontAxisValues::Range(start, _) => start,
        }
    }

    pub fn maximum(&self) -> I16Dot16 {
        match self {
            &FontAxisValues::Fixed(fixed) => fixed,
            &FontAxisValues::Range(_, end) => end,
        }
    }

    pub fn contains(&self, value: I16Dot16) -> bool {
        match self {
            &FontAxisValues::Fixed(fixed) => fixed == value,
            &FontAxisValues::Range(start, end) => start <= value && value <= end,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FontSource {
    File { path: PathBuf, index: i32 },
    Memory(text::Face),
}

impl FontSource {
    pub fn load(&self) -> Result<Face, FreeTypeError> {
        match self {
            &Self::File { ref path, index } => Ok(Face::load_from_file(path, index)?),
            Self::Memory(face) => Ok(face.clone()),
        }
    }
}

fn choose<'a>(fonts: &'a [FaceInfo], style: &FontStyle) -> Option<&'a FaceInfo> {
    let mut score = u32::MAX;
    let mut result = None;

    for font in fonts {
        let mut this_score = 0;

        if font.italic && !style.italic {
            this_score += 4;
        } else if !font.italic && style.italic {
            this_score += 1;
        }

        match &font.weight {
            &FontAxisValues::Fixed(weight) => {
                this_score += (weight - style.weight).unsigned_abs().round_to_inner() / 100;
            }
            &FontAxisValues::Range(start, end) => {
                if style.weight < start || style.weight > end {
                    this_score += ((start - style.weight).unsigned_abs().round_to_inner() / 100)
                        .min((end - style.weight).unsigned_abs().round_to_inner() / 100);
                }
            }
        }

        if this_score < score {
            result = Some(font);
            score = this_score;
        }
    }

    result
}

trait FontProvider: Sealed + std::fmt::Debug {
    fn query(&mut self, request: &FontRequest) -> Result<Vec<FaceInfo>, AnyError>;
    fn query_family(&mut self, family: &str) -> Result<Vec<FaceInfo>, AnyError>;
}

#[derive(Debug)]
// This is only used on platforms where no native font provider is available.
#[cfg_attr(target_family = "unix", expect(dead_code))]
struct NullFontProvider;

impl Sealed for NullFontProvider {}

impl FontProvider for NullFontProvider {
    fn query(&mut self, _request: &FontRequest) -> Result<Vec<FaceInfo>, AnyError> {
        Ok(Vec::new())
    }

    fn query_family(&mut self, _family: &str) -> Result<Vec<FaceInfo>, AnyError> {
        Ok(Vec::new())
    }
}

#[path = ""]
mod provider {
    #[cfg(any(target_family = "unix", target_os = "windows"))]
    #[path = "font_provider/fontconfig.rs"]
    pub mod fontconfig;

    use super::FontProvider;
    use crate::{util::AnyError, Subrandr};

    #[cfg(not(target_family = "unix"))]
    static LOGGED_UNAVAILABLE: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    pub fn platform_default(_sbr: &Subrandr) -> Result<Box<dyn FontProvider>, AnyError> {
        #[cfg(any(target_family = "unix", target_os = "windows"))]
        {
            fontconfig::FontconfigFontProvider::new()
                .map(|x| Box::new(x) as Box<dyn FontProvider>)
                .map_err(Into::into)
        }
        #[cfg(not(any(target_family = "unix", target_os = "windows")))]
        {
            if !LOGGED_UNAVAILABLE.fetch_or(true, std::sync::atomic::Ordering::Relaxed) {
                crate::log::warning!(
                    _sbr,
                    "no default fontprovider available for current platform"
                );
            }
            Ok(Box::new(super::NullFontProvider))
        }
    }
}

#[derive(Debug, Error)]
pub enum SelectError {
    #[error(transparent)]
    // TODO: enum
    Provider(#[from] AnyError),
    #[error("Failed to load font: {0}")]
    Load(#[from] FreeTypeError),
    #[error("No font found")]
    NotFound,
}

#[derive(Debug)]
pub struct FontDb<'a> {
    sbr: &'a Subrandr,
    source_cache: HashMap<FontSource, Face>,
    family_cache: HashMap<Box<str>, Vec<FaceInfo>>,
    request_cache: HashMap<FontRequest, Option<Face>>,
    provider: Box<dyn FontProvider>,
    custom: Vec<FaceInfo>,
}

pub(super) fn set_weight_if_variable(face: &mut Face, weight: I16Dot16) {
    if let Some(axis) = face.axis(WEIGHT_AXIS) {
        face.set_axis(axis.index, weight)
    }
}

impl<'a> FontDb<'a> {
    pub fn new(sbr: &'a Subrandr) -> Result<FontDb<'a>, SelectError> {
        let provider: Box<dyn FontProvider> =
            provider::platform_default(sbr).map_err(SelectError::Provider)?;

        Ok(Self {
            sbr,
            source_cache: HashMap::new(),
            family_cache: HashMap::new(),
            request_cache: HashMap::new(),
            provider,
            custom: Vec::new(),
        })
    }

    pub fn clear_extra(&mut self) {
        self.custom.clear();
    }

    pub fn add_extra(&mut self, font: FaceInfo) {
        self.custom.push(font);
    }

    pub fn advance_cache_generation(&mut self) {
        for face in self.request_cache.values().filter_map(Option::as_ref) {
            face.glyph_cache().advance_generation();
        }
    }

    pub fn open(&mut self, face: &FaceInfo) -> Result<Face, SelectError> {
        if let Some(cached) = self.source_cache.get(&face.source) {
            Ok(cached.clone())
        } else {
            let loaded = face.source.load().map_err(SelectError::Load)?;
            self.source_cache
                .insert(face.source.clone(), loaded.clone());
            Ok(loaded)
        }
    }

    pub fn select(&mut self, request: &FontRequest) -> Result<Face, SelectError> {
        if let Some(cached) = self.request_cache.get(request) {
            cached.as_ref().cloned()
        } else {
            trace!(
                self.sbr,
                "Querying font provider for font matching {request:?}"
            );
            let mut choices = self
                .provider
                .query(request)
                .map_err(SelectError::Provider)?;

            let custom_start = choices.len();
            choices.extend(self.custom.iter().cloned());

            choices[custom_start..].sort_by_cached_key(|font| {
                let score = request
                    .families
                    .iter()
                    .position(|rf| font.family.eq_ignore_ascii_case(rf))
                    .unwrap_or(usize::MAX);

                score
            });

            let mut result = choose(&choices, &request.style)
                .map(|x| self.open(x))
                .transpose()?;

            if let Some(ref mut face) = result {
                set_weight_if_variable(face, request.style.weight);
            }

            trace!(
                self.sbr,
                "Picked face {result:?} from {} choices",
                choices.len()
            );
            self.request_cache.insert(request.clone(), result.clone());
            result
        }
        .ok_or(SelectError::NotFound)
    }

    pub fn query_by_name(&mut self, name: &str) -> Result<&[FaceInfo], SelectError> {
        // NLL problem case 3 again
        let family_cache = &raw mut self.family_cache;
        if let Some(existing) = unsafe { (*family_cache).get(name) } {
            return Ok(existing);
        }

        let result = self
            .provider
            .query_family(name)
            .map_err(SelectError::Provider)?;
        Ok(unsafe {
            (*family_cache)
                .entry(name.into())
                .insert_entry(result)
                .into_mut()
        })
    }
}
