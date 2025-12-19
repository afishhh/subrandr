use util::math::{Point2, Vec2};

use crate::scene::{FixedS, Rect2S};

#[derive(Debug, Clone, Copy)]
struct Quad {
    x: u16,
    y: u16,
    order: u8,
    n_rects: u16,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct QuadRect {
    pub(super) rect: Rect2S,
    pub(super) id: u16,
    pub(super) z: u16,
}

pub(super) struct RectTiler {
    rects: Vec<QuadRect>,
    queue: Vec<Quad>,
    tile_size: Vec2<u16>,
}

impl RectTiler {
    pub(super) fn new() -> Self {
        Self {
            rects: Vec::new(),
            queue: Vec::new(),
            tile_size: Vec2::ZERO,
        }
    }

    pub(super) fn add(&mut self, rect: QuadRect) {
        debug_assert!(self.queue.is_empty());

        self.rects.push(rect);
    }
}

enum ItemPartition {
    First,
    Second,
    Both,
}

impl RectTiler {
    fn partition(
        &mut self,
        n_rects: usize,
        mut cutter: impl FnMut(&QuadRect) -> ItemPartition,
    ) -> (u16, u16) {
        let start = self.rects.len() - n_rects;
        let mut end = self.rects.len();
        let mut i = start;

        while i < end {
            match cutter(&self.rects[i]) {
                ItemPartition::First => {
                    i += 1;
                }
                ItemPartition::Second => {
                    end -= 1;
                    self.rects.swap(i, end);
                }
                ItemPartition::Both => {
                    let v = self.rects[i];
                    self.rects.push(v);
                    i += 1;
                }
            }
        }

        ((end - start) as u16, (self.rects.len() - end) as u16)
    }

    pub(super) fn start(&mut self, size: Vec2<u16>, tile_size: Vec2<u16>) {
        let width = size.x.div_ceil(tile_size.x);
        let height = size.y.div_ceil(tile_size.y);
        if width != 0 && height != 0 {
            let max_dim = width.max(height);
            let order = (max_dim.ilog2() as u8 + !max_dim.is_power_of_two() as u8) << 1;

            assert!(self.rects.len() <= usize::from(u16::MAX));
            assert!(self.queue.is_empty());

            self.queue.push(Quad {
                x: 0,
                y: 0,
                order,
                n_rects: self.rects.len() as u16,
            });
            self.tile_size = tile_size;
        }
    }

    pub(super) fn next(&mut self) -> Option<(Point2<u16>, &mut [QuadRect])> {
        loop {
            let quad = self.queue.pop()?;

            if quad.order == 0 {
                let start = self.rects.len() - usize::from(quad.n_rects);

                unsafe {
                    self.rects.set_len(start);
                    return Some((
                        Point2::new(quad.x, quad.y),
                        std::slice::from_raw_parts_mut(
                            self.rects.as_mut_ptr().add(start),
                            usize::from(quad.n_rects),
                        ),
                    ));
                };
            }

            if quad.order & 1 == 0 {
                let level = (quad.order >> 1) - 1;
                let split_y = (FixedS::new(quad.y.into()) + (1 << level)) * self.tile_size.y as i32;
                let (n_first, n_second) =
                    self.partition(quad.n_rects.into(), |&QuadRect { rect, .. }| {
                        if rect.max.y <= split_y {
                            ItemPartition::First
                        } else if rect.min.y >= split_y {
                            ItemPartition::Second
                        } else {
                            ItemPartition::Both
                        }
                    });

                if n_first > 0 {
                    self.queue.push(Quad {
                        x: quad.x,
                        y: quad.y,
                        order: quad.order - 1,
                        n_rects: n_first,
                    });
                }

                if n_second > 0 {
                    self.queue.push(Quad {
                        x: quad.x,
                        y: quad.y + (1 << level),
                        order: quad.order - 1,
                        n_rects: n_second,
                    });
                }
            } else {
                let level = ((quad.order + 1) >> 1) - 1;
                let split_x = (FixedS::new(quad.x.into()) + (1 << level)) * self.tile_size.x as i32;
                let (n_first, n_second) =
                    self.partition(quad.n_rects.into(), |&QuadRect { rect, .. }| {
                        if rect.max.x <= split_x {
                            ItemPartition::First
                        } else if rect.min.x >= split_x {
                            ItemPartition::Second
                        } else {
                            ItemPartition::Both
                        }
                    });

                if n_first > 0 {
                    self.queue.push(Quad {
                        x: quad.x,
                        y: quad.y,
                        order: quad.order - 1,
                        n_rects: n_first,
                    });
                }

                if n_second > 0 {
                    self.queue.push(Quad {
                        x: quad.x + (1 << level),
                        y: quad.y,
                        order: quad.order - 1,
                        n_rects: n_second,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use util::math::{Point2, Rect2, Vec2};

    use super::{QuadRect, RectTiler};
    use crate::scene::FixedS;

    macro_rules! test_tiler {
        (
            $tiler: ident,
            size ($sx: literal, $sy: literal),
            input {
                $(rect # $id: literal {
                    min $x0: literal, $y0: literal;
                    max $x1: literal, $y1: literal;
                },)*
            },
            output {
                $(tile @ ($tx: literal, $ty: literal) [
                    $($rect_id: literal),* $(,)?
                ],)*
            }
        ) => {
            $($tiler.add(QuadRect {
                rect: Rect2::new(
                    Point2::new(FixedS::new($x0), FixedS::new($y0)),
                    Point2::new(FixedS::new($x1), FixedS::new($y1)),
                ),
                id: $id,
                z: 0,
            });)*

            $tiler.start(Vec2::new($sx, $sy), Vec2::new(8, 4));

            let mut expected = {
                #[allow(unused_mut)]
                let mut result = HashMap::<Point2<u16>, Box<[u16]>>::new();
                $(result.insert(Point2::new($tx, $ty), Box::new({
                    let mut rects = [$($rect_id, )*];
                    rects.sort_unstable();
                    rects
                }));)*
                result
            };

            while let Some((tile_pos, rects)) = $tiler.next() {
                let mut r: Vec<_> = rects.iter().map(|q| q.id).collect();
                r.sort_unstable();
                let exp = expected.remove(&tile_pos);
                assert_eq!(exp.as_deref(), Some(&r[..]),
                    "expected tile {tile_pos:?} to have {exp:?} but got {r:?}"
                );
            }

            if !expected.is_empty() {
                panic!("tiles {:?} should've been emitted but weren't", expected.keys().collect::<Vec<_>>());
            }
        };
    }

    #[test]
    fn one() {
        let mut tiler = RectTiler::new();
        test_tiler! {
            tiler,
            size (10, 7),
            input {
                rect # 0 {
                    min 1, 1;
                    max 9, 3;
                },
            },
            output {
                tile @ (0, 0) [ 0 ], tile @ (1, 0) [ 0 ],
            }
        };
    }

    #[test]
    fn one_large() {
        let mut tiler = RectTiler::new();
        test_tiler! {
            tiler,
            size (16, 16),
            input {
                rect # 0 {
                    min 1, 1;
                    max 15, 15;
                },
            },
            output {
                tile @ (0, 0) [ 0 ], tile @ (1, 0) [ 0 ],
                tile @ (0, 1) [ 0 ], tile @ (1, 1) [ 0 ],
                tile @ (0, 2) [ 0 ], tile @ (1, 2) [ 0 ],
                tile @ (0, 3) [ 0 ], tile @ (1, 3) [ 0 ],
            }
        };
    }

    #[test]
    fn many() {
        let mut tiler = RectTiler::new();
        test_tiler! {
            tiler,
            size (16, 16),
            input {
                rect # 0 {
                    min 1, 1;
                    max 15, 15;
                },
                rect # 1 {
                    min 5, 5;
                    max 8, 10;
                },
                rect # 2 {
                    min 5, 5;
                    max 12, 10;
                },
            },
            output {
                tile @ (0, 0) [ 0 ], tile @ (1, 0) [ 0 ],
                tile @ (0, 1) [ 0, 1, 2 ], tile @ (1, 1) [ 0, 2 ],
                tile @ (0, 2) [ 0, 1, 2 ], tile @ (1, 2) [ 0, 2 ],
                tile @ (0, 3) [ 0 ], tile @ (1, 3) [ 0 ],
            }
        };
    }

