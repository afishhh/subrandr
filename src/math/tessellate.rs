use std::{
    cmp::{Ordering, Reverse},
    fmt::Debug,
    ops::Sub,
};

use crate::{
    color::{rotated_color_iterator, RotatedColorIterator, BGRA8},
    util::{btree, OrderedF32},
    Painter,
};

use super::{Fixed, Point2};

type IFixed24Dot8 = Fixed<8, i32>;

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Fixed2 {
    pub x: IFixed24Dot8,
    pub y: IFixed24Dot8,
}

impl Fixed2 {
    fn normalize(self) -> Self {
        Point2::from(self).to_vec().normalize().to_point().into()
    }

    fn dot(self, other: Fixed2) -> IFixed24Dot8 {
        self.x * other.x + self.y * other.y
    }

    fn cross(self, other: Fixed2) -> IFixed24Dot8 {
        self.x * other.y - self.y * other.x
    }
}

impl From<Point2> for Fixed2 {
    fn from(value: Point2) -> Self {
        Self {
            x: value.x.into(),
            y: value.y.into(),
        }
    }
}

impl From<Fixed2> for Point2 {
    fn from(value: Fixed2) -> Self {
        Self::new(value.x.into_f32(), value.y.into_f32())
    }
}

impl Sub for Fixed2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    upper: Fixed2,
    lower: Fixed2,
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
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // match self.upper.x.cmp(&other.upper.x) {
        //     // FIXME: this comment is somewhat wrong and outdated.
        //     // Edges that start below should be considered "more to the right" as that aligns
        //     // with how the queries should work.

        //     // Note that this comparson **assumes no self intersections** as one
        //     // invariant of this tree ensures that segments are always going
        //     // to be ordered by the upper vertex, if a self intersection is found
        //     // this upper vertex must be fixed up so that the invariant still holds.
        //     std::cmp::Ordering::Equal => {
        //         self.upper.y.cmp(&other.upper.y).reverse().then_with(|| {
        //             self.lower
        //                 .x
        //                 .cmp(&other.lower.x)
        //                 .then(self.lower.y.cmp(&other.lower.y).reverse())
        //         })
        //     }
        //     order => order,
        // }
        match self.lower.x.cmp(&other.lower.x) {
            // FIXME: this comment is somewhat wrong and outdated.
            // Edges that start below should be considered "more to the right" as that aligns
            // with how the queries should work.

            // Note that this comparson **assumes no self intersections** as one
            // invariant of this tree ensures that segments are always going
            // to be ordered by the upper vertex, if a self intersection is found
            // this upper vertex must be fixed up so that the invariant still holds.
            std::cmp::Ordering::Equal => {
                self.lower.y.cmp(&other.lower.y).reverse().then_with(|| {
                    self.upper
                        .x
                        .cmp(&other.upper.x)
                        .then(self.upper.y.cmp(&other.upper.y).reverse())
                })
            }
            order => order,
        }
    }
}

fn point_left_right_det(a: Fixed2, b: Fixed2, p: Fixed2) -> IFixed24Dot8 {
    a.x * (b.y - p.y) + b.x * (p.y - a.y) + p.x * (a.y - b.y)
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

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq)]
struct Segment2 {
    upper: IFixed24Dot8,
    lower: IFixed24Dot8,
}

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq)]
struct Fixed2ByX(Fixed2);

impl PartialOrd for Fixed2ByX {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fixed2ByX {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.0.x.cmp(&other.0.x) {
            Ordering::Less => Ordering::Less,
            Ordering::Equal => self.0.y.cmp(&other.0.y),
            Ordering::Greater => Ordering::Greater,
        }
    }
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

    fn finish(&mut self, mid: usize, vertices: &[Fixed2]) {
        assert_eq!(self.kind, VertexEdgesKind::AllBelow);

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
                panic!("VertexEdges::finish: zero-length edge")
            }
        };

        match self.kind {
            VertexEdgesKind::AllAbove | VertexEdgesKind::AllBelow => {
                self.values
                    .sort_by_key(|&(id, _)| Fixed2ByX(vertices[id as usize]));
            }
            VertexEdgesKind::Regular => {}
        }
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

    triangulation_stack: Vec<(u32, u32)>,
    out_triangles: Vec<(Fixed2, Fixed2, Fixed2)>,
}

