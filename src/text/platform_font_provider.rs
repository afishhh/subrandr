use std::{fmt::Debug, sync::RwLock};

use once_cell::sync::OnceCell as OnceLock;
use thiserror::Error;

use crate::{
    text::{FaceInfo, FaceRequest, FontFallbackRequest},
    Subrandr,
};

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum InitError {
    #[error(transparent)]
    #[cfg(target_family = "unix")]
    Fontconfig(#[from] fontconfig::NewError),
    #[error(transparent)]
    #[cfg(target_family = "windows")]
    DirectWrite(#[from] directwrite::NewError),
}

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum UpdateError {
    #[error(transparent)]
    #[cfg(target_family = "unix")]
    Fontconfig(#[from] fontconfig::UpdateError),
    #[error(transparent)]
    #[cfg(target_family = "windows")]
    DirectWrite(#[from] directwrite::UpdateError),
}

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum SubstituteError {
    #[error(transparent)]
    #[cfg(target_family = "unix")]
    Fontconfig(#[from] fontconfig::SubstituteError),
    #[error(transparent)]
    #[cfg(target_family = "windows")]
    DirectWrite(#[from] directwrite::SubstituteError),
}

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum FallbackError {
    #[error(transparent)]
    #[cfg(target_family = "unix")]
    Fontconfig(#[from] fontconfig::FallbackError),
    #[error(transparent)]
    #[cfg(target_family = "windows")]
    DirectWrite(#[from] directwrite::FallbackError),
}

pub trait PlatformFontProvider: Debug + Send + Sync {
    fn update_if_changed(&mut self, sbr: &Subrandr) -> Result<bool, UpdateError> {
        _ = sbr;
        Ok(false)
    }

    fn substitute(&self, sbr: &Subrandr, request: &mut FaceRequest) -> Result<(), SubstituteError>;
    fn fonts(&self) -> &[FaceInfo];
    fn fallback(&self, request: &FontFallbackRequest) -> Result<Vec<FaceInfo>, FallbackError>;
}

#[cfg(target_family = "unix")]
pub mod fontconfig;

#[cfg(target_family = "windows")]
pub mod directwrite;

pub type LockedPlatformFontProvider = RwLock<dyn PlatformFontProvider>;

static PLATFORM_FONT_SOURCE: OnceLock<Box<LockedPlatformFontProvider>> = OnceLock::new();

fn init_platform_default(sbr: &Subrandr) -> Result<Box<LockedPlatformFontProvider>, InitError> {
    _ = sbr;

    #[cfg(target_family = "unix")]
    {
        fontconfig::FontconfigFontProvider::new()
            .map(|x| Box::new(RwLock::new(x)) as Box<LockedPlatformFontProvider>)
            .map_err(Into::into)
    }
    #[cfg(target_os = "windows")]
    {
        directwrite::DirectWriteFontProvider::new()
            .map(|x| Box::new(RwLock::new(x)) as Box<LockedPlatformFontProvider>)
            .map_err(Into::into)
    }
    #[cfg(not(any(target_family = "unix", target_os = "windows")))]
    {
        #[derive(Debug)]
        struct NullFontProvider;

        impl PlatformFontProvider for NullFontProvider {
            fn update_if_changed(&mut self, _sbr: &Subrandr) -> Result<bool, UpdateError> {
                Ok(false)
            }

            fn substitute(
                &self,
                _sbr: &Subrandr,
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
            ) -> Result<Vec<FaceInfo>, FallbackError> {
                Ok(Vec::new())
            }
        }

        static LOGGED_UNAVAILABLE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        if !LOGGED_UNAVAILABLE.fetch_or(true, std::sync::atomic::Ordering::Relaxed) {
            crate::log::warning!(
                sbr,
                "no default fontprovider available for current platform"
            );
        }

        Ok(Box::new(RwLock::new(NullFontProvider)) as Box<LockedPlatformFontProvider>)
    }
}

pub fn platform_default(sbr: &Subrandr) -> Result<&'static LockedPlatformFontProvider, InitError> {
    PLATFORM_FONT_SOURCE
        .get_or_try_init(|| init_platform_default(sbr))
        .map(|x| &**x)
}
