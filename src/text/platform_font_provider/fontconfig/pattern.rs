use std::{
    alloc::Layout,
    ffi::{c_char, c_double, c_int, CStr},
    mem::{ManuallyDrop, MaybeUninit},
    ops::{Deref, RangeInclusive},
};

use text_sys::fontconfig::*;
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub enum Value<'a> {
    Integer(c_int),
    Double(c_double),
    String(&'a CStr),
    Unknown,
}

impl Value<'_> {
    unsafe fn from_raw(raw: FcValue) -> Self {
        #[expect(non_upper_case_globals)]
        match raw.type_ {
            FcTypeInteger => Value::Integer(raw.u.i),
            FcTypeDouble => Value::Double(raw.u.d),
            #[allow(clippy::unnecessary_cast)]
            FcTypeString => Value::String(unsafe { CStr::from_ptr(raw.u.s as *const c_char) }),
            _ => Value::Unknown,
        }
    }

    unsafe fn into_raw(self) -> FcValue {
        FcValue {
            type_: match self {
                Value::Integer(_) => FcTypeInteger,
                Value::Double(_) => FcTypeDouble,
                Value::String(_) => FcTypeString,
                Value::Unknown => FcTypeUnknown,
            },
            u: match self {
                Value::Integer(value) => _FcValue__bindgen_ty_1 { i: value },
                Value::Double(value) => _FcValue__bindgen_ty_1 { d: value },
                Value::String(cstr) => _FcValue__bindgen_ty_1 {
                    #[allow(clippy::unnecessary_cast)]
                    s: cstr.as_ptr() as *const FcChar8,
                },
                Value::Unknown => _FcValue__bindgen_ty_1 {
                    s: std::ptr::null(),
                },
            },
        }
    }
}

pub trait FromPatternValue<'a>: Sized + 'a {
    fn get(pattern: &'a Pattern, object: &CStr, index: i32) -> Result<Self, u32>;
}

macro_rules! impl_pattern_get {
    (for $ty: ty, with $getter: ident, |$ptr: ident| $($convert: tt)*) => {
        impl<'a> FromPatternValue<'a> for $ty {
            fn get(pattern: &'a Pattern, object: &CStr, index: i32) -> Result<Self, FcResult> {
                let $ptr = unsafe {
                    let mut output = MaybeUninit::uninit();
                    let ret = $getter(pattern.inner, object.as_ptr(), index, output.as_mut_ptr());
                    if ret != FcResultMatch {
                        return Err(ret);
                    }
                    output.assume_init()
                };

                #[allow(clippy::unnecessary_cast)]
                Ok($($convert)*)// unsafe { CStr::from_ptr(ptr.cast_const() as *const c_char) }
            }
        }
    };
}

impl_pattern_get!(for Value<'a>, with FcPatternGet, |value| unsafe { Value::from_raw(value) });
impl_pattern_get!(for *mut FcCharSet, with FcPatternGetCharSet, |ptr| ptr);
impl_pattern_get!(for &'a CStr, with FcPatternGetString, |ptr| unsafe { CStr::from_ptr(ptr.cast_const() as *const c_char)});
impl_pattern_get!(for c_int, with FcPatternGetInteger, |value| value);
impl_pattern_get!(for c_double, with FcPatternGetDouble, |value| value);
impl_pattern_get!(for RangeInclusive<c_double>, with FcPatternGetRange, |ptr| unsafe {
    let mut begin = MaybeUninit::uninit();
    let mut end = MaybeUninit::uninit();
    #[allow(clippy::unnecessary_cast)]
    {
        assert_eq!(FcRangeGetDouble(ptr, begin.as_mut_ptr(), end.as_mut_ptr()), FcTrue as FcBool);
    }
    begin.assume_init()..=end.assume_init()
});

#[derive(Error, Debug)]
#[error("{0}")]
pub struct PatternAddError(String);

#[derive(Error, Debug)]
pub enum PatternGetError {
    #[error("Index out of range")]
    NoId,
    #[error("Element does not exist")]
    NoMatch,
    #[error("Out of memory")]
    OutOfMemory,
    #[error("Mismatched value type")]
    TypeMismatch,
    #[error("FcPatternGet failed: {0}")]
    Other(u32),
}

impl PatternGetError {
    fn from_raw(value: FcResult) -> Self {
        #[expect(non_upper_case_globals)]
        match value {
            FcResultNoId => Self::NoId,
            FcResultNoMatch => Self::NoMatch,
            FcResultOutOfMemory => Self::OutOfMemory,
            FcResultTypeMismatch => Self::TypeMismatch,
            _ => Self::Other(value),
        }
    }
}

#[derive(Debug)]
pub struct Pattern {
    inner: *mut FcPattern,
}

impl Pattern {
    pub fn new() -> Self {
        let pattern = unsafe { FcPatternCreate() };
        if pattern.is_null() {
            // This is not going to be a meaningful layout (FcPattern is zero-sized)
            // but this is better than nothing.
            std::alloc::handle_alloc_error(Layout::new::<FcPattern>());
        }
        Self { inner: pattern }
    }

    pub unsafe fn from_raw(ptr: *mut _FcPattern) -> Self {
        Self { inner: ptr }
    }

    pub fn add(
        &mut self,
        name: &CStr,
        value: Value<'_>,
        append: bool,
    ) -> Result<(), PatternAddError> {
        if unsafe {
            FcPatternAdd(
                self.inner,
                name.as_ptr(),
                value.into_raw(),
                append as FcBool,
            )
        } == 0
        {
            return Err(PatternAddError(format!(
                "Failed to {} {value:?} to {name:?} in pattern",
                if append { "append" } else { "add" }
            )));
        }

        Ok(())
    }

    pub fn get<'a, T: FromPatternValue<'a>>(
        &'a self,
        name: &CStr,
        index: i32,
    ) -> Result<T, PatternGetError> {
        T::get(self, name, index).map_err(PatternGetError::from_raw)
    }

    pub fn get_with_binding<'a>(
        &'a self,
        name: &CStr,
        index: i32,
    ) -> Result<(Value<'a>, FcValueBinding), PatternGetError> {
        let mut output = MaybeUninit::uninit();
        let mut binding = MaybeUninit::uninit();
        let ret = unsafe {
            FcPatternGetWithBinding(
                self.inner,
                name.as_ptr(),
                index,
                output.as_mut_ptr(),
                binding.as_mut_ptr(),
            )
        };

        if ret != FcResultMatch {
            return Err(PatternGetError::from_raw(ret));
        }

        Ok(unsafe { (Value::from_raw(output.assume_init()), binding.assume_init()) })
    }

    pub fn as_mut_ptr(&self) -> *mut _FcPattern {
        self.inner
    }
}

impl Drop for Pattern {
    fn drop(&mut self) {
        unsafe { FcPatternDestroy(self.inner) };
    }
}

pub struct PatternRef {
    inner: ManuallyDrop<Pattern>,
}

impl PatternRef {
    pub fn from_raw(ptr: *mut FcPattern) -> Self {
        Self {
            inner: ManuallyDrop::new(unsafe { Pattern::from_raw(ptr) }),
        }
    }
}

impl Deref for PatternRef {
    type Target = Pattern;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
