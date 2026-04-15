use std::ffi::{c_char, CStr};

use icu_locale::{LanguageIdentifier, LocaleCanonicalizer};
use log::{debug, warn};
use util::rc::Rc;

use crate::{capi::library::CLibrary, renderer::SubtitleEvent, srv3, vtt, Subtitles};

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

#[repr(C)]
struct CSubtitleIteratorPublic {
    exhausted: bool,
    start: u32,
    end: u32,
}

impl CSubtitleIteratorPublic {
    fn update(&mut self, current: Option<impl SubtitleEvent>) {
        let Some(event) = current else {
            self.exhausted = true;
            return;
        };

        self.exhausted = false;
        let range = event.time_range();
        self.start = range.start;
        self.end = range.end;
    }
}

#[repr(C)]
struct CSubtitleIterator {
    public: CSubtitleIteratorPublic,
    /* Public fields end here */
    text_buffer: String,
    inner: Option<(Subtitles, CSubtitleIteratorImpl<'static>)>,
}

enum CSubtitleIteratorImpl<'a> {
    Srv3(StatefulIterator<srv3::SubtitleIterator<'a>>),
    Vtt(StatefulIterator<vtt::SubtitleIterator<'a>>),
}

struct StatefulIterator<I: Iterator<Item: SubtitleEvent + Copy>> {
    current: Option<I::Item>,
    inner: I,
}

impl<I: Iterator<Item: SubtitleEvent + Copy>> StatefulIterator<I> {
    fn new(inner: I) -> Self {
        Self {
            current: None,
            inner,
        }
    }
}

impl<I: Iterator<Item: SubtitleEvent + Copy>> StatefulIterator<I> {
    fn text(&self, output: &mut String) -> Option<()> {
        self.current.as_ref().map(|event| {
            event.text(output);
        })
    }

    fn advance(&mut self, public: &mut CSubtitleIteratorPublic) {
        self.current = self.inner.next();
        public.update(self.current);
    }
}

impl CSubtitleIteratorImpl<'_> {
    fn text(&self, output: &mut String) -> Option<()> {
        match self {
            CSubtitleIteratorImpl::Srv3(srv3) => srv3.text(output),
            CSubtitleIteratorImpl::Vtt(vtt) => vtt.text(output),
        }
    }

    fn advance(&mut self, public: &mut CSubtitleIteratorPublic) {
        match self {
            CSubtitleIteratorImpl::Srv3(srv3) => srv3.advance(public),
            CSubtitleIteratorImpl::Vtt(vtt) => vtt.advance(public),
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_subtitle_iterator_new() -> *mut CSubtitleIterator {
    Box::into_raw(Box::new(CSubtitleIterator {
        public: CSubtitleIteratorPublic {
            exhausted: true,
            start: 0,
            end: 0,
        },
        text_buffer: String::new(),
        inner: None,
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_subtitle_iterator_next(this: *mut CSubtitleIterator) {
    let Some((_, iter)) = &mut (*this).inner else {
        return;
    };

    iter.advance(&mut (*this).public);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_subtitle_iterator_get_text(
    this: *mut CSubtitleIterator,
    flags: u64,
) -> *const c_char {
    if flags != 0 {
        cthrow!(
            InvalidArgument,
            "non-zero flags passed to `sbr_subtitle_iterator_get_text`"
        );
    }

    let Some((_, iter)) = &mut (*this).inner else {
        return std::ptr::null();
    };

    (*this).text_buffer.clear();
    match iter.text(&mut (*this).text_buffer) {
        Some(()) => {
            (*this).text_buffer.push('\0');
            (*this).text_buffer.as_ptr() as _
        }
        None => std::ptr::null(),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_subtitle_iterator_reset(this: *mut CSubtitleIterator) {
    (*this).public.exhausted = true;
    (*this).inner = None;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_subtitle_iterator_destroy(this: *mut CSubtitleIterator) {
    drop(Box::from_raw(this));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_subtitles_iter(this: *mut Subtitles, citer: *mut CSubtitleIterator) {
    let subs = (*this).clone();
    // NOTE: This whole `Rc::as_ptr` dance is to avoid any potential "technically UB"
    //       references by going through the internal raw pointer directly.
    let mut iter: CSubtitleIteratorImpl<'static> = match &subs {
        Subtitles::Srv3(srv3) => {
            CSubtitleIteratorImpl::Srv3(StatefulIterator::new((*Rc::as_ptr(srv3)).iter()))
        }
        Subtitles::Vtt(vtt) => {
            CSubtitleIteratorImpl::Vtt(StatefulIterator::new((*Rc::as_ptr(vtt)).iter()))
        }
    };

    iter.advance(&mut (*citer).public);
    (*citer).inner = Some((subs, iter));
}
