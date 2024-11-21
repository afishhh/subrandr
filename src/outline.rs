use std::{fmt::Debug, mem::MaybeUninit};

use crate::util::{fmt_from_fn, math::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplineDegree {
    Linear = 1,
    Quadratic = 2,
    Cubic = 3,
}

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    degree: SplineDegree,
    end_of_contour: bool,
    // Not usize so that `Segment` fits in 8 bytes
    end: u32,
}

impl Segment {
    pub fn degree(&self) -> SplineDegree {
        self.degree
    }

    pub fn end_of_contour(&self) -> bool {
        self.end_of_contour
    }
}

#[derive(Clone)]
pub struct Outline {
    points: Vec<Point2>,
    segments: Vec<Segment>,
}

pub struct OutlineBuilder {
    outline: Outline,
    first_point_of_contour: u32,
    last_segment_end: u32,
}

impl OutlineBuilder {
    pub const fn new() -> Self {
        Self {
            outline: Outline::new(),
            first_point_of_contour: 0,
            last_segment_end: 1,
        }
    }

    #[inline(always)]
    pub fn add_point(&mut self, point: Point2) {
        self.outline.points.push(point)
    }

    #[inline(always)]
    pub fn add_segment(&mut self, degree: SplineDegree) {
        self.last_segment_end += degree as u32;
        self.outline.segments.push(Segment {
            degree,
            end_of_contour: false,
            end: self.last_segment_end,
        });
    }

    pub fn close_contour(&mut self) {
        self.outline.segments.last_mut().unwrap().end_of_contour = true;
        self.outline
            .points
            .push(self.outline.points[self.first_point_of_contour as usize]);
        self.last_segment_end += 1;
        self.first_point_of_contour = self.outline.points.len() as u32;
    }

    #[inline(always)]
    pub fn current_contour_points_mut(&mut self) -> &mut [Point2] {
        &mut self.outline.points[self.first_point_of_contour as usize..]
    }

    pub fn build(self) -> Outline {
        let expected = self.last_segment_end - 1;
        if self.outline.points.len() != expected as usize {
            panic!(
                "Invalid outline: Incorrect number of points: expected {} found {}\npoints: {:?}\nsegments: {:?}",
                expected, self.outline.points.len(),
                self.outline.points, self.outline.segments
            );
        }

        if !self
            .outline
            .segments
            .last()
            .is_none_or(|x| x.end_of_contour)
        {
            panic!("Invalid outline: last segment is not marked end of contour")
        }

        self.outline
    }
}

impl Default for OutlineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Outline {
    pub const fn new() -> Self {
        Self {
            points: vec![],
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

    pub fn points_for_segment(&self, Segment { end, degree, .. }: Segment) -> &[Point2] {
        let start;
        let nend;

        // if end_of_contour {
        //     start = end as usize - degree as usize - 1;
        //     nend = end as usize;
        // } else {
        start = end as usize - degree as usize - 1;
        nend = end as usize;
        // }

        &self.points[start..nend]
    }

    pub fn points(&self) -> &[Point2] {
        &self.points
    }

    fn evaluate_segment_normalized(&self, i: usize, t: f32) -> Point2 {
        assert!(0.0 <= t && t <= 1.0);

        let value = evaluate_bezier(self.points_for_segment(self.segments[i]), t);
        eprintln!(
            "evaluate_segment_normalized({i}, {t}), {:?} = {value:?}",
            self.points_for_segment(self.segments[i])
        );
        value
    }

    pub fn evaluate_segment(&self, segment: Segment, t: f32) -> Point2 {
        assert!(0.0 <= t && t <= 1.0);

        let value = evaluate_bezier(self.points_for_segment(segment), t);
        // eprintln!(
        //     "evaluate_segment({segment:?}, {t}), {:?} = {value:?}",
        //     self.points_for_segment(segment)
        // );
        value
    }

    pub fn evaluate(&self, t: f32) -> Point2 {
        self.evaluate_segment_normalized(t.trunc() as usize, t.fract())
    }

    pub fn calculate_control_box(&self, bb: &mut BoundingBox) {
        for point in self.points.iter() {
            bb.add(point);
        }
    }

    pub fn scale(&mut self, xy: f32) {
        for p in self.points.iter_mut() {
            *p = (p.to_vec() * xy).to_point()
        }
    }
}

impl Debug for Outline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Outline ")?;
        let mut list = f.debug_list();
        let mut it = self.segments.iter().copied().enumerate();
        let mut last = 0;
        while let Some(end_of_contour) =
            it.find_map(|(i, s)| if s.end_of_contour { Some(i) } else { None })
        {
            let segments = &self.segments[last..end_of_contour];
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
            last = end_of_contour;
        }
        list.finish()
    }
}

