use crate::{math::*, util::ArrayVec};

fn push_root<const N: usize>(out: &mut ArrayVec<N, f32>, t: f32) {
    if t >= 0.0 && t <= 1.0 {
        out.push(t);
    } else {
        println!("root: {t:?} out of range");
    }
}

pub fn quadratic_x_to_t(quadr: &QuadraticBezier, x: f32, roots: &mut ArrayVec<2, f32>) {
    let a = quadr[2].x - 2.0 * quadr[1].x + quadr[0].x;
    let b = -2.0 * quadr[0].x + 2.0 * quadr[1].x;
    let c = quadr[0].x - x;

    let det = b * b - 4.0 * a * c;
    let detr = det.sqrt();
    if detr == 0.0 {
        push_root(roots, -b / (2.0 * a));
    } else if !detr.is_nan() {
        let a2 = 2.0 * a;
        push_root(roots, (-b - detr) / a2);
        push_root(roots, (-b + detr) / a2);
    }
}

// TODO: Check if this is any faster or more accurate than a simple heuristic solution
// Probably not more accurate since the values skyrocket
pub fn cubic_x_to_t(cubic: &CubicBezier, x: f32, roots: &mut ArrayVec<3, f32>) {
    // These polynomials can be acquired in the following way:
    // - Expand all bernstein basis polynomials of the desired degree
    // - Substitute x**i = points[n-i].x
    // Then you have to solve for the roots of nth order polynomial with
    // coefficients corresponding to the polynomials from earlier steps but with
    // an additional "- x" term.
    let a = (-cubic[0].x + 3.0 * cubic[1].x - 3.0 * cubic[2].x + cubic[3].x) as f64;
    let b = (3.0 * cubic[0].x - 6.0 * cubic[1].x + 3.0 * cubic[2].x) as f64;
    let c = (-3.0 * cubic[0].x + 3.0 * cubic[1].x) as f64;
    let d = (cubic[0].x - x) as f64;

    println!("{a}x**3 + {b}x**2 + {c}x + {d}");
    solve_cubic(a, b, c, d, |r| push_root(roots, dbg!(r) as f32));
}
