use std::ffi::c_void;
use std::hash::Hash;
use std::sync::Arc;

use util::math::I16Dot16;
use windows::core::{implement, Interface, PCWSTR};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFactory2, IDWriteFont, IDWriteFontCollection,
    IDWriteFontFallback, IDWriteFontFamily, IDWriteFontFile, IDWriteFontFileLoader,
    IDWriteTextAnalysisSource, IDWriteTextAnalysisSource_Impl, DWRITE_FACTORY_TYPE_ISOLATED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_READING_DIRECTION_LEFT_TO_RIGHT,
};
use windows_core::BOOL;

use crate::text::font_db::FontProvider;
use crate::text::{Face, FaceInfo, LoadError};

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
    font_collection: IDWriteFontCollection,
    fallback: IDWriteFontFallback,
}

impl DirectWriteFontProvider {
    pub fn new() -> Result<Self, windows::core::Error> {
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED)?;
            let mut font_collection = None;
            factory.GetSystemFontCollection(&mut font_collection, false)?;
            let font_collection = font_collection.unwrap();

            Ok(Self {
                font_collection,
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
            let mut name_buffer = vec![0u16; names.GetStringLength(0)? as usize + 1];
            names.GetString(0, &mut name_buffer)?;

            Ok(Some(FaceInfo {
                family: match String::from_utf16(&name_buffer) {
                    Ok(name) => name.into(),
                    // TODO: Return an error
                    Err(_) => return Ok(None),
                },
                // TODO: Width conversion
                width: crate::text::FontAxisValues::Fixed(I16Dot16::new(100)),
                weight: crate::text::FontAxisValues::Fixed(I16Dot16::new(weight.0)),
                italic: style == DWRITE_FONT_STYLE_ITALIC,
                source: crate::text::FontSource::DirectWrite(source),
            }))
        }
    }

    fn gather_fonts_from_family(
        result: &mut Vec<FaceInfo>,
        set: IDWriteFontFamily,
    ) -> Result<(), windows::core::Error> {
        unsafe {
            let n = set.GetFontCount();
            for i in 0..n {
                result.extend(Self::info_from_font(set.GetFont(i)?)?)
            }

            Ok(())
        }
    }
}

impl FontProvider for DirectWriteFontProvider {
    fn query_fallback(
        &mut self,
        request: &crate::text::FontFallbackRequest,
    ) -> Result<Vec<crate::text::FaceInfo>, util::AnyError> {
        unsafe {
            let (utf16, len) = codepoint_to_utf16(request.codepoint);
            let source = TextAnalysisSource { text: utf16, len };

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

    fn query_family(&mut self, family: &str) -> Result<Vec<crate::text::FaceInfo>, util::AnyError> {
        let mut result = Vec::new();

        unsafe {
            let family_w = family
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let mut family_index = 0;
            let mut family_exists = BOOL(0);
            self.font_collection.FindFamilyName(
                PCWSTR::from_raw(family_w.as_ptr()),
                &mut family_index,
                &mut family_exists,
            )?;
            if !family_exists.as_bool() {
                return Ok(result);
            }
            let dwrite_family = self.font_collection.GetFontFamily(family_index)?;

            Self::gather_fonts_from_family(&mut result, dwrite_family)?;
        }

        Ok(result)
    }
}
