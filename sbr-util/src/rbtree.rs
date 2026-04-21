use std::{
    cmp::Ordering,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

pub trait Node {
    // Called when a node is relocated in the tree.
    fn update(&mut self, left: Option<&Self>, right: Option<&Self>) {
        _ = left;
        _ = right;
    }
}

/// Simple policy-based red-black tree for implementing asymptotically-cool
/// data structures.
//
// NOTE: Last time I implemented a B-Tree it was **1.5 thousand lines** so
//       I decided to implement a simple insert-only red-black tree instead.
struct RedBlackNode<N: Node> {
    tagged_parent: ParentPointer<N>,
    children: [Option<NonNull<RedBlackNode<N>>>; 2],
    inner: N,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Red = 1,
    Black = 0,
}

// Tagged with color bit
struct ParentPointer<N: Node>(*mut RedBlackNode<N>);

impl<N: Node> ParentPointer<N> {
    fn new(ptr: *mut RedBlackNode<N>, color: Color) -> Self {
        Self(ptr.map_addr(|a| a | color as usize))
    }

    fn color(&self) -> Color {
        if self.0.addr() & 0b1 == 0 {
            Color::Black
        } else {
            Color::Red
        }
    }

    fn ptr(&self) -> Option<NonNull<RedBlackNode<N>>> {
        NonNull::new(self.0.map_addr(|a| a & !0b1))
    }

    fn set_color(&mut self, color: Color) {
        self.0 = self.0.map_addr(|a| (a & !0b1) | color as usize);
    }

    fn set_ptr(&mut self, ptr: Option<NonNull<RedBlackNode<N>>>) {
        self.0 = ptr
            .map_or(std::ptr::null_mut(), |p| p.as_ptr())
            .map_addr(|a| a | (self.0.addr() & 0b1));
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Left = 0,
    Right = 1,
}

impl Direction {
    fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Raw pointer to a valid [`RedBlackNode`].
#[repr(transparent)]
struct CursorRaw<N: Node>(NonNull<RedBlackNode<N>>);

impl<N: Node> Clone for CursorRaw<N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<N: Node> Copy for CursorRaw<N> {}

impl<N: Node> PartialEq for CursorRaw<N> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<N: Node> Eq for CursorRaw<N> {}

impl<N: Node> std::fmt::Debug for CursorRaw<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&format!("RedBlackNode@{:?}", self.0))
            .field("parent", &self.parent().map(|x| x.0))
            .field("color", &self.color())
            .field("left", &self.child(Direction::Left).map(|x| x.0))
            .field("right", &self.child(Direction::Right).map(|x| x.0))
            .finish()
    }
}

impl<N: Node> CursorRaw<N> {
    #[must_use]
    fn step(mut self, dir: Direction) -> Option<Self> {
        if let Some(other) = self.child(dir) {
            return Some(other.leaf(dir.opposite()));
        }

        loop {
            let parent = self.parent()?;
            if Some(self) == parent.child(dir) {
                self = parent;
            } else {
                return Some(parent);
            }
        }
    }

    #[inline]
    #[must_use]
    fn parent(self) -> Option<Self> {
        unsafe { (*self.0.as_ptr()).tagged_parent.ptr().map(Self) }
    }

    #[inline]
    fn set_parent(self, to: Option<CursorRaw<N>>) {
        unsafe { (*self.0.as_ptr()).tagged_parent.set_ptr(to.map(|x| x.0)) }
    }

    #[inline]
    #[must_use]
    fn direction_in(self, parent: CursorRaw<N>) -> Direction {
        match Some(self) == parent.child(Direction::Right) {
            true => Direction::Right,
            false => Direction::Left,
        }
    }

    #[inline]
    #[must_use]
    fn color(self) -> Color {
        unsafe { (*self.0.as_ptr()).tagged_parent.color() }
    }

    #[inline]
    fn set_color(self, to: Color) {
        unsafe { (*self.0.as_ptr()).tagged_parent.set_color(to) }
    }

    #[inline]
    #[must_use]
    fn child(self, dir: Direction) -> Option<Self> {
        unsafe { (*self.0.as_ptr()).children[dir as usize] }.map(Self)
    }

