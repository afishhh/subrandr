use std::collections::HashMap;
use std::ffi::c_void;
use std::hash::Hash;
use std::sync::{Arc, LazyLock};

use icu_locale::subtags::{script, Script};
use icu_locale::LanguageIdentifier;
use log::{warn, LogContext};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GenericFamily {
    SansSerif,
    Serif,
    Monospace,
    Cursive,
    Fantasy,
    Emoji,
}

impl GenericFamily {
    fn from_str(family: &str) -> Option<Self> {
        Some(match family {
            "sans-serif" => Self::SansSerif,
            "serif" => Self::Serif,
            "monospace" => Self::Monospace,
            "cursive" => Self::Cursive,
            "fantasy" => Self::Fantasy,
            "emoji" => Self::Emoji,
            _ => return None,
        })
    }
}

macro_rules! make_script_substitutes {
    ($(($generic: ident, $first_script: literal $(| $more_scripts: literal)*) => [ $($fonts: literal),* ]),* $(,)?) => {{
        let mut result = HashMap::new();

        $({
            let fonts: &[&'static str] = &[ $($fonts),* ];

            result.insert((GenericFamily::$generic, script!($first_script)), fonts);
            $(result.insert((GenericFamily::$generic, script!($more_scripts)), fonts);)*
        })*

        result
    }};
}

// TODO: This can't hold `Rc<str>` because `Rc` is not Sync *even though* static
//       `Rc`s are perfectly fine to share.
//       Make static `Rc`s a separate type (could convert to both `Rc` and `Arc`).
type ScriptSubstituteMap = HashMap<(GenericFamily, Script), &'static [&'static str]>;
static SCRIPT_FONT_SUBSTITUTES: LazyLock<ScriptSubstituteMap> = LazyLock::new(|| {
    make_script_substitutes! {
        (Serif, "Arab") => ["Times New Roman"],
        (SansSerif, "Arab") => ["Segoe UI", "Tahoma", "Arial"],
        (Monospace, "Arab") => ["Consolas"],
        (Cursive, "Arab") => ["Comic Sans MS"],

        (Serif, "Grek") => ["Times New Roman"],
        (SansSerif, "Grek") => ["Arial"],
        (Monospace, "Grek") => ["Consolas"],
        (Cursive, "Grek") => ["Comic Sans MS"],

        (Serif, "Hebr") => ["Narkisim", "David"],
        (SansSerif, "Hebr") => ["Arial"],
        (Monospace, "Hebr") => ["Fixed Miriam Transparent", "Miriam Fixed", "Rod", "Consolas", "Cursor New"],
        (Cursive, "Hebr") => ["Guttman Yad", "Ktav", "Arial"],

        (Serif, "Kata" | "Hira" | "Hrkt" | "Hani" | "Jpan") => ["Yu Mincho", "MS PMincho", "MS Mincho", "Noto Serif CJK JP", "Noto Seif JP", "Meiryo", "Yu Gothic", "MS PGothic", "MS Gothic"],
        (SansSerif, "Kata" | "Hira" | "Hrkt" | "Hani" | "Jpan") => ["Meiryo", "Yu Gothic", "MS PGothic", "MS Gothic", "Noto Sans CJK JP", "Noto Sans JP"],
        (Monospace, "Kata" | "Hira" | "Hrkt" | "Hani" | "Jpan") => ["BIZ UDGothic", "MS Gothic", "MS Mincho", "Meiryo", "Yu Gothic", "Yu Mincho"],

        (Serif, "Hang" | "Jamo" | "Kore") => ["Batang", "Noto Serif CJK KR", "Noto Serif KR", "Gulim"],
        (SansSerif, "Hang" | "Jamo" | "Kore") => ["Malgun Gothic", "Gulim", "Noto Sans CJK KR", "Noto Sans KR"],
        (Monospace, "Hang" | "Jamo" | "Kore") => ["GulimChe"],
        (Cursive, "Hang" | "Jamo" | "Kore") => ["Gungsuh"],

        (Serif, "Thai") => ["Tahoma"],
        (SansSerif, "Thai") => ["Tahoma"],
        (Monospace, "Thai") => ["Tahoma"],
        (Cursive, "Thai") => ["Tahoma"],

        (Serif, "Hans") => ["SimSun", "MS Song", "SimSum-ExtB", "Noto Serif CJK SC", "Noto Serif SC"],
        (SansSerif, "Hans") => ["Microsoft YaHei", "SimHei", "Noto Sans CJK SC", "Noto Sans SC"],
        (Monospace, "Hans") => ["NSimSun", "SimSun", "MS Song", "SimSun-ExtB"],
        (Cursive, "Hans") => ["KaiTi", "KaiTi_GB2312"],

        // TODO: Firefox has different rules for zh-CN/zh-TW/zh-HK but we don't have region here.
        //       Either way we can't really guess region easily so just distinguish Hans/Hant for now.

        (Serif, "Hant" | "Hanb") => ["Times New Roman", "PMingLiU", "MingLiU", "MingLiU-ExtB", "Noto Serif CJK TC", "Noto Serif CJK TC"],
        (SansSerif, "Hant" | "Hanb") => ["Arial", "Microsoft JhengHei", "PMingLiU", "MingLiU", "MingLiU-ExtB", "Noto Sans CJK TC", "Noto Sans TC"],
        (Monospace, "Hant" | "Hanb") => ["MingLiU", "MingLiU-ExtB"],
        (Cursive, "Hant" | "Hanb") => ["DFKai-SB"],

        (Serif, "Deva") => ["Kokila", "Raghindi"],
        (SansSerif, "Deva") => ["Nirmala UI", "Mangal"],
        (Monospace, "Deva") => ["Mangal", "Nirmala UI"],

        (Serif, "Taml") => ["Latha"],
        (Monospace, "Taml") => ["Latha"],

        (Serif, "Armn") => ["Sylfaen"],
        (SansSerif, "Armn") => ["Arial AMU"],
        (Monospace, "Armn") => ["Arial AMU"],

        (Serif, "Beng") => ["Vrinda", "Akaash", "Likhan", "Ekushey Punarbhaba"],
        (SansSerif, "Beng") => ["Nirmala Text", "Vrinda", "Akaash", "Likhan", "Ekushey Punarbhaba"],
        (Monospace, "Beng") => ["Mitra Mono", "Likhan", "Mukti Narrow"],

        (Serif, "Cans") => ["Aboriginal Serif", "BJCree Uni"],
        (SansSerif, "Cans") => ["Aboriginal Sans"],
        (Monospace, "Cans") => ["Aboriginal Sans", "OskiDakelh", "Pigiarniq", "Uqammaq"],

        (Serif, "Ethi") => ["Visual Geez Unicode", "Visual Geez Unicode Agazian"],
        (SansSerif, "Ethi") => ["GF Zemen Unicode"],
        (Monospace, "Ethi") => ["Ethiopia Jiret"],
        (Cursive, "Ethi") => ["Visual Geez Unicode Title"],

        (Serif, "Geor") => ["Sylfaen", "BPG Paata Khutsuri U", "TITUS Cyberbit Basic"],
        (SansSerif, "Geor") => ["BPG Classic 99U"],
        (Monospace, "Geor") => ["BPG Classic 99U"],

        (Serif, "Gujr") => ["Shruti"],
        (SansSerif, "Gujr") => ["Shruti"],
        (Monospace, "Gujr") => ["Shruti"],

        (Serif, "Guru") => ["Raavi", "Saab"],
        (SansSerif, "Guru") => [""],
        (Monospace, "Guru") => ["Raavi", "Saab"],

        (Serif, "Khmr") => ["PhnomPenh OT", "Mondulkiri U GR 1.5", "Khmer OS"],
        (SansSerif, "Khmr") => ["Khmer OS"],
        (Monospace, "Khmr") => ["Khmer OS", "Khmer OS System"],

        (Serif, "Mlym") => ["Rachana_w01", "AnjaliOldLipi", "Kartika", "ThoolikaUnicode"],
        (SansSerif, "Mlym") => ["Rachana_w01", "AnjaliOldLipi", "Kartika", "ThoolikaUnicode"],
        (Monospace, "Mlym") => ["Rachana_w01", "AnjaliOldLipi", "Kartika", "ThoolikaUnicode"],

        (Serif, "Orya") => ["ori1Uni", "Kalinga"],
        (SansSerif, "Orya") => ["ori1Uni", "Kalinga"],
        (Monospace, "Orya") => ["ori1Uni", "Kalinga"],

        (Serif, "Telu") => ["Gautami", "Akshar Unicode"],
        (SansSerif, "Telu") => ["Gautami", "Akshar Unicode"],
        (Monospace, "Telu") => ["Gautami", "Akshar Unicode"],

        (Serif, "Knda") => ["Tunga", "AksharUnicode"],
        (SansSerif, "Knda") => ["Tunga", "AksharUnicode"],
        (Monospace, "Knda") => ["Tunga", "AksharUnicode"],

        (Serif, "Sinh") => ["Iskoola Pota", "AksharUnicode"],
        (SansSerif, "Sinh") => ["Iskoola Pota", "AksharUnicode"],
        (Monospace, "Sinh") => ["Iskoola Pota", "AksharUnicode"],

        (Serif, "Tibt") => ["Tibetan Machine Uni", "Jomolhari", "Microsoft Himalaya"],
        (SansSerif, "Tibt") => ["Tibetan Machine Uni", "Jomolhari", "Microsoft Himalaya"],
        (Monospace, "Tibt") => ["Tibetan Machine Uni", "Jomolhari", "Microsoft Himalaya"],
    }
});

