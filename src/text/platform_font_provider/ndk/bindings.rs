use std::{ffi::CStr, ptr::NonNull};

use thiserror::Error;

pub struct AFont(NonNull<ndk_sys::AFont>);

impl AFont {
    pub fn path(&self) -> &CStr {
        unsafe { CStr::from_ptr(ndk_sys::AFont_getFontFilePath(self.0.as_ptr())) }
    }

    pub fn collection_index(&self) -> usize {
        unsafe { ndk_sys::AFont_getCollectionIndex(self.0.as_ptr()) }
    }
}

impl Drop for AFont {
    fn drop(&mut self) {
        unsafe { ndk_sys::AFont_close(self.0.as_ptr()) };
    }
}

pub struct ASystemFontIterator(NonNull<ndk_sys::ASystemFontIterator>);

#[derive(Debug, Error)]
#[error("Failed to open Android system font iterator")]
pub struct SystemFontIteratorOpenError;

impl ASystemFontIterator {
    pub fn open() -> Result<Self, SystemFontIteratorOpenError> {
        NonNull::new(unsafe { ndk_sys::ASystemFontIterator_open() })
            .map(ASystemFontIterator)
            .ok_or(SystemFontIteratorOpenError)
    }
}

impl Iterator for ASystemFontIterator {
    type Item = AFont;

    fn next(&mut self) -> Option<Self::Item> {
        NonNull::new(unsafe { ndk_sys::ASystemFontIterator_next(self.0.as_ptr()) }).map(AFont)
    }
}

impl Drop for ASystemFontIterator {
    fn drop(&mut self) {
        unsafe { ndk_sys::ASystemFontIterator_close(self.0.as_ptr()) };
    }
}

pub struct AFontMatcher(NonNull<ndk_sys::AFontMatcher>);

impl AFontMatcher {
    pub fn create() -> Self {
        Self(unsafe { NonNull::new_unchecked(ndk_sys::AFontMatcher_create()) })
    }

    pub fn set_style(&self, weight: u16, italic: bool) {
        unsafe { ndk_sys::AFontMatcher_setStyle(self.0.as_ptr(), weight, italic) };
    }

    pub fn match_(
        &self,
        family_name: &CStr,
        text: &[u16],
        run_length_out: Option<&mut u32>,
    ) -> AFont {
        unsafe {
            AFont(NonNull::new_unchecked(ndk_sys::AFontMatcher_match(
                self.0.as_ptr(),
                family_name.as_ptr(),
                text.as_ptr(),
                text.len() as u32,
                run_length_out.map_or(std::ptr::null_mut(), |x| x),
            )))
        }
    }
}

impl Drop for AFontMatcher {
    fn drop(&mut self) {
        unsafe { ndk_sys::AFontMatcher_destroy(self.0.as_ptr()) };
    }
}
