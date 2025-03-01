use crate::util::btree::{self, NodeTraits};

use super::{Fixed2, Segment};

const B: usize = 2;
const MIN_VALUES: usize = B - 1;
const CAP: usize = 2 * B - 1;

#[derive(Debug, Clone, Copy)]
struct NodeValue {
    id: u32,
    winding: i32,
}

struct WindingNodeTraits;
impl NodeTraits for WindingNodeTraits {
    // winding count
    type Metadata = i32;
    type Key = Segment;
    type Value = NodeValue;

    fn combine_element_metadata(
        metadata: &mut Self::Metadata,
        _key: &Self::Key,
        value: &Self::Value,
    ) {
        *metadata += value.winding;
    }

    fn combine_metadata(metadata: &mut Self::Metadata, other: &Self::Metadata) {
        *metadata += *other;
    }
}

#[derive(Debug)]
pub struct WindingTree {
    tree: btree::Tree<WindingNodeTraits>,
}

impl WindingTree {
    pub const fn new() -> Self {
        Self {
            tree: btree::Tree::new(),
        }
    }

    pub fn add(&mut self, upper: Fixed2, lower: Fixed2, id: u32, winding: i32) {
        self.tree
            .insert(Segment { upper, lower }, NodeValue { id, winding });
    }

    pub fn before(&self, p: Fixed2) -> Option<i32> {
        self.tree
            .exclusive_upper_bound_by(|k| super::compare_segment_with_point(k, p))
            .map(|(_, _, m)| m)
    }

    pub fn before_inclusive(&self, p: Fixed2) -> Option<i32> {
        self.tree
            .inclusive_upper_bound_by(|k| super::compare_segment_with_point(k, p))
            .map(|(_, _, m)| m)
    }

    pub fn remove(&mut self, upper: Fixed2, lower: Fixed2) -> bool {
        self.tree.remove(&Segment { upper, lower }).is_some()
    }

    pub fn dump(&self) {
        self.tree.dump();
    }

    pub fn validate(&self) {
        self.tree.validate();
    }
}

// TODO: make smaller, the tree itself is already tested in the btree module
#[test]
fn test() {
    let mut tree = WindingTree::new();
    tree.add(
        Fixed2 {
            x: 200.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        0,
        1,
    );
    tree.add(
        Fixed2 {
            x: 500.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        1,
        1,
    );
    tree.add(
        Fixed2 {
            x: 600.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        2,
        1,
    );

    // Root greater split happens here
    tree.add(
        Fixed2 {
            x: 800.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        3,
        -1,
    );
    tree.add(
        Fixed2 {
            x: 900.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        4,
        1,
    );
    tree.add(
        Fixed2 {
            x: 100.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        5,
        -1,
    );

    tree.dump();
    tree.validate();

    tree.add(
        Fixed2 {
            x: 6900.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        6,
        1,
    );

    // Non-root greater split happens here
    tree.add(
        Fixed2 {
            x: 7000.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        7,
        -1,
    );

    // Non-root equal split happens here
    tree.add(
        Fixed2 {
            x: 6800.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        8,
        1,
    );

    tree.dump();
    tree.validate();

    tree.add(
        Fixed2 {
            x: 0.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        9,
        1,
    );
    // Non-root less split happens here
    tree.add(
        Fixed2 {
            x: 20.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
        10,
        1,
    );
    tree.dump();

    assert_eq!(
        tree.before(Fixed2 {
            x: 700.0.into(),
            y: 169.0.into()
        }),
        Some(4)
    );

    assert_eq!(
        tree.before(Fixed2 {
            x: 762.0.into(),
            y: 363.0.into()
        }),
        Some(3)
    );

    assert_eq!(
        tree.before(Fixed2 {
            x: 0.0.into(),
            y: 363.0.into()
        }),
        None
    );

    assert!(tree.remove(
        Fixed2 {
            x: 600.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
    ));

    tree.dump();
    tree.validate();

    assert!(tree.remove(
        Fixed2 {
            x: 7000.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
    ));

    assert!(tree.remove(
        Fixed2 {
            x: 20.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
    ));

    tree.dump();
    tree.validate();

    assert!(tree.remove(
        Fixed2 {
            x: 100.0.into(),
            y: 400.0.into(),
        },
        Fixed2 {
            x: 69.0.into(),
            y: 69.0.into(),
        },
    ));

    tree.dump();
    tree.validate();
}
