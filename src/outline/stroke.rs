// This file is mostly a port of the stroker from libass/ass_outline.c.
// The license header from that file is as follows:

/*
 * Copyright (C) 2016 Vabishchevich Nikolay <vabnick@gmail.com>
 *
 * This file is part of libass.
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

// Additional notes:
// libass's stroker produces self intersections, FreeType avoids this in the
// ft_stroker_inside function.
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

use std::{mem::MaybeUninit, ops::BitAnd};

use crate::util::array_assume_init_ref;

use super::*;

const STROKER_PRINT_DEBUG: bool = false;

struct Stroker {
    result_top: OutlineBuilder,
    result_bottom: OutlineBuilder,

    /// Normal vector for [`first_point`](Self::first_point).
    first_normal: Vec2f,
    /// Normal vector for [`last_point`](Self::last_point).
    last_normal: Vec2f,
    first_point: Point2f,
    last_point: Point2f,

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

    const fn includes(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

impl BitAnd for StrokerDir {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
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

// Why does this normal store a length?
#[derive(Debug)]
struct WeirdNormal {
    v: Vec2f,
    len: f32,
}

impl WeirdNormal {
    const fn new(v: Vec2f, len: f32) -> Self {
        Self { v, len }
    }
}

impl Stroker {
    fn emit_point(
        &mut self,
        point: Point2f,
        normal: Vec2f,
        segment: Option<SegmentDegree>,
        dir: StrokerDir,
    ) {
        let offset = Vec2f::new(normal.x * self.xbord, normal.y * self.ybord);

        if STROKER_PRINT_DEBUG && dir.0 != 0 {
            let mut dirstr = String::with_capacity(2);
            if dir.includes(StrokerDir::UP) {
                dirstr.push('+')
            }
            if dir.includes(StrokerDir::DOWN) {
                dirstr.push('-')
            };
            eprintln!(
                "stroker: emitting point (normal={normal:?}) {point:?}{dirstr}{offset:?}{}",
                segment.map_or_else(String::new, |segment| format!(" and segment {segment:?}"))
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

    fn emit_first_point(
        &mut self,
        point: Point2f,
        segment: Option<SegmentDegree>,
        dir: StrokerDir,
    ) {
        self.last_skip.0 &= !dir.0;
        self.emit_point(point, self.last_normal, segment, dir);
    }

    fn process_arc(
        &mut self,
        point: Point2f,
        normal0: Vec2f,
        normal1: Vec2f,
        coeffs: &[f32],
        dir: StrokerDir,
    ) {
        // TODO: replace with take_last once stable
        let coeff = coeffs.last().copied().unwrap();

        let center = (normal0 + normal1) * coeff;

        // WHAT: Hopefully this is correct

        if coeffs.len() > 1 {
            self.process_arc(point, normal0, center, &coeffs[..coeffs.len() - 1], dir);
            self.process_arc(point, center, normal1, &coeffs[..coeffs.len() - 1], dir);
        } else {
            self.emit_point(point, normal0, Some(SegmentDegree::Quadratic), dir);
            self.emit_point(point, center, None, dir);
        }
    }

    /// Constructs a circular arc between `normal0` and `normal1` anchored at `point`.
    ///
    /// `dir` must contain only a single direction and cannot be zero.
    fn draw_arc(
        &mut self,
        point: Point2f,
        normal0: Vec2f,
        normal1: Vec2f,
        mut cos: f32,
        dir: StrokerDir,
    ) {
        assert!(dir.0.count_ones() == 1);

        /// Max subdivisions to be done when drawing arcs.
        const MAX_SUBDIVISIONS: usize = 15;

        let mut mul = [MaybeUninit::<f32>::uninit(); MAX_SUBDIVISIONS + 1];

        let center: Vec2f;
        let small_angle = cos >= 0.0;
        // If the angle is greater than 90° (i.e. the cosine is smaller than zero)
        // split the arc into two separate arcs between a center normal vector.
        if !small_angle {
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
            center = Vec2f::new(normal1.y - normal0.y, normal0.x - normal1.x) * mul;
            // We know cos(θ) is going to be positive, therefore
            // sqrt(1 + cos(θ)) is going to give us cos(θ/2).
            cos = (0.5 + 0.5 * cos).max(0.0).sqrt();
        } else {
            center = Vec2f::default();
        }

        let mut subdivisions_left = MAX_SUBDIVISIONS;
        while cos < self.split_cos && subdivisions_left > 0 {
            // 1 / cos(θ/2)
            // WHAT: why is N cos(θ/2) here? N should be 2 * cos(θ/2)
            let cmul = (0.5f32).sqrt() / (1.0 + cos).sqrt();
            mul[subdivisions_left].write(cmul);
            // cos(θ/2)**2 * (1 / cos(0/2)) = cos(θ/2)
            cos = (1.0 + cos) * cmul;
            subdivisions_left -= 1;
        }

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
    fn start_segment(&mut self, point: Point2f, normal: Vec2f, dir: StrokerDir) {
        if self.contour_start {
            self.contour_start = false;
            self.first_normal = normal;
            self.last_normal = normal;
            self.first_point = point;
            self.first_skip = StrokerDir::NONE;
            self.last_skip = StrokerDir::NONE;

            if STROKER_PRINT_DEBUG {
                eprintln!(
                "stroker: starting new contour (first point: {point:?}, first normal: {normal:?})",
            );
            }

            return;
        } else if STROKER_PRINT_DEBUG {
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
                    Some(SegmentDegree::Linear),
                    StrokerDir(!self.last_skip.0 & skip.0),
                );
                self.emit_point(point, Vec2f::ZERO, Some(SegmentDegree::Linear), skip)
            }
            self.last_skip = skip;
            // WHAT: Hopefully this is correct

            let dir = StrokerDir(dir.0 & !skip.0);
            if dir.0 != 0 {
                if STROKER_PRINT_DEBUG {
                    eprintln!("stroker: adding circular cap for direction {dir:?} between {previous_normal:?} and {normal:?} (cos = {cos})");
                }
                self.draw_arc(point, previous_normal, normal, cos, dir);
            }
        }
    }

    fn fix_first_point(&mut self, point: Point2f, normalized_offset: Vec2f, dir: StrokerDir) {
        let offset = Vec2f::new(
            normalized_offset.x * self.xbord,
            normalized_offset.y * self.ybord,
        );

        if dir.includes(StrokerDir::UP) {
            self.result_top.contour_points_mut()[0] = point + offset;
        }

        if dir.includes(StrokerDir::DOWN) {
            self.result_bottom.contour_points_mut()[0] = point - offset;
        }
    }

    fn is_epsilon_vec(&self, v: Vec2f) -> bool {
        v.x > -self.eps && v.x < self.eps && v.y > -self.eps && v.y < self.eps
    }

    // FIXME: Lines may result in self-intersections!!
    fn add_line(&mut self, p1: Point2f, dir: StrokerDir) {
        let d = p1 - self.last_point;

        // Ignore lines shorter than eps.
        if self.is_epsilon_vec(d) {
            return;
        }

        // Scaled perpendicular to current line
        let deriv = Vec2f::new(d.y * self.yscale, -d.x * self.xscale);
        let normal = deriv.normalize();

        if STROKER_PRINT_DEBUG {
            eprintln!(
            "stroker: adding line from {:?} to {p1:?} (last normal: {:?}, current normal: {normal:?})",
            self.last_point, self.last_normal
        );
        }

        self.start_segment(self.last_point, normal, dir);
        self.emit_first_point(self.last_point, Some(SegmentDegree::Linear), dir);
        self.last_normal = normal;
        self.last_point = p1;
    }

    fn prepare_skip(&mut self, point: Point2f, dir: StrokerDir, first: bool) {
        if first {
            self.first_skip.0 |= dir.0;
        }
        self.emit_point(
            point,
            self.last_normal,
            Some(SegmentDegree::Linear),
            StrokerDir(!self.last_skip.0 & dir.0),
        );
        self.last_skip.0 |= dir.0;
    }

    // WHAT: quadratic
    /// Returns optimal offset for quadratic bezier control point
    /// or None if the error is too large.
    fn estimate_quadratic_error(
        &self,
        cos: f32,
        sin: f32,
        // WHAT: these normals have a len??
        normals: &[WeirdNormal; 2],
    ) -> Option<Vec2f> {
        // WHAT: quadratic
        if (3. + cos) * (3. + cos) >= self.err_q * (1. + cos) {
            return None;
        }

        // sqrt(2/cos(θ/2))
        let mul = (1.0 + cos).recip();
        let l0 = 2.0 * normals[0].len;
        let l1 = 2.0 * normals[1].len;
        let dot0 = l0 + normals[1].len * cos;
        let crs0 = (l0 * mul - normals[1].len) * sin;

        let dot1 = l1 + normals[0].len * cos;
        let crs1 = (l1 * mul - normals[0].len) * sin;

        if crs0.abs() >= self.err_a * dot0 || crs1.abs() >= self.err_a * dot1 {
            return None;
        }

        Some(Vec2f::new(
            (normals[0].v.x + normals[1].v.x) * mul,
            (normals[0].v.y + normals[1].v.y) * mul,
        ))
    }

    // WHAT: quadratic
    fn process_quadratic(
        &mut self,
        points: &[Point2f; 3],
        deriv: &[Vec2f; 2],
        normals: &[WeirdNormal; 2],
        mut dir: StrokerDir,
        first: bool,
    ) {
        if STROKER_PRINT_DEBUG {
            eprintln!("stroker: process quadratic {points:?} {deriv:?} {normals:?}");
        }

        let cos = normals[0].v.dot(normals[1].v);
        let sin = normals[0].v.cross(normals[1].v);

        let mut check_dir = dir;
        let skip_dir = if sin < 0.0 {
            StrokerDir::UP
        } else {
            StrokerDir::DOWN
        };

        if dir.includes(skip_dir) {
            let abs_sin = sin.abs();
            let f0 = normals[0].len * cos + normals[1].len;
            let f1 = normals[1].len * cos + normals[0].len;
            let g0 = normals[0].len * abs_sin;
            let g1 = normals[1].len * abs_sin;

            if f0 < abs_sin && f1 < abs_sin {
                let d2 = (f0 * normals[1].len + f1 * normals[0].len) / 2.0;
                if d2 < g0 && d2 < g1 {
                    self.prepare_skip(points[0], skip_dir, first);
                    if f0 < 0.0 || f1 < 0.0 {
                        self.emit_point(
                            points[0],
                            Vec2f::ZERO,
                            Some(SegmentDegree::Linear),
                            skip_dir,
                        );
                        self.emit_point(
                            points[2],
                            Vec2f::ZERO,
                            Some(SegmentDegree::Linear),
                            skip_dir,
                        );
                    } else {
                        let mul = f0 / abs_sin;
                        let offs = normals[0].v * mul;
                        self.emit_point(points[0], offs, Some(SegmentDegree::Linear), skip_dir);
                    }
                    dir.0 &= !skip_dir.0;
                    if dir.0 == 0 {
                        self.last_normal = normals[1].v;
                        return;
                    }
                }
                check_dir.0 ^= skip_dir.0;
            } else if cos + g0 < 1.0 && cos + g1 < 1.0 {
                check_dir.0 ^= skip_dir.0;
            }
        }

        if let Some(Some(offset)) =
            (check_dir.0 != 0).then(|| self.estimate_quadratic_error(cos, sin, normals))
        {
            self.emit_first_point(points[0], Some(SegmentDegree::Quadratic), check_dir);
            self.emit_point(points[1], offset, None, check_dir);
            dir.0 &= !check_dir.0;
            if dir.0 == 0 {
                self.last_normal = normals[1].v;
                return;
            }
        }

        let mut next = [MaybeUninit::<Point2f>::uninit(); 5];
        next[1].write(points[0] + points[1].to_vec());
        next[3].write(points[1] + points[2].to_vec());
        unsafe {
            next[2].write(
                (((next[1].assume_init().to_vec() + next[3].assume_init().to_vec())
                    + Vec2f::new(0.0, 0.0))
                    * 0.25)
                    .to_point(),
            );
            *next[1].assume_init_mut() = (next[1].assume_init().to_vec() * 0.5).to_point();
            *next[3].assume_init_mut() = (next[3].assume_init().to_vec() * 0.5).to_point();
        }
        next[0].write(points[0]);
        next[4].write(points[2]);

        let mut next_deriv = [MaybeUninit::<Vec2f>::uninit(); 3];
        next_deriv[0].write(deriv[0] * 0.5);
        next_deriv[2].write(deriv[1] * 0.5);
        next_deriv[1]
            .write(unsafe { next_deriv[0].assume_init() + next_deriv[2].assume_init() } * 0.5);

        let next = unsafe { array_assume_init_ref(&next) };
        let next_deriv = unsafe { array_assume_init_ref(&next_deriv) };

        let len = next_deriv[1].length();
        if len < self.min_len {
            self.emit_first_point(next[0], Some(SegmentDegree::Linear), dir);
            self.start_segment(next[2], normals[1].v, dir);
            self.last_skip.0 &= !dir.0;
            self.emit_point(next[2], normals[1].v, Some(SegmentDegree::Linear), dir);
            return;
        }

        let scale = 1.0 / len;
        let next_normal = [
            WeirdNormal::new(normals[0].v, normals[0].len / 2.0),
            WeirdNormal::new(next_deriv[1] * scale, len),
            WeirdNormal::new(normals[1].v, normals[1].len / 2.0),
        ];

        unsafe {
            self.process_quadratic(
                next[..3].try_into().unwrap_unchecked(),
                next_deriv[..2].try_into().unwrap_unchecked(),
                next_normal[..2].try_into().unwrap_unchecked(),
                dir,
                first,
            );
            self.process_quadratic(
                next[2..].try_into().unwrap_unchecked(),
                next_deriv[1..].try_into().unwrap_unchecked(),
                next_normal[1..].try_into().unwrap_unchecked(),
                dir,
                false,
            );
        }
    }

    fn add_quadratic(&mut self, p1: Point2f, p2: Point2f, dir: StrokerDir) {
        let d0 = p1 - self.last_point;

        if self.is_epsilon_vec(d0) {
            self.add_line(p2, dir);
            return;
        }

        let d1 = p2 - p1;

        if self.is_epsilon_vec(d1) {
            self.add_line(p2, dir);
            return;
        }

        let points = [self.last_point, p1, p2];
        self.last_point = p2;

        let deriv = [
            Vec2f::new(d0.y * self.yscale, -d0.x * self.xscale),
            Vec2f::new(d1.y * self.yscale, -d1.x * self.xscale),
        ];

        let len0 = deriv[0].length();
        let scale0 = len0.recip();
        let len1 = deriv[1].length();
        let scale1 = len1.recip();
        let normals = [
            WeirdNormal::new(deriv[0] * scale0, len0),
            WeirdNormal::new(deriv[1] * scale1, len1),
        ];

        let first = self.contour_start;
        self.start_segment(points[0], normals[0].v, dir);
        self.process_quadratic(&points, &deriv, &normals, dir, first);
    }

    // FIXME: Probably doesn't handle all the self intersection stuff...
    fn add_cubic(&mut self, p1: Point2f, p2: Point2f, p3: Point2f, dir: StrokerDir) {
        let curve = CubicBezier::new([self.last_point, p1, p2, p3]);
        for quadratic in curve.to_quadratics(0.01) {
            self.add_quadratic(quadratic[1], quadratic[2], dir);
        }
    }

    // WHAT: TODO
    // TODO: contour_start case
    fn close_contour(&mut self, mut dir: StrokerDir) {
        if self.contour_start {
            if dir == StrokerDir::ALL {
                dir = StrokerDir::UP;
            }
            todo!("contour_start case in stroker close_contour {dir:?}")
            // self.draw_circle(self.last_point, dir);
        } else {
            self.add_line(self.first_point, dir);
            self.start_segment(self.first_point, self.first_normal, dir);
            self.emit_point(
                self.first_point,
                self.first_normal,
                Some(SegmentDegree::Linear),
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
                SegmentDegree::Linear => self.add_line(points[1], StrokerDir::ALL),
                SegmentDegree::Quadratic => {
                    self.add_quadratic(points[1], points[2], StrokerDir::ALL)
                }
                SegmentDegree::Cubic => {
                    self.add_cubic(points[1], points[2], points[3], StrokerDir::ALL)
                }
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

        first_normal: Vec2f::default(),
        last_normal: Vec2f::default(),
        first_point: Point2f::default(),
        last_point: Point2f::default(),

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
        err_a: e,
    };

    let (top, bottom) = stroker.stroke(outline);

    if STROKER_PRINT_DEBUG {
        eprintln!("stroker: stroked outline {outline:?}");
        eprintln!("stroker: result top {top:?}");
        eprintln!("stroker: result bottom {bottom:?}");
    }

    (top, bottom)
}
