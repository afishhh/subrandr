use std::iter::FusedIterator;

pub struct RevIf<I> {
    inner: I,
    reverse: bool,
}

impl<I: DoubleEndedIterator> RevIf<I> {
    pub fn new(iterator: I, reverse: bool) -> Self {
        Self {
            inner: iterator,
            reverse,
        }
    }

    pub fn into_inner(self) -> I {
        self.inner
    }
}

impl<I: DoubleEndedIterator> Iterator for RevIf<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.reverse {
            self.inner.next_back()
        } else {
            self.inner.next()
        }
    }

    fn fold<B, F>(self, init: B, f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        if self.reverse {
            self.inner.rfold(init, f)
        } else {
            self.inner.fold(init, f)
        }
    }
}

impl<I: DoubleEndedIterator> DoubleEndedIterator for RevIf<I> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.reverse {
            self.inner.next()
        } else {
            self.inner.next_back()
        }
    }

    fn rfold<B, F>(self, init: B, f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        if self.reverse {
            self.inner.fold(init, f)
        } else {
            self.inner.rfold(init, f)
        }
    }
}

impl<I: DoubleEndedIterator + FusedIterator> FusedIterator for RevIf<I> {}
