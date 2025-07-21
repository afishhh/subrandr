use std::ffi::c_void;
use std::hash::Hash;
use std::sync::Arc;

use util::math::I16Dot16;
use windows::core::{implement, Interface, PCWSTR};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFactory2, IDWriteFont, IDWriteFontCollection,
    IDWriteFontFallback, IDWriteFontFile, IDWriteFontFileLoader, IDWriteTextAnalysisSource,
    IDWriteTextAnalysisSource_Impl, DWRITE_FACTORY_TYPE_ISOLATED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_READING_DIRECTION_LEFT_TO_RIGHT,
};

use super::PlatformFontProvider;
use crate::text::{Face, FaceInfo, LoadError};

pub type NewError = windows_core::Error;
pub type UpdateError = windows_core::Error;
pub type SubstituteError = windows_core::Error;
pub type FallbackError = windows_core::Error;

fn codepoint_to_utf16(mut value: u32) -> ([u16; 2], bool) {
    if value < 0x10000 {
        ([value as u16, 0], false)
    } else {
        value -= 0x10000;
        (
            [
                (((value & 0b1111_1111_1100_0000_0000) >> 10) + 0xD800) as u16,
                ((value & 0b0000_0000_0011_1111_1111) + 0xDC00) as u16,
            ],
            true,
        )
    }
}

#[implement(IDWriteTextAnalysisSource)]
struct TextAnalysisSource {
    text: [u16; 2],
    len: bool,
}

impl IDWriteTextAnalysisSource_Impl for TextAnalysisSource_Impl {
    fn GetTextAtPosition(
        &self,
        textposition: u32,
        textstring: *mut *mut u16,
        textlength: *mut u32,
    ) -> windows::core::Result<()> {
        unsafe {
            let Some(result) = self.text.get(textposition as usize..self.len as usize + 1) else {
                *textstring = std::ptr::null_mut();
                *textlength = 0;
                return Ok(());
            };

            *textstring = result.as_ptr().cast_mut();
            *textlength = result.len() as u32;

            Ok(())
        }
    }

    fn GetTextBeforePosition(
        &self,
        textposition: u32,
        textstring: *mut *mut u16,
        textlength: *mut u32,
    ) -> windows::core::Result<()> {
        unsafe {
            let Some(result) = self.text[..self.len as usize + 1]
                .get(..textposition as usize)
                .filter(|s| !s.is_empty())
            else {
                *textstring = std::ptr::null_mut();
                *textlength = 0;
                return Ok(());
            };

            *textstring = result.as_ptr().cast_mut();
            *textlength = result.len() as u32;

            Ok(())
        }
    }

    fn GetParagraphReadingDirection(
        &self,
    ) -> windows::Win32::Graphics::DirectWrite::DWRITE_READING_DIRECTION {
        DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
    }

    fn GetLocaleName(
        &self,
        textposition: u32,
        textlength: *mut u32,
        localename: *mut *mut u16,
    ) -> windows::core::Result<()> {
        unsafe {
            *textlength = (self.len as u32 + 1).saturating_sub(textposition);
            *localename = windows_core::w!("").as_ptr().cast_mut();

            Ok(())
        }
    }

    fn GetNumberSubstitution(
        &self,
        _textposition: u32,
        textlength: *mut u32,
        numbersubstitution: windows::core::OutRef<
            '_,
            windows::Win32::Graphics::DirectWrite::IDWriteNumberSubstitution,
        >,
    ) -> windows::core::Result<()> {
        unsafe {
            textlength.write(0);
            numbersubstitution.write(None)?;
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct Source {
    // This owns `reference_key`!
    _file: IDWriteFontFile,
    loader: IDWriteFontFileLoader,
    reference_key: *mut c_void,
    reference_key_size: u32,
    index: u32,
}

impl Hash for Source {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.loader.as_raw().hash(state);
        self.reference_key_slice().hash(state);
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.loader.as_raw() == other.loader.as_raw()
            && self.reference_key_slice() == other.reference_key_slice()
    }
}

impl Eq for Source {}

impl Source {
    fn from_font_file(file: IDWriteFontFile, index: u32) -> windows::core::Result<Self> {
        unsafe {
            let mut reference_key = std::ptr::null_mut();
            let mut reference_key_size = 0;
            file.GetReferenceKey(&mut reference_key, &mut reference_key_size)?;

            Ok(Self {
                loader: file.GetLoader()?,
                reference_key,
                reference_key_size,
                index,
                _file: file,
            })
        }
    }

    fn reference_key_slice(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self.reference_key.cast(), self.reference_key_size as usize)
        }
    }

    fn get_data(&self) -> Result<Arc<[u8]>, windows::core::Error> {
        unsafe {
            let stream = self
                .loader
                .CreateStreamFromKey(self.reference_key, self.reference_key_size)?;
            let size = stream.GetFileSize()?;
            let mut output = Vec::with_capacity(size as usize);
            let mut context = std::ptr::null_mut();
            let mut data = std::ptr::null_mut();
            stream.ReadFileFragment(&mut data, 0, size, &mut context)?;
            output.extend_from_slice(std::slice::from_raw_parts(data as *const u8, size as usize));
            stream.ReleaseFileFragment(context);
            output.set_len(size as usize);
            Ok(output.into())
        }
    }

    pub fn open(&self) -> Result<Face, LoadError> {
        Ok(Face::load_from_bytes(
            self.get_data().map_err(LoadError::DirectWrite)?,
            self.index as i32,
        )?)
    }
}

#[derive(Debug)]
pub struct DirectWriteFontProvider {
    fallback: IDWriteFontFallback,
    fonts: Vec<FaceInfo>,
}