    #[inline]
    fn set_child(self, dir: Direction, to: Option<CursorRaw<N>>) {
        unsafe { (*self.0.as_ptr()).children[dir as usize] = to.map(|x| x.0) }
    }

    #[inline]
    #[must_use]
    fn leaf(mut self, dir: Direction) -> Self {
        while let Some(right) = self.child(dir) {
            self = right;
        }
        self
    }

    #[inline]
    #[must_use]
    unsafe fn inner<'a>(&self) -> &'a N {
        &(*self.0.as_ptr()).inner
    }

    #[inline]
    #[must_use]
    unsafe fn inner_mut<'a>(&mut self) -> &'a mut N {
        &mut (*self.0.as_ptr()).inner
    }
}

pub struct RedBlackTree<N: Node> {
    root: Option<CursorRaw<N>>,
}

impl<N: Node> Drop for RedBlackTree<N> {
    fn drop(&mut self) {
        if let Some(CursorRaw(root)) = self.root.take() {
            unsafe { Self::free_node(root) };
        }
    }
}

impl<N: Node> RedBlackTree<N> {
    pub fn new() -> Self {
        Self { root: None }
    }

    fn alloc_node(inner: N) -> CursorRaw<N> {
        unsafe {
            CursorRaw(NonNull::new_unchecked(Box::into_raw(Box::new(
                RedBlackNode {
                    tagged_parent: ParentPointer(std::ptr::null_mut()),
                    children: [None; 2],
                    inner,
                },
            ))))
        }
    }

    unsafe fn free_node(node: NonNull<RedBlackNode<N>>) {
        for child in (*node.as_ptr()).children.into_iter().flatten() {
            Self::free_node(child);
        }

        _ = Box::from_raw(node.as_ptr());
    }

    unsafe fn rotate(&mut self, pivot: CursorRaw<N>, parent: Option<CursorRaw<N>>, dir: Direction) {
        let new_root = pivot.child(dir.opposite()).unwrap();
        let new_child = new_root.child(dir);

        pivot.set_child(dir.opposite(), new_child);

        if let Some(new_child) = new_child {
            new_child.set_parent(Some(pivot));
        }

        new_root.set_child(dir, Some(pivot));

        new_root.set_parent(parent);
        pivot.set_parent(Some(new_root));
        if let Some(parent) = parent {
            parent.set_child(pivot.direction_in(parent), Some(new_root));
        } else {
            self.root = Some(new_root);
        }
    }