    #[test]
    fn reset() {
        let mut tiler = RectTiler::new();
        test_tiler! {
            tiler,
            size (8, 8),
            input {
                rect # 0 {
                    min 1, 1;
                    max 3, 3;
                },
            },
            output {
                tile @ (0, 0) [ 0 ],
            }
        };
        test_tiler! {
            tiler,
            size (16, 16),
            input {
                rect # 0 {
                    min 1, 5;
                    max 3, 8;
                },
            },
            output {
                tile @ (0, 1) [ 0 ],
            }
        };
    }

    #[test]
    fn complicated() {
        let mut tiler = RectTiler::new();
        test_tiler! {
            tiler,
            size (16, 16),
            input {
                rect # 0 {
                    min 0, 0;
                    max 8, 8;
                },
                rect # 1 {
                    min 8, 0;
                    max 16, 8;
                },
                rect # 2 {
                    min 4, 11;
                    max 12, 15;
                },
            },
            output {
                tile @ (0, 0) [ 0 ], tile @ (1, 0) [ 1 ],
                tile @ (0, 1) [ 0 ], tile @ (1, 1) [ 1 ],
                tile @ (0, 2) [ 2 ], tile @ (1, 2) [ 2 ],
                tile @ (0, 3) [ 2 ], tile @ (1, 3) [ 2 ],
            }
        };
    }

    #[test]
    fn larger() {
        let mut tiler = RectTiler::new();
        test_tiler! {
            tiler,
            size (32, 32),
            input {
                rect # 0 {
                    min 0, 0;
                    max 16, 8;
                },
                rect # 1 {
                    min 16, 0;
                    max 32, 8;
                },
                rect # 2 {
                    min 14, 16;
                    max 23, 32;
                },
                rect # 3 {
                    min 21, 16;
                    max 32, 30;
                },
            },
            output {
                tile @ (0, 0) [ 0 ], tile @ (1, 0) [ 0 ],
                tile @ (0, 1) [ 0 ], tile @ (1, 1) [ 0 ],
                tile @ (2, 0) [ 1 ], tile @ (3, 0) [ 1 ],
                tile @ (2, 1) [ 1 ], tile @ (3, 1) [ 1 ],

                tile @ (1, 4) [ 2 ], tile @ (2, 4) [ 2, 3 ], tile @ (3, 4) [ 3 ],
                tile @ (1, 5) [ 2 ], tile @ (2, 5) [ 2, 3 ], tile @ (3, 5) [ 3 ],
                tile @ (1, 6) [ 2 ], tile @ (2, 6) [ 2, 3 ], tile @ (3, 6) [ 3 ],
                tile @ (1, 7) [ 2 ], tile @ (2, 7) [ 2, 3 ], tile @ (3, 7) [ 3 ],
            }
        };
    }
}