impl MonotoneTessellator {
    const fn new() -> Self {
        Self {
            xd: rotated_color_iterator(BGRA8::new(255, 0, 0, 255), 0.4),

            triangulation_stack: Vec::new(),
            out_triangles: Vec::new(),
        }
    }

    fn tessellate(
        &mut self,
        vertices: &[Fixed2],
        monotone_vertices: &mut [(u32, u32)],
        painter: &mut Painter,
    ) {
        dbg!(&monotone_vertices);

        let mut max = Fixed2 {
            x: IFixed24Dot8::MIN,
            y: IFixed24Dot8::MIN,
        };
        let mut maxi = 0;
        let mut min = Fixed2 {
            x: IFixed24Dot8::MAX,
            y: IFixed24Dot8::MAX,
        };
        let mut mini = 0;
        let mut rightmost = Fixed2 {
            x: IFixed24Dot8::MIN,
            y: IFixed24Dot8::MAX,
        };
        let mut rightmosti = u32::MAX;
        for (vertex, i) in monotone_vertices
            .iter()
            .map(|&(idx, i)| (vertices[idx as usize], i))
        {
            if vertex.y < min.y || (vertex.y == min.y && vertex.x > min.x) {
                min = vertex;
                mini = i;
            }
            if vertex.y > max.y || (vertex.y == max.y && vertex.x < max.x) {
                max = vertex;
                maxi = i;
            }
        }

        for (vertex, i) in monotone_vertices
            .iter()
            .map(|&(idx, i)| (vertices[idx as usize], i))
        {
            // maybe works but this is terrible :sob::sob:
            if vertex.x > rightmost.x && i != maxi && i != mini {
                rightmost = vertex;
                rightmosti = i;
            }
        }

        {
            let mut starti = maxi;
            let mut endi = mini;
            if starti > endi {
                std::mem::swap(&mut starti, &mut endi);
            }

            let chain = |i: u32| i <= endi && i > starti;
            let right_chain = chain(rightmosti);

            for (_, ch) in monotone_vertices.iter_mut() {
                *ch = u32::from(chain(*ch) == right_chain);
            }
        }

        // from now on chain is 1 for the right chain, 0 for the left chain

        {
            let mut last = vertices[monotone_vertices.last().unwrap().0 as usize];
            let m = self.xd.next();

            for &(vi, a) in monotone_vertices.iter() {
                let next = vertices[vi as usize];
                painter.line(
                    700 + last.x.round_to_inner(),
                    1400 - last.y.round_to_inner(),
                    700 + next.x.round_to_inner(),
                    1400 - next.y.round_to_inner(),
                    if a == 1 {
                        // m
                        BGRA8::new(255, 0, 0, 255)
                    } else {
                        // BGRA8::new(m.g, m.b, m.r, m.a)
                        BGRA8::new(0, 255, 0, 255)
                    },
                );
                last = next;
            }
        }

        monotone_vertices.sort_unstable_by_key(|&(idx, _)| {
            (Reverse(vertices[idx as usize].y), vertices[idx as usize].x)
        });

        self.triangulation_stack.clear();
        self.triangulation_stack
            .extend([monotone_vertices[0], monotone_vertices[1]]);

        for i in 2..monotone_vertices.len() - 1 {
            eprintln!(
                "current stack: {:?}",
                self.triangulation_stack
                    .iter()
                    .map(|&(idx, i)| (vertices[idx as usize], i))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "next vertex: {:?} {:?}",
                vertices[monotone_vertices[i].0 as usize], monotone_vertices[i].1
            );

            if monotone_vertices[i].1 != self.triangulation_stack.last().unwrap().1 {
                let mut last = self.triangulation_stack[0];
                for &vert in self.triangulation_stack[1..].iter().rev() {
                    {
                        let prev = vertices[vert.0 as usize];
                        let next = vertices[monotone_vertices[i].0 as usize];
                        painter.line(
                            700 + prev.x.round_to_inner(),
                            700 - prev.y.round_to_inner(),
                            700 + next.x.round_to_inner(),
                            700 - next.y.round_to_inner(),
                            BGRA8::new(255, 0, 0, 255),
                        )
                    }
                    println!("diagonal type 1");
                    self.out_triangles.push((
                        vertices[last.0 as usize],
                        vertices[vert.0 as usize],
                        vertices[monotone_vertices[i].0 as usize],
                    ));

                    last = vert;
                }

                self.triangulation_stack.clear();
                self.triangulation_stack
                    .extend(&monotone_vertices[i - 1..i + 1]);
            } else {
                let mut last = self.triangulation_stack.pop().unwrap();
                while let Some(&next) = self.triangulation_stack.last() {
                    let angle = (vertices[next.0 as usize]
                        - vertices[monotone_vertices[i].0 as usize])
                        .cross(
                            vertices[last.0 as usize] - vertices[monotone_vertices[i].0 as usize],
                        );

                    if monotone_vertices[i].1 == 0 && angle < 0
                        || (monotone_vertices[i].1 != 0 && angle > 0)
                    {
                        break;
                    }

                    self.triangulation_stack.pop();

                    {
                        let prev = vertices[next.0 as usize];
                        let next = vertices[monotone_vertices[i].0 as usize];
                        painter.line(
                            700 + prev.x.round_to_inner(),
                            700 - prev.y.round_to_inner(),
                            700 + next.x.round_to_inner(),
                            700 - next.y.round_to_inner(),
                            BGRA8::new(255, 0, 0, 255),
                        )
                    }
                    {
                        let prev = vertices[last.0 as usize];
                        let next = vertices[monotone_vertices[i].0 as usize];
                        painter.line(
                            700 + prev.x.round_to_inner(),
                            700 - prev.y.round_to_inner(),
                            700 + next.x.round_to_inner(),
                            700 - next.y.round_to_inner(),
                            BGRA8::new(0, 255, 0, 255),
                        )
                    }
                    println!("diagonal type 2");

                    self.out_triangles.push((
                        vertices[last.0 as usize],
                        vertices[next.0 as usize],
                        vertices[monotone_vertices[i].0 as usize],
                    ));

                    last = next;
                }

                self.triangulation_stack
                    .extend([last, monotone_vertices[i]]);
            }
        }

        dbg!(&self.triangulation_stack);

        for w in self.triangulation_stack.windows(2) {
            let &[last, current] = w else { unreachable!() };

            {
                let prev = vertices[current.0 as usize];
                let next = vertices[monotone_vertices.last().unwrap().0 as usize];
                painter.line(
                    700 + prev.x.round_to_inner(),
                    700 - prev.y.round_to_inner(),
                    700 + next.x.round_to_inner(),
                    700 - next.y.round_to_inner(),
                    BGRA8::new(255, 0, 0, 255),
                )
            }

            self.out_triangles
                .push((vertices[last.0 as usize], vertices[current.0 as usize], min))
        }

        // self.out_triangles.push((
        //     self.vertices[self.triangulation_stack[0].0 as usize],
        //     self.vertices[self.triangulation_stack.last().unwrap().0 as usize],
        //     min,
        // ))
    }
}