    #[inline]
    unsafe fn run_update(mut node: CursorRaw<N>) {
        let [left, right] = (*node.0.as_ptr())
            .children
            .map(|x| x.map(|p| &p.as_ref().inner));

        // This tree cannot handle node update panics gracefully.
        // An alternative to aborting the process would be poisoning which would cause
        // all operations on the tree to panic when poisoned, but this is simpler and
        // node updates really should not panic.
        // Note that we build with `panic=abort` by default so this is just a precaution
        // in case someone decides to build the library with unwinding panics^[1].
        //
        // [1]: This is not a recommendation to build subrandr with `panic=unwind`,
        //      other parts of the library (like `Cache` in this crate) are still
        //      unsound under unwinding panics. (TODO)
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            node.inner_mut().update(left, right);
        }))
        .is_err()
        {
            std::process::abort();
        }
    }

    unsafe fn insert_child(
        &mut self,
        mut node: CursorRaw<N>,
        parent: Option<CursorRaw<N>>,
        dir: Direction,
    ) {
        let parent_ptr = parent.map_or(std::ptr::null_mut(), |x| x.0.as_ptr());
        (*node.0.as_ptr()).tagged_parent = ParentPointer::new(parent_ptr, Color::Red);

        let Some(mut parent) = parent else {
            self.root = Some(node);
            Self::run_update(node);
            return;
        };

        parent.set_child(dir, Some(node));

        loop {
            if parent.color() == Color::Black {
                break;
            }

            // If our parent is `red` but it is the root then we can trivially
            // recolor it to black.
            let Some(grandparent) = parent.parent() else {
                parent.set_color(Color::Black);
                break;
            };

            let pdir = parent.direction_in(grandparent);
            let uncle = grandparent.child(pdir.opposite());

            if let Some(uncle) = uncle.filter(|uncle| uncle.color() == Color::Red) {
                // Our black grandparent is overfilled and we need to split it
                // into two black nodes.
                parent.set_color(Color::Black);
                uncle.set_color(Color::Black);
                // This may cause the grandparent to overfill another node
                // higher up the tree which we will take care of next iteration.
                grandparent.set_color(Color::Red);
                Self::run_update(node);
                Self::run_update(parent);
                node = grandparent;
            } else {
                // If our node is an inner child of parent then our rotation
                // wouldn't work and would instead swap in some random black
                // node where we want our node to end up so we first have to
                // rotate parent.
                if Some(node) == parent.child(pdir.opposite()) {
                    self.rotate(parent, Some(grandparent), pdir);
                    Self::run_update(parent);
                    parent = grandparent.child(pdir).unwrap_unchecked();
                }

                // Our child is an outer child of the parent so we can now
                // rotate through the grandparent and recolor the nodes
                // according to their new positions.
                let grandgrandparent = grandparent.parent();
                self.rotate(grandparent, grandgrandparent, pdir.opposite());
                Self::run_update(grandparent);
                if let Some(grandgrandparent) = grandgrandparent {
                    Self::run_update(grandgrandparent);
                }
                parent.set_color(Color::Black);
                grandparent.set_color(Color::Red);
                break;
            }

            parent = match node.parent() {
                Some(next_parent) => next_parent,
                None => break,
            }
        }

        loop {
            Self::run_update(node);

            node = match node.parent() {
                Some(next_parent) => next_parent,
                None => break,
            }
        }
    }

    unsafe fn insert_at(&mut self, node: CursorRaw<N>, at: CursorRaw<N>, dir: Direction) {
        match at.child(dir) {
            Some(child) => {
                self.insert_child(node, Some(child.leaf(dir.opposite())), dir.opposite());
            }
            None => self.insert_child(node, Some(at), dir),
        }
    }

    fn find_by_raw(
        &self,
        mut cmp: impl FnMut(Cursor<N>) -> Ordering,
    ) -> Result<CursorRaw<N>, InsertPosRaw<N>> {
        let mut current = match self.root {
            Some(root) => root,
            None => return Err(InsertPosRaw::End(None)),
        };

        loop {
            match cmp(Cursor::from_raw(current)) {
                Ordering::Less => {
                    current = match current.child(Direction::Right) {
                        Some(ptr) => ptr,
                        None => {
                            return Err(current
                                .step(Direction::Right)
                                .map_or(InsertPosRaw::End(Some(current)), InsertPosRaw::Before));
                        }
                    }
                }
                Ordering::Equal => return Ok(current),
                Ordering::Greater => {
                    current = match current.child(Direction::Left) {
                        Some(ptr) => ptr,
                        None => return Err(InsertPosRaw::Before(current)),
                    }
                }
            }
        }
    }

    #[inline]
    pub fn find_by_mut(
        &mut self,
        cmp: impl FnMut(Cursor<N>) -> Ordering,
    ) -> Result<CursorMut<'_, N>, InsertPosMut<'_, N>> {
        match self.find_by_raw(cmp) {
            Ok(raw) => Ok(CursorMut { tree: self, raw }),
            Err(InsertPosRaw::Before(raw)) => {
                Err(InsertPosMut::Before(CursorMut { tree: self, raw }))
            }
            Err(InsertPosRaw::End(raw)) => Err(InsertPosMut::End(EndCursorMut { tree: self, raw })),
        }
    }

    #[inline]
    pub fn find_mut(&mut self, value: &N) -> Result<CursorMut<'_, N>, InsertPosMut<'_, N>>
    where
        N: Ord,
    {
        self.find_by_mut(|c| (*c).cmp(value))
    }

    #[inline]
    pub fn insert(&mut self, value: N) -> bool
    where
        N: Ord,
    {
        match self.find_mut(&value) {
            Ok(_) => false,
            Err(mut pos) => {
                pos.insert(value);
                true
            }
        }
    }

    #[inline]
    pub fn find_by(
        &self,
        cmp: impl FnMut(Cursor<N>) -> Ordering,
    ) -> Result<Cursor<'_, N>, InsertPos<'_, N>> {
        match self.find_by_raw(cmp) {
            Ok(raw) => Ok(Cursor::from_raw(raw)),
            Err(InsertPosRaw::Before(raw)) => Err(InsertPos::Before(Cursor::from_raw(raw))),
            Err(InsertPosRaw::End(raw)) => Err(InsertPos::End(EndCursor::from_raw(raw))),
        }
    }

    #[inline]
    pub fn find(&self, value: &N) -> Result<Cursor<'_, N>, InsertPos<'_, N>>
    where
        N: Ord,
    {
        self.find_by(|c| (*c).cmp(value))
    }

    #[inline]
    fn edge_raw(&self, dir: Direction) -> Option<CursorRaw<N>> {
        self.root.map(|raw| raw.leaf(dir))
    }

    #[inline]
    pub fn first_mut(&mut self) -> Option<CursorMut<'_, N>> {
        self.edge_raw(Direction::Left)
            .map(|raw| CursorMut { tree: self, raw })
    }

    #[inline]
    pub fn first(&self) -> Option<Cursor<'_, N>> {
        self.edge_raw(Direction::Left).map(Cursor::from_raw)
    }

    #[inline]
    pub fn last_mut(&mut self) -> Option<CursorMut<'_, N>> {
        self.edge_raw(Direction::Right)
            .map(|raw| CursorMut { tree: self, raw })
    }

    #[inline]
    pub fn last(&self) -> Option<Cursor<'_, N>> {
        self.edge_raw(Direction::Right).map(Cursor::from_raw)
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_, N> {
        Iter(self.first())
    }
}

