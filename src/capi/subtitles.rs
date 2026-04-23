use std::ffi::{c_char, CStr};

use icu_locale::{LanguageIdentifier, LocaleCanonicalizer};
use log::{debug, warn};
use util::rc::Rc;

use crate::{capi::library::CLibrary, Subtitles};

c_enum! {
    #[repr(i16)]
    #[try_from(InvalidArgument, "{value} is not a valid subtitle format")]
    enum SubtitleFormat {
        Unknown = 0,
        Srv3 = 1,
        WebVTT = 2
    }
}

// TODO: This should be pulled out into the main module if a Rust API is desired
//       in the future.
fn probe(content: &str) -> SubtitleFormat {
    if crate::srv3::probe(content) {
        SubtitleFormat::Srv3
    } else if crate::vtt::probe(content) {
        SubtitleFormat::WebVTT
    } else {
        SubtitleFormat::Unknown
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_probe_text(content: *const c_char, content_len: usize) -> SubtitleFormat {
    let Ok(content) = std::str::from_utf8(std::slice::from_raw_parts(
        content.cast::<u8>(),
        content_len,
    )) else {
        return SubtitleFormat::Unknown;
    };

    probe(content)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_load_text(
    lib: &CLibrary,
    content: *const c_char,
    content_len: usize,
    format: i16,
    language_hint: *const c_char,
) -> *mut Subtitles {
    let mut format = ctry!(SubtitleFormat::try_from_value(format));
    let content = ctrywrap!(
        Other("Invalid UTF-8"),
        std::str::from_utf8(std::slice::from_raw_parts(
            content.cast::<u8>(),
            content_len
        ))
    );
    let log = lib.root_logger.new_ctx();
    let language_hint = if !language_hint.is_null() {
        let cstr = CStr::from_ptr(language_hint);
        match LanguageIdentifier::try_from_locale_bytes(cstr.to_bytes()) {
            Ok(lang_id) => {
                let mut locale = icu_locale::Locale {
                    id: lang_id,
                    extensions: icu_locale::extensions::Extensions::new(),
                };
                // NOTE: Locale canonicalization is performed here because YouTube uses
                //       the obsolete "iw" language tag for Hebrew which identifies itself
                //       as `LeftToRight` with `icu_locale::LocaleDirectionality`.
                //       This step converts "iw" to "he" which is correctly `RightToLeft`.
                //       There probably exist other good reasons for doing this and it avoids
                //       special casing "iw" internally.
                LocaleCanonicalizer::new_common().canonicalize(&mut locale);
                debug!(log, "Language hint resolved to {:?}", locale.id);
                Some(locale.id)
            }
            Err(error) => {
                warn!(log, "Failed to parse language hint {cstr:?}: {error}");
                None
            }
        }
    } else {
        None
    };

    if format == SubtitleFormat::Unknown {
        format = probe(content);
    }

    match format {
        SubtitleFormat::Srv3 => Box::into_raw(Box::new(Subtitles::Srv3(Rc::new(ctry!(
            crate::srv3::convert(
                &log,
                ctry!(crate::srv3::parse(&log, content)),
                language_hint.as_ref(),
            )
        ))))),
        SubtitleFormat::WebVTT => {
            Box::into_raw(Box::new(Subtitles::Vtt(Rc::new(crate::vtt::convert(
                &log,
                match crate::vtt::parse(content) {
                    Some(captions) => captions,
                    None => cthrow!(Other, "Invalid WebVTT"),
                },
            )))))
        }
        SubtitleFormat::Unknown => {
            cthrow!(UnrecognizedFormat, "Unrecognized subtitle format")
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_subtitles_destroy(subtitles: *mut Subtitles) {
    drop(Box::from_raw(subtitles));
}
