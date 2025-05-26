use std::{
    alloc::{Layout, LayoutError},
    borrow::Borrow,
    cell::{Cell, UnsafeCell},
    collections::HashSet,
    fmt::Debug,
    hash::Hash,
};

#[repr(C)]
struct InlineStr {
    refcount: Cell<usize>,
    length: usize,
    data: [u8; 0],
}

// macro_rules! const_inline_str {
//     ($value: literal) => {
//         const {
//             #[repr(C)]
//             struct LengthedInlineStr<const LENGTH: usize> {
//                 refcount: Cell<usize>,
//                 length: usize,
//                 data: [u8; LENGTH],
//             }

//             const VALUE: &'static str = $value;
//             static LENGTHED: LengthedInlineStr<{ VALUE.len() }> = {
//                 LengthedInlineStr {
//                     refcount: Cell::new(0),
//                     length: VALUE.len(),
//                     data: {
//                         let mut data = [0; VALUE.len()];
//                         data.copy_from_slice(VALUE.as_bytes());
//                         data
//                     },
//                 }
//             };

//             (&raw const LENGTHED).cast::<InlineStr>().cast_mut()
//         }
//     };
// }

// static ROOT: *mut InlineStr = const_inline_str!("root");

impl InlineStr {
    unsafe fn as_str<'a>(this: *const InlineStr) -> &'a str {
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                &raw const (*this).data as *const u8,
                (*this).length,
            ))
        }
    }

    fn layout_for_len(len: usize) -> Result<Layout, LayoutError> {
        Ok(Layout::new::<Self>()
            .extend(Layout::array::<u8>(len)?)?
            .0
            .pad_to_align())
    }
}

struct SymbolRc {
    ptr: *mut InlineStr,
}

impl SymbolRc {
    fn new(string: &str) -> Self {
        Self {
            ptr: unsafe {
                let ptr = std::alloc::alloc(InlineStr::layout_for_len(string.len()).unwrap());
                ptr.cast::<InlineStr>().write(InlineStr {
                    refcount: Cell::new(0),
                    length: string.len(),
                    data: [],
                });
                ptr.cast::<u8>()
                    .add(std::mem::size_of::<InlineStr>())
                    .copy_from(string.as_ptr(), string.len());
                ptr.cast()
            },
        }
    }

    fn is_alive(&self) -> bool {
        unsafe { (*self.ptr).refcount.get() != 0 }
    }

    fn is_dead(&self) -> bool {
        !self.is_alive()
    }

    fn drop_final(self) {
        unsafe {
            assert_eq!((*self.ptr).refcount.get(), 0);

            std::alloc::dealloc(
                self.ptr.cast(),
                InlineStr::layout_for_len((*self.ptr).length).unwrap(),
            );

            std::mem::forget(self);
        }
    }
}

impl Debug for SymbolRc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe { Debug::fmt(InlineStr::as_str(self.ptr), f) }
    }
}

impl Clone for SymbolRc {
    fn clone(&self) -> Self {
        unsafe {
            (*self.ptr).refcount.set((*self.ptr).refcount.get() + 1);
        }
        Self { ptr: self.ptr }
    }
}

impl PartialEq<str> for SymbolRc {
    fn eq(&self, other: &str) -> bool {
        unsafe { InlineStr::as_str(self.ptr) == other }
    }
}

impl PartialEq for SymbolRc {
    fn eq(&self, other: &Self) -> bool {
        unsafe { InlineStr::as_str(self.ptr) == InlineStr::as_str(other.ptr) }
    }
}

impl Eq for SymbolRc {}

impl Hash for SymbolRc {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        unsafe {
            InlineStr::as_str(self.ptr).hash(state);
        }
    }
}

impl Borrow<str> for SymbolRc {
    fn borrow(&self) -> &str {
        unsafe { InlineStr::as_str(self.ptr) }
    }
}

impl Drop for SymbolRc {
    fn drop(&mut self) {
        unsafe {
            let refcnt = (*self.ptr).refcount.get();
            if refcnt == 0 {
                std::alloc::dealloc(
                    self.ptr.cast(),
                    InlineStr::layout_for_len((*self.ptr).length).unwrap(),
                );
            } else {
                (*self.ptr).refcount.set(refcnt - 1);
            }
        }
    }
}

// This could be an integer or use a borrowed pointer/reference but:
// - The first one makes debugging more annoying (opaque integer instead of string)
// - The second one would require Symbol to take a lifetime and that is a pain since
//   100% of its uses are (deeply) self-referential.
#[derive(Debug, Clone)]
pub struct Symbol(SymbolRc);

impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0.ptr, other.0.ptr)
    }
}

impl Eq for Symbol {}

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.ptr.cmp(&other.0.ptr)
    }
}

impl Hash for Symbol {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.ptr.hash(state);
    }
}

#[derive(Debug)]
pub struct SymbolInterner {
    symbols: UnsafeCell<HashSet<SymbolRc>>,
}

impl SymbolInterner {
    pub fn new() -> Self {
        Self {
            symbols: UnsafeCell::new(HashSet::new()),
        }
    }

    pub fn intern(&self, string: &str) -> Symbol {
        {
            let symbols = unsafe { &mut *self.symbols.get() };
            // TODO: feature(hash_set_entry) get_or_insert_with
            //       If that feature ends up containing get_or_insert_with
            //       (tracking issue seems conflicted)
            if let Some(symbol) = symbols.get(string) {
                Symbol(symbol.clone())
            } else {
                let rc = SymbolRc::new(string);
                symbols.insert(rc.clone());
                Symbol(rc)
            }
        }
    }

    pub fn clean(&self) {
        let symbols = unsafe { &mut *self.symbols.get() };
        symbols
            .extract_if(SymbolRc::is_dead)
            .for_each(SymbolRc::drop_final);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn works() {
        let interner = SymbolInterner::new();

        let hello = interner.intern("hello");
        let hello2 = interner.intern("hello");
        let world = interner.intern("world");

        assert_eq!(hello, hello2);
        assert_ne!(hello, world);

        interner.clean();

        assert_eq!(hello, hello2);
        assert_ne!(hello, world);

        drop(hello);
        drop(world);

        assert_eq!(interner.intern("hello"), hello2);

        drop(hello2);

        interner.clean();
    }
}