pub struct Tessellator {
    vertices: Vec<Fixed2>,

    queue: Vec<u32>,
    edges: Vec<VertexEdges>,

    helpers: Vec<Helper>,
    winding_tree: winding_tree::WindingTree,
    // TODO: this will run into edge cases where it shouldn't actually remove the edge
    //       i.e. when two edges are colinear
    //       maybe edge id should be part of the key?
    material_edge_tree: btree::Tree<btree::MapTraits<Segment, u32>>,

    // good solution? no. works? yes.
    out_holeness: Vec<bool>,
    out_edges: Vec<Vec<u32>>,

    visited_marker: u32,
    // TODO: make this a Vec<BitVec> or some other more smart encoding
    visited: Vec<Vec<u32>>,
    monotone_vertices: Vec<(u32, u32)>,

    monotone_tessellator: MonotoneTessellator,
}

impl Tessellator {
    pub fn new() -> Tessellator {
        Self {
            vertices: Vec::new(),
            queue: Vec::new(),
            edges: Vec::new(),

            helpers: Vec::new(),
            winding_tree: winding_tree::WindingTree::new(),
            material_edge_tree: btree::Tree::new(),

            out_holeness: Vec::new(),
            out_edges: Vec::new(),

            visited_marker: 0,
            visited: Vec::new(),
            monotone_vertices: Vec::new(),

            monotone_tessellator: MonotoneTessellator::new(),
        }
    }

