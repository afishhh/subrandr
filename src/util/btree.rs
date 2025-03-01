use std::{
    alloc::Layout, borrow::Borrow, cmp::Ordering, fmt::Debug, marker::PhantomData, mem::offset_of,
    ops::Range, ptr::NonNull,
};

use crate::util::ArrayVec;

// FIXME: This implementation enounters a segmentation fault for B values
//        higher than 2
const B: usize = 2;
const MIN_VALUES: usize = B - 1;
const CAP: usize = 2 * B - 1;

pub trait NodeTraits: Sized {
    // TODO: default = ()
    type Metadata: Default + Copy;
    type Key: Clone + Clone + Ord;
    type Value: Clone + Clone;

    fn combine_metadata(metadata: &mut Self::Metadata, other: &Self::Metadata) {
        _ = metadata;
        _ = other;
    }

    fn combine_element_metadata(
        metadata: &mut Self::Metadata,
        key: &Self::Key,
        value: &Self::Value,
    ) {
        _ = metadata;
        _ = key;
        _ = value;
    }

    fn metadata_debug_name() -> &'static str {
        "METADATA"
    }
}

pub struct Tree<T: NodeTraits> {
    root: Option<NonNull<Node<T>>>,
}

impl<T: NodeTraits> Tree<T> {
    pub const fn new() -> Self {
        Self { root: None }
    }

    pub fn insert(&mut self, key: T::Key, value: T::Value) {
        if let Some(root) = self.root.as_mut() {
            unsafe {
                let mut node = root.as_mut().find_leaf_for(&|x: &T::Key| x.cmp(&key));

                if let Some(new_root) = node.as_mut().insert(key, value, None) {
                    if new_root.as_ref().values.is_empty() {
                        dealloc_simple(new_root);
                        self.root = None;
                    } else {
                        self.root = Some(new_root);
                    }
                }
            }
        } else {
            self.root = Some(alloc_simple(Node::with_values({
                let mut values = ArrayVec::new();
                values.push((key, value));
                values
            })));
        }
    }

    pub fn get<Q>(&self, key: &T::Key) -> Option<(&T::Key, &T::Value, T::Metadata)>
    where
        T::Key: Borrow<Q> + Ord,
        Q: Ord,
    {
        self.get_by(|k| k.cmp(key))
    }

    pub fn get_by(
        &self,
        compare: impl Fn(&T::Key) -> Ordering,
    ) -> Option<(&T::Key, &T::Value, T::Metadata)> {
        if let Some(root) = self.root {
            unsafe {
                let (key, value, metadata) = root.as_ref().get(&compare, T::Metadata::default())?;
                Some((key, value, metadata))
            }
        } else {
            None
        }
    }

    pub fn exclusive_upper_bound<Q>(&self, key: &Q) -> Option<(&T::Key, &T::Value, T::Metadata)>
    where
        T::Key: Borrow<Q> + Ord,
        Q: Ord,
    {
        // doesn't work without it, and I don't know if there's a cleaner way to write this
        #[allow(clippy::needless_borrow)]
        self.exclusive_upper_bound_by(|k| k.borrow().cmp(&key))
    }

    pub fn exclusive_upper_bound_by(
        &self,
        compare: impl Fn(&T::Key) -> Ordering,
    ) -> Option<(&T::Key, &T::Value, T::Metadata)> {
        if let Some(root) = self.root {
            unsafe {
                let (key, value, metadata) = root
                    .as_ref()
                    .upper_bound::<true>(&compare, T::Metadata::default())?;
                Some((key, value, metadata))
            }
        } else {
            None
        }
    }

    pub fn inclusive_upper_bound<Q>(&self, key: &Q) -> Option<(&T::Key, &T::Value, T::Metadata)>
    where
        T::Key: Borrow<Q> + Ord,
        Q: Ord,
    {
        #[allow(clippy::needless_borrow)] // see above
        self.inclusive_upper_bound_by(|k| k.borrow().cmp(&key))
    }

    pub fn inclusive_upper_bound_by(
        &self,
        compare: impl Fn(&T::Key) -> Ordering,
    ) -> Option<(&T::Key, &T::Value, T::Metadata)> {
        if let Some(root) = self.root {
            unsafe {
                let (key, value, metadata) = root
                    .as_ref()
                    .upper_bound::<false>(&compare, T::Metadata::default())?;
                Some((key, value, metadata))
            }
        } else {
            None
        }
    }

    pub fn remove(&mut self, key: &T::Key) -> Option<(T::Key, T::Value)> {
        // eprintln!("REMOVE ({:?} -- {:?})", upper, lower);

        if let Some(mut root) = self.root {
            unsafe {
                let (mut node, idx) = root.as_mut().find_for_remove(key)?;
                let (result, maybe_new_root) = node.as_mut().remove(idx);
                if let Some(new_root) = maybe_new_root {
                    dealloc_simple(root);
                    self.root = Some(new_root);
                }
                result
            }
        } else {
            None
        }
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            raw: RawIter::new(self.root),
            _data: PhantomData,
        }
    }

    fn dump_to(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result
    where
        T::Key: Debug,
        T::Value: Debug,
        T::Metadata: Debug,
    {
        if let Some(root) = self.root {
            unsafe { root.as_ref().dump(fmt, 0) }
        } else {
            writeln!(fmt, "EMPTY TREE")
        }
    }

    pub fn validate(&self)
    where
        T::Metadata: PartialEq + Debug,
    {
        if let Some(root) = self.root {
            unsafe { root.as_ref().validate() };
        }
    }

    pub fn dump(&self)
    where
        T::Key: Debug,
        T::Value: Debug,
        T::Metadata: Debug,
    {
        eprintln!("{self:?}");
    }
}

