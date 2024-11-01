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

    #[inline(always)]
    pub fn nsegments(&self) -> usize {
        self.segments.len()
    }

    #[inline(always)]
    pub fn has_segments(&self) -> bool {
        !self.segments.is_empty()
    }

    fn evaluate_segment_normalized(&self, i: usize, t: f32) -> Point2 {
        let (degree, end) = self.segments[i];
        let start = i
            .checked_sub(1)
            .map(|x| self.segments[x].1 - 1)
            .unwrap_or(0);

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

    pub fn push_line_point(&mut self, next: Point2) {
        self.push_spline(SplineDegree::Linear)
            .add_point(next)
            .finish()
            .unwrap()
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

        if let Some(npoints) = self
            .parent
            .segments
            .last_mut()
            .and_then(|(degree, npoints)| (*degree == self.degree).then_some(npoints))
        {
            *npoints = self.parent.points.len();
        } else {
            self.parent
                .segments
                .push((self.degree, self.parent.points.len()));
        }

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
    de_boor(t.trunc() as usize, t, points, degree)
}
