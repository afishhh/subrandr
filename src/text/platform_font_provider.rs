use std::{fmt::Debug, sync::RwLock};

use log::LogContext;
use once_cell::sync::OnceCell as OnceLock;
use thiserror::Error;

use crate::text::{FaceInfo, FaceRequest, FontFallbackRequest};

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum InitError {
    #[error(transparent)]
    #[cfg(font_provider = "fontconfig")]
    Fontconfig(#[from] fontconfig::NewError),
    #[error(transparent)]
    #[cfg(font_provider = "directwrite")]
    DirectWrite(#[from] directwrite::NewError),
    #[error(transparent)]
    #[cfg(font_provider = "android-ndk")]
    AndroidNdk(#[from] ndk::NewError),
}

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum UpdateError {
    #[error(transparent)]
    #[cfg(font_provider = "fontconfig")]
    Fontconfig(#[from] fontconfig::UpdateError),
    #[error(transparent)]
    #[cfg(font_provider = "directwrite")]
    DirectWrite(#[from] directwrite::UpdateError),
}

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum SubstituteError {
    #[error(transparent)]
    #[cfg(font_provider = "fontconfig")]
    Fontconfig(#[from] fontconfig::SubstituteError),
    #[error(transparent)]
    #[cfg(font_provider = "directwrite")]
    DirectWrite(#[from] directwrite::SubstituteError),
}

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum FallbackError {
    #[error(transparent)]
    #[cfg(font_provider = "fontconfig")]
    Fontconfig(#[from] fontconfig::FallbackError),
    #[error(transparent)]
    #[cfg(font_provider = "directwrite")]
    DirectWrite(#[from] directwrite::FallbackError),
    #[error(transparent)]
    #[cfg(font_provider = "android-ndk")]
    AndroidNdk(#[from] ndk::FallbackError),
}

// TODO: Remove or change `FontSource::Memory`? Currently the `Send + Sync` bound is impossible
//       to statically guarantee because `FontSource::Memory` does not fullfil it (but font
//       providers basically must always store a `Vec<FaceInfo>`).
//       It's probably best if `FontSource::Memory` just stores an `Arc<[u8]>` instead of
//       a `Face`.
pub trait PlatformFontProvider: Debug + Send + Sync {
    fn update_if_changed(&mut self, log: &LogContext) -> Result<bool, UpdateError> {
        _ = log;
        Ok(false)
    }

    fn substitute(
        &self,
        log: &LogContext,
        request: &mut FaceRequest,
    ) -> Result<(), SubstituteError>;
    fn fonts(&self) -> &[FaceInfo];
    fn fallback(&self, request: &FontFallbackRequest) -> Result<Option<FaceInfo>, FallbackError>;
}

#[cfg(font_provider = "fontconfig")]
pub mod fontconfig;

#[cfg(font_provider = "directwrite")]
pub mod directwrite;

#[cfg(font_provider = "android-ndk")]
pub mod ndk;

// This is only used on certain configurations and the `cfg()` for that would be very long.
#[allow(dead_code)]
pub mod null {
    use super::*;

    #[derive(Debug)]
    pub struct NullFontProvider;

    impl PlatformFontProvider for NullFontProvider {
        fn update_if_changed(&mut self, _log: &LogContext) -> Result<bool, UpdateError> {
            Ok(false)
        }

        fn substitute(
            &self,
            _log: &LogContext,
            _request: &mut FaceRequest,
        ) -> Result<(), SubstituteError> {
            Ok(())
        }

        fn fonts(&self) -> &[FaceInfo] {
            &[]
        }

        fn fallback(
            &self,
            _request: &FontFallbackRequest,
        ) -> Result<Option<FaceInfo>, FallbackError> {
            Ok(None)
        }
    }
}

pub type LockedPlatformFontProvider = RwLock<dyn PlatformFontProvider>;

static PLATFORM_FONT_SOURCE: OnceLock<Box<LockedPlatformFontProvider>> = OnceLock::new();

fn init_platform_default(log: &LogContext) -> Result<Box<LockedPlatformFontProvider>, InitError> {
    _ = log;

    #[cfg(all(font_provider = "fontconfig", not(font_provider = "android-ndk")))]
    {
        fontconfig::FontconfigFontProvider::new()
            .map(|x| Box::new(RwLock::new(x)) as Box<LockedPlatformFontProvider>)
            .map_err(Into::into)
    }
    #[cfg(font_provider = "directwrite")]
    {
        directwrite::DirectWriteFontProvider::new(log)
            .map(|x| Box::new(RwLock::new(x)) as Box<LockedPlatformFontProvider>)
            .map_err(Into::into)
    }
    #[cfg(font_provider = "android-ndk")]
    {
        ndk::AndroidNdkFontProvider::new(log)
            .map(|x| Box::new(RwLock::new(x)) as Box<LockedPlatformFontProvider>)
            .map_err(Into::into)
    }
    #[cfg(not(any(
        font_provider = "fontconfig",
        font_provider = "directwrite",
        font_provider = "android-ndk"
    )))]
    {
        static LOGGED_UNAVAILABLE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        if !LOGGED_UNAVAILABLE.fetch_or(true, std::sync::atomic::Ordering::Relaxed) {
            log::warn!(
                log,
                "no default fontprovider available for current platform"
            );
        }

        Ok(Box::new(RwLock::new(null::NullFontProvider)) as Box<LockedPlatformFontProvider>)
    }
}

pub fn platform_default(
    log: &LogContext,
) -> Result<&'static LockedPlatformFontProvider, InitError> {
    PLATFORM_FONT_SOURCE
        .get_or_try_init(|| init_platform_default(log))
        .map(|x| &**x)
}