#[inline(always)]
fn b_spline_to_bezier(b0: Point2, b1: Point2, b2: Point2, b3: Point2) -> [Point2; 4] {
    [
        ((b0.to_vec() + b1.to_vec() * 4.0 + b2.to_vec()) / 6.0).to_point(),
        ((b1.to_vec() * 2.0 + b2.to_vec()) / 3.0).to_point(),
        ((b2.to_vec() * 2.0 + b1.to_vec()) / 3.0).to_point(),
        ((b1.to_vec() + b2.to_vec() * 4.0 + b3.to_vec()) / 6.0).to_point(),
    ]
}

fn evaluate_bezier(points: &[Point2], t: f32) -> Point2 {
    let mut midpoints_buffer = [MaybeUninit::<Vec2>::uninit(); 10];
    let mut midpoints = {
        for (i, point) in points.iter().copied().enumerate() {
            midpoints_buffer[i].write(point.to_vec());
        }
        unsafe { &mut *(&mut midpoints_buffer[..points.len()] as *mut [_] as *mut [Vec2]) }
    };

    let one_minus_t = 1.0 - t;

    while midpoints.len() > 1 {
        let new_len = midpoints.len() - 1;
        for i in 0..new_len {
            midpoints[i] = midpoints[i] * one_minus_t + midpoints[i + 1] * t
        }
        midpoints = &mut midpoints[..new_len];
    }

    midpoints[0].to_point()
}

// libass/ass_outline.c

// WHAT: I think "normal" space is defined as the space where we're aiming for a
//       1 unit offsfet spline which we then multiply to get our spline in
//       outline space.
//
// NOTE: sqrt(0.5) = 2/sqrt2 = cos(45) = sin(45)
//
// Some vector math:
//   For vectors u, v and the angle between them θ:
//   sin(θ) / sin(θ/2) = 2 * cos(θ/2) = 1.0 / length(u+v)
//
// The following trigonometric identities:
//   cos(θ/2) = sgn(cos(θ/2)) * sqrt((1 + cos(θ)) / 2)
//   sin(θ/2) = sgn(sin(θ/2)) * sqrt((1 - cos(θ)) / 2)

// TODO: Make sqrt(0.5) a const once sqrt is stable in constants
//       ... or just paste the value as a literal.

struct Stroker {
    result_top: OutlineBuilder,
    result_bottom: OutlineBuilder,

    /// Normal vector for [`first_point`](Self::first_point).
    ///
    /// # Note
    /// These normal vectors always should have length 1.
    first_normal: Vec2,
    /// Normal vector for [`last_point`](Self::last_point).
    ///
    /// # Note
    /// These normal vectors always should have length 1.
    last_normal: Vec2,
    first_point: Point2,
    last_point: Point2,

    xbord: f32,
    ybord: f32,
    /// Reciprocal of xbord
    xscale: f32,
    /// Reciprocal of ybord
    yscale: f32,

    /// Maximum allowable approximation error
    eps: f32,

    /// True if no points have been added to the outlines yet.
    contour_start: bool,

    // WHAT: What exactly is this "skip", I'm pretty sure it has to do with the
    //       rounded caps.
    /// Outlines to which the first point **was not** added.
    first_skip: StrokerDir,
    /// Outlines to which the last point **was not** added.
    last_skip: StrokerDir,

    // WHAT: Write documentation for these as I learn what they're really for
    merge_cos: f32,
    /// The maximum value of the cosine for an angle which we want to split
    /// when drawing arcs.
    ///
    /// Arcs larger than 90° will be split into two, therefore compared-to cosine
    /// will never be negative.
    /// Since cosine decreases along with the angle in [0°, 90°] this will
    /// establish a *minimum* angle.
    split_cos: f32,
    min_len: f32,
    err_q: f32,
    err_c: f32,
    err_a: f32,
}

/// A bitmask representing what "direction" (i.e. result_top or result_bottom) to
/// add points to.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct StrokerDir(u8);

