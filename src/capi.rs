use std::{
    borrow::Cow,
    cell::UnsafeCell,
    ffi::{c_int, CStr, CString},
    fmt::Formatter,
};

use crate::{color::BGRA8, Painter, Renderer, SubtitleContext, Subtitles};

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

    UnrecognizedFile = 10,
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

impl From<CErrorValue> for c_int {
    fn from(_: CErrorValue) -> Self {
        -1
    }
}

macro_rules! cthrow {
    ($error: expr) => {{
        fill_last_error($error);
        return CErrorValue.into();
    }};
    ($kind: ident, $message: expr) => {
        cthrow!(CError::new(ErrorKind::$kind, $message))
    };
}

macro_rules! ctry {
    ($result: expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => cthrow!(CError::from_error(error)),
        }
    };
}

macro_rules! ctrywrap {
    ($error_type: ident($message: literal), $value: expr) => {
        match $value {
            Ok(value) => value,
            Err(error) => cthrow!(CError::with_context(
                ErrorKind::$error_type,
                $message,
                Box::new(error)
            )),
        }
    };
}

c_enum! {
    #[repr(u16)]
    #[try_from(InvalidArgument, "Invalid subtitle format: {value}")]
    enum SubtitleFormat {
        Ass = 1,
        Srv3 = 2,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_load_file(path: *const i8) -> *mut Subtitles {
    let str = CStr::from_ptr(path);
    let bytes = ctrywrap!(InvalidArgument("Path is not valid UTF-8"), str.to_str());
    if bytes.ends_with(".ass") {
        let text = ctry!(std::fs::read_to_string(bytes));
        Box::into_raw(Box::new(crate::ass::convert(ctry!(crate::ass::parse(
            &text
        )))))
    } else if bytes.ends_with(".srv3") {
        let text = ctry!(std::fs::read_to_string(bytes));
        Box::into_raw(Box::new(crate::srv3::convert(ctry!(crate::srv3::parse(
            &text
        )))))
    } else {
        cthrow!(UnrecognizedFile, "Unrecognized file format")
    }
}

// #[unsafe(no_mangle)]
// unsafe extern "C" fn sbr_subtitles_get_class_name(subtitles: *mut Subtitles) -> *const i8 {
//   TODO: SubtitleClass::get_name_cstr()
// }

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_get_last_error_string() -> *const i8 {
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
unsafe extern "C" fn sbr_renderer_create(subs: *mut Subtitles) -> *mut Renderer<'static> {
    Box::into_raw(Box::new(Renderer::new(unsafe { &*subs })))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_render(
    renderer: *mut Renderer<'static>,
    ctx: *const SubtitleContext,
    t: u32,
    buffer: *mut BGRA8,
    width: u32,
    height: u32,
) -> c_int {
    let buffer = std::slice::from_raw_parts_mut(buffer, width as usize * height as usize);
    (*renderer).render(&*ctx, t, &mut Painter::new(width, height, buffer));
    0
}
