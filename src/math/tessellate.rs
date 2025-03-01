use std::{cell::Cell, cmp::Ordering, collections::BinaryHeap, fmt::Debug};

use crate::{
    color::{rotated_color_iterator, RotatedColorIterator, BGRA8},
    math::Vec2,
    rasterize::RenderTarget,
    util::btree,
    Rasterizer,
};

use super::{Fixed, Point2, Point2f};

type IFixed24Dot8 = Fixed<8, i32>;
type IFixed48Dot16 = Fixed<16, i64>;

fn widen(v: IFixed24Dot8) -> IFixed48Dot16 {
    IFixed48Dot16::from_raw((v.into_raw() as i64) << 8)
}

type Fixed2 = Point2<IFixed24Dot8>;

pub const TESS_DBG_H: f32 = 700.0;
pub const TESS_DBG_W: f32 = 700.0;

#[derive(Debug, Clone, Copy)]
struct Segment {
    upper: Fixed2,
    lower: Fixed2,
}

impl Segment {
    fn solve_x_for_y_numerator(&self, y: IFixed24Dot8, den: IFixed48Dot16) -> IFixed48Dot16 {
        widen(self.lower.x) * den + widen(y - self.lower.y) * widen(self.upper.x - self.lower.x)
    }

    fn solve_x_for_y_denominator(&self) -> IFixed48Dot16 {
        widen(self.upper.y - self.lower.y)
    }

    fn cmp_at_y(&self, y: IFixed24Dot8, x: IFixed24Dot8) -> Ordering {
        let den = self.solve_x_for_y_denominator();
        let num = self.solve_x_for_y_numerator(y, den);
        // dbg!(den, num.into_raw());
        // dbg!(num, den.into_raw());
        num.cmp(&(widen(x) * den))
    }

    fn cmp_upper(&self, other: &Segment) -> Ordering {
        if other.upper.y >= self.upper.y || self.lower.y == self.upper.y {
            if other.lower.y == other.upper.y {
                // eprintln!("cmp upper branch 1");
                self.upper.x.cmp(&other.upper.x)
            } else {
                // eprintln!("cmp upper branch 2");
                other.cmp_at_y(self.upper.y, self.upper.x).reverse()
            }
        } else {
            // eprintln!("cmp upper branch 3");
            self.cmp_at_y(other.upper.y, other.upper.x)
        }
    }

    fn cmp_lower(&self, other: &Segment) -> Ordering {
        if other.lower.y <= self.lower.y || self.lower.y == self.upper.y {
            if other.lower.y == other.upper.y {
                // eprintln!("cmp lower branch 1");
                self.lower.x.cmp(&other.lower.x)
            } else {
                // eprintln!("cmp lower branch 2");
                other.cmp_at_y(self.lower.y, self.lower.x).reverse()
            }
        } else {
            // eprintln!("cmp lower branch 3");
            self.cmp_at_y(other.lower.y, other.lower.x)
        }
    }
}

impl PartialEq for Segment {
    fn eq(&self, other: &Self) -> bool {
        self.upper == other.upper && self.lower == other.lower
    }
}

impl Eq for Segment {}

impl PartialOrd for Segment {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Segment {
    // FIXME: This comparison routine is probably slow as heck, any way to improve it?
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_upper(other)
            .then_with(|| self.cmp_lower(other))
            .then_with(|| {
                if self.lower == other.upper {
                    self.upper.x.cmp(&other.lower.x).reverse()
                } else if other.lower == self.upper {
                    self.lower.x.cmp(&other.upper.x).reverse()
                } else {
                    Ordering::Equal
                }
            })
    }
}

#[cfg(test)]
mod test {
    use std::cmp::Ordering;

    use super::{Point2f, Segment};

    macro_rules! test_compare_both_sided {
        ($a: expr, $b: expr, $ord: expr) => {{
            let (a, b) = ($a, $b);
            assert_eq!(a.cmp(&b), $ord);
            if $ord != Ordering::Equal {
                assert_eq!(b.cmp(&a), $ord.reverse());
            }
        }};
    }

    #[test]
    fn segment_compare_vertical_lines() {
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(0.0, 0.0).cast()
            },
            Segment {
                upper: Point2f::new(50.0, 100.0).cast(),
                lower: Point2f::new(50.0, 0.0).cast()
            },
            Ordering::Less
        );

        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 50.0).cast(),
                lower: Point2f::new(0.0, 0.0).cast()
            },
            Segment {
                upper: Point2f::new(50.0, 100.0).cast(),
                lower: Point2f::new(50.0, 0.0).cast()
            },
            Ordering::Less
        );

        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(0.0, 20.0).cast()
            },
            Segment {
                upper: Point2f::new(50.0, 50.0).cast(),
                lower: Point2f::new(50.0, 0.0).cast()
            },
            Ordering::Less
        );
    }

    #[test]
    fn segment_compare_1() {
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(150.0, -80.0).cast()
            },
            Segment {
                upper: Point2f::new(50.0, 95.0).cast(),
                lower: Point2f::new(100.0, 0.0).cast()
            },
            Ordering::Less
        );

        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(150.0, -80.0).cast()
            },
            Segment {
                upper: Point2f::new(0.0, 75.0).cast(),
                lower: Point2f::new(90.0, -20.0).cast()
            },
            Ordering::Greater
        );

        // TODO: Add some stuff with epsilon differences for precision testing
    }

    #[test]
    fn segment_compare_2() {
        // a < b
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(100.0, 500.0).cast(),
                lower: Point2f::new(100.0, 0.0).cast()
            },
            Segment {
                upper: Point2f::new(200.0, 300.0).cast(),
                lower: Point2f::new(100.0, 0.0).cast()
            },
            Ordering::Less
        );

        // b < c
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(200.0, 300.0).cast(),
                lower: Point2f::new(100.0, 0.0).cast()
            },
            Segment {
                upper: Point2f::new(300.0, 350.0).cast(),
                lower: Point2f::new(200.0, 300.0).cast()
            },
            Ordering::Less
        );

        // a < c
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(100.0, 500.0).cast(),
                lower: Point2f::new(100.0, 0.0).cast()
            },
            Segment {
                upper: Point2f::new(300.0, 350.0).cast(),
                lower: Point2f::new(200.0, 300.0).cast()
            },
            Ordering::Less
        );
    }

    #[test]
    fn segment_compare_horizontal() {
        // nearly horizontal
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(150.0, -80.0).cast()
            },
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(50.0, 99.0).cast()
            },
            Ordering::Less
        );

        // horizontal
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(150.0, -80.0).cast()
            },
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(50.0, 100.0).cast()
            },
            Ordering::Less
        );

        // horizontal on the right from the middle of the first segment
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(100.0, 0.0).cast()
            },
            Segment {
                upper: Point2f::new(50.0, 50.0).cast(),
                lower: Point2f::new(100.0, 50.0).cast()
            },
            Ordering::Less
        );

        // horizontal on the left from the middle of the first segment
        test_compare_both_sided!(
            Segment {
                upper: Point2f::new(0.0, 100.0).cast(),
                lower: Point2f::new(100.0, 0.0).cast()
            },
            Segment {
                upper: Point2f::new(50.0, 50.0).cast(),
                lower: Point2f::new(10.0, 50.0).cast()
            },
            Ordering::Greater
        );
    }
}