impl StrokerDir {
    const NONE: Self = Self(0);
    const UP: Self = Self(1);
    const DOWN: Self = Self(2);
    const ALL: Self = Self(3);

    fn includes(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

impl Debug for StrokerDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            0 => "StrokerDir::NONE",
            1 => "StrokerDir::UP",
            2 => "StrokerDir::DOWN",
            3 => "StrokerDir::ALL",
            _ => "<StrokerDir INVALID>",
        })
    }
}

impl Stroker {
    fn emit_point(
        &mut self,
        point: Point2,
        normalized_offset: Vec2,
        segment: Option<SplineDegree>,
        dir: StrokerDir,
    ) {
        let offset = Vec2::new(
            normalized_offset.x * self.xbord,
            normalized_offset.y * self.ybord,
        );

        if dir.0 != 0 {
            let mut dirstr = String::with_capacity(2);
            if dir.includes(StrokerDir::UP) {
                dirstr.push('+')
            }
            if dir.includes(StrokerDir::DOWN) {
                dirstr.push('-')
            };
            eprintln!(
                "stroker: emitting point (normal={normalized_offset:?}) {point:?}{dirstr}{offset:?}{}",
                if let Some(segment) = segment {
                    format!(" and segment {segment:?}")
                } else {
                    String::new()
                }
            );
        }

        if dir.includes(StrokerDir::UP) {
            self.result_top.add_point(point + offset);
            if let Some(d) = segment {
                self.result_top.add_segment(d);
            }
        }

        if dir.includes(StrokerDir::DOWN) {
            self.result_bottom.add_point(point - offset);
            if let Some(d) = segment {
                self.result_bottom.add_segment(d);
            }
        }
    }

    fn emit_first_point(&mut self, point: Point2, segment: Option<SplineDegree>, dir: StrokerDir) {
        self.last_skip.0 &= !dir.0;
        self.emit_point(point, self.last_normal, segment, dir);
    }

    // TODO: emit_bezier_segment which transforms bezier points to B-spline points.

    fn process_arc(
        &mut self,
        point: Point2,
        normal0: Vec2,
        normal1: Vec2,
        coeffs: &[f32],
        dir: StrokerDir,
    ) {
        // TODO: replace with take_last once stable
        let coeff = coeffs.last().copied().unwrap();

        let center = (normal0 + normal1) * coeff;

        dbg!(center);

        // WHAT: Hopefully this is correct

        if coeffs.len() > 1 {
            self.process_arc(point, normal0, center, &coeffs[..coeffs.len() - 1], dir);
            self.process_arc(point, center, normal1, &coeffs[..coeffs.len() - 1], dir);
        } else {
            self.emit_point(point, normal0, Some(SplineDegree::Quadratic), dir);
            self.emit_point(point, center, None, dir);
        }
    }

