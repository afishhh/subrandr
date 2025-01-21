use std::{collections::HashMap, path::PathBuf};

use thiserror::Error;

use crate::util::{AnyError, OrderedF32, Sealed};

use super::{Face, WEIGHT_AXIS};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontRequest {
    pub families: Vec<String>,
    pub weight: OrderedF32,
    pub italic: bool,
    pub codepoint: Option<u32>,
}

#[derive(Debug, Clone)]
struct FontInfo {
    family: String,
    weight: FontWeight,
    italic: bool,
    source: FontSource,
}

#[derive(Debug, Clone, Copy)]
enum FontWeight {
    Static(f32),
    Variable,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FontSource {
    File(PathBuf),
    // DirectWrite will have to get it from memory
}

impl FontSource {
    pub fn load(&self) -> Result<Face, AnyError> {
        match self {
            FontSource::File(file) => Ok(Face::load_from_file(file)),
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

#[path = ""]
mod provider {
    #[cfg(target_family = "unix")]
    #[path = "font_provider/fontconfig.rs"]
    pub mod fontconfig;

    use super::FontProvider;
    use crate::util::AnyError;

    pub fn platform_default() -> Result<Box<dyn FontProvider>, AnyError> {
        #[cfg(target_family = "unix")]
        {
            fontconfig::FontconfigFontProvider::new()
                .map(|x| Box::new(x) as Box<dyn FontProvider>)
                .map_err(Into::into)
        }
        #[cfg(not(target_family = "unix"))]
        todo!("no fontprovider available for current platform")
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
}

fn set_weight_if_variable(face: &mut Face, weight: f32) {
    if let Some(axis) = face.axis(WEIGHT_AXIS) {
        face.set_axis(axis.index, weight)
    }
}

impl FontSelect {
    pub fn new() -> Result<FontSelect, Error> {
        let provider: Box<dyn FontProvider> =
            provider::platform_default().map_err(Error::Provider)?;

        Ok(Self {
            source_cache: HashMap::new(),
            request_cache: HashMap::new(),
            provider,
        })
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
            let mut result = choose(
                &self.provider.query(request).map_err(Error::Provider)?,
                request,
            )
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