    pub fn add_polygon(&mut self, points: &[Point2], painter: &mut Painter, invert: bool) {
        if points.len() < 3 {
            panic!("Tessellator::add_polygon called with less than three points");
        }

        if let Some(mut prev) = points.last().copied().map(Fixed2::from) {
            let offset = self.vertices.len();
            let mut pi = offset + points.len() - 1;
            let mut i = offset;

            self.vertices.reserve(points.len());
            self.edges
                .resize_with(self.edges.len() + points.len(), VertexEdges::new);

            for next in points.iter().copied().map(Fixed2::from) {
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

                painter.line(
                    prev.x.round_to_inner(),
                    700 - prev.y.round_to_inner(),
                    next.x.round_to_inner(),
                    700 - next.y.round_to_inner(),
                    BGRA8::new(255, 0, 0, 255),
                );

                {
                    let start = Point2::from(prev);
                    let end = Point2::from(next);

                    let middle = start + ((end - start) / 2.0);
                    let dir = (end - start).normalize();
                    let deriv = dir.normal();
                    const ARROW_SCALE: f32 = 20.0;

                    let f = if invert { -1.0 } else { 1.0 };
                    let top = middle + dir * f * ARROW_SCALE;
                    let left = middle - deriv * f * ARROW_SCALE;
                    let right = middle + deriv * f * ARROW_SCALE;

                    painter.fill_triangle(
                        top.x as i32,
                        700 - top.y as i32,
                        left.x as i32,
                        700 - left.y as i32,
                        right.x as i32,
                        700 - right.y as i32,
                        BGRA8::new(255, 0, 0, 255),
                    );
                }

                self.queue.push(self.vertices.len() as u32);
                self.vertices.push(next);
                prev = next;
                pi = i;
                i += 1;
            }

            for (i, edges) in self.edges.iter_mut().enumerate().skip(offset) {
                edges.finish(i, &self.vertices);
            }

            for (i, edge) in self.edges.iter_mut().enumerate().skip(offset) {
                println!("{:?}", self.vertices[edge.values[0].0 as usize]);
                println!("  {:?} {:?}", self.vertices[i], edge.kind);
                println!("{:?}", self.vertices[edge.values[1].0 as usize]);
                println!()
            }

            if i != self.vertices.len() {
                panic!()
            }
            // TODO: the below code would also have to fix `pi` in the edges to point
            //       to the new index...
            // self.edges[i - 1] = self.edges[offset + points.len() - 1];
            // self.edges.truncate(i);
        }
    }

    fn walk_and_triangulate_faces(&mut self, painter: &mut Painter) {
        self.visited_marker += 1;
        self.visited.resize(self.out_edges.len(), Vec::new());

        for (i, adj) in self.out_edges.iter_mut().enumerate() {
            let center = self.vertices[i];
            adj.sort_unstable_by_key(|&idx| {
                let v = self.vertices[idx as usize];
                Reverse(OrderedF32(f32::atan2(
                    (v.y - center.y).into_f32(),
                    (v.x - center.x).into_f32(),
                )))
            });
            adj.dedup();
            self.visited[i].resize(adj.len(), 0);
        }

        for i in 0..self.out_edges.len() {
            eprint!("{i} ({}, {}):", self.vertices[i].x, self.vertices[i].y);
            for &v in &self.out_edges[i] {
                eprint!(" {v}")
            }
            eprintln!()
        }

        for start in 0..self.out_edges.len() {
            for mut edge in 0..self.out_edges[start].len() {
                if self.visited[start][edge] == self.visited_marker {
                    continue;
                }

                self.monotone_vertices.clear();
                let mut current = start;
                while self.visited[current][edge] != self.visited_marker {
                    println!("{current}[{edge}]");

                    self.visited[current][edge] = self.visited_marker;
                    self.monotone_vertices
                        .push((current as u32, self.monotone_vertices.len() as u32));
                    let next = self.out_edges[current][edge];

                    let center = self.vertices[next as usize];
                    let next_edge = self.out_edges[next as usize]
                        .binary_search_by_key(
                            &{
                                let v = self.vertices[current];
                                Reverse(OrderedF32(f32::atan2(
                                    (v.y - center.y).into_f32(),
                                    (v.x - center.x).into_f32(),
                                )))
                            },
                            |&idx| {
                                let v = self.vertices[idx as usize];
                                Reverse(OrderedF32(f32::atan2(
                                    (v.y - center.y).into_f32(),
                                    (v.x - center.x).into_f32(),
                                )))
                            },
                        )
                        .unwrap()
                        + 1;

                    current = next as usize;
                    edge = if next_edge == self.out_edges[next as usize].len() {
                        0
                    } else {
                        next_edge
                    };
                }

                let mut area = IFixed24Dot8::ZERO;
                let mut last = self.vertices[self.monotone_vertices.last().unwrap().0 as usize];
                let mut min = self.monotone_vertices[0].0 as usize;
                for (i, vertex) in self
                    .monotone_vertices
                    .iter()
                    .map(|&(idx, _)| (idx as usize, self.vertices[idx as usize]))
                {
                    area += (vertex.x - last.x) * (vertex.y + last.y);
                    last = vertex;
                    if vertex.x < self.vertices[min].x {
                        min = i;
                    }
                }

                if area < 0 && !self.out_holeness[min] {
                    self.monotone_tessellator.tessellate(
                        &self.vertices,
                        &mut self.monotone_vertices,
                        painter,
                    );
                }
            }
        }
    }

