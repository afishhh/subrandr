use std::{
    fmt::Debug,
    marker::PhantomData,
    mem::{offset_of, MaybeUninit},
};

use crate::{color::BGRA8, util::UniqueArc};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra,
    Mono,
}

impl PixelFormat {
    pub fn width(self) -> u8 {
        match self {
            PixelFormat::Bgra => 4,
            PixelFormat::Mono => 1,
        }
    }
}

#[doc(hidden)]
pub trait BitmapPixel {
    const KIND: PixelFormat;
}

impl BitmapPixel for BGRA8 {
    const KIND: PixelFormat = PixelFormat::Bgra;
}

impl BitmapPixel for u8 {
    const KIND: PixelFormat = PixelFormat::Mono;
}

#[doc(hidden)]
pub trait InitBitmapFormat {
    type Value;
}

impl<P: BitmapPixel> InitBitmapFormat for P {
    type Value = P;
}

pub struct Dynamic;

impl InitBitmapFormat for Dynamic {
    type Value = u8;
}

#[doc(hidden)]
pub trait BitmapFormat {
    type Value;
    type DiscriminantType;

    fn debug_kind(kind: &Self::DiscriminantType) -> impl Debug;
}

#[doc(hidden)]
impl<F: InitBitmapFormat> BitmapFormat for F {
    type Value = F::Value;
    type DiscriminantType = PixelFormat;

    fn debug_kind(kind: &Self::DiscriminantType) -> impl Debug {
        kind
    }
}

pub struct Uninit;

impl BitmapFormat for Uninit {
    type Value = MaybeUninit<u8>;
    type DiscriminantType = MaybeUninit<PixelFormat>;

    fn debug_kind(_kind: &Self::DiscriminantType) -> impl Debug {
        struct A;
        impl Debug for A {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("uninit")
            }
        }
    }
}

#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

#[repr(C)]
pub struct Bitmap<T: BitmapFormat> {
    width: u32,
    height: u32,
    format: T::DiscriminantType,
    data: Aligned<()>,
    _marker: PhantomData<T::Value>,
}

impl<T: BitmapFormat> Debug for Bitmap<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bitmap")
            .field("witdh", &self.width)
            .field("height", &self.height)
            .field("format", &T::debug_kind(&self.format))
            .field("data", unsafe {
                &(self as *const _ as *const u8).add(offset_of!(Self, data))
            })
            .finish_non_exhaustive()
    }
}

pub enum BitmapCast<'a> {
    Bgra(&'a Bitmap<BGRA8>),
    Mono(&'a Bitmap<u8>),
}

impl Bitmap<Uninit> {
    pub unsafe fn new_uninit(len: usize, width: u32, height: u32) -> UniqueArc<Self> {
        unsafe {
            let mut uninit = UniqueArc::<[MaybeUninit<Aligned<u8>>]>::new_uninit_slice(
                2 * std::mem::size_of::<usize>() +
                /* u8 + alignment */
                4
                + len,
            );
            let ptr = uninit.as_mut_ptr();
            ptr.byte_add(offset_of!(Self, width))
                .cast::<u32>()
                .write(width);
            ptr.byte_add(offset_of!(Self, height))
                .cast::<u32>()
                .write(height);
            UniqueArc::transmute(uninit)
        }
    }

    pub fn bytes_mut(&mut self, len: usize) -> &mut [MaybeUninit<u8>] {
        unsafe {
            std::slice::from_raw_parts_mut(
                (self as *mut Self).byte_add(offset_of!(Self, data)).cast(),
                len,
            )
        }
    }

    pub unsafe fn assume_init<F: BitmapPixel>(mut this: UniqueArc<Self>) -> UniqueArc<Self> {
        this.format.write(F::KIND);
        UniqueArc::transmute(this)
    }

    pub unsafe fn assume_init_dynamic(
        mut this: UniqueArc<Self>,
        format: PixelFormat,
    ) -> UniqueArc<Bitmap<Dynamic>> {
        this.format.write(format);
        UniqueArc::transmute(this)
    }
}

impl<P: BitmapFormat> Bitmap<P> {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl<P: BitmapPixel> Bitmap<P> {
    pub fn data(&self) -> &[P] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self)
                    .byte_add(offset_of!(Self, data))
                    .cast::<P>(),
                (self.width as usize) * (self.height as usize),
            )
        }
    }

    pub fn into_dynamic(this: UniqueArc<Self>) -> UniqueArc<Dynamic> {
        unsafe { UniqueArc::transmute(this) }
    }
}

impl<P: InitBitmapFormat> Bitmap<P> {
    pub fn bytes(&self) -> &[u8] {
        let width = self.format.width();
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self)
                    .byte_add(offset_of!(Self, data))
                    .cast::<u8>(),
                (self.width as usize) * (self.height as usize) * usize::from(width),
            )
        }
    }
}

impl Bitmap<Dynamic> {
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn cast(&self) -> BitmapCast<'_> {
        match self.format {
            PixelFormat::Bgra => BitmapCast::Bgra(unsafe { std::mem::transmute(self) }),
            PixelFormat::Mono => BitmapCast::Mono(unsafe { std::mem::transmute(self) }),
        }
    }
}