enum InsertPosRaw<N: Node> {
    Before(CursorRaw<N>),
    End(Option<CursorRaw<N>>),
}

pub enum InsertPosMut<'t, N: Node> {
    Before(CursorMut<'t, N>),
    End(EndCursorMut<'t, N>),
}

impl<N: Node> InsertPosMut<'_, N> {
    pub fn insert(&mut self, value: N) {
        match self {
            InsertPosMut::Before(cursor) => cursor.insert_before(value),
            InsertPosMut::End(cursor) => cursor.insert_before(value),
        }
    }
}

pub struct CursorMut<'t, N: Node> {
    tree: &'t mut RedBlackTree<N>,
    raw: CursorRaw<N>,
}

impl<N: Node> CursorMut<'_, N> {
    #[inline]
    pub fn move_prev(&mut self) -> bool {
        match self.raw.step(Direction::Left) {
            Some(prev) => {
                self.raw = prev;
                true
            }
            None => false,
        }
    }

    #[inline]
    pub fn move_next(mut self) -> bool {
        match self.raw.step(Direction::Right) {
            Some(next) => {
                self.raw = next;
                true
            }
            None => false,
        }
    }

    pub fn insert_before(&mut self, value: N) {
        unsafe {
            self.tree
                .insert_at(RedBlackTree::alloc_node(value), self.raw, Direction::Left);
        }
    }

    pub fn insert_after(&mut self, value: N) {
        unsafe {
            self.tree
                .insert_at(RedBlackTree::alloc_node(value), self.raw, Direction::Right);
        }
    }
}

impl<N: Node> Deref for CursorMut<'_, N> {
    type Target = N;

    fn deref(&self) -> &Self::Target {
        unsafe { self.raw.inner() }
    }
}

impl<N: Node> DerefMut for CursorMut<'_, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.raw.inner_mut() }
    }
}

pub struct EndCursorMut<'t, N: Node> {
    tree: &'t mut RedBlackTree<N>,
    raw: Option<CursorRaw<N>>,
}

impl<'t, N: Node> EndCursorMut<'t, N> {
    #[inline]
    pub fn try_into_prev(self) -> Result<CursorMut<'t, N>, EndCursorMut<'t, N>> {
        match self.raw {
            Some(prev) => Ok(CursorMut {
                tree: self.tree,
                raw: prev,
            }),
            None => Err(self),
        }
    }