impl<T: NodeTraits> Debug for Tree<T>
where
    T::Key: Debug,
    T::Value: Debug,
    T::Metadata: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.dump_to(f)
    }
}

impl<T: NodeTraits> Drop for Tree<T> {
    fn drop(&mut self) {
        if let Some(node) = self.root {
            unsafe { Node::free(node) }
        }
    }
}

// NOTE: B-Trees can theoretically be represented in a more compact fashion by storing the height
//       of the tree in the tree, and then inferring whether nodes are leaves or not during
//       traversal. This means nodes themselves don't even have to know whether they're leaves or not
//       but this makes the representation more complex, so not implemented for now.
// PERF: Another optimisation is not checking for null children everywhere, a B-tree node that is
//       not a leaf will ALWAYS have values.len() + 1 children.
//
// TODO: Types with a Drop impl are likely mishandled currently.
// TODO: Remove Clone bound on key and value, it is not necessary just makes things easier.
//       Clone is currently expected to be basically Copy-fast.
#[derive(Debug)]
struct Node<T: NodeTraits> {
    parent: Option<NonNull<Node<T>>>,
    index_in_parent: u8,
    values: ArrayVec<CAP, (T::Key, T::Value)>,
    children: [Option<NonNull<Node<T>>>; CAP + 1],
    metadata: T::Metadata,
}

fn alloc_simple<T>(value: T) -> NonNull<T> {
    unsafe {
        let layout = Layout::new::<T>();
        let ptr = std::alloc::alloc(layout) as *mut T;
        if let Some(nonnull) = NonNull::new(ptr) {
            nonnull.write(value);
            nonnull
        } else {
            std::alloc::handle_alloc_error(layout)
        }
    }
}

unsafe fn dealloc_simple<T>(ptr: NonNull<T>) {
    std::alloc::dealloc(ptr.as_ptr() as *mut u8, Layout::new::<T>());
}

// FIXME: Violates Stacked Borrows everywhere
// TODO: Fix UB once arbitrary self types are stable or something idk
impl<T: NodeTraits> Node<T> {
    fn with_values(values: ArrayVec<CAP, (T::Key, T::Value)>) -> Self {
        Self {
            parent: None,
            index_in_parent: u8::MAX,
            children: [const { None }; CAP + 1],
            metadata: {
                let mut metadata = T::Metadata::default();
                for (key, value) in values.iter() {
                    T::combine_element_metadata(&mut metadata, key, value);
                }
                metadata
            },
            values,
        }
    }

    unsafe fn free(node: NonNull<Self>) {
        unsafe {
            for child in node.as_ref().children[..node.as_ref().values.len() + 1]
                .iter()
                .flatten()
            {
                Node::free(*child)
            }

            dealloc_simple(node);
        }
    }

    fn dump(&self, fmt: &mut std::fmt::Formatter, mut depth: usize) -> std::fmt::Result
    where
        T::Key: Debug,
        T::Value: Debug,
        T::Metadata: Debug,
    {
        let print_indent = |fmt: &mut std::fmt::Formatter, depth: usize| {
            for _ in 0..depth {
                write!(fmt, "  ")?;
            }
            Ok(())
        };

        print_indent(fmt, depth)?;
        writeln!(
            fmt,
            "NODE ORDER={} {}={:?} PTR={:?} PARENT_SLOT={:?}",
            self.values.len(),
            T::metadata_debug_name(),
            self.metadata,
            self as *const Self,
            self.parent.is_some().then_some(self.index_in_parent)
        )?;

        depth += 1;
        if let Some(child0) = self.children[0] {
            unsafe {
                child0.as_ref().dump(fmt, depth)?;
            }

            assert_eq!(
                unsafe { child0.as_ref().parent.unwrap().as_ptr() as *const Self },
                (self as *const Self),
                "b-tree tree structure is inconsistent: incorrect parent pointer"
            );
        }

        let mut c = 1;
        for (key, value) in self.values.iter() {
            print_indent(fmt, depth)?;
            writeln!(fmt, "VALUE KEY={key:?} VALUE={value:?}")?;
            if let Some(nchild) = self.children[c] {
                unsafe {
                    nchild.as_ref().dump(fmt, depth)?;
                }

                assert_eq!(
                    unsafe { nchild.as_ref().parent.unwrap().as_ptr() as *const Self },
                    (self as *const Self),
                    "b-tree tree structure is inconsistent: incorrect parent pointer"
                );
            }
            c += 1;
        }

        for i in self.values.len() + 1..self.children.len() {
            print_indent(fmt, depth)?;
            writeln!(fmt, "GHOST CHILD PTR={:?}", self.children[i])?;
        }

        Ok(())
    }

