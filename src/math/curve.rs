use std::{mem::MaybeUninit, ops::Deref};

use crate::util::{slice_assume_init_mut, ArrayVec};

use super::{Point2, Rect2, Vec2};

const MAX_BEZIER_CONTROL_POINTS: usize = 4;

mod flatten;
mod intersect;
mod solve_x;

pub fn evaluate_bezier(points: &[Point2], t: f32) -> Point2 {
    assert!(points.len() <= MAX_BEZIER_CONTROL_POINTS);

    let mut midpoints_buffer = [MaybeUninit::<Vec2>::uninit(); MAX_BEZIER_CONTROL_POINTS];
    let mut midpoints = {
        unsafe {
            std::ptr::copy_nonoverlapping(
                points.as_ptr(),
                midpoints_buffer.as_mut_ptr() as *mut _,
                points.len(),
            );
            slice_assume_init_mut(&mut midpoints_buffer[..points.len()])
        }
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

pub trait Bezier {
    fn points(&self) -> &[Point2];
    fn sample(&self, t: f32) -> Point2 {
        evaluate_bezier(self.points(), t)
    }

    fn subcurve(&self, t0: f32, t1: f32) -> Self
    where
        Self: Sized;
    fn split_at(&self, t: f32) -> (Self, Self)
    where
        Self: Sized;

    fn flatten(&self, tolerance: f32) -> Vec<Point2> {
        let mut output = vec![self.points()[0]];
        self.flatten_into(tolerance, &mut output);
        output
    }
    fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2>);

    fn bounding_box(&self) -> Rect2 {
        Rect2::bounding_from_points(self.points())
    }
}

macro_rules! define_curve {
    ($name: ident, $npoints: literal) => {
        #[repr(transparent)]
        #[derive(Clone)]
        pub struct $name([Point2; $npoints]);

        impl $name {
            pub const fn new(points: [Point2; $npoints]) -> Self {
                Self(points)
            }

            pub const fn from_ref(points: &[Point2; $npoints]) -> &Self {
                unsafe { &*(points as *const _ as *const Self) }
            }

            pub const fn into_points(self) -> [Point2; $npoints] {
                self.0
            }
        }

        impl Deref for $name {
            type Target = [Point2; $npoints];

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

define_curve!(QuadraticBezier, 3);
define_curve!(CubicBezier, 4);

impl Bezier for QuadraticBezier {
    fn points(&self) -> &[Point2] {
        &self.0
    }

    fn split_at(&self, t: f32) -> (Self, Self)
    where
        Self: Sized,
    {
        let mid = self.sample(t);

        let d = [
            (self[1] - self[0]).to_point(),
            (self[2] - self[1]).to_point(),
        ];

        let vl = super::evaluate_bezier(&d, t).to_vec() * t;
        let pr = mid + vl;

        (
            QuadraticBezier([self[0], vl.to_point(), mid]),
            QuadraticBezier([mid, pr, self[2]]),
        )
    }

    fn subcurve(&self, t0: f32, t1: f32) -> Self
    where
        Self: Sized,
    {
        let from = self.sample(t0);
        let to = self.sample(t1);

        let d = [
            (self[1] - self[0]).to_point(),
            (self[2] - self[1]).to_point(),
        ];

        let dt = t1 - t0;
        let p1 = from + super::evaluate_bezier(&d, t0).to_vec() * dt;

        QuadraticBezier([from, p1, to])
    }

    fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2>) {
        flatten::flatten_quadratic(self, tolerance, output);
    }
}

impl QuadraticBezier {
    pub fn solve_for_t(&self, x: f32, out: &mut ArrayVec<2, f32>) {
        solve_x::quadratic_x_to_t(self, x, out);
    }
}

impl Bezier for CubicBezier {
    fn points(&self) -> &[Point2] {
        &self.0
    }

    fn split_at(&self, t: f32) -> (Self, Self)
    where
        Self: Sized,
    {
        let mid = self.sample(t);

        let d = [
            (self[1] - self[0]).to_point(),
            (self[2] - self[1]).to_point(),
            (self[3] - self[2]).to_point(),
        ];

        let offmid = super::evaluate_bezier(&d, t).to_vec() * t;

        let left = {
            let p1 = d[0].to_vec() * t;
            let p2 = mid - offmid;

            CubicBezier([self[0], p1.to_point(), p2, mid])
        };

        let right = {
            let p1 = mid + offmid;
            let p2 = self[3] - d[2].to_vec() * t;

            CubicBezier([mid, p1, p2, self[3]])
        };

        (left, right)
    }

    fn subcurve(&self, t0: f32, t1: f32) -> Self
    where
        Self: Sized,
    {
        let from = self.sample(t0);
        let to = self.sample(t1);

        let d = [
            (self[1] - self[0]).to_point(),
            (self[2] - self[1]).to_point(),
            (self[3] - self[2]).to_point(),
        ];

        let dt = t1 - t0;
        let p1 = from + super::evaluate_bezier(&d, t0).to_vec() * dt;
        let p2 = to - super::evaluate_bezier(&d, t1).to_vec() * dt;

        CubicBezier([from, p1, p2, to])
    }

    fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2>) {
        flatten::flatten_cubic(self, tolerance, output);
    }
}

impl CubicBezier {
    pub fn to_quadratics(&self, tolerance: f32) -> impl Iterator<Item = QuadraticBezier> + use<'_> {
        flatten::cubic_to_quadratics(self, tolerance)
    }

    pub fn solve_for_t(&self, x: f32, out: &mut ArrayVec<3, f32>) {
        solve_x::cubic_x_to_t(self, x, out);
    }
}

pub use intersect::intersect_curves;
