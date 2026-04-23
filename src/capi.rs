use std::{
    borrow::Cow,
    cell::UnsafeCell,
    ffi::{c_char, CString},
    fmt::Formatter,
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

            $(fn try_from_value(value: $type) -> Result<Self, $crate::capi::CError> {
                Self::from_value(value).ok_or_else(||
                    $crate::capi::CError::new(
                        $crate::capi::ErrorKind::$try_from_kind,
                        format!($try_from_fmtstr, value = value)
                    )
                )
            })?
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i16)]
enum ErrorKind {
    Other = -1,
    InvalidArgument = -2,

    UnrecognizedFormat = -11,
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
        Self {
            kind: ErrorKind::Other,
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
    string: CString,
}

thread_local! {
    static LAST_ERROR: UnsafeCell<Option<LastError>> = const { UnsafeCell::new(None) };
}

fn fill_last_error(error: CError) {
    LAST_ERROR.with(|x| unsafe {
        (*x.get()) = Some(LastError {
            string: CString::new(error.to_string()).unwrap(),
        });
    })
}

trait CErrorReturn {
    fn from(err: &CError) -> Self;
}

impl<T> CErrorReturn for *mut T {
    fn from(_: &CError) -> Self {
        std::ptr::null_mut()
    }
}

impl<T> CErrorReturn for *const T {
    fn from(_: &CError) -> Self {
        std::ptr::null()
    }
}

impl CErrorReturn for i64 {
    fn from(e: &CError) -> Self {
        e.kind as _
    }
}

impl CErrorReturn for i32 {
    fn from(e: &CError) -> Self {
        e.kind as _
    }
}

impl CErrorReturn for i16 {
    fn from(e: &CError) -> Self {
        e.kind as _
    }
}

macro_rules! cthrow {
    ($error: expr) => {{
        return {
            let error = $error;
            let ret = $crate::capi::CErrorReturn::from(&error);
            $crate::capi::fill_last_error(error);
            ret
        };
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

mod library;
mod renderer;
mod subtitles;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_get_last_error_string() -> *const c_char {
    LAST_ERROR.with(|x| {
        (*x.get())
            .as_ref()
            .map_or(std::ptr::null(), |e| e.string.as_ptr())
    })
}