    fn validate(&self)
    where
        T::Metadata: PartialEq + Debug,
    {
        let mut metadata = T::Metadata::default();

        for i in 0..self.values.len() + 1 {
            if let Some(nchild) = self.children[i] {
                assert_eq!(
                    unsafe { nchild.as_ref().parent.unwrap().as_ptr() as *const Self },
                    (self as *const Self),
                    "b-tree tree structure is inconsistent: incorrect parent pointer"
                );
                assert_eq!(
                    usize::from(unsafe { nchild.as_ref().index_in_parent }),
                    i,
                    "b-tree tree structure is inconsistent: incorrect parent slot index"
                );

                unsafe { nchild.as_ref().validate() };
                T::combine_metadata(&mut metadata, unsafe { &nchild.as_ref().metadata });
            }
        }

        for (key, value) in self.values.iter() {
            T::combine_element_metadata(&mut metadata, key, value);
        }

        assert_eq!(
            self.metadata, metadata,
            "b-tree tree structure is inconsistent: metadata is incorrect"
        );
    }

    // TODO: Result<(NonNull<Node>, usize), NonNull<Node>>?
    //       Ok for found, Err for not found but a viable insert node returned
    fn find_leaf_for(&mut self, compare: &impl Fn(&T::Key) -> Ordering) -> NonNull<Self> {
        let idx = self
            .values
            .iter()
            .position(|(key, _)| compare(key) == Ordering::Greater)
            .unwrap_or(self.values.len());

        if let Some(mut child) = self.children[idx] {
            unsafe { child.as_mut().find_leaf_for(compare) }
        } else {
            NonNull::from(self)
        }
    }

    fn set_left_right(
        &mut self,
        insert_index: usize,
        new_children: Option<(NonNull<Self>, NonNull<Self>)>,
    ) {
        if let Some((left, right)) = new_children {
            self.children[insert_index] = Some(left);
            self.children[insert_index + 1] = Some(right);

            unsafe {
                let this = NonNull::from(self);
                Self::set_parent_info(left, Some(this), insert_index as u8);
                Self::set_parent_info(right, Some(this), (insert_index + 1) as u8);
            }
        }
    }

    #[inline(always)]
    unsafe fn set_parent_info(
        this: NonNull<Self>,
        parent: Option<NonNull<Self>>,
        index_in_parent: u8,
    ) {
        this.byte_add(offset_of!(Self, parent))
            .cast::<Option<NonNull<Self>>>()
            .write(parent);
        Self::set_index_in_parent(this, index_in_parent);
    }

    #[inline(always)]
    unsafe fn set_index_in_parent(this: NonNull<Self>, index_in_parent: u8) {
        this.byte_add(offset_of!(Self, index_in_parent))
            .cast::<u8>()
            .write(index_in_parent);
    }

    fn recompute_statistics(&mut self) {
        unsafe {
            self.metadata = T::Metadata::default();

            for child in self.children[..self.values.len() + 1].iter().flatten() {
                T::combine_metadata(&mut self.metadata, &child.as_ref().metadata);
            }

            for (key, value) in self.values.iter() {
                T::combine_element_metadata(&mut self.metadata, key, value);
            }
        }
    }

    fn recompute_statistics_until_root(&mut self) {
        let mut current = self;
        loop {
            current.recompute_statistics();
            match current.parent {
                Some(mut parent) => {
                    current = unsafe { parent.as_mut() };
                }
                None => return,
            }
        }
    }

