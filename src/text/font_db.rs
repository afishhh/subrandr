use std::{cell::RefCell, collections::HashMap, hash::Hash, path::PathBuf, rc::Rc};

use text::panose;
use thiserror::Error;
use util::{math::I16Dot16, AnyError};

use crate::{log::trace, text, Subrandr};

use super::{ft_utils::FreeTypeError, Face, WEIGHT_AXIS};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontFallbackRequest {
    pub families: Vec<Box<str>>,
    pub style: FontStyle,
    pub codepoint: u32,
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

impl FaceInfo {
    fn from_face(face: Face) -> Self {
        Self {
            family: face.family_name().into(),
            // TODO: fetch this from the font
            width: FontAxisValues::Fixed(I16Dot16::ZERO),
            weight: face.weight_range(),
            // TODO: italic_range()
            italic: face.italic(),
            source: FontSource::Memory(face),
        }
    }
}

#[derive(Debug, Clone)]
pub enum FontAxisValues {
    Fixed(I16Dot16),
    Range(I16Dot16, I16Dot16),
}

impl FontAxisValues {
    pub fn minimum(&self) -> I16Dot16 {
        match *self {
            FontAxisValues::Fixed(fixed) => fixed,
            FontAxisValues::Range(start, _) => start,
        }
    }

    pub fn maximum(&self) -> I16Dot16 {
        match *self {
            FontAxisValues::Fixed(fixed) => fixed,
            FontAxisValues::Range(_, end) => end,
        }
    }

    pub fn contains(&self, value: I16Dot16) -> bool {
        match *self {
            FontAxisValues::Fixed(fixed) => fixed == value,
            FontAxisValues::Range(start, end) => start <= value && value <= end,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FontSource {
    #[cfg_attr(not(target_family = "unix"), expect(dead_code))]
    File {
        path: PathBuf,
        index: i32,
    },
    Memory(text::Face),
    #[cfg(target_os = "windows")]
    DirectWrite(provider::directwrite::Source),
}

impl FontSource {
    pub fn load(&self) -> Result<Face, LoadError> {
        match self {
            &Self::File { ref path, index } => Ok(Face::load_from_file(path, index)?),
            Self::Memory(face) => Ok(face.clone()),
            #[cfg(target_os = "windows")]
            Self::DirectWrite(source) => source.open(),
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

        match font.weight {
            FontAxisValues::Fixed(weight) => {
                this_score += (weight - style.weight).unsigned_abs().round_to_inner() / 100;
            }
            FontAxisValues::Range(start, end) => {
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

trait FontProvider: std::fmt::Debug {
    fn query_fallback(&mut self, request: &FontFallbackRequest) -> Result<Vec<FaceInfo>, AnyError>;
    fn query_family(&mut self, family: &str) -> Result<Vec<FaceInfo>, AnyError>;
}

#[path = ""]
mod provider {
    #[cfg(target_family = "unix")]
    #[path = "font_provider/fontconfig.rs"]
    pub mod fontconfig;

    #[cfg(target_family = "windows")]
    #[path = "font_provider/directwrite.rs"]
    pub mod directwrite;

    use util::AnyError;

    use super::FontProvider;
    use crate::Subrandr;

    pub fn platform_default(_sbr: &Subrandr) -> Result<Box<dyn FontProvider>, AnyError> {
        #[cfg(target_family = "unix")]
        {
            fontconfig::FontconfigFontProvider::new()
                .map(|x| Box::new(x) as Box<dyn FontProvider>)
                .map_err(Into::into)
        }
        #[cfg(target_os = "windows")]
        {
            directwrite::DirectWriteFontProvider::new()
                .map(|x| Box::new(x) as Box<dyn FontProvider>)
                .map_err(Into::into)
        }
        #[cfg(not(any(target_family = "unix", target_os = "windows")))]
        {
            static LOGGED_UNAVAILABLE: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);

            if !LOGGED_UNAVAILABLE.fetch_or(true, std::sync::atomic::Ordering::Relaxed) {
                crate::log::warning!(
                    _sbr,
                    "no default fontprovider available for current platform"
                );
            }

            #[derive(Debug)]
            struct NullFontProvider;

            impl FontProvider for NullFontProvider {
                fn query_fallback(
                    &mut self,
                    _request: &super::FontFallbackRequest,
                ) -> Result<Vec<super::FaceInfo>, AnyError> {
                    Ok(Vec::new())
                }

                fn query_family(
                    &mut self,
                    _family: &str,
                ) -> Result<Vec<super::FaceInfo>, AnyError> {
                    Ok(Vec::new())
                }
            }

            Ok(Box::new(NullFontProvider))
        }
    }
}

#[derive(Debug)]
pub struct CustomFontProvider {
    faces: Vec<CustomFontInfo>,
    /// Fonts sorted by serif type (serif fonts first).
    serif_fallback: Vec<(Option<SerifType>, FaceInfo)>,
    /// Fonts sorted by serif type (sans serif fonts first).
    sans_serif_fallback: Vec<(Option<SerifType>, FaceInfo)>,
    /// Fonts sorted by proportionality (monospace fonts first).
    monospace_fallback: Vec<(Option<bool>, FaceInfo)>,
    /// Fonts keyed on lowercase name.
    by_name: HashMap<Box<str>, Vec<FaceInfo>>,
    /// If true then the fallback lists may be unsorted and have to be sorted
    /// before being used.
    dirty: bool,
}

#[derive(Debug)]
struct CustomFontInfo {
    face: Face,
    monospace: Option<bool>,
    serif: Option<SerifType>,
}

impl CustomFontInfo {
    fn classify(face: text::freetype::Face) -> Self {
        let mut monospace = None;
        let mut serif = None;

        if let Some(panose) = face.panose() {
            match panose {
                panose::Classification::LatinText(text) => {
                    monospace = Some(matches!(text.proportion, panose::Proportion::Monospaced));
                    serif = Some(if text.serif_style.is_sans_serif() {
                        SerifType::Sans
                    } else {
                        SerifType::Serif
                    })
                }
                _ => (),
            }
        }

        Self {
            monospace,
            serif,
            face: Face::FreeType(face),
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum SerifType {
    Sans,
    Serif,
}

impl CustomFontProvider {
    pub fn new() -> Self {
        Self {
            faces: Vec::new(),
            serif_fallback: Vec::new(),
            sans_serif_fallback: Vec::new(),
            monospace_fallback: Vec::new(),
            by_name: HashMap::new(),
            dirty: false,
        }
    }

    pub fn add_font(&mut self, face: text::freetype::Face) {
        let full_info = CustomFontInfo::classify(face);
        let face_info = FaceInfo::from_face(full_info.face.clone());

        self.serif_fallback
            .push((full_info.serif, face_info.clone()));
        self.sans_serif_fallback
            .push((full_info.serif, face_info.clone()));
        self.monospace_fallback
            .push((full_info.monospace, face_info.clone()));
        self.faces.push(full_info);
        self.dirty = true;
    }

    fn sort_if_dirty(&mut self) {
        if self.dirty {
            self.dirty = false;
        }
    }

    fn has_last_resort_fallback_for_family(family: &str) -> bool {
        matches!(family, "monospace" | "sans-serif" | "serif")
    }

    fn last_resort_query_generic_family(
        &mut self,
        family: &str,
    ) -> Result<Vec<FaceInfo>, AnyError> {
        self.sort_if_dirty();

        let mut result = Vec::new();

        // Attempt to put together possible candidates for generic family names so we don't
        // just fail to find these if no platform font provider is available.
        //
        // TODO: Make an API for setting this explicitly, although a fallback is still good
        // TODO: Better heuristics?
        match family {
            "monospace" => {
                for (name, face) in &self.by_name {
                    if name.contains("mono") {
                        result.extend(face.iter().cloned());
                    }
                }
            }
            "sans-serif" => {
                for (name, face) in &self.by_name {
                    if name.contains("sans") {
                        result.extend(face.iter().cloned());
                    }
                }
            }
            "serif" => {
                for (name, face) in &self.by_name {
                    if name.contains("serif") {
                        result.extend(face.iter().cloned());
                    }
                }
            }
            // TODO: srv3 also uses "cursive" although I have no idea how to fallback sensibly
            //       for it.
            _ => (),
        }

        Ok(result)
    }
}

impl FontProvider for CustomFontProvider {
    fn query_fallback(&mut self, request: &FontFallbackRequest) -> Result<Vec<FaceInfo>, AnyError> {
        self.sort_if_dirty();

        let mut result = Vec::new();

        for info in &mut self.faces {
            if !info.face.contains_codepoint(request.codepoint) {
                continue;
            }

            result.push(FaceInfo::from_face(info.face.clone()));
        }

        result.sort_by_cached_key(|font| {
            let score = request
                .families
                .iter()
                .position(|rf| font.family.eq_ignore_ascii_case(rf))
                .unwrap_or(usize::MAX);

            score
        });

        Ok(result)
    }

    fn query_family(&mut self, family: &str) -> Result<Vec<FaceInfo>, AnyError> {
        Ok(self
            .by_name
            .get(family.to_lowercase().as_str())
            .map_or_else(Vec::new, |v| v.clone()))
    }
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error(transparent)]
    #[cfg(target_os = "windows")]
    DirectWrite(#[from] windows::core::Error),
    #[error(transparent)]
    FreeType(#[from] FreeTypeError),
}

#[derive(Debug, Error)]
pub enum SelectError {
    #[error(transparent)]
    // TODO: enum
    Provider(#[from] AnyError),
    #[error("Failed to load font: {0}")]
    Load(#[from] LoadError),
    #[error("No font found")]
    NotFound,
}

impl From<FreeTypeError> for SelectError {
    fn from(value: FreeTypeError) -> Self {
        Self::Load(LoadError::FreeType(value))
    }
}

#[derive(Debug)]
pub struct FontDb<'a> {
    sbr: &'a Subrandr,
    source_cache: HashMap<FontSource, Face>,
    family_cache: HashMap<Box<str>, Vec<FaceInfo>>,
    request_cache: HashMap<FontFallbackRequest, Option<Face>>,
    provider: Box<dyn FontProvider>,
    custom: Option<Rc<RefCell<CustomFontProvider>>>,
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
            custom: None,
        })
    }

    pub fn set_custom_font_provider(&mut self, provider: Option<Rc<RefCell<CustomFontProvider>>>) {
        self.custom = provider;
    }

    pub fn advance_cache_generation(&mut self) {
        for face in self.source_cache.values() {
            face.advance_cache_generation();
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

    pub fn select_fallback(&mut self, request: &FontFallbackRequest) -> Result<Face, SelectError> {
        if let Some(cached) = self.request_cache.get(request) {
            cached.as_ref().cloned()
        } else {
            trace!(
                self.sbr,
                "Querying font provider for font matching {request:?}"
            );

            let mut choices = Vec::new();

            if let Some(provider) = &self.custom {
                choices = provider
                    .borrow_mut()
                    .query_fallback(request)
                    .map_err(SelectError::Provider)?;
            }

            if choices.is_empty() {
                choices = self
                    .provider
                    .query_fallback(request)
                    .map_err(SelectError::Provider)?;
            }

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

        trace!(self.sbr, "Querying font provider for font family {name:?}");

        let mut result = Vec::new();

        if let Some(provider) = &self.custom {
            result = provider
                .borrow_mut()
                .query_family(name)
                .map_err(SelectError::Provider)?;
        }

        if result.is_empty() {
            result = self
                .provider
                .query_family(name)
                .map_err(SelectError::Provider)?;
        }

        trace!(self.sbr, "Font family query {name:?} returned {result:?}");

        if result.is_empty() && CustomFontProvider::has_last_resort_fallback_for_family(name) {
            if let Some(provider) = &self.custom {
                trace!(
                    self.sbr,
                    "Querying custom font provider for generic font family fallback {name:?}"
                );

                result = provider
                    .borrow_mut()
                    .last_resort_query_generic_family(name)?;
            }
        }

        Ok(self
            .family_cache
            .entry(name.into())
            .insert_entry(result)
            .into_mut())
    }
}
