use std::{collections::HashMap, fmt::Debug, hash::Hash, path::PathBuf, sync::Arc};

use thiserror::Error;
use util::math::I16Dot16;

use crate::{
    log::trace,
    text::{
        self,
        platform_font_provider::{
            self, FallbackError, LockedPlatformFontProvider, SubstituteError,
        },
    },
    Subrandr,
};

use super::{ft_utils::FreeTypeError, Face, WEIGHT_AXIS};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontFallbackRequest {
    pub families: Vec<Box<str>>,
    pub style: FontStyle,
    pub codepoint: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FaceRequest {
    pub families: Vec<Box<str>>,
    // TODO: script?
    pub language: Option<Box<str>>,
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
    pub family_names: Arc<[Arc<str>]>,
    // TODO: Implement font width support
    //       SRV3 (I think) and WebVTT-without-CSS don't need this but may be
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
    DirectWrite(platform_font_provider::directwrite::Source),
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
    Substitute(#[from] SubstituteError),
    #[error(transparent)]
    Fallback(#[from] FallbackError),
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
    provider: &'static LockedPlatformFontProvider,
    extra_faces: Vec<FaceInfo>,
    family_lookup_cache: HashMap<Box<str>, Vec<FaceInfo>>,
}

pub(super) fn set_weight_if_variable(face: &mut Face, weight: I16Dot16) {
    if let Some(axis) = face.axis(WEIGHT_AXIS) {
        face.set_axis(axis.index, weight)
    }
}

impl<'a> FontDb<'a> {
    pub fn new(sbr: &'a Subrandr) -> Result<FontDb<'a>, platform_font_provider::InitError> {
        Ok({
            let mut result = Self {
                sbr,
                source_cache: HashMap::new(),
                family_cache: HashMap::new(),
                request_cache: HashMap::new(),
                family_lookup_cache: HashMap::new(),
                provider: platform_font_provider::platform_default(sbr)?,
                extra_faces: Vec::new(),
            };
            result.rebuild_family_lookup_cache();
            result
        })
    }

    pub fn clear_extra(&mut self) {
        self.extra_faces.clear();
    }

    pub fn add_extra(&mut self, font: FaceInfo) {
        Self::add_to_family_lookup_cache(&mut self.family_lookup_cache, &font);
        self.extra_faces.push(font);
    }

    pub fn update_platform_font_list(&mut self) -> Result<(), platform_font_provider::UpdateError> {
        if self.provider.write().unwrap().update_if_changed(self.sbr)? {
            self.family_cache.clear();
            self.request_cache.clear();
            self.rebuild_family_lookup_cache();
        }

        Ok(())
    }

    fn add_to_family_lookup_cache(cache: &mut HashMap<Box<str>, Vec<FaceInfo>>, face: &FaceInfo) {
        for name in &*face.family_names {
            cache
                .entry(name.to_lowercase().into_boxed_str())
                .or_default()
                .push(face.clone());
        }
    }

    fn rebuild_family_lookup_cache(&mut self) {
        self.family_lookup_cache.clear();
        let provider = self.provider.read().unwrap();
        for face in &self.extra_faces {
            Self::add_to_family_lookup_cache(&mut self.family_lookup_cache, face);
        }
        for face in provider.fonts() {
            Self::add_to_family_lookup_cache(&mut self.family_lookup_cache, face)
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

    pub fn select_family(&mut self, name: &str) -> Result<&[FaceInfo], SelectError> {
        // NLL problem case 3 again
        let family_cache = &raw mut self.family_cache;
        if let Some(existing) = unsafe { (*family_cache).get(name) } {
            return Ok(existing);
        }

        trace!(self.sbr, "Substituting font family {name:?}");

        let mut request = FaceRequest {
            families: vec![name.into()],
            language: None,
        };

        self.provider
            .read()
            .unwrap()
            .substitute(self.sbr, &mut request)
            .map_err(SelectError::Substitute)?;

        trace!(self.sbr, "Substition resulted in {:?}", request.families);

        let mut result = None;
        for candidate in &request.families {
            let lowercase_name = candidate.to_lowercase();
            if let Some(faces) = self.family_lookup_cache.get(lowercase_name.as_str()) {
                result = Some(faces);
                break;
            }
        }

        let faces = match result {
            Some(faces) => {
                trace!(
                    self.sbr,
                    "Font family query {name:?} matched {} {:?} faces",
                    faces.len(),
                    faces[0].family_names[0]
                );
                faces.clone()
            }
            None => {
                trace!(self.sbr, "Font family query {name:?} matched no faces",);
                Vec::new()
            }
        };

        Ok(self
            .family_cache
            .entry(name.into())
            .insert_entry(faces)
            .into_mut())
    }

    pub fn select_fallback(&mut self, request: &FontFallbackRequest) -> Result<Face, SelectError> {
        if let Some(cached) = self.request_cache.get(request) {
            cached.as_ref().cloned()
        } else {
            trace!(
                self.sbr,
                "Querying font provider for font matching {request:?}"
            );
            let mut choices = self
                .provider
                .read()
                .unwrap()
                .fallback(request)
                .map_err(SelectError::Fallback)?;

            choices.extend(
                self.extra_faces
                    .iter()
                    .filter(|face| match &face.source {
                        FontSource::Memory(face) => face.contains_codepoint(request.codepoint),
                        _ => unreachable!(),
                    })
                    .cloned(),
            );

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
}