    pub fn insert(
        &mut self,
        key: T::Key,
        value: T::Value,
        new_children: Option<(NonNull<Self>, NonNull<Self>)>,
    ) -> Option<NonNull<Self>> {
        let insert_index = self
            .values
            .iter()
            .position(|(current, _)| current > &key)
            .unwrap_or(self.values.len());

        // eprintln!(
        //     "INSERT ({:?} -- {:?}) INDEX={insert_index}",
        //     element.upper, element.lower
        // );

        if self.values.len() < CAP {
            self.values.insert(insert_index, (key, value));

            self.children
                .copy_within(insert_index + 1..self.values.len(), insert_index + 2);
            for i in insert_index + 2..self.values.len() + 1 {
                if let Some(child) = self.children[i] {
                    unsafe {
                        Self::set_index_in_parent(child, i as u8);
                    }
                }
            }

            self.set_left_right(insert_index, new_children);
            self.recompute_statistics_until_root();

            None
        } else {
            // This node is going to be split in two, so remove ourselves from the parent.
            let parent = if let Some(mut parent) = self.parent {
                unsafe {
                    parent.as_mut().children[usize::from(self.index_in_parent)] = None;
                }
                Some(parent)
            } else {
                None
            };

            let median;
            let (left, right) = match insert_index.cmp(&(CAP / 2)) {
                // self.values[CAP / 2 - 1] is the median
                std::cmp::Ordering::Less => {
                    const MEDIAN_IDX: usize = CAP / 2 - 1;
                    median = unsafe { std::ptr::read(&self.values[MEDIAN_IDX]) };

                    let mut new_right = alloc_simple(Node::with_values(ArrayVec::from_slice(
                        &self.values[MEDIAN_IDX + 1..],
                    )));

                    unsafe {
                        new_right.as_mut().children[..CAP - MEDIAN_IDX]
                            .copy_from_slice(&self.children[MEDIAN_IDX + 1..]);
                        for (i, child) in new_right.as_mut().children[..CAP - MEDIAN_IDX]
                            .iter()
                            .enumerate()
                            .filter_map(|(i, o)| o.map(|p| (i, p)))
                        {
                            Self::set_parent_info(child, Some(new_right), i as u8);
                        }

                        new_right.as_mut().recompute_statistics();

                        self.set_left_right(insert_index, new_children);
                        self.values.set_len(MEDIAN_IDX);
                        self.values.insert(insert_index, (key, value));
                        self.recompute_statistics();

                        (NonNull::from(self), new_right)
                    }
                }
                // the inserted element is the median
                std::cmp::Ordering::Equal => {
                    const MEDIAN_IDX: usize = CAP / 2;
                    median = (key, value);

                    let mut new_right = alloc_simple(Node::with_values(ArrayVec::from_slice(
                        &self.values[MEDIAN_IDX..],
                    )));

                    unsafe {
                        new_right.as_mut().children[..CAP + 1 - MEDIAN_IDX]
                            .copy_from_slice(&self.children[MEDIAN_IDX..]);
                        for (i, child) in new_right.as_mut().children[..CAP + 1 - MEDIAN_IDX]
                            .iter()
                            .enumerate()
                            .filter_map(|(i, o)| o.map(|p| (i, p)))
                        {
                            Self::set_parent_info(child, Some(new_right), i as u8);
                        }
                        self.values.set_len(MEDIAN_IDX);

                        if let Some((left, right)) = new_children {
                            new_right.as_mut().children[0] = Some(right);
                            Self::set_parent_info(right, Some(new_right), 0);
                            self.children[self.values.len()] = Some(left);
                            Self::set_parent_info(
                                left,
                                Some(NonNull::from(&mut *self)),
                                self.values.len() as u8,
                            );
                        }

                        new_right.as_mut().recompute_statistics();
                        self.recompute_statistics();

                        (NonNull::from(self), new_right)
                    }
                }
                // self.values[CAP / 2] is the median
                std::cmp::Ordering::Greater => {
                    const MEDIAN_IDX: usize = CAP / 2;
                    median = unsafe { std::ptr::read(&self.values[MEDIAN_IDX]) };

                    let mut right_elements =
                        ArrayVec::from_slice(&self.values[MEDIAN_IDX + 1..insert_index]);
                    right_elements.push((key, value));
                    right_elements.extend_from_slice(&self.values[insert_index..]);
                    let mut new_right = alloc_simple(Node::with_values(right_elements));

                    unsafe {
                        new_right.as_mut().children[..CAP - MEDIAN_IDX]
                            .copy_from_slice(&self.children[MEDIAN_IDX + 1..]);
                        for (i, child) in new_right.as_mut().children[..CAP - MEDIAN_IDX]
                            .iter()
                            .enumerate()
                            .filter_map(|(i, o)| o.map(|p| (i, p)))
                        {
                            Self::set_parent_info(child, Some(new_right), i as u8);
                        }

                        let inner_index = insert_index - (MEDIAN_IDX + 1);
                        let len = new_right.as_ref().values.len();
                        new_right
                            .as_mut()
                            .children
                            .copy_within(inner_index + 1..len, inner_index + 2);
                        for i in inner_index + 2..new_right.as_ref().values.len() + 1 {
                            if let Some(child) = new_right.as_mut().children[i] {
                                Self::set_index_in_parent(child, i as u8);
                            }
                        }
                        new_right
                            .as_mut()
                            .set_left_right(insert_index - (MEDIAN_IDX + 1), new_children);
                        new_right.as_mut().recompute_statistics();
                        self.values.set_len(MEDIAN_IDX);
                        self.recompute_statistics();

                        (NonNull::from(self), new_right)
                    }
                }
            };

            match parent {
                Some(mut parent) => {
                    unsafe { parent.as_mut() }.insert(median.0, median.1, Some((left, right)))
                }
                None => {
                    let mut new_root =
                        alloc_simple(Self::with_values(ArrayVec::from_array([median])));
                    unsafe {
                        let root = new_root.as_mut();
                        root.children[0] = Some(left);
                        root.children[1] = Some(right);
                        Self::set_parent_info(left, Some(new_root), 0);
                        Self::set_parent_info(right, Some(new_root), 1);
                        root.recompute_statistics();
                    }
                    Some(new_root)
                }
            }
        }
    }

    fn get(
        &self,
        compare: &impl Fn(&T::Key) -> Ordering,
        mut meta: T::Metadata,
    ) -> Option<(&T::Key, &T::Value, T::Metadata)> {
        let idx = self
            .values
            .iter()
            .position(|(key, _)| compare(key) == Ordering::Greater)
            .unwrap_or(self.values.len());

        for (key, value) in &self.values[..idx] {
            T::combine_element_metadata(&mut meta, key, value);
        }

        for value in self.children[..idx].iter().flatten() {
            unsafe {
                T::combine_metadata(&mut meta, &value.as_ref().metadata);
            }
        }

        if let Some(child) = self.children[idx] {
            if let Some(result) = unsafe { child.as_ref().get(compare, meta) } {
                return Some(result);
            }
        }

        let candidate = &self.values[idx.checked_sub(1)?];

        if compare(&candidate.0) != Ordering::Equal {
            return None;
        }

        Some((&candidate.0, &candidate.1, meta))
    }