// fn point_left_right_det(a: Fixed2, b: Fixed2, p: Fixed2) -> IFixed48Dot16 {
//     let (ax, ay) = (widen(a.x), widen(a.y));
//     let (bx, by) = (widen(b.x), widen(b.y));
//     let (px, py) = (widen(p.x), widen(p.y));

//     ax * (by - py) + bx * (py - ay) + px * (ay - by)
// }

fn compare_segment_with_point(segment: &Segment, point: Fixed2) -> Ordering {
    if segment.upper.y == segment.lower.y {
        segment.lower.x.cmp(&point.x)
    } else {
        segment.cmp_at_y(point.y, point.x)
    }
    // point_left_right_det(segment.lower, segment.upper, point).cmp(&IFixed48Dot16::ZERO)
}

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq)]
enum PointOrdering {
    Above,
    Equal,
    Below,
}

fn lexicographic_compare(a: Fixed2, b: Fixed2) -> PointOrdering {
    match a.y.cmp(&b.y) {
        Ordering::Less => PointOrdering::Below,
        Ordering::Equal => match a.x.cmp(&b.x) {
            Ordering::Greater => PointOrdering::Below,
            Ordering::Equal => PointOrdering::Equal,
            Ordering::Less => PointOrdering::Above,
        },
        Ordering::Greater => PointOrdering::Above,
    }
}

#[derive(Debug, Clone, Copy)]
struct Helper {
    vertex: u32,
    is_merge_vertex: bool,
}

mod winding_tree;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeWinding {
    Positive = 1,
    Negative = -1,
}

impl EdgeWinding {
    pub fn inverse(self) -> EdgeWinding {
        unsafe { std::mem::transmute(-(self as i32)) }
    }

