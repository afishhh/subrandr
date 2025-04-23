use std::{mem::MaybeUninit, ops::Deref};

use crate::util::slice_assume_init_mut;

use super::{Point2f, Vec2f};

const MAX_BEZIER_CONTROL_POINTS: usize = 4;

mod flatten;

pub fn evaluate_bezier(points: &[Point2f], t: f32) -> Point2f {
    assert!(points.len() <= MAX_BEZIER_CONTROL_POINTS);

    let mut midpoints_buffer = [MaybeUninit::<Vec2f>::uninit(); MAX_BEZIER_CONTROL_POINTS];
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
    fn points(&self) -> &[Point2f];
    fn sample(&self, t: f32) -> Point2f {
        evaluate_bezier(self.points(), t)
    }

    fn subcurve(&self, t0: f32, t1: f32) -> Self
    where
        Self: Sized;

    fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2f>);
}

macro_rules! define_curve {
    ($name: ident, $npoints: literal) => {
        #[repr(transparent)]
        #[derive(Clone)]
        pub struct $name(pub [Point2f; $npoints]);

        #[allow(dead_code)]
        impl $name {
            pub const fn new(points: [Point2f; $npoints]) -> Self {
                Self(points)
            }

            pub const fn from_ref(points: &[Point2f; $npoints]) -> &Self {
                unsafe { &*(points as *const _ as *const Self) }
            }
        }

        impl Deref for $name {
            type Target = [Point2f; $npoints];

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

define_curve!(QuadraticBezier, 3);
define_curve!(CubicBezier, 4);

impl Bezier for QuadraticBezier {
    fn points(&self) -> &[Point2f] {
        &self.0
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

        Self([from, p1, to])
    }

    fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2f>) {
        flatten::flatten_quadratic(self, tolerance, output);
    }
}

impl Bezier for CubicBezier {
    fn points(&self) -> &[Point2f] {
        &self.0
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

        Self([from, p1, p2, to])
    }

    fn flatten_into(&self, tolerance: f32, output: &mut Vec<Point2f>) {
        flatten::flatten_cubic(self, tolerance, output);
    }
}

impl CubicBezier {
    #[expect(dead_code)]
    pub fn to_quadratics(&self, tolerance: f32) -> impl Iterator<Item = QuadraticBezier> + use<'_> {
        flatten::cubic_to_quadratics(self, tolerance)
    }

    #[expect(dead_code, reason = "useful for ASS")]
    pub fn from_b_spline(b0: Point2f, b1: Point2f, b2: Point2f, b3: Point2f) -> Self {
        Self([
            ((b0.to_vec() + b1.to_vec() * 4.0 + b2.to_vec()) / 6.0).to_point(),
            ((b1.to_vec() * 2.0 + b2.to_vec()) / 3.0).to_point(),
            ((b1.to_vec() + b2.to_vec() * 2.0) / 3.0).to_point(),
            ((b1.to_vec() + b2.to_vec() * 4.0 + b3.to_vec()) / 6.0).to_point(),
        ])
    }
}