    fn upper_bound<const EXCLUSIVE: bool>(
        &self,
        compare: &impl Fn(&T::Key) -> Ordering,
        mut meta: T::Metadata,
    ) -> Option<(&T::Key, &T::Value, T::Metadata)> {
        let idx = self
            .values
            .iter()
            .position(|(key, _)| {
                if EXCLUSIVE {
                    compare(key) != Ordering::Less
                } else {
                    compare(key) == Ordering::Greater
                }
            })
            .unwrap_or(self.values.len());

        for (key, value) in &self.values[..idx] {
            T::combine_element_metadata(&mut meta, key, value);
        }

        for value in self.children[..idx].iter().flatten() {
            unsafe {
                T::combine_metadata(&mut meta, &value.as_ref().metadata);
            }
        }

        if let Some(child) = self.children[idx] {
            if let Some(result) = unsafe { child.as_ref().upper_bound::<EXCLUSIVE>(compare, meta) }
            {
                return Some(result);
            }
        }

        let candidate = &self.values[idx.checked_sub(1)?];

        if if EXCLUSIVE {
            compare(&candidate.0) != Ordering::Less
        } else {
            compare(&candidate.0) == Ordering::Greater
        } {
            return None;
        }

        Some((&candidate.0, &candidate.1, meta))
    }

    fn find_for_remove(&mut self, key: &T::Key) -> Option<(NonNull<Self>, usize)> {
        let idx = self
            .values
            .iter()
            .position(|(current, _)| current > key)
            .unwrap_or(self.values.len());

        if let Some(candidate_idx) = idx.checked_sub(1) {
            if &self.values[candidate_idx].0 == key {
                return Some((NonNull::from(self), candidate_idx));
            }
        }

        if let Some(mut child) = self.children[idx] {
            unsafe { child.as_mut().find_for_remove(key) }
        } else {
            None
        }
    }

    fn find_rightmost_leaf(&mut self) -> NonNull<Node<T>> {
        unsafe {
            self.children[self.values.len()].map_or(NonNull::from(self), |mut child| {
                child.as_mut().find_rightmost_leaf()
            })
        }
    }

    fn find_leftmost_leaf(&self) -> NonNull<Node<T>> {
        unsafe {
            self.children[0].map_or(NonNull::from(self), |mut child| {
                child.as_mut().find_leftmost_leaf()
            })
        }
    }

    fn copy_children(&mut self, src: Range<usize>, dst: usize) {
        for i in src.clone() {
            if let Some(child) = self.children[i] {
                unsafe {
                    Self::set_index_in_parent(child, (dst + i - src.start) as u8);
                }
            }
        }

        self.children.copy_within(src, dst);
    }

    fn get_child_checked(&self, index: usize) -> Option<NonNull<Self>> {
        if index > self.values.len() {
            return None;
        }

        unsafe { *self.children.get_unchecked(index) }
    }

    fn rebalance(&mut self) -> Option<NonNull<Self>> {
        let Some(mut parent) = self.parent else {
            unreachable!("cannot rebalance root");
        };
        let index_in_parent = usize::from(self.index_in_parent);

        unsafe {
            let left_index = index_in_parent.checked_sub(1);
            let left_sibling =
                left_index.and_then(|idx| *parent.as_ref().children.get_unchecked(idx));
            let right_sibling = parent.as_ref().get_child_checked(index_in_parent + 1);

            if let Some(mut sibling) = left_sibling.filter(|n| n.as_ref().values.len() > MIN_VALUES)
            {
                // println!(
                //     "Rebalance taking excess values from left sibling {sibling:?} (has {})",
                //     sibling.as_ref().values.len()
                // );

                let p = parent.as_mut();
                self.values
                    .insert(0, std::ptr::read(&p.values[index_in_parent - 1]));

                let child = sibling.as_ref().children[sibling.as_ref().values.len()];
                if let Some(mut child) = child {
                    child.as_mut().parent = NonNull::new(&mut *self);
                    child.as_mut().index_in_parent = 0;
                }
                self.copy_children(0..self.values.len(), 1);
                self.children[0] = child;

                std::ptr::write(
                    &mut p.values[index_in_parent - 1],
                    sibling.as_mut().values.pop().unwrap(),
                );

                self.recompute_statistics();
                sibling.as_mut().recompute_statistics();
                p.recompute_statistics_until_root();

                None
            } else if let Some(mut sibling) =
                right_sibling.filter(|n| n.as_ref().values.len() > MIN_VALUES)
            {
                // println!(
                //     "Rebalance taking excess values from right sibling {sibling:?} (has {})",
                //     sibling.as_ref().values.len()
                // );

                let p = parent.as_mut();
                self.values.push(std::mem::replace(
                    &mut p.values[index_in_parent],
                    sibling.as_mut().values.remove(0),
                ));

                let child = sibling.as_ref().children[0];
                if let Some(mut child) = child {
                    child.as_mut().parent = NonNull::new(&mut *self);
                    child.as_mut().index_in_parent = self.values.len() as u8;
                }
                self.children[self.values.len()] = child;

                sibling
                    .as_mut()
                    .copy_children(1..sibling.as_ref().values.len() + 2, 0);

                self.recompute_statistics();
                sibling.as_mut().recompute_statistics();
                p.recompute_statistics_until_root();

                None
            } else {
                let Some((mut sibling, is_right)) = left_sibling
                    .map(|a| (a, false))
                    .or(right_sibling.map(|b| (b, true)))
                else {
                    unreachable!("impossible b-tree state");
                };

                let (right_index, left, right) = if is_right {
                    (index_in_parent + 1, self, sibling.as_mut())
                } else {
                    (index_in_parent, sibling.as_mut(), self)
                };

                // println!(
                //     "Rebalance merging nodes {:?} {:?}",
                //     left as *mut Self, right as *mut Self
                // );

                if right_index < parent.as_ref().values.len() {
                    parent.as_mut().copy_children(
                        right_index + 1..parent.as_ref().values.len() + 1,
                        right_index,
                    );
                }
                left.values
                    .push(parent.as_mut().values.remove(right_index - 1));

                let new_children_range =
                    left.values.len()..left.values.len() + 1 + right.values.len();
                left.children[new_children_range.clone()]
                    .copy_from_slice(&right.children[..right.values.len() + 1]);

                let left_ptr = NonNull::from(&mut *left);
                for (i, child) in left.children[new_children_range.clone()]
                    .iter_mut()
                    .flatten()
                    .enumerate()
                {
                    child.as_mut().parent = Some(left_ptr);
                    child.as_mut().index_in_parent = (i + new_children_range.start) as u8;
                }

                left.values.extend_from_slice(&right.values);
                right.values.set_len(0);
                left.recompute_statistics();

                dealloc_simple(NonNull::from(right));

                if parent.as_mut().parent.is_none() {
                    if parent.as_ref().values.is_empty() {
                        left.parent = None;
                        return Some(NonNull::from(left));
                    } else {
                        parent.as_mut().recompute_statistics();
                        return None;
                    }
                } else if parent.as_ref().values.len() < MIN_VALUES {
                    parent.as_mut().rebalance()
                } else {
                    parent.as_mut().recompute_statistics_until_root();
                    None
                }
            }
        }
    }