    fn subdivide(&mut self, painter: &mut Painter) {
        macro_rules! insert_diagonal {
            ($a: expr, $b: expr) => {
                let (a, b) = ($a, $b);
                if a != b {
                    {
                        let prev = self.vertices[a as usize];
                        let next = self.vertices[b as usize];
                        painter.line(
                            prev.x.round_to_inner(),
                            700 - prev.y.round_to_inner(),
                            next.x.round_to_inner(),
                            700 - next.y.round_to_inner(),
                            BGRA8::new(0, 255, 0, 255),
                        )
                    }

                    println!(
                        "diagonal {:?} -- {:?}",
                        self.vertices[a as usize], self.vertices[b as usize]
                    );

                    self.out_edges[a as usize].push(b);
                    self.out_edges[b as usize].push(a);
                }
            };
        }

        self.queue.sort_unstable_by(|&ai, &bi| {
            let a = self.vertices[ai as usize];
            let b = self.vertices[bi as usize];
            match a.y.cmp(&b.y) {
                Ordering::Less => Ordering::Greater,
                Ordering::Equal => a.x.cmp(&b.x),
                Ordering::Greater => Ordering::Less,
            }
        });

        self.helpers.resize_with(self.vertices.len(), || Helper {
            vertex: u32::MAX,
            is_merge_vertex: false,
        });

        for adj in self.out_edges.iter_mut() {
            adj.clear();
        }
        self.out_edges.resize(self.vertices.len(), Vec::new());
        self.out_holeness.resize(self.vertices.len(), false);

        println!("{:#?}", self.vertices);
        println!("{:?}", self.queue);
        for next in self.queue.iter().copied() {
            let winding_count = self
                .winding_tree
                .before(self.vertices[next as usize])
                .unwrap_or(0);
            let edges = self.edges[next as usize];
            let class = edges.kind.classify(winding_count != 0);

            let winding_count_inclusive = self
                .winding_tree
                .before_inclusive(self.vertices[next as usize])
                .unwrap_or(0);

            eprintln!("processing vertex {:?}", self.vertices[next as usize]);
            eprintln!("  winding {winding_count}");
            eprintln!("  inclusive winding {winding_count_inclusive}");
            eprintln!("  {:?}", self.edges[next as usize]);
            eprintln!("  {:?}", class);

            self.winding_tree.dump();

            // NOTE: The edges in VertexEdges are sorted by x so this works
            let mut insert_winding_count = winding_count;
            let mut is_materialized_vertex = self.helpers[next as usize].vertex != u32::MAX
                || self.helpers[next as usize].is_merge_vertex;
            for down_v in self.edges[next as usize].down() {
                let materialized_edge =
                    (insert_winding_count + down_v.1 as i32) == 0 || insert_winding_count == 0;
                if materialized_edge {
                    self.material_edge_tree.insert(
                        Segment {
                            upper: self.vertices[next as usize],
                            lower: self.vertices[down_v.0 as usize],
                        },
                        next,
                    );
                    self.helpers[next as usize] = Helper {
                        vertex: next,
                        is_merge_vertex: false,
                    };
                    self.helpers[down_v.0 as usize].is_merge_vertex = true;
                    is_materialized_vertex |= true;
                }

                self.winding_tree.add(
                    self.vertices[next as usize],
                    self.vertices[down_v.0 as usize],
                    next,
                    down_v.1 as i32,
                );
                insert_winding_count += down_v.1 as i32;
            }

            for up_v in self.edges[next as usize].up() {
                self.winding_tree
                    .remove(self.vertices[up_v.0 as usize], self.vertices[next as usize]);
            }

            let before = self.material_edge_tree.exclusive_upper_bound_by(|k| {
                point_left_right_det(k.lower, k.upper, self.vertices[next as usize])
                    .cmp(&IFixed24Dot8::ZERO)
            });

            self.material_edge_tree.dump();

            if is_materialized_vertex {
                match class {
                    VertexClass::Start => {}
                    VertexClass::End => {}
                    VertexClass::Regular => {
                        if winding_count == 0 {
                            if let Some(helper) = self
                                .material_edge_tree
                                .get(&Segment {
                                    upper: self.vertices
                                        [self.edges[next as usize].up()[0].0 as usize],
                                    lower: self.vertices[next as usize],
                                })
                                .map(|(_, &id, _)| self.helpers[id as usize])
                            {
                                if helper.is_merge_vertex {
                                    insert_diagonal!(next, helper.vertex);
                                }
                            }
                        } else if let Some((_, &helper, _)) = before {
                            let old = std::mem::replace(
                                &mut self.helpers[helper as usize],
                                Helper {
                                    vertex: next,
                                    is_merge_vertex: false,
                                },
                            );
                            if old.is_merge_vertex {
                                insert_diagonal!(next, old.vertex);
                            }
                        }
                    }
                    VertexClass::Split => {
                        let (_, &helper, _) = before.unwrap();
                        let old = std::mem::replace(
                            &mut self.helpers[helper as usize],
                            Helper {
                                vertex: next,
                                is_merge_vertex: false,
                            },
                        );
                        insert_diagonal!(next, old.vertex);
                    }
                    VertexClass::Merge => {
                        let helper = &self.helpers[next as usize];
                        if helper.vertex != u32::MAX && helper.is_merge_vertex {
                            insert_diagonal!(next, helper.vertex);
                        }

                        if let Some((_, &id, _)) = before {
                            let helper = &mut self.helpers[id as usize];
                            if helper.vertex != u32::MAX
                                && helper.is_merge_vertex
                                && self.edges[next as usize].up()[0].0 != helper.vertex
                            {
                                insert_diagonal!(next, helper.vertex);
                            }
                            helper.vertex = next;
                            helper.is_merge_vertex = true;
                        }
                    }
                }

                for up in self.edges[next as usize].up() {
                    if let Some((_, _)) = self.material_edge_tree.remove(&Segment {
                        upper: self.vertices[up.0 as usize],
                        lower: self.vertices[next as usize],
                    }) {
                        let helper = self.helpers[up.0 as usize];
                        if helper.is_merge_vertex {
                            insert_diagonal!(next, helper.vertex);
                        }

                        self.out_edges[up.0 as usize].push(next);
                        self.out_edges[next as usize].push(up.0);

                        eprintln!(
                            "edge {:?} -- {:?}",
                            self.vertices[next as usize], self.vertices[up.0 as usize]
                        );
                    }
                }
            }
        }

        for (a, adj) in self.out_edges.iter().enumerate() {
            for &b in adj {
                let prev = self.vertices[a];
                let next = self.vertices[b as usize];

                painter.line(
                    700 + prev.x.round_to_inner(),
                    700 - prev.y.round_to_inner(),
                    700 + next.x.round_to_inner(),
                    700 - next.y.round_to_inner(),
                    BGRA8::new(0, 0, 255, 255),
                );
            }
        }

        //     for &(a, b) in self.out_diagonals.iter() {
        //         let prev = self.vertices[a as usize];
        //         let next = self.vertices[b as usize];

        //         painter.line(
        //             700 + prev.x.round_to_inner(),
        //             700 - prev.y.round_to_inner(),
        //             700 + next.x.round_to_inner(),
        //             700 - next.y.round_to_inner(),
        //             BGRA8::new(0, 255, 0, 255),
        //         );
        //     }
    }

