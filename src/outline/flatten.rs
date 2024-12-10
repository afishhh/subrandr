///! An implementation of (this algorithm)[https://raphlinus.github.io/graphics/curves/2019/12/23/flatten-quadbez.html].
///! Cubic curves are converted into quadratics using (this algorithm)[https://web.archive.org/web/20150403003715/http://www.caffeineowl.com/graphics/2d/vectorial/cubic2quad01.html], found via (this post)[https://minus-ze.ro/posts/flattening-bezier-curves-and-arcs/].
use super::{Outline, Point2, Segment};

/// Does not include the first point of the segment in the flattened version
pub fn flatten(outline: &Outline, segment: Segment, tolerance: f32, out: &mut Vec<Point2>) {
    let points = outline.points_for_segment(segment);
    match points.len() {
        2 => out.extend_from_slice(points),
        3 => flatten_quadratic(points.try_into().unwrap(), tolerance, out),
        4 => {
            for quadratic in cubic_to_quadratics(points.try_into().unwrap(), tolerance) {
                flatten_quadratic(&quadratic, tolerance, out)
            }
        }
        _ => unreachable!(),
    }
}

struct Basic {
    x0: f32,
    x2: f32,
    scale: f32,
}

/// Map a quadratic bezier to a scaled, translated and rotated segment of y=x^2
/// (map_to_basic)[https://github.com/raphlinus/raphlinus.github.io/blob/main/_posts/2019-12-23-flatten-quadbez.md]
fn map_to_basic(a: Point2, b: Point2, c: Point2) -> Basic {
    // (b.x - a.x) + (b.x - c.x)
    let ddx = 2.0 * b.x - a.x - c.x;
    // (b.y - a.y) + (b.y - c.y)
    let ddy = 2.0 * b.y - a.y - c.y;
    let u0 = (b.x - a.x) * ddx + (b.y - a.y) * ddy;
    let u2 = (c.x - b.x) * ddx + (c.y - b.y) * ddy;
    let cross = (c.x - a.x) * ddy - (c.y - a.y) * ddx;
    let x0 = u0 / cross;
    let x2 = u2 / cross;
    let scale = cross.abs() / (ddx.hypot(ddy) * (x2 - x0).abs());
    Basic { x0, x2, scale }
}

// integral((1 + 4x²)**-0.25)
fn approximate_segments_integral(x: f32) -> f32 {
    let d: f32 = 0.67;
    x / (1.0 - d + (d.powi(4) + 0.25 * x * x).powf(0.25))
}

fn approximate_inverse_segments_integral(x: f32) -> f32 {
    let b: f32 = 0.39;
    x * (1.0 - b + (b * b + 0.25 * x * x).sqrt())
}

fn flatten_quadratic(points: &[Point2; 3], tolerance: f32, out: &mut Vec<Point2>) {
    let basic = map_to_basic(points[0], points[1], points[2]);
    let a0 = approximate_segments_integral(basic.x0);
    let a2 = approximate_segments_integral(basic.x2);
    let count = 0.5 * (a2 - a0).abs() * (basic.scale / tolerance).sqrt();
    let count = count.ceil();
    let x0 = approximate_inverse_segments_integral(a0);
    let x2 = approximate_inverse_segments_integral(a2);

    for i in 1..count as u32 {
        let x = approximate_inverse_segments_integral(a0 + ((a2 - a0) * i as f32) / count);
        let t = (x - x0) / (x2 - x0);
        out.push(super::evaluate_bezier(points, t));
    }
    out.push(points[2]);
}

fn cubic_bezier_subcurve(points: &[Point2; 4], t0: f32, t1: f32) -> [Point2; 4] {
    let from = super::evaluate_bezier(points, t0);
    let to = super::evaluate_bezier(points, t1);

    let d = [
        (points[1] - points[0]).to_point(),
        (points[2] - points[1]).to_point(),
        (points[3] - points[2]).to_point(),
    ];

    let dt = t1 - t0;
    let p1 = from + super::evaluate_bezier(&d, t0).to_vec() * dt;
    let p2 = to - super::evaluate_bezier(&d, t1).to_vec() * dt;

    [from, p1, p2, to]
}

fn naive_cubic_to_quadratic(points: &[Point2; 4]) -> [Point2; 3] {
    let c1_2 = points[1].to_vec() * 3.0 - points[0].to_vec();
    let c2_2 = points[2].to_vec() * 3.0 - points[3].to_vec();

    [points[0], ((c1_2 + c2_2) * 0.25).to_point(), points[3]]
}

fn quadratic_count_for_cubic(points: &[Point2; 4], tolerance: f32) -> f32 {
    let p = points[0].to_vec() - points[1].to_vec() * 3.0 + points[2].to_vec() * 3.0
        - points[3].to_vec();
    let err = p.length_sq();

    let result = err / (432.0 * tolerance * tolerance);
    result.powf(1.0 / 6.0).ceil().max(1.0)
}

pub fn cubic_to_quadratics(
    points: &[Point2; 4],
    tolerance: f32,
) -> impl Iterator<Item = [Point2; 3]> + use<'_> {
    let count = quadratic_count_for_cubic(points, tolerance);
    let step = 1.0 / count as f32;

    let mut t = 0.0;
    (0..count as u32).map(move |_| {
        let tnext = t + step;
        let cubic = cubic_bezier_subcurve(points, t, tnext);
        t = tnext;
        naive_cubic_to_quadratic(&cubic)
    })
}