    pub fn invert_if(self, invert: bool) -> EdgeWinding {
        if invert {
            self.inverse()
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VertexEdgesKind {
    AllAbove,
    Regular,
    AllBelow,
}

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq)]
enum VertexClass {
    Start,
    End,
    Regular,
    Split,
    Merge,
}

impl VertexEdgesKind {
    fn classify(self, interior_on_the_left: bool) -> VertexClass {
        match self {
            VertexEdgesKind::AllBelow => {
                if interior_on_the_left {
                    VertexClass::Split
                } else {
                    VertexClass::Start
                }
            }
            VertexEdgesKind::AllAbove => {
                if interior_on_the_left {
                    VertexClass::Merge
                } else {
                    VertexClass::End
                }
            }
            VertexEdgesKind::Regular => VertexClass::Regular,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct VertexEdges {
    values: [(u32, EdgeWinding); 2],
    kind: VertexEdgesKind,
}

impl VertexEdges {
    fn new() -> Self {
        Self {
            values: [(u32::MAX, EdgeWinding::Negative); 2],
            kind: VertexEdgesKind::AllAbove,
        }
    }

    fn push(&mut self, value: (u32, EdgeWinding)) {
        if self.kind == VertexEdgesKind::AllAbove {
            self.values[0] = value;
            self.kind = VertexEdgesKind::Regular;
        } else {
            self.values[1] = value;
            self.kind = VertexEdgesKind::AllBelow;
        }
    }

    fn sort(&mut self, mid: usize, vertices: &[Fixed2]) {
        self.kind = match (
            lexicographic_compare(vertices[self.values[0].0 as usize], vertices[mid]),
            lexicographic_compare(vertices[self.values[1].0 as usize], vertices[mid]),
        ) {
            (PointOrdering::Below, PointOrdering::Below) => VertexEdgesKind::AllBelow,
            (PointOrdering::Above, PointOrdering::Above) => VertexEdgesKind::AllAbove,
            (PointOrdering::Below, PointOrdering::Above) => {
                self.values.reverse();
                VertexEdgesKind::Regular
            }
            (PointOrdering::Above, PointOrdering::Below) => VertexEdgesKind::Regular,
            (PointOrdering::Equal, _) | (_, PointOrdering::Equal) => {
                panic!("VertexEdges::sort: zero-length edge")
            }
        };

        match self.kind {
            VertexEdgesKind::AllAbove => {
                match self.cross(vertices[mid], vertices).cmp(&IFixed24Dot8::ZERO) {
                    Ordering::Less => (),
                    Ordering::Equal => {
                        self.values.sort_by_key(|&(id, _)| vertices[id as usize].x);
                    }
                    Ordering::Greater => self.values.reverse(),
                }
            }
            VertexEdgesKind::AllBelow => {
                match self.cross(vertices[mid], vertices).cmp(&IFixed24Dot8::ZERO) {
                    Ordering::Less => self.values.reverse(),
                    Ordering::Equal => {
                        self.values.sort_by_key(|&(id, _)| vertices[id as usize].x);
                    }
                    Ordering::Greater => (),
                }
            }
            VertexEdgesKind::Regular => {}
        }
    }

    fn finish(&mut self, mid: usize, vertices: &[Fixed2]) {
        assert_eq!(self.kind, VertexEdgesKind::AllBelow);

        self.sort(mid, vertices);
    }

    fn up(&self) -> &[(u32, EdgeWinding)] {
        match self.kind {
            VertexEdgesKind::AllAbove => &self.values[..],
            VertexEdgesKind::Regular => &self.values[..1],
            VertexEdgesKind::AllBelow => &self.values[..0],
        }
    }

    fn down(&self) -> &[(u32, EdgeWinding)] {
        match self.kind {
            VertexEdgesKind::AllAbove => &self.values[2..],
            VertexEdgesKind::Regular => &self.values[1..],
            VertexEdgesKind::AllBelow => &self.values[0..],
        }
    }

    fn rightmost(&self, vertices: &[Fixed2]) -> &(u32, EdgeWinding) {
        let a = vertices[self.values[0].0 as usize];
        let b = vertices[self.values[1].0 as usize];
        if a.x > b.x {
            &self.values[0]
        } else {
            &self.values[1]
        }
    }

    fn leftmost(&self, vertices: &[Fixed2]) -> &(u32, EdgeWinding) {
        let a = vertices[self.values[0].0 as usize];
        let b = vertices[self.values[1].0 as usize];
        if a.x < b.x {
            &self.values[0]
        } else {
            &self.values[1]
        }
    }

    fn cross(&self, mid: Fixed2, vertices: &[Fixed2]) -> IFixed24Dot8 {
        let a = vertices[self.values[0].0 as usize];
        let b = vertices[self.values[1].0 as usize];
        let av = a - mid;
        let bv = b - mid;
        av.cross(bv)
    }

    fn replace(&mut self, previous: u32, new: u32) {
        let mut found = false;
        for (v, _) in self.values.iter_mut() {
            if *v == previous {
                *v = new;
                found = true;
            }
        }
        debug_assert!(found);
    }

    fn as_array(&self) -> &[(u32, EdgeWinding); 2] {
        &self.values
    }
}

#[derive(Debug, Default)]
struct ResultEdge {
    target: u32,
    left_poly: bool,
    right_poly: bool,
}

struct MonotoneTessellator {
    xd: RotatedColorIterator,

    prev: (Fixed2, bool),
    triangulation_stack: Vec<(Fixed2, bool)>,
    out_triangles: Vec<(Fixed2, Fixed2, Fixed2)>,
}

impl MonotoneTessellator {
    const fn new() -> Self {
        Self {
            xd: rotated_color_iterator(BGRA8::new(255, 0, 0, 255), 0.4),

            prev: (Fixed2::ZERO, false),
            triangulation_stack: Vec::new(),
            out_triangles: Vec::new(),
        }
    }

    fn start(&mut self, vertex: Fixed2, right: bool) {
        if PRINT_MONOTONE_CALLS {
            eprintln!("monotone.start({vertex:?}, {right})");
        }

        self.triangulation_stack.clear();
        self.triangulation_stack.push((vertex, right));
        self.prev = (vertex, right);
    }

    fn vertex(&mut self, vertex: Fixed2, right: bool) {
        if PRINT_MONOTONE_CALLS {
            eprintln!("monotone.vertex({vertex:?}, {right})");
        }

        if self.triangulation_stack.len() == 1 {
            self.triangulation_stack.push((vertex, right));
            self.prev = (vertex, right);
            return;
        }

        assert!(self.triangulation_stack.len() >= 2);

        if right != self.triangulation_stack.last().unwrap().1 {
            let mut last = self.triangulation_stack[0];
            for &vert in self.triangulation_stack[1..].iter() {
                self.out_triangles.push((last.0, vert.0, vertex));

                last = vert;
            }

            self.triangulation_stack.clear();
            self.triangulation_stack
                .extend([self.prev, (vertex, right)]);
        } else {
            let mut last = self.triangulation_stack.pop().unwrap();
            while let Some(&next) = self.triangulation_stack.last() {
                let angle = (next.0 - vertex).cross(last.0 - vertex);

                if (right && angle < 0) || (!right && angle > 0) {
                    break;
                }

                self.triangulation_stack.pop();

                self.out_triangles.push((last.0, next.0, vertex));

                last = next;
            }

            self.triangulation_stack.extend([last, (vertex, right)]);
        }

        self.prev = (vertex, right);
    }

    fn end(&mut self, max: Fixed2) {
        if PRINT_MONOTONE_CALLS {
            eprintln!("monotone.end({max:?})");
        }

        for w in self.triangulation_stack.windows(2) {
            let &[last, current] = w else { unreachable!() };

            self.out_triangles.push((last.0, current.0, max))
        }
    }
}

#[test]
fn monotone_tesselation_triangle() {
    let mut tess = MonotoneTessellator::new();

    let start = Point2f::new(400.0, 350.0).cast();
    let mid = Point2f::new(150.0, 450.0).cast();
    let end = Point2f::new(345.0, 455.0).cast();
    tess.start(start, false);
    tess.vertex(mid, false);
    tess.end(end);

    assert_eq!(&tess.out_triangles[..], &[(start, mid, end)])
}

#[test]
fn monotone_tesselation_1() {
    let mut tess = MonotoneTessellator::new();

    tess.start(Point2f::new(600.0, 200.0).cast(), false);
    tess.vertex(Point2f::new(100.0, 200.0).cast(), false);
    tess.vertex(Point2f::new(400.0, 400.0).cast(), false);
    tess.vertex(Point2f::new(300.0, 500.0).cast(), false);
    tess.vertex(Point2f::new(100.0, 600.0).cast(), false);
    tess.vertex(Point2f::new(600.0, 700.0).cast(), true);
    tess.end(Point2f::new(300.0, 700.0).cast());

    assert_eq!(tess.out_triangles.len(), 5);
}

#[test]
fn monotone_tesselation_2() {
    let mut tess = MonotoneTessellator::new();

    tess.start(Point2f::new(700.0, 200.0).cast(), false);
    tess.vertex(Point2f::new(100.0, 300.0).cast(), false);
    tess.vertex(Point2f::new(300.0, 400.0).cast(), true);
    tess.vertex(Point2f::new(600.0, 500.0).cast(), true);
    tess.vertex(Point2f::new(480.0, 530.0).cast(), true);
    tess.end(Point2f::new(300.0, 700.0).cast());

    assert_eq!(tess.out_triangles.len(), 4);
}

#[test]
fn monotone_tesselation_3() {
    let mut tess = MonotoneTessellator::new();

    tess.start(Point2f::new(700.0, 100.0).cast(), false);
    tess.vertex(Point2f::new(100.0, 100.0).cast(), false);
    tess.vertex(Point2f::new(400.0, 300.0).cast(), false);
    tess.vertex(Point2f::new(350.0, 500.0).cast(), false);
    tess.vertex(Point2f::new(600.0, 700.0).cast(), true);
    tess.end(Point2f::new(300.0, 700.0).cast());

    assert_eq!(tess.out_triangles.len(), 4);
}

#[derive(Debug)]
struct Queued {
    point: Fixed2,
    id: u32,
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.point == other.point
    }
}

impl Eq for Queued {}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        self.point
            .y
            .cmp(&other.point.y)
            .then_with(|| self.point.x.cmp(&other.point.x).reverse())
    }
}

pub struct Tessellator {
    vertices: Vec<Fixed2>,

