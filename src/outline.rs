use thiserror::Error;

use crate::util::math::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplineDegree {
    Linear = 1,
    Quadratic = 2,
    Cubic = 3,
}

#[derive(Debug, Clone)]
pub struct Outline {
    pub points: Vec<Point2>,
    pub segments: Vec<(SplineDegree, usize)>,
}

impl Outline {
    pub fn new(point: Point2) -> Self {
        Self {
            points: vec![point],
            segments: Vec::new(),
        }
    }

    pub fn from_cubic_bezier(p0: Point2, p1: Point2, p2: Point2, p3: Point2) -> Self {
        Self {
            points: cubic_bezier_to_bspline(p0, p1, p2, p3).to_vec(),
            segments: vec![(SplineDegree::Cubic, 4)],
        }
    }

    pub fn segments(&self) -> usize {
        self.segments.len()
    }

    fn evaluate_segment_normalized(&self, i: usize, t: f32) -> Point2 {
        let (degree, end) = self.segments[i];
        let start = i.checked_sub(1).map(|x| self.segments[x].1).unwrap_or(0);

        evaluate_uniform_bspline(
            &self.points[start..end],
            degree as u32 as f32 + t * (end - start - degree as usize) as f32,
            degree as u32,
        )
    }

    pub fn evaluate(&self, t: f32) -> Point2 {
        self.evaluate_segment_normalized(t.trunc() as usize, t.fract())
    }

    pub fn calculate_control_box(&self, bb: &mut BoundingBox) {
        for point in self.points.iter() {
            bb.add(point);
        }
    }

    pub fn push_spline(&mut self, degree: SplineDegree) -> SplineBuilder<'_> {
        SplineBuilder {
            degree,
            previous_points: self.points.len(),
            parent: self,
        }
    }
}

pub struct SplineBuilder<'a> {
    parent: &'a mut Outline,
    degree: SplineDegree,
    previous_points: usize,
}

#[derive(Debug, Error)]
#[error("Not enough points added to SplineBuilder for spline of this degree")]
pub struct TooFewPointsError(());

impl SplineBuilder<'_> {
    pub fn add_point_mut(&mut self, x: Point2) -> &mut Self {
        self.parent.points.push(x);

        self
    }

    pub fn add_point(mut self, x: Point2) -> Self {
        self.add_point_mut(x);

        self
    }

    pub fn finish(self) -> Result<(), TooFewPointsError> {
        if self.parent.points.len() - self.previous_points < self.degree as usize {
            return Err(TooFewPointsError(()));
        }

        self.parent
            .segments
            .push((self.degree, self.parent.points.len()));

        std::mem::forget(self);

        Ok(())
    }
}

impl Drop for SplineBuilder<'_> {
    fn drop(&mut self) {
        self.parent.points.truncate(self.previous_points);
    }
}

#[inline]
fn cubic_bezier_to_bspline(p0: Point2, p1: Point2, p2: Point2, p3: Point2) -> [Point2; 4] {
    [
        (p0.to_vec() * 6.0 - p1.to_vec() * 7.0 + p2.to_vec() * 2.0).to_point(),
        (p1.to_vec() * 2.0 - p2.to_vec()).to_point(),
        (p2.to_vec() * 2.0 - p1.to_vec()).to_point(),
        (p3.to_vec() * 6.0 - p2.to_vec() * 7.0 + p1.to_vec() * 2.0).to_point(),
    ]
}

/// An implementation of the recursive definition of the B-spline
#[expect(dead_code)]
fn b(i: usize, p: u32, t: f32) -> f32 {
    if p == 0 {
        if i as f32 <= t && t < (i + 1) as f32 {
            return 1.0;
        } else {
            return 0.0;
        }
    } else {
        let lhs = (t - i as f32) / ((i + p as usize) as f32 - i as f32) * b(i, p - 1, t);
        let rhs = ((i + p as usize + 1) as f32 - t)
            / ((i + p as usize + 1) as f32 - (i + 1) as f32)
            * b(i + 1, p - 1, t);
        return lhs + rhs;
    }
}

/// An implementation of [De Boor's algorithm](https://en.wikipedia.org/wiki/De_Boor%27s_algorithm) for evaluating a B-spline.
fn de_boor(k: usize, t: f32, points: &[Point2], degree: u32) -> Point2 {
    let degree = degree as usize;
    let mut d = Box::<[Vec2]>::new_uninit_slice(degree as usize + 1);

    for j in 0..=degree as usize {
        d[j].write(points[j + k - degree as usize].to_vec());
    }

    let mut d = unsafe { d.assume_init() };

    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let alpha = (t - (j + k - degree) as f32) / ((j + 1 + k - r) - (j + k - degree)) as f32;
            d[j] = d[j - 1] * (1.0 - alpha) + d[j] * alpha;
        }
    }

    d[degree].to_point()
}

fn evaluate_uniform_bspline(points: &[Point2], t: f32, degree: u32) -> Point2 {
    // let result = Point2::default()
    //     + points
    //         .iter()
    //         .enumerate()
    //         .map(|(i, p)| {
    //             let coeff = b(i, degree as u32, t);
    //             p.to_vec() * coeff
    //         })
    //         .sum::<Vec2>();

    de_boor(t.trunc() as usize, t, points, degree)
}

// for testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveKind {
    BSpline,
    CubicBezier,
}

impl CurveKind {
    pub fn matrix(self) -> &'static [[f32; 4]; 4] {
        match self {
            CurveKind::BSpline => &[
                [-1., 3., -3., 1.], // this
                [3., -6., 3., 0.],  // is
                [-3., 0., 3., 0.],  // for
                [1., 4., 1., 0.],   // rustfmt
            ],
            CurveKind::CubicBezier => &[
                [1., 0., 0., 0.],   //
                [-3., 3., 0., 0.],  //
                [3., -6., 3., 0.],  //
                [-1., 3., -3., 1.], //
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Curve {
    pub kind: CurveKind,
    pub control_points: [Point2; 4],
}

impl Curve {
    pub fn evaluate(&self, t: f32) -> Point2 {
        assert!(0.0 <= t && t <= 1.0);

        let time_vector = [1.0, t, t * t, t * t * t];
        let coefficients = self.kind.matrix();
        let point_vector = &self.control_points;

        let mut rhs_vector = [Vec2::default(); 4];

        for (i, row) in coefficients.iter().enumerate() {
            rhs_vector[i] = row
                .iter()
                .enumerate()
                .map(|(j, value)| point_vector[j].to_vec() * *value)
                .sum::<Vec2>();
        }

        self.control_points[0]
            + time_vector
                .iter()
                .zip(rhs_vector.iter())
                .map(|(a, b)| *b * *a)
                .sum::<Point2>()
                .to_vec()
    }
}