impl DirectWriteFontProvider {
    pub fn new() -> Result<Self, windows::core::Error> {
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED)?;
            let mut font_collection = None;
            factory.GetSystemFontCollection(&mut font_collection, false)?;
            let font_collection = font_collection.unwrap();

            Ok(Self {
                fonts: Self::collect_font_list(&font_collection)?,
                fallback: factory.cast::<IDWriteFactory2>()?.GetSystemFontFallback()?,
            })
        }
    }

    fn info_from_font(font: IDWriteFont) -> Result<Option<FaceInfo>, windows::core::Error> {
        unsafe {
            let weight = font.GetWeight();
            let style = font.GetStyle();

            let face = font.CreateFontFace()?;
            let mut n_files = 0;
            face.GetFiles(&mut n_files, None)?;

            let mut files: Vec<IDWriteFontFile> = Vec::with_capacity(n_files as usize);
            face.GetFiles(
                &mut n_files,
                Some(files.spare_capacity_mut().as_mut_ptr().cast()),
            )?;
            files.set_len(n_files as usize);

            let source = if let Some(file) = files.drain(..1).next() {
                Source::from_font_file(file, face.GetIndex())?
            } else {
                return Ok(None);
            };

            let names = font.GetFontFamily()?.GetFamilyNames()?;
            let n_names = names.GetCount();
            let mut family_names = Vec::with_capacity(n_names as usize);
            let mut name_buffer = Vec::new();
            for i in 0..n_names {
                name_buffer.resize(names.GetStringLength(i)? as usize + 1, 0);
                names.GetString(i, &mut name_buffer)?;
                let Ok(name) = String::from_utf16(&name_buffer[..name_buffer.len() - 1]) else {
                    continue;
                };
                family_names.push(name.into())
            }

            if family_names.is_empty() {
                // TODO: Return or at least log an error
                return Ok(None);
            }

            Ok(Some(FaceInfo {
                family_names: family_names.into(),
                // TODO: Width conversion
                width: crate::text::FontAxisValues::Fixed(I16Dot16::new(100)),
                weight: crate::text::FontAxisValues::Fixed(I16Dot16::new(weight.0)),
                italic: style == DWRITE_FONT_STYLE_ITALIC,
                source: crate::text::FontSource::DirectWrite(source),
            }))
        }
    }

    fn collect_font_list(collection: &IDWriteFontCollection) -> Result<Vec<FaceInfo>, UpdateError> {
        let mut result = Vec::new();

        unsafe {
            let n_families = collection.GetFontFamilyCount();
            for i in 0..n_families {
                let family = collection.GetFontFamily(i)?;
                let n_fonts = family.GetFontCount();
                for j in 0..n_fonts {
                    let font = family.GetFont(j)?;
                    result.extend(Self::info_from_font(font)?);
                }
            }
        }

        Ok(result)
    }

    fn substitute_family(family: &str) -> &'static [&'static str] {
        // TODO: This should be script-dependent
        match family {
            "sans-serif" => &["Arial"],
            "serif" => &["Times New Roman"],
            "monospace" => &["Consolas"],
            "cursive" => &["Comic Sans MS"],
            "fantasy" => &["Impact"],
            _ => &[],
        }
    }
}

impl PlatformFontProvider for DirectWriteFontProvider {
    fn substitute(
        &self,
        _sbr: &crate::Subrandr,
        request: &mut crate::text::FaceRequest,
    ) -> Result<(), super::SubstituteError> {
        for family in std::mem::take(&mut request.families) {
            let substitutes = Self::substitute_family(&family);
            if substitutes.is_empty() {
                request.families.push(family);
            } else {
                request
                    .families
                    .extend(substitutes.iter().copied().map(Into::into))
            }
        }

        Ok(())
    }

    fn fonts(&self) -> &[FaceInfo] {
        &self.fonts
    }

    fn fallback(
        &self,
        request: &crate::text::FontFallbackRequest,
    ) -> Result<Vec<crate::text::FaceInfo>, super::FallbackError> {
        unsafe {
            let (utf16, len) = codepoint_to_utf16(request.codepoint);
            let source = TextAnalysisSource { text: utf16, len };

            // TODO: This should probably use the "used font" from the initial query.
            let family_w: Option<Vec<u16>> = request.families.first().map(|f| {
                f.encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<_>>()
            });

            let mut mapped_len = 0;
            let mut mapped_font = None;
            let mut scale = 0.0;
            self.fallback.MapCharacters(
                &IDWriteTextAnalysisSource::from(source),
                0,
                len as u32 + 1,
                None,
                family_w
                    .as_ref()
                    .map(|f| PCWSTR::from_raw(f.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                DWRITE_FONT_WEIGHT(request.style.weight.round_to_inner()),
                if request.style.italic {
                    DWRITE_FONT_STYLE_ITALIC
                } else {
                    DWRITE_FONT_STYLE_NORMAL
                },
                DWRITE_FONT_STRETCH_NORMAL,
                &mut mapped_len,
                &mut mapped_font,
                &mut scale,
            )?;

            Ok(match mapped_font {
                Some(font) => match Self::info_from_font(font)? {
                    Some(info) => vec![info],
                    None => Vec::new(),
                },
                None => Vec::new(),
            })
        }
    }
}

// TODO: Is DirectWrite actually thread-safe here?
//       I think the only function we rely on being thread-safe is
//       IDWriteFontFallback::MapCharacters
unsafe impl Send for DirectWriteFontProvider {}
unsafe impl Sync for DirectWriteFontProvider {}