    /// Constructs a circular arc between `normal0` and `normal1` anchored at `point`.
    ///
    /// `dir` must contain only a single direction and cannot be zero.
    fn draw_arc(
        &mut self,
        point: Point2,
        normal0: Vec2,
        normal1: Vec2,
        mut cos: f32,
        dir: StrokerDir,
    ) {
        assert!(dir.0.count_ones() == 1);

        /// Max subdivisions to be done when drawing arcs.
        const MAX_SUBDIVISIONS: usize = 15;

        let mut mul = [MaybeUninit::<f32>::uninit(); MAX_SUBDIVISIONS + 1];

        let center: Vec2;
        let mut small_angle = true;
        // If the angle is greater than 90° (i.e. the cosine is smaller than zero)
        // split the arc into two separate arcs between a center normal vector.
        if cos < 0.0 {
            dbg!(cos);
            dbg!(normal0, normal1);
            // FIXME: The common opinion on the internet seems to be that finding the midpoint
            //        vector is usually quicker using linear interpolation and renormalisation
            //        than with the trigonometric methods.
            //        This should be benchmarked and changed if needed.
            // cos(θ/2) = sgn(cos(θ/2)) * sqrt((1 + cos(θ)) / 2)
            // sin(θ/2) = sgn(sin(θ/2)) * sqrt((1 - cos(θ)) / 2)
            //
            // This is sqrt(1/2) premultiplied based on the desired sign of sin(θ/2).
            // Once multiplied by (1 - cos(θ)) this will give the desired sin(θ/2).
            let mul = if dir == StrokerDir::DOWN {
                -(0.5f32.sqrt())
            } else {
                0.5f32.sqrt()
            };

            // This should be equal to 1 / sin(θ/2)
            let mul = mul / (1.0 - cos).sqrt();
            // WHAT: why are we dividing this by sin(θ/2)???? what
            //       the normalization coefficient should be sin(θ) / sin(θ/2)
            center = Vec2::new(normal1.y - normal0.y, normal0.x - normal1.x) * mul;
            // We know cos(θ) is going to be positive, therefore
            // sqrt(1 + cos(θ)) is going to give us cos(θ/2).
            cos = (0.5 + 0.5 * cos).max(0.0).sqrt();
            small_angle = false;
            dbg!(center, cos);
        } else {
            center = Vec2::default();
        }

        let mut subdivisions_left = MAX_SUBDIVISIONS;
        while cos < self.split_cos && subdivisions_left > 0 {
            // 1 / cos(θ/2)
            // WHAT: why is N cos(θ/2) here? N should be 2 * cos(θ/2)
            let cmul = (0.5f32).sqrt() / (1.0 + cos).sqrt();
            mul[subdivisions_left].write(cmul);
            // cos(θ/2)**2 * (1 / cos(0/2)) = cos(θ/2)
            cos = (1.0 + cos) * cmul;
            eprintln!("cmul={cmul} new cos={cos}");
            subdivisions_left -= 1;
        }

        eprintln!("{center:?}");

        // cos²(θ/2)
        mul[subdivisions_left].write((1.0 + cos).recip());
        let mul = unsafe { &*(&mul[subdivisions_left..] as *const [_] as *const [f32]) };

        if small_angle {
            self.process_arc(point, normal0, normal1, mul, dir)
        } else {
            self.process_arc(point, normal0, center, mul, dir);
            self.process_arc(point, center, normal1, mul, dir);
        }
    }

    /// Starts a new segment and adds a circular cap if necessary.
    ///
    /// A circular cap is added if the angle between the normal vector of the
    // WHAT: Figure out how the merge_cos corresponds to the angle and specify
    //       a more exact definition in the doc comment.
    /// previous segment and the current normal vector is too large.
    fn start_segment(&mut self, point: Point2, normal: Vec2, dir: StrokerDir) {
        if self.contour_start {
            self.contour_start = false;
            self.first_normal = normal;
            self.last_normal = normal;
            self.first_point = point;
            self.first_skip = StrokerDir::NONE;
            self.last_skip = StrokerDir::NONE;

            eprintln!(
                "stroker: starting new contour (first point: {point:?}, first normal: {normal:?})",
            );

            return;
        } else {
            eprintln!(
                "stroker: starting new segment (last point: {:?}, last normal: {:?}, first point: {point:?}, first normal: {normal:?})",
                self.last_point,
                self.last_normal
            );
        }

        assert!(self.last_normal.length().abs() < 1.0 + self.eps);
        assert!(normal.length().abs() < 1.0 + self.eps);

        let cos = self.last_normal.dot(normal);
        if cos > self.merge_cos {
            // cos(θ)**2 * sqrt(2)
            let factor = (1.0 + cos).recip();
            self.last_normal = (self.last_normal + normal) * factor;
        } else {
            let previous_normal = self.last_normal;
            self.last_normal = normal;

            let sin = previous_normal.cross(normal);
            // If the current vector is "to the right" of the previous vector
            // then WHAT: are we going to add a cap here?
            let skip = if sin < 0.0 {
                StrokerDir::UP
            } else {
                StrokerDir::DOWN
            };

            if dir.includes(skip) {
                self.emit_point(
                    point,
                    previous_normal,
                    Some(SplineDegree::Linear),
                    StrokerDir(!self.last_skip.0 & skip.0),
                );
                self.emit_point(point, Vec2::ZERO, Some(SplineDegree::Linear), skip)
            }
            self.last_skip = skip;
            // WHAT: Hopefully this is correct

            let dir = StrokerDir(dir.0 & !skip.0);
            if dir.0 != 0 {
                eprintln!("stroker: adding circular cap for direction {dir:?} between {previous_normal:?} and {normal:?} (cos = {cos})");
                self.draw_arc(point, previous_normal, normal, cos, dir);
            }
        }
    }