    pub fn tessellate(&mut self) {
        let mut dummy = Painter::empty();
        self.subdivide(&mut dummy);
        self.walk_and_triangulate_faces(&mut dummy);
        self.vertices.clear();
        self.edges.clear();
    }

    pub fn tessellate_d(&mut self, painter: &mut Painter) {
        self.subdivide(painter);
        self.walk_and_triangulate_faces(painter);
        self.vertices.clear();
        self.edges.clear();
    }

    pub fn triangles(&self) -> &[(Fixed2, Fixed2, Fixed2)] {
        &self.monotone_tessellator.out_triangles
    }
}

#[test]
fn monotone_tessellator_test_1() {
    let mut tess = MonotoneTessellator::new();

    tess.tessellate(
        &[
            Fixed2 {
                x: 300.0.into(),
                y: 700.0.into(),
            },
            Fixed2 {
                x: 600.0.into(),
                y: 700.0.into(),
            },
            Fixed2 {
                x: 600.0.into(),
                y: 200.0.into(),
            },
            Fixed2 {
                x: 100.0.into(),
                y: 200.0.into(),
            },
            Fixed2 {
                x: 400.0.into(),
                y: 400.0.into(),
            },
            Fixed2 {
                x: 300.0.into(),
                y: 500.0.into(),
            },
            Fixed2 {
                x: 100.0.into(),
                y: 600.0.into(),
            },
        ],
        &mut [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)],
        &mut Painter::new(1, 1, &mut [BGRA8::ZERO][..]),
    );

    assert_eq!(tess.out_triangles.len(), 4);
}