    pub fn insert_before(&mut self, value: N) {
        let node = RedBlackTree::alloc_node(value);
        unsafe {
            match self.raw {
                Some(prev) => self.tree.insert_at(node, prev, Direction::Right),
                None => self.tree.insert_child(node, None, Direction::Right),
            }
        }
        self.raw = Some(node);
    }
}

pub enum InsertPos<'t, N: Node> {
    Before(Cursor<'t, N>),
    End(EndCursor<'t, N>),
}

pub struct Cursor<'t, N: Node> {
    tree: PhantomData<&'t RedBlackTree<N>>,
    raw: CursorRaw<N>,
}

impl<N: Node> Clone for Cursor<'_, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<N: Node> Copy for Cursor<'_, N> {}

impl<'t, N: Node> Cursor<'t, N> {
    fn from_raw(raw: CursorRaw<N>) -> Self {
        Self {
            tree: PhantomData,
            raw,
        }
    }

    #[inline]
    #[must_use]
    pub fn prev(self) -> Option<Self> {
        self.raw.step(Direction::Left).map(Self::from_raw)
    }

    #[inline]
    #[must_use]
    pub fn next(self) -> Option<Self> {
        self.raw.step(Direction::Right).map(Self::from_raw)
    }

    #[inline]
    #[must_use]
    pub fn parent(self) -> Option<Self> {
        self.raw.parent().map(|raw| Cursor {
            tree: PhantomData,
            raw,
        })
    }

    #[inline]
    #[must_use]
    pub fn left_child(self) -> Option<Self> {
        self.raw.child(Direction::Left).map(Self::from_raw)
    }

    #[inline]
    #[must_use]
    pub fn right_child(self) -> Option<Self> {
        self.raw.child(Direction::Right).map(Self::from_raw)
    }

    #[inline]
    #[must_use]
    pub fn left_leaf(self) -> Self {
        Cursor {
            tree: PhantomData,
            raw: self.raw.leaf(Direction::Left),
        }
    }

    #[inline]
    #[must_use]
    pub fn right_leaf(self) -> Self {
        Cursor {
            tree: PhantomData,
            raw: self.raw.leaf(Direction::Right),
        }
    }

    #[inline]
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }

    #[inline]
    #[must_use]
    pub fn inner(&self) -> &'t N {
        unsafe { self.raw.inner() }
    }
}

impl<N: Node> Deref for Cursor<'_, N> {
    type Target = N;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

pub struct EndCursor<'t, N: Node> {
    tree: PhantomData<&'t RedBlackTree<N>>,
    raw: Option<CursorRaw<N>>,
}

impl<N: Node> Clone for EndCursor<'_, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<N: Node> Copy for EndCursor<'_, N> {}

impl<'t, N: Node> EndCursor<'t, N> {
    fn from_raw(raw: Option<CursorRaw<N>>) -> Self {
        Self {
            tree: PhantomData,
            raw,
        }
    }

    #[must_use]
    pub fn prev(self) -> Option<Cursor<'t, N>> {
        self.raw.map(Cursor::from_raw)
    }
}

pub struct Iter<'t, N: Node>(Option<Cursor<'t, N>>);

impl<'t, N: Node> Iterator for Iter<'t, N> {
    type Item = &'t N;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let current = self.0?;
        self.0 = current.next();
        Some(unsafe { current.raw.inner() })
    }
}

impl<N: Node> std::iter::FusedIterator for Iter<'_, N> {}

#[cfg(test)]
mod test {
    use super::RedBlackTree;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    struct SumIntNode {
        value: i64,
        sum: i64,
    }

    impl SumIntNode {
        fn new(value: i64) -> Self {
            Self { value, sum: value }
        }
    }

    impl super::Node for SumIntNode {
        fn update(&mut self, left: Option<&Self>, right: Option<&Self>) {
            self.sum = self.value;
            for child in [left, right].into_iter().flatten() {
                self.sum += child.sum;
            }
        }
    }

