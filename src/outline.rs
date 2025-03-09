use std::fmt::Debug;

use text_sys::{FT_Outline, FT_Vector, FT_CURVE_TAG_CONIC, FT_CURVE_TAG_CUBIC, FT_CURVE_TAG_ON};

use crate::{math::*, util::fmt_from_fn};

pub struct OutlineBuilder {
    outline: Outline,
    first_point_of_contour: usize,
    segment_start: u32,
}

impl OutlineBuilder {
    pub const fn new() -> Self {
        Self {
            outline: Outline::empty(),
            first_point_of_contour: 0,
            segment_start: 0,
        }
    }

    #[inline(always)]
    pub fn points(&self) -> &[Point2f] {
        self.outline.points()
    }

    #[inline(always)]
    pub fn contour_points(&self) -> &[Point2f] {
        &self.outline.points()[self.first_point_of_contour..]
    }

    #[inline(always)]
    pub fn contour_points_mut(&mut self) -> &mut [Point2f] {
        &mut self.outline.points[self.first_point_of_contour..]
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.outline.is_empty()
    }

    pub fn is_closed(&self) -> bool {
        self.outline
            .segments
            .last()
            .is_none_or(Segment::end_of_contour)
    }

    #[inline(always)]
    pub fn add_point(&mut self, point: Point2f) {
        self.outline.points.push(point)
    }

    #[inline(always)]
    pub fn add_segment(&mut self, degree: SegmentDegree) {
        self.outline.segments.push(Segment {
            degree,
            end_of_contour: false,
            start: self.segment_start,
        });
        self.segment_start += degree as u32;
    }

    pub fn close_contour(&mut self) {
        self.outline.segments.last_mut().unwrap().end_of_contour = true;
        self.outline
            .points
            .push(self.outline.points[self.first_point_of_contour]);
        self.segment_start += 1;
        self.first_point_of_contour = self.outline.points.len();
    }

    pub fn build(self) -> Outline {
        let expected = self.segment_start;
        if self.outline.points.len() != expected as usize {
            panic!(
                "Invalid outline: Incorrect number of points: expected {} found {}\npoints: {:?}\nsegments: {:?}",
                expected, self.outline.points.len(),
                self.outline.points, self.outline.segments
            );
        }

        if !self.is_closed() {
            panic!("Invalid outline: Last segment is not marked end of contour")
        }

        self.outline
    }
}

impl Default for OutlineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentDegree {
    Linear = 1,
    Quadratic = 2,
    Cubic = 3,
}

#[derive(Debug, Clone, Copy)]
#[repr(Rust, packed(8))]
pub struct Segment {
    end_of_contour: bool,
    degree: SegmentDegree,
    start: u32,
}

impl Segment {
    #[inline(always)]
    pub const fn degree(&self) -> SegmentDegree {
        self.degree
    }

    #[inline(always)]
    pub const fn end_of_contour(&self) -> bool {
        self.end_of_contour
    }
}

#[derive(Clone)]
pub struct Outline {
    points: Vec<Point2f>,
    segments: Vec<Segment>,
}

impl Outline {
    pub const fn empty() -> Self {
        Self {
            points: Vec::new(),
            segments: Vec::new(),
        }
    }

    pub unsafe fn from_freetype(ft: &FT_Outline) -> Self {
        let mut first = 0;
        let mut builder = OutlineBuilder::new();
        let contours = std::slice::from_raw_parts(ft.contours, ft.n_contours as usize);
        let points = std::slice::from_raw_parts(ft.points, ft.n_points as usize);
        let tags = std::slice::from_raw_parts(ft.tags, ft.n_points as usize);

        // TODO: Convert FT_CURVE_TAG* to u8 in text-sys
        for last in contours.iter().map(|&x| x as usize) {
            // FT_Pos in FT_Outline seems to be 26.6
            let to_point = |vec: FT_Vector| {
                Point2f::new(
                    vec.x as f32 * 2.0f32.powi(-6),
                    // FreeType uses a "Y axis at the bottom" coordinate system,
                    // flip that to match ours
                    -vec.y as f32 * 2.0f32.powi(-6),
                )
            };

            let midpoint =
                |a: Point2f, b: Point2f| Point2f::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);

            let mut last_tag;
            let mut final_degree = SegmentDegree::Linear;
            let mut add_range = first..last + 1;
            if (tags[first] & 0b11) != FT_CURVE_TAG_ON as u8 {
                if (tags[last] & 0b11) == FT_CURVE_TAG_CONIC as u8 {
                    builder.add_point(midpoint(to_point(points[first]), to_point(points[last])));
                    last_tag = FT_CURVE_TAG_ON as u8;
                    final_degree = SegmentDegree::Quadratic;
                } else {
                    assert_eq!(tags[last] & 0b11, FT_CURVE_TAG_ON as u8);
                    builder.add_point(to_point(points[last]));
                    last_tag = tags[last] & 0b11;
                    add_range.end -= 1;
                }
            } else {
                builder.add_point(to_point(points[first]));
                last_tag = tags[first] & 0b11;
                add_range.start += 1;
                if tags[last] & 0b11 == FT_CURVE_TAG_CUBIC as u8 {
                    final_degree = SegmentDegree::Cubic;
                } else if tags[last] & 0b11 == FT_CURVE_TAG_CONIC as u8 {
                    final_degree = SegmentDegree::Quadratic;
                }
            }

            for (&point, &tag) in points[add_range.clone()].iter().zip(tags[add_range].iter()) {
                let tag = tag & 0b11;
                let point = to_point(point);

                if tag == FT_CURVE_TAG_ON as u8 {
                    if last_tag == FT_CURVE_TAG_ON as u8 {
                        builder.add_segment(SegmentDegree::Linear);
                    } else if last_tag == FT_CURVE_TAG_CONIC as u8 {
                        builder.add_segment(SegmentDegree::Quadratic);
                    } else {
                        builder.add_segment(SegmentDegree::Cubic);
                    }
                }

                if tag == FT_CURVE_TAG_CONIC as u8 && last_tag == FT_CURVE_TAG_CONIC as u8 {
                    let last = *builder.points().last().unwrap();
                    builder.add_point(midpoint(last, point));
                    builder.add_segment(SegmentDegree::Quadratic);
                }

                last_tag = tag;
                builder.add_point(point);
            }

            builder.add_segment(final_degree);
            builder.close_contour();
            first = last + 1;
        }