#[test]
fn monotone_tessellator_test_2() {
    let mut tess = MonotoneTessellator::new();

    tess.tessellate(
        &[
            Fixed2 {
                x: 300.0.into(),
                y: 700.0.into(),
            },
            Fixed2 {
                x: 100.0.into(),
                y: 300.0.into(),
            },
            Fixed2 {
                x: 700.0.into(),
                y: 200.0.into(),
            },
            Fixed2 {
                x: 300.0.into(),
                y: 400.0.into(),
            },
            Fixed2 {
                x: 600.0.into(),
                y: 500.0.into(),
            },
            Fixed2 {
                x: 480.0.into(),
                y: 530.0.into(),
            },
        ],
        &mut [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)],
        &mut Painter::new(1, 1, &mut [BGRA8::ZERO][..]),
    );

    assert_eq!(tess.out_triangles.len(), 4);
}

#[test]
fn monotone_tessellator_test_3() {
    let mut tess = MonotoneTessellator::new();

    tess.tessellate(
        &[
            Fixed2 {
                x: 300.0.into(),
                y: 700.0.into(),
            },
            Fixed2 {
                x: 600.0.into(),
                y: 700.0.into(),
            },
            Fixed2 {
                x: 700.0.into(),
                y: 100.0.into(),
            },
            Fixed2 {
                x: 100.0.into(),
                y: 100.0.into(),
            },
            Fixed2 {
                x: 400.0.into(),
                y: 300.0.into(),
            },
            Fixed2 {
                x: 350.0.into(),
                y: 500.0.into(),
            },
        ],
        &mut [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)],
        &mut Painter::new(1, 1, &mut [BGRA8::ZERO][..]),
    );

    assert_eq!(tess.out_triangles.len(), 4);
}

pub fn tessellate_simple_polygon_new(points: &[Point2], points2: &[Point2], painter: &mut Painter) {
    let mut tess = Tessellator::new();

    tess.add_polygon(points, painter, false);
    tess.add_polygon(points2, painter, false);
    tess.subdivide(painter);
    tess.walk_and_triangulate_faces(painter);

    let mut rotated = rotated_color_iterator(BGRA8::new(255, 0, 0, 255), 0.04);
    dbg!(&tess.monotone_tessellator.out_triangles);
    for (a, b, c) in tess.monotone_tessellator.out_triangles {
        painter.fill_triangle(
            a.x.round_to_inner(),
            1400 - a.y.round_to_inner(),
            b.x.round_to_inner(),
            1400 - b.y.round_to_inner(),
            c.x.round_to_inner(),
            1400 - c.y.round_to_inner(),
            rotated.next(),
        );
    }
}
