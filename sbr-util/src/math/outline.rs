use super::*;

pub trait Outline<N: Number> {
    fn iter(&self) -> impl Iterator<Item = OutlineEvent<N>>;

    fn control_box(&self) -> Rect2<N> {
        self.iter().fold(Rect2::NOTHING, |mut r, e| {
            match e {
                OutlineEvent::MoveTo(p) => r.expand_to_point(p),
                OutlineEvent::LineTo(end) => r.expand_to_point(end),
                OutlineEvent::QuadTo(c0, end) => {
                    r.expand_to_point(c0);
                    r.expand_to_point(end);
                }
                OutlineEvent::CubicTo(c0, c1, end) => {
                    r.expand_to_point(c0);
                    r.expand_to_point(c1);
                    r.expand_to_point(end);
                }
            };
            r
        })
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(clippy::enum_variant_names)]
pub enum OutlineEvent<N> {
    MoveTo(Point2<N>),
    LineTo(Point2<N>),
    QuadTo(Point2<N>, Point2<N>),
    CubicTo(Point2<N>, Point2<N>, Point2<N>),
}

impl<N> OutlineEvent<N> {
    pub fn map<M>(self, mut mapper: impl FnMut(Point2<N>) -> Point2<M>) -> OutlineEvent<M> {
        match self {
            Self::MoveTo(point) => OutlineEvent::MoveTo(mapper(point)),
            Self::LineTo(end) => OutlineEvent::LineTo(mapper(end)),
            Self::QuadTo(c0, end) => OutlineEvent::QuadTo(mapper(c0), mapper(end)),
            Self::CubicTo(c0, c1, end) => {
                OutlineEvent::CubicTo(mapper(c0), mapper(c1), mapper(end))
            }
        }
    }
}

pub trait OutlineIterExt<N: Number>: Iterator<Item = OutlineEvent<N>> {
    fn map_points<M: Number>(
        self,
        mut mapper: impl FnMut(Point2<N>) -> Point2<M>,
    ) -> impl Iterator<Item = OutlineEvent<M>>
    where
        Self: Sized,
    {
        self.map(move |event| event.map(&mut mapper))
    }
}

impl<N: Number, I: Iterator<Item = OutlineEvent<N>>> OutlineIterExt<N> for I {}

pub trait FloatOutlineIterExt: Iterator<Item = OutlineEvent<f32>> + Sized {
    // This is a pain to make a real iterator with the current non-owned flattenning APIs.
    fn visit_flattened_with(
        self,
        mut callback: impl FnMut(Point2<f32>, Point2<f32>),
        quadratic_flatten_tolerance: f32,
        cubic_reduction_tolerance: f32,
    ) {
        let mut previous = Point2::ZERO;
        for event in self {
            match event {
                OutlineEvent::MoveTo(point) => previous = point,
                OutlineEvent::LineTo(end) => {
                    callback(previous, end);
                    previous = end;
                }
                OutlineEvent::QuadTo(c0, end) => {
                    for next in
                        QuadraticBezier([previous, c0, end]).flatten(quadratic_flatten_tolerance)
                    {
                        callback(previous, next);
                        previous = next;
                    }
                }
                OutlineEvent::CubicTo(c0, c1, end) => {
                    for q in CubicBezier([previous, c0, c1, end])
                        .to_quadratics(cubic_reduction_tolerance)
                    {
                        for next in q.flatten(quadratic_flatten_tolerance) {
                            callback(previous, next);
                            previous = next;
                        }
                    }
                }
            }
        }
    }
}

impl<I: Iterator<Item = OutlineEvent<f32>>> FloatOutlineIterExt for I {}

pub struct StaticOutline<N: Number + 'static> {
    #[doc(hidden)]
    pub _control_box: Rect2<N>,
    #[doc(hidden)]
    pub _events: &'static [OutlineEvent<N>],
}

impl<N: Number> Outline<N> for StaticOutline<N> {
    fn iter(&self) -> impl Iterator<Item = OutlineEvent<N>> {
        self._events.iter().copied()
    }

    fn control_box(&self) -> Rect2<N> {
        self._control_box
    }
}

#[macro_export]
macro_rules! make_static_outline {
    {
        $(# $($command: ident $(($x: expr, $y: expr)),+;)*)*
    } => {{
        $crate::math::StaticOutline::<f32> {
            _control_box: const {
                let mut bbox = $crate::math::Rect2::<f32>::NOTHING;
                $($($(
                    {
                        let point = $crate::math::Point2::new($x as f32, $y as f32);
                        bbox.min.x = bbox.min.x.min(point.x);
                        bbox.min.y = bbox.min.y.min(point.y);
                        bbox.max.x = bbox.max.x.max(point.x);
                        bbox.max.y = bbox.max.y.max(point.y);
                    }
                )*)*)*
                bbox
            },
            _events: &const {
                const N_EVENTS: usize =
                    0 $(+ $crate::make_static_outline!(
                        @contour_length(result)
                        $($command $($crate::math::Point2::new($x as f32, $y as f32)),+;)*
                    ))*;
                let mut array = [$crate::math::OutlineEvent::MoveTo($crate::math::Point2::ZERO); N_EVENTS];
                let mut i = 0;

                $($crate::make_static_outline!(
                    @contour(array, i)
                    $($command $($crate::math::Point2::new($x as f32, $y as f32)),+;)*
                );)*

                assert!(i == N_EVENTS);
                array
            }
        }
    }};

    (@contour_length($result: ident) move_to $first: expr; $($command: ident $($p: expr),+;)*) => {
        1
            $(+ $crate::make_static_outline!(@count_command $command $($p),*))*
            + $crate::make_static_outline!(@count_close_contour($first) $($($p),*),*)
    };
    (@count_command $($anything: tt)*) => { 1 };
    (@count_close_contour($first: expr) $next: expr, $($rest: tt)*) => {
        $crate::make_static_outline!(@count_close_contour($first) $($rest)*);
    };
    (@count_close_contour($first: expr) $last: expr) => {
        if const { $first.x != $last.x || $first.y != $last.y } { 1 } else { 0 }
    };


    (@contour($result: ident, $i: ident) move_to $first: expr; $($command: ident $($p: expr),+;)*) => {
        $result[$i] = $crate::math::OutlineEvent::MoveTo($first);
        $i += 1;
        $(
            $result[$i] = $crate::make_static_outline!(@instruction $command $($p),*);
            $i += 1;
        )*
        $crate::make_static_outline!(@close_contour($result, $i, $first) $($($p),*),*)
    };
    (@close_contour($result: ident, $i: ident, $first: expr) $next: expr, $($rest: tt)*) => {
        $crate::make_static_outline!(@close_contour($result, $i, $first) $($rest)*);
    };
    (@close_contour($result: ident, $i: ident, $first: expr) $last: expr) => {
        if const { $first.x != $last.x || $first.y != $last.y } {
            $result[$i] = $crate::math::OutlineEvent::LineTo($first);
            $i += 1;
        }
    };

    (@instruction line_to $end: expr) => {
        $crate::math::OutlineEvent::LineTo($end)
    };
    (@instruction quad_to $c0: expr, $end: expr) => {
        $crate::math::OutlineEvent::QuadTo($c0, $end)
    };
    (@instruction cubic_to $c0: expr, $c1: expr, $end: expr) => {
        $crate::math::OutlineEvent::CubicTo($c0, $c1, $end)
    };
}
