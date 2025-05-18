#[derive(Debug, Clone, PartialEq)]
enum QuadNode<N> {
    Empty,
    Leaf(N),
    Intermediate { children: Box<[QuadNode<N>; 4]> },
}

impl<N: Default> QuadNode<N> {
    fn visit_leaves(&self, quad: Quad, on_leaf: &mut impl FnMut(&N, Quad)) {
        match self {
            QuadNode::Empty => {}
            QuadNode::Leaf(ref value) => on_leaf(value, quad),
            QuadNode::Intermediate { ref children } => {
                for (child, quad) in children.iter().zip(quad.quadrants()) {
                    Self::visit_leaves(child, quad, on_leaf);
                }
            }
        }
    }
}

macro_rules! merge_equal_leaves {
    ($self: ident, $children: ident) => {
        if matches!($children[0], QuadNode::Leaf(..))
            && $children[1..].iter().all(|other| other == &$children[0])
        {
            let value = std::mem::replace($self, QuadNode::Empty);
            let mut children = match value {
                QuadNode::Intermediate { children } => children,
                _ => unreachable!(),
            };
            std::mem::swap($self, &mut children[0]);
        }
    };
}

impl<N: PartialEq + Default> QuadNode<N> {
    fn visit_leaves_mut(&mut self, on_leaf: &mut impl FnMut(&mut N)) {
        match self {
            QuadNode::Empty => {
                let mut value = N::default();
                on_leaf(&mut value);
                *self = QuadNode::Leaf(value)
            }
            QuadNode::Leaf(ref mut value) => on_leaf(value),
            QuadNode::Intermediate { ref mut children } => {
                for child in children.iter_mut() {
                    Self::visit_leaves_mut(child, on_leaf);
                }

                merge_equal_leaves!(self, children);
            }
        }
    }
}

impl<N: Clone + Default> QuadNode<N> {
    fn visit_leaves_in(&self, quad: Quad, target: Quad, on_leaf: &mut impl FnMut(&N, Quad)) {
        if quad.not_intersects(target) {
            return;
        }

        if quad.contained_in(target) {
            self.visit_leaves(quad, on_leaf);
        } else {
            match self {
                QuadNode::Empty => {}
                QuadNode::Leaf(value) => on_leaf(value, quad),
                QuadNode::Intermediate { children } => {
                    for (child, cquad) in children.iter().zip(quad.quadrants()) {
                        child.visit_leaves_in(cquad, target, on_leaf);
                    }
                }
            }
        }
    }
}

impl<N: std::fmt::Debug + PartialEq + Clone + Default> QuadNode<N> {
    fn visit_leaves_in_mut(&mut self, quad: Quad, tquad: Quad, on_leaf: &mut impl FnMut(&mut N)) {
        if quad.not_intersects(tquad) {
            return;
        }

        if quad.contained_in(tquad) {
            self.visit_leaves_mut(on_leaf);
        } else {
            let children = match self {
                QuadNode::Empty => {
                    *self = QuadNode::Intermediate {
                        children: Box::new([const { QuadNode::Empty }; 4]),
                    };
                    match self {
                        QuadNode::Intermediate { children } => &mut **children,
                        _ => unreachable!(),
                    }
                }
                QuadNode::Leaf(..) => {
                    let value = std::mem::replace(self, QuadNode::Empty);
                    *self = QuadNode::Intermediate {
                        children: Box::new([value.clone(), value.clone(), value.clone(), value]),
                    };
                    match self {
                        QuadNode::Intermediate { children } => &mut **children,
                        _ => unreachable!(),
                    }
                }
                QuadNode::Intermediate { children } => children,
            };

            for (child, quad) in children.iter_mut().zip(quad.quadrants()) {
                child.visit_leaves_in_mut(quad, tquad, on_leaf);
            }

            merge_equal_leaves!(self, children);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quad {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl Quad {
    pub fn new((x1, y1): (u32, u32), (x2, y2): (u32, u32)) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn not_intersects(self, other: Quad) -> bool {
        let Quad { x1, y1, x2, y2 } = self;
        let Quad {
            x1: tx1,
            y1: ty1,
            x2: tx2,
            y2: ty2,
        } = other;
        x2 <= tx1 || y2 <= ty1 || x1 >= tx2 || y1 >= ty2
    }

    fn contained_in(self, other: Quad) -> bool {
        self.x1 >= other.x1 && self.y1 >= other.y1 && self.x2 <= other.x2 && self.y2 <= other.y2
    }

    pub fn quadrants(self) -> [Self; 4] {
        let Quad { x1, y1, x2, y2 } = self;

        let mx = (x1 + x2) >> 1;
        let my = (y1 + y2) >> 1;

        [
            Self::new((x1, y1), (mx, my)),
            Self::new((x1, my), (mx, y2)),
            Self::new((mx, y1), (x2, my)),
            Self::new((mx, my), (x2, y2)),
        ]
    }
}

// TODO: This could store the nodes in a Vec but that requires
//       thinking about some complex borrowing stuff or actually
//       just doing everything by index but nevertheless it's a pain.
// TODO: Investiage R-trees and Z-order curves? Probably not worth it though.
pub struct QuadTree<N> {
    width: u32,
    height: u32,
    root: QuadNode<N>,
}

impl<N: std::fmt::Debug + PartialEq + Clone + Default> QuadTree<N> {
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            root: QuadNode::Empty,
        }
    }

    pub fn resize_and_clear(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.clear();
    }

    pub fn clear(&mut self) {
        self.root = QuadNode::Empty;
    }

    fn root_quad(&self) -> Quad {
        Quad::new((0, 0), (self.width, self.height))
    }

    pub fn insert_with(&mut self, quad: Quad, mut inserter: impl FnMut(&mut N)) {
        self.root
            .visit_leaves_in_mut(self.root_quad(), quad, &mut inserter);
    }

    pub fn set(&mut self, quad: Quad, value: N) {
        self.root
            .visit_leaves_in_mut(self.root_quad(), quad, &mut |v| {
                v.clone_from(&value);
            });
    }

    pub fn query(&self, target: Quad, mut intersector: impl FnMut(&N, Quad)) {
        self.root
            .visit_leaves_in(self.root_quad(), target, &mut intersector);
    }
}

#[cfg(test)]
mod test {
    use super::{Quad, QuadTree};

    fn query_all<V: std::fmt::Debug + Clone + Default + Ord>(
        tree: &QuadTree<V>,
        target: Quad,
    ) -> Vec<(Quad, V)> {
        let mut result = Vec::new();
        tree.query(target, |value, quad| {
            result.push((quad, value.clone()));
        });
        result.sort_unstable();
        result
    }

    #[test]
    fn bool_quad_tree() {
        let mut tree = QuadTree::<u32>::new(100, 100);
        tree.set(Quad::new((0, 0), (75, 50)), 2);
        tree.set(Quad::new((50, 0), (100, 100)), 3);

        let quads = query_all(&tree, Quad::new((0, 0), (100, 100)));
        #[rustfmt::skip]
        assert_eq!(
            quads,
            [
                (Quad { x1: 0, y1: 0, x2: 50, y2: 50, }, 2),
                (Quad { x1: 50, y1: 0, x2: 100, y2: 50, }, 3),
                (Quad { x1: 50, y1: 50, x2: 100, y2: 100, }, 3),
            ]
        );

        tree.set(Quad::new((20, 0), (60, 60)), 4);
        let quads = query_all(&tree, Quad::new((50, 50), (100, 100)));
        assert_eq!(quads.len(), 40)
    }
}