    fn assert_sorted(tree: &mut RedBlackTree<SumIntNode>, count: usize) {
        let Some(mut current) = tree.first() else {
            assert_eq!(0, count);
            return;
        };

        let mut seen = 1;
        let mut seen_sum = current.value;
        while let Some(next) = current.next() {
            assert!(current.value <= next.value);
            if current.value != next.value {
                let mut sum = 0;
                assert!(tree
                    .find_by(|node| {
                        let ord = node.value.cmp(&next.value);
                        if ord <= std::cmp::Ordering::Equal {
                            sum += node.left_child().map_or(0, |left| left.sum);
                        }
                        if ord == std::cmp::Ordering::Less {
                            sum += node.value;
                        }
                        ord
                    })
                    .is_ok());
                assert_eq!(sum, seen_sum)
            }
            current = next;
            seen += 1;
            seen_sum += current.value;
        }
        assert_eq!(seen, count)
    }

    #[test]
    fn four_ints() {
        let mut tree = RedBlackTree::<SumIntNode>::new();
        assert_sorted(&mut tree, 0);
        tree.insert(SumIntNode::new(6));
        assert_sorted(&mut tree, 1);
        tree.insert(SumIntNode::new(9));
        assert_sorted(&mut tree, 2);
        assert!(tree.insert(SumIntNode::new(3)));
        assert_sorted(&mut tree, 3);
        assert!(!tree.insert(SumIntNode::new(3)));
        assert_sorted(&mut tree, 3);
    }

    #[test]
    fn lots_of_ints() {
        let numbers = [
            38512, 42065, -40181, 5897, 18848, -50424, -34515, 30869, 8949, -8329, 22249, 11553,
            -34380, 23837, 13759, 56038, 41263, -49973, -61004, -63525, 3085, 17422, -26772, 4085,
            -16226, 8587, 36113, 29906, -53645, -23416, 12748, 19447, -6086, -2550, -31712, -20085,
            -15893, -51759, 39186, 40595, 59686, -43712, -54412, -26625, 28711, 48515, 41651,
            -35076, -27869, 46743, 33020, -45563, 58150, -64447, -21126, -57158, 36814, 31494,
            10822, 60547, -53704, 40023, 8464, 60798, -50773, -16522, -58949, -54740, -48645,
            -2342, 44544, 2567, 3461, 38256, 3605, -47676, -55672, -11318, 31361, 54631, 62404,
            -55373, -20833, 38141, -3343, 17655, 60367, 64016, 5824, 6556, -31401, -57712, 27043,
            25619, -22519, -6907, 37940, -19926, 53218, 8170, 3456, -52388, 9701, 44971, -35344,
            20377, -44865, -12446, -29075, 58176, -21436, 57072, 43880, 51335, 1188, -53332, 45254,
            -40681, 25102, 8080, -44503, -60662, 42059, 10712, 52738, -47053, 60899, 25938, -35247,
            -55707, -50813, -3914, -34376, 2738, 54511, 19098, 47035, 19497, 64659, 40779, 63361,
            -48595, -6084, -37853, 17156, -44192, 54896, 40543, 32166, 17571, 63249, -48719, 7955,
            -17281, -457, -18577, 42013, 31144, 56439, 44221, -6712, 189, 64506, 40818, 20100,
            -8560, -23698, -18780, 18096, -60696, -26213, 12707, -30959, 14565, -22728, -1299,
            56802, -23173, -18279, 46795, 27904, 683, -52745, 44809, 32078, 15049, -52003, -10125,
            53142, -64494, -9507, -64310, -28003, 57459, 4452, -6550, 19512, -48643, 9527, 17648,
            16649, 43270, 7719, -41665, -6819, 11517, 28695, -8821, -55403, 2737, -63014, -3459,
            29999, -41855, -33381, 20113, -54601, 4249, 47268, 45548, 22441, 10908, 11481, 55230,
            34025, 6716, 43641, -52145, 44844, -3167, 56938, 41912, -49894, -14452, -49229, -53948,
            -51272, 24908, -11773, -43776, 6033, 4781, -30827, -62827, -18278, 12426, -32043,
            -12080, 3706, -52227, 17443, 24812, 27374, 45632, -24200, -8878,
        ];

        let mut tree = RedBlackTree::<SumIntNode>::new();
        for (i, &number) in numbers.iter().enumerate() {
            assert!(tree.insert(SumIntNode::new(number)));
            assert_sorted(&mut tree, i + 1);
        }
    }
}
