use std::{
    borrow::Cow,
    cell::UnsafeCell,
    ffi::{c_char, c_void, CStr, CString},
    fmt::Formatter,
    mem::MaybeUninit,
    sync::Arc,
};

use util::rc::Rc;

use crate::{
    log::{CLogCallback, Logger},
    text::Face,
    Subrandr, Subtitles,
};

macro_rules! c_enum {
    (
        #[repr($type: ident)]
        $(#[try_from($try_from_kind: ident, $try_from_fmtstr: literal)])?
        enum $name: ident {
        $($key: ident = $value: literal),* $(,)?
    }) => {
        #[repr($type)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($key = $value,)*
        }

        impl $name {
            const fn from_value(value: $type) -> Option<Self> {
                match value {
                    $($value => Some(Self::$key),)*
                    _ => None
                }
            }

            $(fn try_from_value(value: $type) -> Result<Self, CError> {
                Self::from_value(value).ok_or_else(||
                    CError::new(
                        ErrorKind::$try_from_kind,
                        format!($try_from_fmtstr, value = value)
                    )
                )
            })?
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum ErrorKind {
    Other = 1,
    InvalidArgument = 2,
    Io = 3,

    UnrecognizedFormat = 10,
}

#[derive(Debug)]
struct CError {
    kind: ErrorKind,
    context: Option<Box<dyn std::error::Error + Sync + 'static>>,
    message: Option<Cow<'static, str>>,
}

impl CError {
    pub fn new(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            context: None,
            message: Some(message.into()),
        }
    }

    pub fn with_context(
        kind: ErrorKind,
        message: impl Into<Cow<'static, str>>,
        context: impl std::error::Error + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            context: Some(Box::new(context)),
            message: Some(message.into()),
        }
    }

    pub fn from_error(error: impl std::error::Error + Sync + 'static) -> Self {
        let mut root_cause = &error as &dyn std::error::Error;
        while let Some(cause) = root_cause.source() {
            root_cause = cause;
        }

        let kind = if root_cause.is::<std::io::Error>() {
            ErrorKind::Io
        } else {
            ErrorKind::Other
        };

        Self {
            kind,
            context: Some(Box::new(error)),
            message: None,
        }
    }
}

impl std::fmt::Display for CError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(message) = self.message.as_ref() {
            fmt.write_str(message)?;
        }
        if let Some(context) = self.context.as_deref() {
            if self.message.is_some() {
                fmt.write_str(": ")?;
            }
            std::fmt::Display::fmt(context, fmt)?;
        }
        Ok(())
    }
}

impl std::error::Error for CError {}

struct LastError {
    error: CError,
    string: CString,
}

thread_local! {
    static LAST_ERROR: UnsafeCell<Option<LastError>> = const { UnsafeCell::new(None) };
}

fn fill_last_error(error: CError) {
    LAST_ERROR.with(|x| unsafe {
        (*x.get()) = Some(LastError {
            string: CString::new(error.to_string()).unwrap(),
            error,
        });
    })
}

struct CErrorValue;

impl<T> From<CErrorValue> for *mut T {
    fn from(_: CErrorValue) -> Self {
        std::ptr::null_mut()
    }
}

impl<T> From<CErrorValue> for *const T {
    fn from(_: CErrorValue) -> Self {
        std::ptr::null()
    }
}

impl From<CErrorValue> for i64 {
    fn from(_: CErrorValue) -> Self {
        -1
    }
}

impl From<CErrorValue> for i32 {
    fn from(_: CErrorValue) -> Self {
        -1
    }
}

impl From<CErrorValue> for i16 {
    fn from(_: CErrorValue) -> Self {
        -1
    }
}

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

macro_rules! cthrow {
    ($error: expr) => {{
        $crate::capi::fill_last_error($error);
        return $crate::capi::CErrorValue.into();
    }};
    ($kind: ident, $message: expr) => {
        cthrow!($crate::capi::CError::new(
            $crate::capi::ErrorKind::$kind,
            $message
        ))
    };
}

macro_rules! ctry {
    ($result: expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => cthrow!($crate::capi::CError::from_error(error)),
        }
    };
}

macro_rules! ctrywrap {
    ($error_type: ident($message: literal), $value: expr) => {
        match $value {
            Ok(value) => value,
            Err(error) => cthrow!(crate::capi::CError::with_context(
                crate::capi::ErrorKind::$error_type,
                $message,
                Box::new(error)
            )),
        }
    };
}

mod renderer;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_init() -> *mut Subrandr {
    Box::into_raw(Box::new(Subrandr::init()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_fini(sbr: *mut Subrandr) {
    drop(Box::from_raw(sbr));
}

const fn const_parse_u32(value: &str) -> u32 {
    match u32::from_str_radix(value, 10) {
        Ok(result) => result,
        Err(_) => panic!("const value is not an integer"),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_version(major: *mut u32, minor: *mut u32, patch: *mut u32) {
    major.write(const { const_parse_u32(env!("CARGO_PKG_VERSION_MAJOR")) });
    minor.write(const { const_parse_u32(env!("CARGO_PKG_VERSION_MINOR")) });
    patch.write(const { const_parse_u32(env!("CARGO_PKG_VERSION_PATCH")) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_set_log_callback(
    sbr: &mut Subrandr,
    callback: CLogCallback,
    user_data: *const c_void,
) {
    sbr.logger = Logger::C {
        callback,
        user_data,
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
    sbr: &Subrandr,
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
    let _language_hint = if !language_hint.is_null() {
        Some(CStr::from_ptr(language_hint))
    } else {
        None
    };

    if format == SubtitleFormat::Unknown {
        format = probe(content);
    }

    match format {
        SubtitleFormat::Srv3 => Box::into_raw(Box::new(Subtitles::Srv3(Rc::new(
            crate::srv3::convert(sbr, ctry!(crate::srv3::parse(sbr, content))),
        )))),
        SubtitleFormat::WebVTT => {
            Box::into_raw(Box::new(Subtitles::Vtt(Rc::new(crate::vtt::convert(
                sbr,
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

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_get_last_error_string() -> *const c_char {
    LAST_ERROR.with(|x| {
        (*x.get())
            .as_ref()
            .map_or(std::ptr::null(), |e| e.string.as_ptr())
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_get_last_error_code() -> u32 {
    LAST_ERROR.with(|x| (*x.get()).as_ref().map_or(0, |e| e.error.kind as u32))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_open_font_from_memory(
    _sbr: *mut Subrandr,
    data: *const u8,
    data_len: usize,
) -> *mut Face {
    let mut uninit = Arc::new_uninit_slice(data_len);
    unsafe {
        std::mem::transmute::<&mut [MaybeUninit<u8>], &mut [u8]>(
            Arc::get_mut(&mut uninit).unwrap(),
        )
        .copy_from_slice(std::slice::from_raw_parts(data, data_len));
    }
    let bytes = Arc::<[MaybeUninit<u8>]>::assume_init(uninit);
    Box::into_raw(Box::new(ctry!(Face::load_from_bytes(bytes, 0))))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_close_font(_sbr: *mut Subrandr, font: *mut Face) {
    std::mem::forget((*font).clone());
}