    fn fix_first_point(&mut self, point: Point2, normalized_offset: Vec2, dir: StrokerDir) {
        let offset = Vec2::new(
            normalized_offset.x * self.xbord,
            normalized_offset.y * self.ybord,
        );

        if dir.includes(StrokerDir::UP) {
            self.result_top.current_contour_points_mut()[0] = point + offset;
        }

        if dir.includes(StrokerDir::DOWN) {
            self.result_bottom.current_contour_points_mut()[0] = point - offset;
        }
    }

    fn add_line(&mut self, p1: Point2, dir: StrokerDir) {
        let dx = p1.x - self.last_point.x;
        let dy = p1.y - self.last_point.y;

        // Ignore lines shorter than eps.
        if dx > -self.eps && dx < self.eps && dy > -self.eps && dy < self.eps {
            return;
        }

        // WHAT: why multiply by yscale and xscale?
        let deriv = Vec2::new(dy * self.yscale, -dx * self.xscale);
        let normal = deriv.normalize();

        eprintln!(
            "stroker: adding line from {:?} to {p1:?} (last normal: {:?}, current normal: {normal:?})",
            self.last_point, self.last_normal
        );

        self.start_segment(self.last_point, normal, dir);
        self.emit_first_point(self.last_point, Some(SplineDegree::Linear), dir);
        self.last_normal = normal;
        self.last_point = p1;
    }

    // WHAT: TODO
    fn close_contour(&mut self, mut dir: StrokerDir) {
        if self.contour_start {
            if dir == StrokerDir::ALL {
                dir = StrokerDir::UP;
            }
            // self.draw_circle(self.last_point, dir);
        } else {
            self.add_line(self.first_point, dir);
            self.start_segment(self.first_point, self.first_normal, dir);
            self.emit_point(
                self.first_point,
                self.first_normal,
                Some(SplineDegree::Linear),
                dir,
            );
            if self.first_normal != self.last_normal {
                self.fix_first_point(
                    self.first_point,
                    self.last_normal,
                    // WHAT: huh
                    StrokerDir(!self.first_skip.0 & dir.0 & !self.last_skip.0),
                );
            }
            self.contour_start = true;
        }

        self.result_top.close_contour();
        self.result_bottom.close_contour();
    }

    pub fn stroke(&mut self, outline: &Outline) -> (Outline, Outline) {
        for segment in outline.segments.iter().copied() {
            let points = outline.points_for_segment(segment);

            if self.contour_start {
                self.last_point = points[0];
            }

            match segment.degree {
                SplineDegree::Linear => self.add_line(points[1], StrokerDir::ALL),
                SplineDegree::Quadratic => todo!(),
                SplineDegree::Cubic => (),
            }

            if segment.end_of_contour {
                self.close_contour(StrokerDir::ALL);
            }
        }

        (
            std::mem::take(&mut self.result_top).build(),
            std::mem::take(&mut self.result_bottom).build(),
        )
    }
}

pub fn stroke(outline: &Outline, x: f32, y: f32, eps: f32) -> (Outline, Outline) {
    let radius = x.max(y);

    assert!(radius >= eps);

    // Error per one unit in normal space
    let relative_err = eps / radius;
    let e = (2.0 * relative_err).sqrt();

    let mut stroker = Stroker {
        result_top: OutlineBuilder::new(),
        result_bottom: OutlineBuilder::new(),

        first_normal: Vec2::default(),
        last_normal: Vec2::default(),
        first_point: Point2::default(),
        last_point: Point2::default(),

        xbord: x,
        ybord: y,
        xscale: x.max(eps).recip(),
        yscale: y.max(eps).recip(),

        eps,

        contour_start: true,
        first_skip: StrokerDir::NONE,
        last_skip: StrokerDir::NONE,

        // WHAT: Explain these as I understand how they are derived
        merge_cos: 1.0 - relative_err,
        split_cos: 1.0 + 8.0 * relative_err - 4.0 * (1.0 + relative_err) * e,
        min_len: relative_err / 4.0,
        err_q: 8.0 * (1.0 + relative_err) * (1.0 + relative_err),
        err_c: 390.0 * relative_err * relative_err,
        err_a: e,
    };

    dbg!(stroker.merge_cos);

    let (top, bottom) = stroker.stroke(outline);

    eprintln!("stroker: stroked outline {outline:?}");
    eprintln!("stroker: result top {top:?}");
    eprintln!("stroker: result bottom {bottom:?}");

    (top, bottom)
}