#[derive(Debug)]
pub struct DirectWriteFontProvider {
    fallback: IDWriteFontFallback,
    fonts: Vec<FaceInfo>,
}

impl DirectWriteFontProvider {
    pub fn new(log: &LogContext) -> Result<Self, windows::core::Error> {
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED)?;
            let mut font_collection = None;
            factory.GetSystemFontCollection(&mut font_collection, false)?;
            let font_collection = font_collection.unwrap();

            Ok(Self {
                fonts: Self::collect_font_list(log, &font_collection)?,
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

    fn collect_font_list(
        log: &LogContext,
        collection: &IDWriteFontCollection,
    ) -> Result<Vec<FaceInfo>, UpdateError> {
        let mut result = Vec::new();

        unsafe {
            let n_families = collection.GetFontFamilyCount();
            for i in 0..n_families {
                let family = collection.GetFontFamily(i)?;
                let n_fonts = family.GetFontCount();
                for j in 0..n_fonts {
                    match family.GetFont(j).and_then(Self::info_from_font) {
                        Ok(face) => result.extend(face),
                        Err(err) => {
                            warn!(
                                log,
                                "Failed to load font {j} from font family {i} in system font collection: {err}"
                            );
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    fn substitute_family(family: &str, language: &LanguageIdentifier) -> &'static [&'static str] {
        let Some(generic) = GenericFamily::from_str(family) else {
            return &[];
        };

        if let Some(script) = language.script {
            if script == script!("Zsye") {
                return &["Segoe UI Emoji"];
            }

            if let Some(subst) = SCRIPT_FONT_SUBSTITUTES.get(&(generic, script)) {
                return subst;
            }
        }

        match generic {
            GenericFamily::Serif => &["Times New Roman"],
            GenericFamily::SansSerif => &["Arial"],
            GenericFamily::Monospace => &["Consolas"],
            GenericFamily::Cursive => &["Comic Sans MS"],
            GenericFamily::Fantasy => &["Impact"],
            GenericFamily::Emoji => &["Segoe UI Emoji"],
        }
    }
}

impl PlatformFontProvider for DirectWriteFontProvider {
    fn substitute(
        &self,
        _log: &LogContext,
        request: &mut crate::text::FaceRequest,
    ) -> Result<(), super::SubstituteError> {
        for family in std::mem::take(&mut request.families) {
            let substitutes = Self::substitute_family(&family, &request.language);
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
    ) -> Result<Option<crate::text::FaceInfo>, super::FallbackError> {
        unsafe {
            let (utf16, len) = codepoint_to_utf16(request.codepoint.into());
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
                Some(font) => Self::info_from_font(font)?,
                None => None,
            })
        }
    }
}

// TODO: Is DirectWrite actually thread-safe here?
//       I think the only function we rely on being thread-safe is
//       IDWriteFontFallback::MapCharacters
unsafe impl Send for DirectWriteFontProvider {}
unsafe impl Sync for DirectWriteFontProvider {}