        assert_eq!(first, points.len());

        builder.build()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    #[inline(always)]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    #[inline(always)]
    pub fn points_for_segment(&self, s: Segment) -> &[Point2f] {
        let start = s.start as usize;
        &self.points[start..(start + s.degree as usize + 1)]
    }

    #[inline(always)]
    pub fn points(&self) -> &[Point2f] {
        &self.points
    }

    fn evaluate_segment_normalized(&self, i: usize, t: f32) -> Point2f {
        assert!((0.0..=1.0).contains(&t));

        let value = evaluate_bezier(self.points_for_segment(self.segments[i]), t);
        value
    }

    pub fn evaluate_segment(&self, segment: Segment, t: f32) -> Point2f {
        assert!((0.0..=1.0).contains(&t));

        let value = evaluate_bezier(self.points_for_segment(segment), t);
        value
    }

    #[inline(always)]
    pub fn evaluate(&self, t: f32) -> Point2f {
        self.evaluate_segment_normalized(t.trunc() as usize, t.fract())
    }

    pub fn control_box(&self) -> Rect2f {
        let mut bb = Rect2f::NOTHING;
        for &point in self.points.iter() {
            bb.expand_to_point(point);
        }
        bb
    }

    pub fn scale(&mut self, xy: f32) {
        for p in self.points.iter_mut() {
            *p = (p.to_vec() * xy).to_point()
        }
    }

    /// Does not include the first point of the segment in the flattened version
    pub fn flatten_segment(&self, segment: Segment, tolerance: f32, out: &mut Vec<Point2f>) {
        let points = self.points_for_segment(segment);
        match points.len() {
            2 => out.push(points[1]),
            3 => QuadraticBezier::from_ref(points.try_into().unwrap()).flatten_into(tolerance, out),
            4 => CubicBezier::from_ref(points.try_into().unwrap()).flatten_into(tolerance, out),
            _ => unreachable!(),
        }
    }

    pub fn iter_contours(&self) -> impl Iterator<Item = &[Segment]> + use<'_> {
        let mut it = self
            .segments
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(i, s)| if s.end_of_contour { Some(i) } else { None });

        let mut last = 0;
        std::iter::from_fn(move || {
            it.next().map(|end_of_contour| {
                let segments = &self.segments[last..=end_of_contour];
                last = end_of_contour + 1;
                segments
            })
        })
    }

    pub fn flatten_contour(&self, segments: &[Segment]) -> Vec<Point2f> {
        self.flatten_contour_with_tolerance(segments, 0.2)
    }

    pub fn flatten_contour_with_tolerance(
        &self,
        segments: &[Segment],
        tolerance: f32,
    ) -> Vec<Point2f> {
        let mut polyline = vec![self.points_for_segment(segments[0])[0]];
        for segment in segments {
            self.flatten_segment(*segment, tolerance, &mut polyline);
        }
        polyline
    }
}

impl Debug for Outline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Outline ")?;
        let mut list = f.debug_list();
        for segments in self.iter_contours() {
            list.entry(&fmt_from_fn(|f| {
                write!(f, "Contour ")?;

                let mut list = f.debug_list();
                for segment in segments.iter().copied() {
                    let points = self.points_for_segment(segment);
                    list.entry(&fmt_from_fn(|f| {
                        f.debug_struct("Curve")
                            .field("degree", &segment.degree)
                            .field("points", &points)
                            .finish()
                    }));
                }
                list.finish()
            }));
        }
        list.finish()
    }
}

#[inline(always)]
fn b_spline_to_bezier(b0: Point2f, b1: Point2f, b2: Point2f, b3: Point2f) -> [Point2f; 4] {
    [
        ((b0.to_vec() + b1.to_vec() * 4.0 + b2.to_vec()) / 6.0).to_point(),
        ((b1.to_vec() * 2.0 + b2.to_vec()) / 3.0).to_point(),
        ((b2.to_vec() * 2.0 + b1.to_vec()) / 3.0).to_point(),
        ((b1.to_vec() + b2.to_vec() * 4.0 + b3.to_vec()) / 6.0).to_point(),
    ]
}

mod stroke;

impl Outline {
    pub fn stroke(&self, x: f32, y: f32, eps: f32) -> (Outline, Outline) {
        stroke::stroke(self, x, y, eps)
    }
}
