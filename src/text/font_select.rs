use std::{collections::HashMap, hash::Hash, path::PathBuf};

use thiserror::Error;

use crate::{
    text,
    util::{AnyError, OrderedF32, Sealed},
    Subrandr,
};

use super::{Face, WEIGHT_AXIS};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontRequest {
    pub families: Vec<String>,
    pub weight: OrderedF32,
    pub italic: bool,
    pub codepoint: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct FontInfo {
    pub family: String,
    pub weight: FontWeight,
    pub italic: bool,
    pub source: FontSource,
}

#[derive(Debug, Clone, Copy)]
pub enum FontWeight {
    Static(f32),
    Variable,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FontSource {
    File(PathBuf),
    Memory(text::Face),
}

impl FontSource {
    pub fn load(&self) -> Result<Face, AnyError> {
        match self {
            Self::File(file) => Ok(Face::load_from_file(file)),
            Self::Memory(face) => Ok(face.clone()),
        }
    }
}

fn choose<'a>(fonts: &'a [FontInfo], request: &FontRequest) -> Option<&'a FontInfo> {
    let mut score = u32::MAX;
    let mut result = None;

    for font in fonts {
        let mut this_score = 0;

        if font.italic && !request.italic {
            this_score += 4;
        } else if !font.italic && request.italic {
            this_score += 1;
        }

        match font.weight {
            FontWeight::Static(weight) => {
                this_score += (weight - request.weight.0).abs() as u32 / 100;
            }
            FontWeight::Variable => (),
        }

        if this_score < score {
            result = Some(font);
            score = this_score;
        }
    }

    result
}

trait FontProvider: Sealed + std::fmt::Debug {
    fn query(&mut self, request: &FontRequest) -> Result<Vec<FontInfo>, AnyError>;
}

#[derive(Debug)]
struct NullFontProvider;

impl Sealed for NullFontProvider {}

impl FontProvider for NullFontProvider {
    fn query(&mut self, _request: &FontRequest) -> Result<Vec<FontInfo>, AnyError> {
        Ok(Vec::new())
    }
}

#[path = ""]
mod provider {
    #[cfg(target_family = "unix")]
    #[path = "font_provider/fontconfig.rs"]
    pub mod fontconfig;

    use super::FontProvider;
    use crate::{util::AnyError, Subrandr};

    #[cfg(not(target_family = "unix"))]
    static LOGGED_UNAVAILABLE: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    pub fn platform_default(_sbr: &Subrandr) -> Result<Box<dyn FontProvider>, AnyError> {
        #[cfg(target_family = "unix")]
        {
            fontconfig::FontconfigFontProvider::new()
                .map(|x| Box::new(x) as Box<dyn FontProvider>)
                .map_err(Into::into)
        }
        #[cfg(not(target_family = "unix"))]
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
pub enum Error {
    #[error("An error occurred while querying system font information")]
    Provider(#[source] AnyError),
    #[error("Failed to load font")]
    FailedToLoadFont(#[source] /*TODO: super::Error */ AnyError),
    #[error("No font found")]
    NotFound,
}

#[derive(Debug)]
pub struct FontSelect {
    source_cache: HashMap<FontSource, Face>,
    request_cache: HashMap<FontRequest, Option<Face>>,
    provider: Box<dyn FontProvider>,
    custom: Vec<FontInfo>,
}

fn set_weight_if_variable(face: &mut Face, weight: f32) {
    if let Some(axis) = face.axis(WEIGHT_AXIS) {
        face.set_axis(axis.index, weight)
    }
}

impl FontSelect {
    pub fn new(sbr: &Subrandr) -> Result<FontSelect, Error> {
        let provider: Box<dyn FontProvider> =
            provider::platform_default(sbr).map_err(Error::Provider)?;

        Ok(Self {
            source_cache: HashMap::new(),
            request_cache: HashMap::new(),
            provider,
            custom: Vec::new(),
        })
    }

    pub fn clear_extra(&mut self) {
        self.custom.clear();
    }

    pub fn add_extra(&mut self, font: FontInfo) {
        self.custom.push(font);
    }

    pub fn advance_cache_generation(&mut self) {
        for face in self.request_cache.values().filter_map(Option::as_ref) {
            face.glyph_cache().advance_generation();
        }
    }

    pub fn select(&mut self, request: &FontRequest) -> Result<Face, Error> {
        if let Some(cached) = self.request_cache.get(request) {
            cached.as_ref().cloned()
        } else {
            let mut choices = self.provider.query(request).map_err(Error::Provider)?;

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

            let mut result = choose(&choices, request)
                .map(|x| {
                    if let Some(cached) = self.source_cache.get(&x.source) {
                        Ok(cached.clone())
                    } else {
                        let loaded = x.source.load().map_err(Error::FailedToLoadFont)?;
                        self.source_cache.insert(x.source.clone(), loaded.clone());
                        Ok(loaded)
                    }
                })
                .transpose()?;

            if let Some(ref mut face) = result {
                set_weight_if_variable(face, request.weight.0);
            }

            self.request_cache.insert(request.clone(), result.clone());
            result
        }
        .ok_or(Error::NotFound)
    }

    pub fn select_simple(&mut self, name: &str, weight: f32, italic: bool) -> Result<Face, Error> {
        self.select(&FontRequest {
            families: vec![name.to_owned()],
            weight: OrderedF32(weight),
            italic,
            codepoint: None,
        })
    }
}
