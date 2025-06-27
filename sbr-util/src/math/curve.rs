use std::{
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

use crate::{
    math::{Number, Point2, Vec2},
    slice_assume_init_mut,
};

use super::Point2f;

const MAX_BEZIER_CONTROL_POINTS: usize = 4;

mod flatten;

pub fn evaluate_bezier<N: Number>(points: &[Point2<N>], t: N) -> Point2<N> {
    assert!(points.len() <= MAX_BEZIER_CONTROL_POINTS);

    let mut midpoints_buffer = [MaybeUninit::<Vec2<N>>::uninit(); MAX_BEZIER_CONTROL_POINTS];
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

    while midpoints.len() > 1 {
        let new_len = midpoints.len() - 1;
        for i in 0..new_len {
            midpoints[i] = midpoints[i] + (midpoints[i + 1] - midpoints[i]) * t;
        }
        midpoints = &mut midpoints[..new_len];
    }

    midpoints[0].to_point()
}

pub trait Bezier<N: Number> {
    fn points(&self) -> &[Point2<N>];
    fn points_mut(&mut self) -> &mut [Point2<N>];
    fn sample(&self, t: N) -> Point2<N> {
        evaluate_bezier(self.points(), t)
    }

    fn subcurve(&self, t0: N, t1: N) -> Self
    where
        Self: Sized;
}

macro_rules! define_curve {
    ($name: ident, $npoints: literal) => {
        #[repr(transparent)]
        #[derive(Debug, Clone)]
        pub struct $name<N: Number>(pub [Point2<N>; $npoints]);

        impl<N: Number> $name<N> {
            pub const fn new(points: [Point2<N>; $npoints]) -> Self {
                Self(points)
            }

            pub const fn from_ref(points: &[Point2<N>; $npoints]) -> &Self {
                unsafe { &*(points as *const _ as *const Self) }
            }
        }

        impl<N: Number> Deref for $name<N> {
            type Target = [Point2<N>; $npoints];

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl<N: Number> DerefMut for $name<N> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

define_curve!(QuadraticBezier, 3);
define_curve!(CubicBezier, 4);

impl<N: Number> Bezier<N> for QuadraticBezier<N> {
    fn points(&self) -> &[Point2<N>] {
        &self.0
    }

    fn points_mut(&mut self) -> &mut [Point2<N>] {
        &mut self.0
    }

    fn subcurve(&self, t0: N, t1: N) -> Self
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

        Self([from, p1, to])
    }
}

impl QuadraticBezier<f32> {
    pub fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2f>) {
        flatten::flatten_quadratic(self, tolerance, output);
    }
}

impl<N: Number> QuadraticBezier<N> {
    pub fn derivative(&self) -> [Point2<N>; 2] {
        let a = self[1] - self[0];
        let b = self[2] - self[1];
        [(a + a).to_point(), (b + b).to_point()]
    }

    pub fn split_at(&self, t: N) -> (Self, Self) {
        let ctrl1 = (self.0[0].to_vec() + (self.0[1] - self.0[0]) * t).to_point();
        let ctrl2 = (self.0[1].to_vec() + (self.0[2] - self.0[1]) * t).to_point();
        let mid = ctrl1.midpoint(ctrl2);
        (
            QuadraticBezier([self.0[0], ctrl1, mid]),
            QuadraticBezier([mid, ctrl2, self.0[2]]),
        )
    }
}

impl<N: Number> Bezier<N> for CubicBezier<N> {
    fn points(&self) -> &[Point2<N>] {
        &self.0
    }

    fn points_mut(&mut self) -> &mut [Point2<N>] {
        &mut self.0
    }

    fn subcurve(&self, t0: N, t1: N) -> Self
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

        Self([from, p1, p2, to])
    }
}

impl CubicBezier<f32> {
    pub fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2f>) {
        flatten::flatten_cubic(self, tolerance, output);
    }

    pub fn to_quadratics(
        &self,
        tolerance: f32,
    ) -> impl Iterator<Item = QuadraticBezier<f32>> + use<'_> {
        flatten::cubic_to_quadratics(self, tolerance)
    }

    pub fn from_b_spline(b0: Point2f, b1: Point2f, b2: Point2f, b3: Point2f) -> Self {
        Self([
            ((b0.to_vec() + b1.to_vec() * 4.0 + b2.to_vec()) / 6.0).to_point(),
            ((b1.to_vec() * 2.0 + b2.to_vec()) / 3.0).to_point(),
            ((b1.to_vec() + b2.to_vec() * 2.0) / 3.0).to_point(),
            ((b1.to_vec() + b2.to_vec() * 4.0 + b3.to_vec()) / 6.0).to_point(),
        ])
    }
}