    // skill issue
    #[allow(clippy::type_complexity)]
    fn leaf_swap_remove(
        &mut self,
        index: usize,
        slot: Option<(NonNull<Self>, usize)>,
    ) -> (Option<(T::Key, T::Value)>, Option<NonNull<Self>>) {
        if let Some((mut parent, parent_slot)) = slot {
            unsafe {
                parent.as_mut().values[parent_slot] = std::ptr::read(&self.values[index]);
            }
        }

        let result = self.values.remove(index);

        (
            Some(result),
            if self.parent.is_some() && self.values.len() < MIN_VALUES {
                self.rebalance()
            } else {
                self.recompute_statistics_until_root();
                None
            },
        )
    }

    // see above
    #[allow(clippy::type_complexity)]
    fn remove(&mut self, index: usize) -> (Option<(T::Key, T::Value)>, Option<NonNull<Self>>) {
        if let Some(mut left) = self.children[index] {
            unsafe {
                let mut left = left.as_mut().find_rightmost_leaf();
                let idx = left.as_ref().values.len() - 1;
                left.as_mut()
                    .leaf_swap_remove(idx, Some((NonNull::from(self), index)))
            }
        } else if let Some(mut right) = self.children[index + 1] {
            unsafe {
                let mut right = right.as_mut().find_leftmost_leaf();
                right
                    .as_mut()
                    .leaf_swap_remove(0, Some((NonNull::from(self), index)))
            }
        } else {
            self.leaf_swap_remove(index, None)
        }
    }
}

struct RawIter<T: NodeTraits> {
    current: Option<NonNull<Node<T>>>,
    element: usize,
    metadata: T::Metadata,
}

impl<T: NodeTraits> RawIter<T> {
    fn new(mut node: Option<NonNull<Node<T>>>) -> Self {
        if let Some(ref mut node) = node {
            if unsafe { node.as_ref().values.is_empty() } {
                return Self {
                    current: None,
                    element: 0,
                    metadata: T::Metadata::default(),
                };
            }
            unsafe {
                Self::traverse_down(node);
            }
        }

        Self {
            current: node,
            element: 0,
            metadata: T::Metadata::default(),
        }
    }

    unsafe fn traverse_down(node: &mut NonNull<Node<T>>) {
        while let Some(left) = node.as_ref().children[0] {
            *node = left;
        }
    }
}

impl<T: NodeTraits> RawIter<T> {
    fn next(&mut self) -> Option<(NonNull<Node<T>>, usize, T::Metadata)> {
        unsafe {
            let mut current = self.current?;

            T::combine_element_metadata(
                &mut self.metadata,
                &current.as_ref().values[self.element].0,
                &current.as_ref().values[self.element].1,
            );

            let result = (current, self.element, self.metadata);

            self.element += 1;

            if let Some(mut right) = current.as_ref().children[self.element] {
                Self::traverse_down(&mut right);
                self.current = Some(right);
                self.element = 0;
            }

            loop {
                if self.element == current.as_ref().values.len() {
                    self.current = current.as_ref().parent;
                    self.element = usize::from(current.as_ref().index_in_parent);
                    if let Some(next) = self.current {
                        current = next;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            Some(result)
        }
    }
}

pub struct Iter<'a, T: NodeTraits> {
    raw: RawIter<T>,
    _data: PhantomData<&'a (T::Key, T::Value)>,
}

impl<'a, T: NodeTraits + 'a> Iterator for Iter<'a, T> {
    type Item = (&'a T::Key, &'a T::Value, T::Metadata);

    fn next(&mut self) -> Option<Self::Item> {
        let (node, idx, metadata) = self.raw.next()?;
        let (key, value) = unsafe { &node.as_ref().values[idx] };
        Some((key, value, metadata))
    }
}

pub struct SetTraits<K: Ord + Clone>(PhantomData<fn() -> K>);

impl<K: Ord + Clone> NodeTraits for SetTraits<K> {
    type Metadata = ();
    type Key = K;
    type Value = ();
}

pub struct MapTraits<K: Ord + Clone, V: Clone>(PhantomData<fn() -> (K, V)>);

impl<K: Ord + Clone, V: Clone> NodeTraits for MapTraits<K, V> {
    type Metadata = ();
    type Key = K;
    type Value = V;
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use super::*;
    use rand::{
        distr::{Distribution, StandardUniform},
        seq::SliceRandom,
    };

