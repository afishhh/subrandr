use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

use log::{warn, LogContext};
use thiserror::Error;

use super::PlatformFontProvider;
use crate::text::{
    font_db::{FaceInfo, FontSource},
    Face, FontFallbackRequest, FreeTypeError,
};

mod bindings;
use bindings::*;

#[derive(Debug)]
/// Font provider using the [Android NDK Font API] introduced in API level 29.
///
/// [Android NDK Font API]: https://developer.android.com/ndk/reference/group/font
pub struct AndroidNdkFontProvider {
    fonts: Vec<FaceInfo>,
}

#[derive(Error, Debug)]
pub enum NewError {
    #[error(transparent)]
    Update(#[from] UpdateError),
}

#[derive(Error, Debug)]
pub enum UpdateError {
    #[error(transparent)]
    CreateFontIterator(#[from] bindings::SystemFontIteratorOpenError),
}

#[derive(Error, Debug)]
pub enum FallbackError {
    #[error("Failed to open font")]
    OpenError(#[from] FreeTypeError),
}

impl AndroidNdkFontProvider {
    pub fn new(log: &LogContext) -> Result<Self, NewError> {
        Ok({
            let mut result = Self { fonts: Vec::new() };
            result.update_font_list(log)?;
            result
        })
    }
}

fn font_info_from_afont(afont: &AFont) -> Result<FaceInfo, FreeTypeError> {
    let path_ptr = afont.path();
    let index = afont.collection_index() as i32;
    let path = Path::new(OsStr::from_bytes(path_ptr.to_bytes()));
    let face = Face::load_from_file(path, index)?;

    Ok(FaceInfo::from_face_and_source(
        &face,
        FontSource::File {
            path: path.to_owned(),
            index,
        },
    ))
}

impl AndroidNdkFontProvider {
    fn update_font_list(&mut self, log: &LogContext) -> Result<(), UpdateError> {
        self.fonts.clear();

        for font in ASystemFontIterator::open()? {
            let info = match font_info_from_afont(&font) {
                Ok(info) => info,
                Err(err) => {
                    warn!(
                        log,
                        "Failed to inspect system font {:?}: {err}",
                        font.path()
                    );
                    continue;
                }
            };

            self.fonts.push(info);
        }

        Ok(())
    }
}

impl PlatformFontProvider for AndroidNdkFontProvider {
    fn update_if_changed(&mut self, _log: &LogContext) -> Result<bool, super::UpdateError> {
        // The NDK does not seem to provide a mechanism for detecting changes
        // to the system font list.
        Ok(false)
    }

    fn substitute(
        &self,
        _log: &LogContext,
        _request: &mut super::FaceRequest,
    ) -> Result<(), super::SubstituteError> {
        // The NDK does not seem to provide any substitution functionality.
        Ok(())
    }

    fn fonts(&self) -> &[FaceInfo] {
        &self.fonts
    }

    fn fallback(
        &self,
        request: &FontFallbackRequest,
    ) -> Result<Option<FaceInfo>, super::FallbackError> {
        let matcher = AFontMatcher::create();
        matcher.set_style(
            request.style.weight.round_to_inner() as u16,
            request.style.italic,
        );

        // TODO: Factor out the generic family check somewhere else
        let generic_family = request
            .families
            .iter()
            .find_map(|x| {
                Some(match &**x {
                    "sans-serif" => c"sans-serif",
                    "serif" => c"serif",
                    "monospace" => c"monospace",
                    "cursive" => c"cursive",
                    "fantasy" => c"fantasy",
                    _ => return None,
                })
            })
            .unwrap_or(c"sans-serif");

        let mut buffer = [0; 2];
        let text_utf16 = char::from_u32(request.codepoint)
            .map(|x| x.encode_utf16(&mut buffer))
            .unwrap_or(&mut []);
        let font = matcher.match_(generic_family, text_utf16, None);
        Ok(Some(
            font_info_from_afont(&font).map_err(FallbackError::OpenError)?,
        ))
    }
}

unsafe impl Send for AndroidNdkFontProvider {}
unsafe impl Sync for AndroidNdkFontProvider {}
