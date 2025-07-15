use std::{cell::Cell, sync::atomic::AtomicUsize};

#[doc(hidden)]
pub mod base;

pub use base::rc_static;

unsafe impl base::Refcount for Cell<usize> {
    fn get(&self) -> usize {
        self.get()
    }

    unsafe fn fetch_inc(&self) -> usize {
        let prev = self.get();
        self.set(prev + 1);
        prev
    }

    unsafe fn dec(&self) -> bool {
        let prev = self.get();
        self.set(prev - 1);
        prev == 1
    }

    fn is_unique(&self) -> bool {
        self.get() == 1
    }
}

pub type Rc<T> = base::RcBase<Cell<usize>, T>;
pub type UniqueRc<T> = base::UniqueRcBase<Cell<usize>, T>;

/* impl<T: ?Sized> !Send for Rc<T> {} */
/* impl<T: ?Sized> !Sync for Rc<T> {} */

unsafe impl base::Refcount for AtomicUsize {
    fn get(&self) -> usize {
        self.load(std::sync::atomic::Ordering::Relaxed)
    }

    unsafe fn fetch_inc(&self) -> usize {
        // As explained in the [Boost documentation][1], Increasing the
        // reference counter can always be done with memory_order_relaxed: New
        // references to an object can only be formed from an existing
        // reference, and passing an existing reference from one thread to
        // another must already provide any required synchronization.
        //
        // [1]: (www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html)
        // ^ obligatory copy-paste from Rust standard library
        self.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    unsafe fn dec(&self) -> bool {
        if self.fetch_sub(1, std::sync::atomic::Ordering::Release) != 1 {
            return false;
        }

        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        true
    }

    fn is_unique(&self) -> bool {
        self.load(std::sync::atomic::Ordering::Acquire) == 1
    }
}

pub type Arc<T> = base::RcBase<AtomicUsize, T>;
pub type UniqueArc<T> = base::UniqueRcBase<AtomicUsize, T>;

unsafe impl<T: Sync + Send + ?Sized> Send for Arc<T> {}
unsafe impl<T: Sync + Send + ?Sized> Sync for Arc<T> {}
