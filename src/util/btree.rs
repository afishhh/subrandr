use std::{
    alloc::Layout, borrow::Borrow, cmp::Ordering, fmt::Debug, marker::PhantomData, ptr::NonNull,
};

use crate::util::ArrayVec;

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
                let (result, maybe_new_root) = node.as_mut().remove(idx, None);
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

// FIXME: This is 8 bytes larger than two cachelines...
// const _: [(); 0 - !{ size_of::<Node>() <= 128 } as usize] = [];
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

// FIXME: Violates Stacked Borrows after an Equal split but I have no
//        idea why.
impl<T: NodeTraits> Node<T> {
    fn with_values(values: ArrayVec<CAP, (T::Key, T::Value)>) -> Self {
        Self {
            values,
            parent: None,
            index_in_parent: u8::MAX,
            children: [const { None }; CAP + 1],
            metadata: T::Metadata::default(),
        }
    }

    unsafe fn free(node: NonNull<Self>) {
        unsafe {
            for child in node.as_ref().children[..=node.as_ref().values.len()]
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
            assert_eq!(
                unsafe { child0.as_ref().parent.unwrap().as_ptr() as *const Self },
                (self as *const Self),
                "b-tree tree structure is inconsistent: incorrect parent pointer"
            );

            unsafe {
                child0.as_ref().dump(fmt, depth)?;
            }
        }

        let mut c = 1;
        for (key, value) in self.values.iter() {
            print_indent(fmt, depth)?;
            writeln!(fmt, "VALUE KEY={key:?} VALUE={value:?}")?;
            if let Some(nchild) = self.children[c] {
                assert_eq!(
                    unsafe { nchild.as_ref().parent.unwrap().as_ptr() as *const Self },
                    (self as *const Self),
                    "b-tree tree structure is inconsistent: incorrect parent pointer"
                );

                unsafe {
                    nchild.as_ref().dump(fmt, depth)?;
                }
            }
            c += 1;
        }

        Ok(())
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
            unsafe { NonNull::new_unchecked(self as *mut Self) }
        }
    }

    fn set_left_right(
        &mut self,
        insert_index: usize,
        new_children: Option<(NonNull<Self>, NonNull<Self>)>,
    ) {
        if let Some((mut left, mut right)) = new_children {
            self.children[insert_index] = Some(left);
            self.children[insert_index + 1] = Some(right);

            unsafe {
                left.as_mut().parent = Some(NonNull::new_unchecked(self as *mut _));
                left.as_mut().index_in_parent = insert_index as u8;
                right.as_mut().parent = Some(NonNull::new_unchecked(self as *mut _));
                right.as_mut().index_in_parent = (insert_index + 1) as u8;
            }
        }
    }

    fn recompute_statistics(&mut self) {
        unsafe {
            self.metadata = T::Metadata::default();

            for child in self.children[..=self.values.len()].iter().flatten() {
                T::combine_metadata(&mut self.metadata, &child.as_ref().metadata);
            }

            for (key, value) in self.values.iter() {
                T::combine_element_metadata(&mut self.metadata, key, value);
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
            if insert_index + 1 < self.values.len() {
                self.shift_children::<1>(insert_index + 1);
            }
            self.set_left_right(insert_index, new_children);
            self.recompute_statistics();

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
            let (mut left, mut right) = match dbg!(insert_index.cmp(&(CAP / 2))) {
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
                        for (i, mut child) in new_right.as_mut().children[..CAP - MEDIAN_IDX]
                            .iter()
                            .enumerate()
                            .filter_map(|(i, o)| o.map(|p| (i, p)))
                        {
                            child.as_mut().parent = Some(new_right);
                            child.as_mut().index_in_parent = i as u8;
                        }

                        new_right.as_mut().recompute_statistics();

                        self.set_left_right(insert_index, new_children);
                        self.values.set_len(MEDIAN_IDX);
                        self.values.insert(insert_index, (key, value));
                        self.recompute_statistics();

                        (NonNull::new_unchecked(self as *mut _), new_right)
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
                        for (i, mut child) in new_right.as_mut().children[..CAP + 1 - MEDIAN_IDX]
                            .iter()
                            .enumerate()
                            .filter_map(|(i, o)| o.map(|p| (i, p)))
                        {
                            child.as_mut().parent = Some(new_right);
                            child.as_mut().index_in_parent = i as u8;
                        }
                        self.values.set_len(MEDIAN_IDX);

                        // ???? seems to work
                        if let Some((mut left, mut right)) = new_children {
                            new_right.as_mut().children[0] = Some(right);
                            right.as_mut().parent = Some(new_right);
                            right.as_mut().index_in_parent = 0;
                            self.children[self.values.len()] = Some(left);
                            left.as_mut().parent = Some(NonNull::new_unchecked(self as *mut _));
                            left.as_mut().index_in_parent = self.values.len() as u8;
                            // FIXME: do these need recompute_statistics? probably
                        }

                        new_right.as_mut().recompute_statistics();
                        self.recompute_statistics();

                        (NonNull::new_unchecked(self as *mut _), new_right)
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
                        for (i, mut child) in new_right.as_mut().children[..CAP - MEDIAN_IDX]
                            .iter()
                            .enumerate()
                            .filter_map(|(i, o)| o.map(|p| (i, p)))
                        {
                            child.as_mut().parent = Some(new_right);
                            child.as_mut().index_in_parent = i as u8;
                        }
                        new_right
                            .as_mut()
                            .set_left_right(insert_index - (MEDIAN_IDX + 1), new_children);
                        new_right.as_mut().recompute_statistics();
                        self.values.set_len(MEDIAN_IDX);
                        self.recompute_statistics();

                        (NonNull::new_unchecked(self as *mut _), new_right)
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
                        left.as_mut().parent = Some(new_root);
                        left.as_mut().index_in_parent = 0;
                        right.as_mut().parent = Some(new_root);
                        right.as_mut().index_in_parent = 1;
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
                unsafe {
                    return Some((NonNull::new_unchecked(self as *mut Self), candidate_idx));
                }
            }
        }

        if let Some(mut child) = self.children[idx] {
            unsafe { child.as_mut().find_for_remove(key) }
        } else {
            None
        }
    }

    fn shift_children<const OFFSET: isize>(&mut self, start: usize) {
        let src = (start as isize - OFFSET) as usize;
        // TODO: check whether these ..= hurt performance (quite possible)
        for mut child in self.children[src..=self.values.len()]
            .iter()
            .flatten()
            .copied()
        {
            unsafe {
                let slot = &mut child.as_mut().index_in_parent;
                *slot = (*slot as isize + OFFSET) as u8;
            }
        }

        let end = if OFFSET > 0 {
            self.values.len() - OFFSET as usize + 1
        } else {
            self.values.len() + 1
        };
        self.children.copy_within(src..end, start);
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
                let p = parent.as_mut();
                self.values
                    .insert(0, std::ptr::read(&p.values[index_in_parent - 1]));
                self.shift_children::<1>(1);
                self.children[0] = sibling.as_ref().children[sibling.as_ref().values.len()];
                std::ptr::write(
                    &mut p.values[index_in_parent - 1],
                    sibling.as_mut().values.pop().unwrap(),
                );

                self.recompute_statistics();
                p.recompute_statistics();
                sibling.as_mut().recompute_statistics();

                None
            } else if let Some(mut sibling) =
                right_sibling.filter(|n| n.as_ref().values.len() > MIN_VALUES)
            {
                let p = parent.as_mut();
                self.values.push(std::mem::replace(
                    &mut p.values[index_in_parent],
                    sibling.as_mut().values.remove(0),
                ));
                self.children[self.values.len()] = sibling.as_ref().children[0];
                sibling.as_mut().shift_children::<-1>(0);

                self.recompute_statistics();
                p.recompute_statistics();
                sibling.as_mut().recompute_statistics();

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

                if right_index < parent.as_ref().values.len() {
                    parent.as_mut().shift_children::<-1>(right_index);
                }
                left.values
                    .push(parent.as_mut().values.remove(right_index - 1));

                let new_children_range =
                    left.values.len()..left.values.len() + 1 + right.values.len();
                left.children[new_children_range.clone()]
                    .copy_from_slice(&right.children[..right.values.len() + 1]);

                let left_ptr = NonNull::new_unchecked(left as *mut Self);
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

                dealloc_simple(NonNull::new_unchecked(right as *mut Self));

                if parent.as_mut().parent.is_none() {
                    if parent.as_ref().values.is_empty() {
                        left.parent = None;
                        return Some(NonNull::new_unchecked(left));
                    } else {
                        parent.as_mut().recompute_statistics();
                        return None;
                    }
                } else if parent.as_ref().values.len() < MIN_VALUES {
                    parent.as_mut().rebalance()
                } else {
                    None
                }
            }
        }
    }

    fn remove(
        &mut self,
        index: usize,
        slot: Option<(NonNull<Self>, usize)>,
    ) -> (Option<(T::Key, T::Value)>, Option<NonNull<Self>>) {
        if let Some((mut parent, parent_slot)) = slot {
            unsafe {
                parent.as_mut().values[parent_slot] = std::ptr::read(&self.values[index]);
            }
        }

        if let Some(mut left) = self.children[index] {
            unsafe {
                let left_idx = left.as_ref().values.len() - 1;
                left.as_mut().remove(
                    left_idx,
                    Some((NonNull::new_unchecked(self as *mut Self), index)),
                )
            }
        } else if let Some(mut right) = self.children[index + 1] {
            unsafe {
                right
                    .as_mut()
                    .remove(0, Some((NonNull::new_unchecked(self as *mut Self), index)))
            }
        } else {
            let result = self.values.remove(index);

            (
                Some(result),
                if self.parent.is_some() && self.values.len() < MIN_VALUES {
                    self.rebalance()
                } else {
                    self.recompute_statistics();
                    None
                },
            )
        }
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

#[test]
fn simple_int_set() {
    let mut tree = Tree::<SetTraits<i64>>::new();

    tree.insert(128, ());
    tree.insert(54, ());
    tree.insert(256, ());

    assert_eq!(tree.exclusive_upper_bound(&54), None);
    assert_eq!(tree.exclusive_upper_bound(&128), Some((&54, &(), ())));
    assert_eq!(tree.exclusive_upper_bound(&129), Some((&128, &(), ())));

    // Triggers a Greater split
    tree.insert(2048, ());
    tree.insert(-119, ());
    tree.insert(44, ());
    tree.insert(68, ());

    assert_eq!(tree.exclusive_upper_bound(&2049), Some((&2048, &(), ())));

    tree.insert(512, ());
    // Triggers an Equal split
    tree.insert(384, ());

    tree.insert(47, ());
    // Triggers a Less split
    tree.insert(46, ());

    assert!(tree.remove(&2048).is_some());
    assert!(tree.remove(&512).is_some());
    assert!(tree.remove(&256).is_some());
    assert!(tree.remove(&384).is_some());
}

#[test]
fn simple_int_sum_set() {
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

    let mut tree = Tree::<IntSumSetTraits>::new();

    tree.insert(128, ());
    tree.insert(54, ());
    tree.insert(256, ());

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

    assert_eq!(tree.exclusive_upper_bound(&54), None);
    assert_eq!(tree.inclusive_upper_bound(&128), Some((&128, &S_128, ())));
    assert_eq!(tree.inclusive_upper_bound(&129), Some((&128, &S_128, ())));
    assert_eq!(tree.exclusive_upper_bound(&128), Some((&54, &S_54, ())));
    assert_eq!(tree.exclusive_upper_bound(&129), Some((&128, &S_128, ())));

    tree.insert(2048, "");
    tree.insert(-119, S_M128);
    tree.insert(44, "54");
    tree.insert(68, "abc");

    assert_eq!(tree.exclusive_upper_bound(&i64::MIN), None);
    assert_eq!(tree.exclusive_upper_bound(&0), Some((&-119, &S_M128, ())));
    assert_eq!(tree.exclusive_upper_bound(&2049), Some((&2048, &"", ())));
    assert_eq!(tree.inclusive_upper_bound(&2049), Some((&2048, &"", ())));
    assert_eq!(tree.inclusive_upper_bound(&2048), Some((&2048, &"", ())));
}