    queue: BinaryHeap<Queued>,
    edges: Vec<VertexEdges>,

    real: Vec<bool>,
    winding_tree: winding_tree::WindingTree,
    // TODO: this will run into edge cases where it shouldn't actually remove the edge
    //       i.e. when two edges are colinear
    //       maybe edge id should be part of the key?
    helper_edge_tree: btree::Tree<btree::MapTraits<Segment, Cell<Helper>>>,

    monotone_tessellator: MonotoneTessellator,
}

const PRINT_VERTEX_INFO_ON_EVENT: bool = true;
const PRINT_MONOTONE_CALLS: bool = false;
const DUMP_TREES_ON_EVENT: bool = true;
const DRAW_EDGES_AND_VERTEX_INFO: bool = true;
const DRAW_MONOTONE_CHAINS: bool = true;

impl Tessellator {
    pub fn new() -> Tessellator {
        Self {
            vertices: Vec::new(),
            queue: BinaryHeap::new(),
            edges: Vec::new(),

            real: Vec::new(),
            winding_tree: winding_tree::WindingTree::new(),
            helper_edge_tree: btree::Tree::new(),

            monotone_tessellator: MonotoneTessellator::new(),
        }
    }

    pub fn add_polygon(
        &mut self,
        points: &[Point2f],
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        invert: bool,
    ) {
        if points.len() < 3 {
            panic!("Tessellator::add_polygon called with less than three points");
        }

        if let Some(mut prev) = points.last().copied().map(Point2::cast) {
            let offset = self.vertices.len();
            let mut pi = offset + points.len() - 1;
            let mut i = offset;

            self.vertices.reserve(points.len());
            self.edges
                .resize_with(self.edges.len() + points.len(), VertexEdges::new);

            for next in points.iter().copied().map(Point2::cast) {
                match lexicographic_compare(prev, next) {
                    PointOrdering::Above => {
                        let winding = EdgeWinding::Negative.invert_if(invert);
                        self.edges[i].push((pi as u32, winding));
                        self.edges[pi].push((i as u32, winding));
                        winding
                    }
                    PointOrdering::Equal => continue,
                    PointOrdering::Below => {
                        let winding = EdgeWinding::Positive.invert_if(invert);
                        self.edges[pi].push((i as u32, winding));
                        self.edges[i].push((pi as u32, winding));
                        winding
                    }
                };

                if DRAW_EDGES_AND_VERTEX_INFO {
                    rasterizer.line(
                        target,
                        Point2::new(prev.x.into_f32(), TESS_DBG_H - prev.y.into_f32()),
                        Point2::new(next.x.into_f32(), TESS_DBG_H - next.y.into_f32()),
                        BGRA8::new(255, 0, 0, 255),
                    );

                    let start = prev.cast::<f32>();
                    let end = next.cast::<f32>();

                    let middle = start + ((end - start) / 2.0);
                    let dir = (end - start).normalize();
                    let deriv = dir.normal();
                    const ARROW_SCALE: f32 = 8.0;

                    let f = if invert { -1.0 } else { 1.0 };
                    let top = middle + dir * f * ARROW_SCALE;
                    let left = middle - deriv * f * ARROW_SCALE;
                    let right = middle + deriv * f * ARROW_SCALE;

                    rasterizer.fill_triangle(
                        target,
                        &[
                            Point2::new(top.x, TESS_DBG_H - top.y),
                            Point2::new(left.x, TESS_DBG_H - left.y),
                            Point2::new(right.x, TESS_DBG_H - right.y),
                        ],
                        BGRA8::new(255, 0, 0, 255),
                    );
                }

                self.queue.push(Queued {
                    point: next,
                    id: self.vertices.len() as u32,
                });
                self.vertices.push(next);
                prev = next;
                pi = i;
                i += 1;
            }

            if i - offset < 3 {
                self.vertices.truncate(i);
                self.edges.truncate(i);

                // TODO: Instead emit a tiny triangle here?
                panic!("polygon too small")
            }

            // FIXME: this is kind of untested
            let full_last = offset + points.len() - 1;
            if i - 1 < full_last {
                self.edges[i - 1] = self.edges[full_last];
                self.edges.truncate(i);
                self.edges[offset].replace(full_last as u32, (i - 1) as u32);
                self.edges[i - 2].replace(full_last as u32, (i - 1) as u32);
            }

            for (i, edges) in self.edges.iter_mut().enumerate().skip(offset) {
                edges.finish(i, &self.vertices);
            }
        }
    }