    struct IntSumSetTraits;

    impl NodeTraits for IntSumSetTraits {
        type Metadata = i64;
        type Key = i64;
        type Value = ();

        fn combine_element_metadata(
            metadata: &mut Self::Metadata,
            key: &Self::Key,
            _value: &Self::Value,
        ) {
            *metadata += *key;
        }

        fn combine_metadata(metadata: &mut Self::Metadata, other: &Self::Metadata) {
            *metadata += *other;
        }
    }

    #[test]
    fn simple_int_set() {
        let mut tree = Tree::<SetTraits<i64>>::new();

        tree.insert(128, ());
        tree.insert(54, ());
        tree.insert(256, ());

        tree.validate();
        assert_eq!(
            tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>(),
            vec![54, 128, 256]
        );

        assert_eq!(tree.exclusive_upper_bound(&54), None);
        assert_eq!(tree.exclusive_upper_bound(&128), Some((&54, &(), ())));
        assert_eq!(tree.exclusive_upper_bound(&129), Some((&128, &(), ())));

        // Triggers a Greater split
        tree.insert(2048, ());
        tree.insert(-119, ());
        tree.insert(44, ());
        tree.insert(68, ());

        tree.validate();
        assert_eq!(
            tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>(),
            vec![-119, 44, 54, 68, 128, 256, 2048]
        );

        assert_eq!(tree.exclusive_upper_bound(&2049), Some((&2048, &(), ())));

        tree.insert(512, ());
        // Triggers an Equal split
        tree.insert(384, ());

        tree.validate();
        assert_eq!(
            tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>(),
            vec![-119, 44, 54, 68, 128, 256, 384, 512, 2048]
        );

        tree.insert(47, ());
        // Triggers a Less split
        tree.insert(46, ());

        tree.validate();
        assert_eq!(
            tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>(),
            vec![-119, 44, 46, 47, 54, 68, 128, 256, 384, 512, 2048]
        );

        assert!(tree.remove(&2048).is_some());
        assert!(tree.remove(&512).is_some());
        assert!(tree.remove(&256).is_some());
        assert!(tree.remove(&384).is_some());

        tree.validate();
        assert_eq!(
            tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>(),
            vec![-119, 44, 46, 47, 54, 68, 128]
        );

        assert!(tree.remove(&44).is_some());
        assert!(tree.remove(&54).is_some());

        tree.validate();
        assert_eq!(
            tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>(),
            vec![-119, 46, 47, 68, 128]
        );
    }

    #[test]
    fn simple_int_sum_set() {
        let mut tree = Tree::<IntSumSetTraits>::new();

        tree.insert(128, ());
        tree.insert(54, ());
        tree.insert(256, ());

        tree.validate();
        assert_eq!(tree.exclusive_upper_bound(&54), None);
        assert_eq!(tree.exclusive_upper_bound(&128), Some((&54, &(), 54)));
        assert_eq!(
            tree.exclusive_upper_bound(&129),
            Some((&128, &(), 54 + 128))
        );

        tree.insert(2048, ());
        tree.insert(-119, ());
        tree.insert(44, ());
        tree.insert(68, ());

        tree.validate();
        assert_eq!(
            tree.exclusive_upper_bound(&2049),
            Some((&2048, &(), 128 + 54 + 256 + 44 + 68 - 119 + 2048))
        );
    }

    #[test]
    fn simple_int_string_map() {
        let mut tree = Tree::<MapTraits<i64, &'static str>>::new();

        const S_54: &str = "fifty-four";
        const S_128: &str = "one hundred twenty-eight";
        const S_256: &str = "two hundred fifty-six";
        const S_M128: &str = "a negative number";

        tree.insert(128, S_128);
        tree.insert(54, S_54);
        tree.insert(256, S_256);

        tree.validate();
        assert_eq!(tree.exclusive_upper_bound(&54), None);
        assert_eq!(tree.inclusive_upper_bound(&128), Some((&128, &S_128, ())));
        assert_eq!(tree.inclusive_upper_bound(&129), Some((&128, &S_128, ())));
        assert_eq!(tree.exclusive_upper_bound(&128), Some((&54, &S_54, ())));
        assert_eq!(tree.exclusive_upper_bound(&129), Some((&128, &S_128, ())));

        tree.insert(2048, "");
        tree.insert(-119, S_M128);
        tree.insert(44, "54");
        tree.insert(68, "abc");

        tree.validate();
        assert_eq!(tree.exclusive_upper_bound(&i64::MIN), None);
        assert_eq!(tree.exclusive_upper_bound(&0), Some((&-119, &S_M128, ())));
        assert_eq!(tree.exclusive_upper_bound(&2049), Some((&2048, &"", ())));
        assert_eq!(tree.inclusive_upper_bound(&2049), Some((&2048, &"", ())));
        assert_eq!(tree.inclusive_upper_bound(&2048), Some((&2048, &"", ())));
    }

