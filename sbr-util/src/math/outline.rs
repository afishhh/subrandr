use std::fmt::Debug;

use super::*;
use crate::fmt_from_fn;

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

    #[inline]
    pub fn move_to(&mut self, point: Point2f) {
        self.close_contour_with_line();
        self.add_point(point);
    }

    #[inline]
    pub fn line_to(&mut self, point: Point2f) {
        self.add_point(point);
        self.add_segment(SegmentDegree::Linear);
    }

    #[inline]
    pub fn quad_to(&mut self, control: Point2f, point: Point2f) {
        self.add_point(control);
        self.add_point(point);
        self.add_segment(SegmentDegree::Quadratic);
    }

    #[inline]
    pub fn cubic_to(&mut self, control1: Point2f, control2: Point2f, point: Point2f) {
        self.add_point(control1);
        self.add_point(control2);
        self.add_point(point);
        self.add_segment(SegmentDegree::Cubic);
    }

    #[inline]
    pub fn contour_points_mut(&mut self) -> &mut [Point2f] {
        &mut self.outline.points[self.first_point_of_contour..]
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        self.outline
            .segments
            .last()
            .is_none_or(Segment::end_of_contour)
    }

    #[inline]
    pub fn add_point(&mut self, point: Point2f) {
        self.outline.points.push(point)
    }

    #[inline]
    pub fn add_segment(&mut self, degree: SegmentDegree) {
        self.outline.segments.push(Segment {
            degree,
            end_of_contour: false,
            start: self.segment_start,
        });
        self.segment_start += degree as u32;
    }

    fn close_contour_with_line(&mut self) {
        if let Some(&last) = self.outline.points.last() {
            if self.outline.segments.last_mut().unwrap().end_of_contour {
                return;
            }

            let first = self.outline.points[self.first_point_of_contour];
            if last == first {
                self.outline.segments.last_mut().unwrap().end_of_contour = true;
                self.first_point_of_contour = self.outline.points.len();
            } else {
                self.add_segment(SegmentDegree::Linear);
                self.close_contour();
            }
        }
    }

    pub fn close_contour(&mut self) {
        self.outline.segments.last_mut().unwrap().end_of_contour = true;
        self.outline
            .points
            .push(self.outline.points[self.first_point_of_contour]);
        self.segment_start += 1;
        self.first_point_of_contour = self.outline.points.len();
    }

    pub fn build(mut self) -> Outline {
        self.close_contour_with_line();

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
        let mut polyline = vec![self.points_for_segment(segments[0])[0]];
        for segment in segments {
            self.flatten_segment(*segment, 0.2, &mut polyline);
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