    fn paint_chain_from_end(
        &mut self,
        mut prev: u32,
        mut current: u32,
        right: bool,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
    ) {
        loop {
            {
                let prev = self.vertices[prev as usize];
                let next = self.vertices[current as usize];
                rasterizer.line(
                    target,
                    Point2::new(prev.x.into_f32(), 2. * TESS_DBG_H - prev.y.into_f32()),
                    Point2::new(next.x.into_f32(), 2. * TESS_DBG_H - next.y.into_f32()),
                    if right {
                        BGRA8::new(255, 0, 0, 255)
                    } else {
                        BGRA8::new(0, 255, 0, 255)
                    },
                )
            }

            if let Some(next) = self.step_up(current, right) {
                if current == next {
                    break;
                }
                prev = current;
                current = next;
            } else {
                break;
            }
        }
    }

    fn step_up(&self, current: u32, right: bool) -> Option<u32> {
        let edges = self.edges[current as usize];
        match edges.kind {
            VertexEdgesKind::AllBelow => None,
            VertexEdgesKind::AllAbove => Some(edges.values[(!right) as usize].0),
            VertexEdgesKind::Regular => Some(edges.up()[0].0),
        }
    }

    fn walk_polygon_from_end(
        &mut self,
        end: u32,
        left_up: u32,
        right_up: u32,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
    ) {
        if DRAW_MONOTONE_CHAINS {
            self.paint_chain_from_end(end, left_up, false, rasterizer, target);
            self.paint_chain_from_end(end, right_up, true, rasterizer, target);
        }

        self.monotone_tessellator
            .start(self.vertices[end as usize], false);

        let mut current_left = left_up;
        let mut current_right = right_up;
        while current_left != current_right {
            let left = self.vertices[current_left as usize];
            let right = self.vertices[current_right as usize];
            if left.y < right.y || (left.y == right.y && left.x > right.x) {
                self.monotone_tessellator
                    .vertex(self.vertices[current_left as usize], false);
                current_left = match self.step_up(current_left, false) {
                    Some(next) => next,
                    None => {
                        // TODO: Document this case
                        //       (something went wrong, should only happen on intersection rounding error)
                        return;
                    }
                };
            } else {
                self.monotone_tessellator
                    .vertex(self.vertices[current_right as usize], true);
                current_right = match self.step_up(current_right, true) {
                    Some(next) => next,
                    None => {
                        return;
                    }
                };
            }
        }

        self.monotone_tessellator
            .end(self.vertices[current_left as usize]);
    }

    fn insert_non_split_diagonal(
        &mut self,
        class: VertexClass,
        lower: u32,
        upper: u32,
        interior_on_the_left: bool,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
    ) {
        match class {
            VertexClass::End => {
                // This is an end vertex so it has two upwards edges like this:
                // \       /
                //  \     /
                //   \   /
                //    \ /
                //     x
                //
                // And we're adding a diagonal somewhere in the middle like this:
                // \     y  /
                //  \    | /
                //   \  / /
                //    \| /
                //     x
                //
                // This means we can immediately get two output monotone polygons from this.
                self.walk_polygon_from_end(
                    lower,
                    self.edges[lower as usize].values[0].0,
                    upper,
                    rasterizer,
                    target,
                );
                self.walk_polygon_from_end(
                    lower,
                    upper,
                    self.edges[lower as usize].values[1].0,
                    rasterizer,
                    target,
                );
            }
            VertexClass::Regular => {
                // This is a regular vertex so it has one upwards and one downwards edge like this:
                //  \ PPPPPPPPPP
                //   \ PPPPPPPPP
                //    \ PPPPPPPP
                //     x PPPPPPP
                //      \ PPPPPP
                //       \ PPPPP
                //        \ PPPP
                //
                // or this:
                // PPPPPPPPPP /
                // PPPPPPPPP /
                // PPPPPPPP /
                // PPPPPPP x
                // PPPPPP /
                // PPPPP /
                // PPPP /
                //
                // (where P is the polygon's interior)
                //
                // Both these cases will result in one output monotone polygon and
                // require us to rewrite our polygon to replace this cut off polygon
                // with a diagonal edge instead.
                // Since they may result in polygons on different sides they have to
                // be handled separately.
                let edges = self.edges[lower as usize];
                let up = edges.values[0].0;
                if !interior_on_the_left {
                    // Case 1
                    self.walk_polygon_from_end(lower, up, upper, rasterizer, target);
                    self.edges[lower as usize].values[0] = (upper, EdgeWinding::Positive);
                    self.edges[upper as usize].values = [
                        (lower, EdgeWinding::Positive),
                        // TODO: should this work by angle instead?
                        *self.edges[upper as usize].rightmost(&self.vertices),
                    ];
                } else {
                    // Case 2
                    self.walk_polygon_from_end(lower, upper, up, rasterizer, target);
                    self.edges[lower as usize].values[0] = (upper, EdgeWinding::Positive);
                    self.edges[upper as usize].values = [
                        // TODO: should this work by angle instead?
                        *self.edges[upper as usize].leftmost(&self.vertices),
                        (lower, EdgeWinding::Positive),
                    ];
                }
                self.edges[upper as usize].sort(upper as usize, &self.vertices);
            }
            VertexClass::Merge => {
                // This is a merge vertex so it has two upwards edges like this:
                // \       /
                //  \     /
                //   \   /
                //    \ /
                //     x
                let lower_x = self.vertices[lower as usize].x;
                let upper_x = self.vertices[upper as usize].x;
                let left = self.edges[lower as usize].values[0].0;
                let right = self.edges[lower as usize].values[1].0;
                // TODO: this can actually be
                //        / /
                //      y |/
                //       //
                //      |/
                //      x
                // I think
                if upper_x <= lower_x {
                    self.walk_polygon_from_end(lower, upper, left, rasterizer, target);
                    self.edges[lower as usize].values[0] = (upper, EdgeWinding::Positive);
                    self.edges[upper as usize].values = [
                        (lower, EdgeWinding::Positive),
                        *self.edges[upper as usize].leftmost(&self.vertices),
                    ];
                } else {
                    self.walk_polygon_from_end(lower, right, upper, rasterizer, target);
                    self.edges[lower as usize].values[1] = (upper, EdgeWinding::Positive);
                    self.edges[upper as usize].values = [
                        (lower, EdgeWinding::Positive),
                        *self.edges[upper as usize].rightmost(&self.vertices),
                    ];
                }
                self.edges[lower as usize].sort(lower as usize, &self.vertices);
                self.edges[upper as usize].sort(upper as usize, &self.vertices);
            }
            _ => unreachable!(),
        }
    }