    #[test]
    fn greater_insert_shift_test() {
        let mut tree = Tree::<SetTraits<i64>>::new();

        tree.insert(0, ());
        tree.insert(1, ());
        tree.insert(2, ());
        tree.insert(3, ());
        tree.insert(4, ());
        tree.insert(25, ());
        tree.insert(26, ());
        tree.insert(27, ());
        tree.insert(28, ());
        tree.insert(5, ());
        tree.insert(6, ());

        tree.dump();
        tree.insert(7, ());
        tree.dump();

        tree.validate();
        assert_eq!(
            tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 25, 26, 27, 28]
        );
    }

    fn random_set_with_size(size: usize, remove: bool) {
        let mut tree = Tree::<SetTraits<u16>>::new();
        let mut set = BTreeSet::new();

        let mut rng = rand::rng();
        for value in StandardUniform.sample_iter(&mut rng).take(size) {
            if set.insert(value) {
                println!("Inserting {value}");
                tree.insert(value, ());
            }

            tree.validate();

            assert_eq!(
                set.iter().copied().collect::<Vec<_>>(),
                tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>()
            );
        }

        for value in StandardUniform.sample_iter(&mut rng).take(1000) {
            let _: u16 = value;
            assert_eq!(
                set.range(..value).last(),
                tree.exclusive_upper_bound(&value).map(|(v, _, _)| v)
            );
        }

        if remove {
            let mut values = set.iter().copied().collect::<Vec<_>>();
            values.shuffle(&mut rng);

            for value in values {
                println!("Removing {value}");
                assert!(tree.remove(&value).is_some());
                assert!(set.remove(&value));

                assert_eq!(
                    set.iter().copied().collect::<Vec<_>>(),
                    tree.iter().map(|(&v, _, _)| v).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn random_int_set_10_insert_only() {
        random_set_with_size(10, false);
    }

    #[test]
    fn random_int_set_10_remove_all() {
        random_set_with_size(10, true);
    }

    #[test]
    fn random_int_set_20_insert_only() {
        random_set_with_size(20, false);
    }

    #[test]
    fn random_int_set_20_remove_all() {
        random_set_with_size(20, true);
    }

    #[test]
    fn random_int_set_150_insert_only() {
        random_set_with_size(150, false);
    }

    #[test]
    fn random_int_set_150_remove_all() {
        random_set_with_size(150, true);
    }

    #[test]
    fn random_int_set_many_500_insert_only() {
        for _ in 0..50 {
            random_set_with_size(500, false);
        }
    }

    #[test]
    fn random_int_set_many_500_remove_all() {
        for _ in 0..50 {
            random_set_with_size(500, true);
        }
    }

    fn random_sum_set_with_size(size: usize, remove: bool) {
        let mut tree = Tree::<IntSumSetTraits>::new();
        let mut set = BTreeSet::new();

        let mut rng = rand::rng();
        for value in StandardUniform.sample_iter(&mut rng).take(size) {
            let _: i16 = value;

            if set.insert(value) {
                println!("Inserting {value}");
                tree.insert(value.into(), ());
            }

            tree.validate();

            assert_eq!(
                set.iter()
                    .copied()
                    .scan(0, |state, value| {
                        *state += value as i64;
                        Some(*state)
                    })
                    .collect::<Vec<_>>(),
                tree.iter().map(|(_, _, m)| m).collect::<Vec<_>>()
            );
        }

        if remove {
            let mut values = set.iter().copied().collect::<Vec<_>>();
            values.shuffle(&mut rng);

            for value in values {
                println!("Removing {value}");
                assert!(tree.remove(&value.into()).is_some());
                assert!(set.remove(&value));

                tree.validate();

                assert_eq!(
                    set.iter()
                        .copied()
                        .scan(0, |state, value| {
                            *state += value as i64;
                            Some(*state)
                        })
                        .collect::<Vec<_>>(),
                    tree.iter().map(|(_, _, m)| m).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn random_sum_int_set_10_insert_only() {
        random_sum_set_with_size(10, false);
    }

    #[test]
    fn random_sum_int_set_10_remove_all() {
        random_sum_set_with_size(10, true);
    }

    #[test]
    fn random_sum_int_set_20_insert_only() {
        random_sum_set_with_size(20, false);
    }

    #[test]
    fn random_sum_int_set_20_remove_all() {
        random_sum_set_with_size(20, true);
    }

    #[test]
    fn random_sum_int_set_150_insert_only() {
        random_sum_set_with_size(150, false);
    }

    #[test]
    fn random_sum_int_set_150_remove_all() {
        random_sum_set_with_size(150, true);
    }

    #[test]
    fn random_sum_int_set_many_500_insert_only() {
        for _ in 0..50 {
            random_sum_set_with_size(500, false);
        }
    }

    #[test]
    fn random_sum_int_set_many_500_remove_all() {
        for _ in 0..50 {
            random_sum_set_with_size(500, true);
        }
    }
}