    fn subdivide(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        dbg_font: Option<&crate::text::Font>,
    ) {
        macro_rules! insert_diagonal {
            ($a: expr, $b: expr) => {
                if DRAW_EDGES_AND_VERTEX_INFO {
                    let (a, b) = ($a, $b);
                    {
                        let prev = self.vertices[a as usize];
                        let next = self.vertices[b as usize];
                        rasterizer.line(
                            target,
                            Point2::new(prev.x.into_f32(), TESS_DBG_H - prev.y.into_f32()),
                            Point2::new(next.x.into_f32(), TESS_DBG_H - next.y.into_f32()),
                            BGRA8::new(0, 255, 0, 255),
                        )
                    }
                }
            };
        }

        self.real.clear();
        self.real.resize(self.vertices.len(), false);

        while let Some(Queued { point: _, id: next }) = self.queue.pop() {
            let winding_count = self
                .winding_tree
                .before(self.vertices[next as usize])
                .unwrap_or(0);
            let edges = self.edges[next as usize];
            let interior_on_the_left = winding_count != 0;
            let class = edges.kind.classify(interior_on_the_left);

            if DUMP_TREES_ON_EVENT {
                self.winding_tree.dump();
                self.winding_tree.validate();
            }

            let mut insert_winding_count = winding_count;
            let mut is_materialized_vertex = self.real[next as usize];
            for down_v in self.edges[next as usize].down() {
                let materialized_edge =
                    (insert_winding_count + down_v.1 as i32) == 0 || insert_winding_count == 0;
                if materialized_edge {
                    self.helper_edge_tree.insert(
                        Segment {
                            upper: self.vertices[next as usize],
                            lower: self.vertices[down_v.0 as usize],
                        },
                        Cell::new(Helper {
                            vertex: next,
                            is_merge_vertex: false,
                        }),
                    );
                }
                self.real[down_v.0 as usize] = true;
                is_materialized_vertex |= true;

                self.winding_tree.add(
                    self.vertices[next as usize],
                    self.vertices[down_v.0 as usize],
                    next,
                    down_v.1 as i32,
                );
                insert_winding_count += down_v.1 as i32;
            }
            self.real[next as usize] |= is_materialized_vertex;

            for up_v in self.edges[next as usize].up() {
                self.winding_tree
                    .remove(self.vertices[up_v.0 as usize], self.vertices[next as usize]);
            }

            if class != VertexClass::Regular || interior_on_the_left {
                let mut j = 0;
                while let Some(&up) = self.edges[next as usize].up().get(j) {
                    j += 1;

                    if let Some(helper) = self
                        .helper_edge_tree
                        .remove(&Segment {
                            upper: self.vertices[up.0 as usize],
                            lower: self.vertices[next as usize],
                        })
                        .map(|(_, helper)| helper.get())
                    {
                        if helper.is_merge_vertex && up.0 != helper.vertex {
                            debug_assert_ne!(class, VertexClass::Split);
                            // TODO: unmess this
                            match class {
                                VertexClass::Regular | VertexClass::End | VertexClass::Merge => {
                                    insert_diagonal!(next, helper.vertex);
                                    self.insert_non_split_diagonal(
                                        class,
                                        next,
                                        helper.vertex,
                                        interior_on_the_left,
                                        rasterizer,
                                        target,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // maybe this?
            // let before = interior_on_the_left
            //     .then(|| {
            //         self.material_edge_tree.exclusive_upper_bound_by(|k| {
            //             compare_segment_with_point(k, self.vertices[next as usize])
            //         })
            //     })
            //     .flatten();
            let before = self.helper_edge_tree.exclusive_upper_bound_by(|k| {
                compare_segment_with_point(k, self.vertices[next as usize])
            });

            if DUMP_TREES_ON_EVENT {
                self.helper_edge_tree.dump();
                self.helper_edge_tree.validate();
            }

            if PRINT_VERTEX_INFO_ON_EVENT {
                eprintln!(
                    "processing vertex {next} {:?}",
                    self.vertices[next as usize]
                );
                eprintln!("  winding {winding_count}");
                eprintln!("  {:?}", self.edges[next as usize]);
                eprintln!("  {:?}", class);
                eprintln!("  interior_on_the_left {:?}", interior_on_the_left);
                eprintln!("  materialized {:?}", is_materialized_vertex);
                eprintln!("  before {:?}", before.map(|x| x.1));
            }

            if is_materialized_vertex {
                match class {
                    VertexClass::Start => {}
                    VertexClass::End => {
                        let up = self.edges[next as usize].up()[0];
                        if let Some(helper) = self
                            .helper_edge_tree
                            .get(&Segment {
                                upper: self.vertices[up.0 as usize],
                                lower: self.vertices[next as usize],
                            })
                            .map(|(_, helper, _)| helper.get())
                            .filter(|helper| helper.is_merge_vertex)
                        {
                            self.insert_non_split_diagonal(
                                VertexClass::End,
                                next,
                                helper.vertex,
                                interior_on_the_left,
                                rasterizer,
                                target,
                            );
                            insert_diagonal!(next, helper.vertex);
                        } else {
                            self.walk_polygon_from_end(
                                next,
                                self.edges[next as usize].values[0].0,
                                self.edges[next as usize].values[1].0,
                                rasterizer,
                                target,
                            );
                        }
                    }
                    VertexClass::Regular => {
                        if !interior_on_the_left {
                            if let Some(helper) = self
                                .helper_edge_tree
                                .remove(&Segment {
                                    upper: self.vertices
                                        [self.edges[next as usize].up()[0].0 as usize],
                                    lower: self.vertices[next as usize],
                                })
                                .map(|(_, helper)| helper.get())
                            {
                                if helper.is_merge_vertex {
                                    insert_diagonal!(next, helper.vertex);
                                    self.insert_non_split_diagonal(
                                        VertexClass::Regular,
                                        next,
                                        helper.vertex,
                                        interior_on_the_left,
                                        rasterizer,
                                        target,
                                    );
                                }
                            }
                        } else if let Some((_, helper, _)) = before {
                            let old = helper.replace(Helper {
                                vertex: next,
                                is_merge_vertex: false,
                            });
                            if old.is_merge_vertex {
                                insert_diagonal!(next, old.vertex);
                                self.insert_non_split_diagonal(
                                    VertexClass::Regular,
                                    next,
                                    old.vertex,
                                    interior_on_the_left,
                                    rasterizer,
                                    target,
                                );
                            }
                        }
                    }
                    VertexClass::Split => {
                        let (_, helper, _) = before.unwrap();
                        let old = helper.replace(Helper {
                            vertex: next,
                            is_merge_vertex: false,
                        });

                        // This is a split vertex so it has two downwards edges like this:
                        //     x
                        //    / \
                        //   /   \
                        //  /     \
                        // /       \
                        //
                        // And we're adding a diagonal somewhere above like this:
                        //         y
                        //        /
                        //       /
                        //      /
                        //     x
                        //    / \
                        //
                        // We do this by linking the lower vertex to the upper vertex and then
                        // choosing the correct direction when walking up the resulting
                        // monotone polygon.

                        self.edges[next as usize].values[0] = (old.vertex, EdgeWinding::Positive);
                        self.edges[next as usize].kind = VertexEdgesKind::Regular;

                        insert_diagonal!(next, old.vertex);
                    }
                    VertexClass::Merge => {
                        if let Some((_, helper, _)) = before {
                            let old = helper.replace(Helper {
                                vertex: next,
                                is_merge_vertex: true,
                            });
                            if old.vertex != u32::MAX
                                && old.is_merge_vertex
                                && self.edges[next as usize].up()[0].0 != old.vertex
                            {
                                insert_diagonal!(next, old.vertex);
                                self.insert_non_split_diagonal(
                                    VertexClass::Merge,
                                    next,
                                    old.vertex,
                                    interior_on_the_left,
                                    rasterizer,
                                    target,
                                );
                            }
                        }
                    }
                }
            }

            if DRAW_EDGES_AND_VERTEX_INFO {
                const SIZE: IFixed24Dot8 = IFixed24Dot8::from_f32(7.5);
                let pos = self.vertices[next as usize];
                match class {
                    VertexClass::Start => {
                        let top_right = pos + Vec2::new(SIZE, SIZE);
                        let top_left = pos + Vec2::new(-SIZE, SIZE);
                        let bottom_right = pos + Vec2::new(SIZE, -SIZE);
                        let bottom_left = pos + Vec2::new(-SIZE, -SIZE);

                        let mut last = bottom_left;
                        for point in [bottom_right, top_right, top_left, bottom_left] {
                            rasterizer.line(
                                target,
                                Point2::new(last.x.into_f32(), TESS_DBG_H - last.y.into_f32()),
                                Point2::new(point.x.into_f32(), TESS_DBG_H - point.y.into_f32()),
                                BGRA8::new(0, 0, 255, 255),
                            );
                            last = point;
                        }
                    }
                    VertexClass::End => {
                        let top_right = pos + Vec2::new(SIZE, SIZE);
                        let top_left = pos + Vec2::new(-SIZE, SIZE);
                        let bottom_right = pos + Vec2::new(SIZE, -SIZE);
                        let bottom_left = pos + Vec2::new(-SIZE, -SIZE);

                        rasterizer.fill_triangle(
                            target,
                            &[
                                Point2::new(
                                    top_left.x.into_f32(),
                                    TESS_DBG_H - top_left.y.into_f32(),
                                ),
                                Point2::new(
                                    top_right.x.into_f32(),
                                    TESS_DBG_H - top_right.y.into_f32(),
                                ),
                                Point2::new(
                                    bottom_left.x.into_f32(),
                                    TESS_DBG_H - bottom_left.y.into_f32(),
                                ),
                            ],
                            BGRA8::new(0, 0, 255, 255),
                        );
                        rasterizer.fill_triangle(
                            target,
                            &[
                                Point2::new(
                                    top_right.x.into_f32(),
                                    TESS_DBG_H - top_right.y.into_f32(),
                                ),
                                Point2::new(
                                    bottom_left.x.into_f32(),
                                    TESS_DBG_H - bottom_left.y.into_f32(),
                                ),
                                Point2::new(
                                    bottom_right.x.into_f32(),
                                    TESS_DBG_H - bottom_right.y.into_f32(),
                                ),
                            ],
                            BGRA8::new(0, 0, 255, 255),
                        );
                    }
                    VertexClass::Regular => {}
                    VertexClass::Split => {
                        let top = pos + Vec2::new(IFixed24Dot8::ZERO, SIZE);
                        let bottom_right = pos + Vec2::new(SIZE, -SIZE);
                        let bottom_left = pos + Vec2::new(-SIZE, -SIZE);

                        rasterizer.fill_triangle(
                            target,
                            &[
                                Point2::new(top.x.into_f32(), TESS_DBG_H - top.y.into_f32()),
                                Point2::new(
                                    bottom_right.x.into_f32(),
                                    TESS_DBG_H - bottom_right.y.into_f32(),
                                ),
                                Point2::new(
                                    bottom_left.x.into_f32(),
                                    TESS_DBG_H - bottom_left.y.into_f32(),
                                ),
                            ],
                            BGRA8::new(0, 0, 255, 255),
                        );
                    }
                    VertexClass::Merge => {
                        let top_right = pos + Vec2::new(SIZE, SIZE);
                        let top_left = pos + Vec2::new(-SIZE, SIZE);
                        let bottom = pos + Vec2::new(IFixed24Dot8::ZERO, -SIZE);

                        rasterizer.fill_triangle(
                            target,
                            &[
                                Point2::new(bottom.x.into_f32(), TESS_DBG_H - bottom.y.into_f32()),
                                Point2::new(
                                    top_right.x.into_f32(),
                                    TESS_DBG_H - top_right.y.into_f32(),
                                ),
                                Point2::new(
                                    top_left.x.into_f32(),
                                    TESS_DBG_H - top_left.y.into_f32(),
                                ),
                            ],
                            BGRA8::new(0, 0, 255, 255),
                        );
                    }
                }

                if let Some(font) = dbg_font {
                    let v = self.vertices[next as usize];
                    painter.debug_text(
                        v.x.round_to_inner(),
                        TESS_DBG_H as i32 - v.y.round_to_inner(),
                        &format!("{next}"),
                        crate::Alignment::Center,
                        BGRA8::new(255, 255, 255, 255),
                        font,
                    );
                }
            }
        }

        // Edge list debugging
        for (a, adj) in self.edges.iter().enumerate() {
            let mut right = false;
            for &(b, _) in adj.up() {
                let prev = self.vertices[a];
                let next = self.vertices[b as usize];
                let c = if right {
                    BGRA8::new(255, 0, 0, 255)
                } else {
                    BGRA8::new(0, 0, 255, 255)
                };

                rasterizer.line(
                    target,
                    Point2::new(
                        TESS_DBG_W + prev.x.into_f32(),
                        TESS_DBG_H - prev.y.into_f32(),
                    ),
                    Point2::new(
                        TESS_DBG_W + next.x.into_f32(),
                        TESS_DBG_H - next.y.into_f32(),
                    ),
                    c,
                );
                right = true;
            }
        }

        for (a, adj) in self.edges.iter().enumerate() {
            for &(b, _) in adj.down() {
                let prev = self.vertices[a];
                let next = self.vertices[b as usize];

                rasterizer.line(
                    target,
                    Point2::new(
                        TESS_DBG_W + prev.x.into_f32(),
                        2. * TESS_DBG_H - prev.y.into_f32(),
                    ),
                    Point2::new(
                        TESS_DBG_W + next.x.into_f32(),
                        2. * TESS_DBG_H - next.y.into_f32(),
                    ),
                    BGRA8::new(0, 0, 255, 255),
                );
            }
        }
    }

    // pub fn tessellate(&mut self) {
    //     let mut dummy = Painter::empty();
    //     self.subdivide(&mut dummy);
    //     self.vertices.clear();
    //     self.edges.clear();
    // }

    pub fn tessellate_d(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        dbg_font: Option<&crate::text::Font>,
    ) {
        self.subdivide(rasterizer, target, dbg_font);
        self.vertices.clear();
        self.edges.clear();
    }

    pub fn triangles(&self) -> &[(Fixed2, Fixed2, Fixed2)] {
        &self.monotone_tessellator.out_triangles
    }
}

pub fn tessellate_simple_polygon_new(
    points: &[(&[Point2f], bool)],
    rasterizer: &mut dyn Rasterizer,
    target: &mut RenderTarget,
    dbg_font: &crate::text::Font,
) {
    let mut tess = Tessellator::new();

    for &(points, invert) in points {
        tess.add_polygon(points, rasterizer, target, invert);
    }
    tess.subdivide(rasterizer, target, Some(dbg_font));

    let mut rotated = rotated_color_iterator(BGRA8::new(255, 0, 0, 255), 0.04);
    dbg!(&tess.monotone_tessellator.out_triangles);
    for (a, b, c) in tess.monotone_tessellator.out_triangles {
        rasterizer.fill_triangle(
            target,
            &[
                Point2::new(TESS_DBG_W + a.x.into_f32(), TESS_DBG_H - a.y.into_f32()),
                Point2::new(TESS_DBG_W + b.x.into_f32(), TESS_DBG_H - b.y.into_f32()),
                Point2::new(TESS_DBG_W + c.x.into_f32(), TESS_DBG_H - c.y.into_f32()),
            ],
            rotated.next(),
        );
    }
}
