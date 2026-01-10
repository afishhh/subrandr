use std::{path::PathBuf, sync::Arc};

use sbr_rasterize::{
    color::{to_straight_rgba, Premultiplied, Premultiply, BGRA8},
    scene::{self, FixedS},
    sw::{self, InstancedOutputBuilder, OutputImage, OutputPiece},
    Rasterizer as _,
};
use util::math::{I16Dot16, Point2, Rect2, Vec2};

struct DrawChecker {
    base_path: PathBuf,
    display_base_path: String,
    secondary_draw_index: u32,
    size: Vec2<u32>,
    /// Reference image rasterized using a full immediate-mode draw.
    /// This is the one that is actually saved as a snapshot,
    /// other draws are just checked to match this one.
    reference: Vec<Premultiplied<BGRA8>>,
    /// Saved pieces used for running subsequent instanced draws.
    pieces: Vec<OutputPiece>,
    scratch: Vec<Premultiplied<BGRA8>>,
}

impl DrawChecker {
    fn check_immediate(name: &str, size: Vec2<u32>, scene: &[scene::SceneNode]) -> Self {
        let mut buffer = vec![Premultiplied(BGRA8::ZERO); (size.x * size.y) as usize];
        let mut target = sw::RenderTarget::new(&mut buffer, size.x, size.y, size.x);

        let mut rasterizer = sw::Rasterizer::new();
        rasterizer
            .render_scene(&mut target.reborrow().into(), scene, &())
            .expect("failed to rasterize scene to framebuffer");

        let mut scratch = buffer.clone();
        let rgba = to_straight_rgba(&mut scratch);
        let snapshot_dir = test_util::project_dir().join("snapshots/sw");
        let base_path = snapshot_dir.join(name);
        let display_base_path = format!("snapshots/sw/{name}");
        test_util::check_png_snapshot(&base_path, &display_base_path, rgba, size.x, size.y);

        Self {
            base_path,
            display_base_path,
            secondary_draw_index: 0,
            size,
            scratch,
            reference: buffer,
            pieces: {
                let mut result = Vec::new();
                rasterizer
                    .render_scene_pieces(scene, &mut |piece| result.push(piece), &())
                    .unwrap();
                result
            },
        }
    }

    fn check_defaults(name: &str, size: Vec2<u32>, scene: &[scene::SceneNode]) -> Self {
        let mut checker = Self::check_immediate(name, size, scene);
        checker.check_instanced(
            Rect2::from_min_size(Point2::ZERO, Vec2::new(size.x as i32, size.y as i32)),
            false,
        );
        checker
    }
}

struct InstanceCompositor<'t, 'i> {
    rasterizer: sw::Rasterizer,
    target: sw::RenderTarget<'t>,
    images: Vec<OutputImage<'i>>,
}

impl<'i, 't> InstancedOutputBuilder<'i> for InstanceCompositor<'t, 'i> {
    type ImageHandle = usize;

    fn on_image(&mut self, _size: Vec2<u32>, image: sw::OutputImage<'i>) -> Self::ImageHandle {
        let id = self.images.len();
        self.images.push(image);
        id
    }

    fn on_instance(&mut self, handle: Self::ImageHandle, params: sw::OutputInstanceParameters) {
        let image = &self.images[handle];
        match *image {
            OutputImage::Texture(sw::OutputBitmap {
                ref texture,
                filter,
                color,
            }) => {
                let src_texture =
                    if params.src_size != params.dst_size || params.src_size != texture.size() {
                        &self.rasterizer.scale_texture(
                            texture,
                            params.dst_size,
                            params.src_off,
                            params.src_size,
                        )
                    } else {
                        texture
                    };

                self.rasterizer.blit_texture_filtered(
                    &mut self.target.reborrow(),
                    params.dst_pos,
                    src_texture,
                    filter,
                    color,
                );
            }
            OutputImage::Rect(sw::OutputRect { rect, color }) => {
                assert_eq!(params.src_size, params.dst_size);
                assert_eq!(params.src_off, Vec2::ZERO);
                self.rasterizer.fill_axis_aligned_rect(
                    self.target.reborrow(),
                    rect,
                    color.premultiply(),
                );
            }
        }
    }
}

impl DrawChecker {
    fn check_instanced(&mut self, clip_rect: Rect2<i32>, force_snapshot: bool) {
        assert!(!clip_rect.is_empty());

        self.secondary_draw_index += 1;
        self.scratch.clear();
        self.scratch.resize(
            (self.size.x * self.size.y) as usize,
            Premultiplied(BGRA8::ZERO),
        );
        let mut target =
            sw::RenderTarget::new(&mut self.scratch, self.size.x, self.size.y, self.size.x);

        sw::pieces_to_instanced_images(
            &mut InstanceCompositor {
                rasterizer: sw::Rasterizer::new(),
                target: target.reborrow(),
                images: Vec::new(),
            },
            self.pieces.iter(),
            clip_rect,
        );

        let min_x = u32::try_from(clip_rect.min.x).unwrap_or(0).min(self.size.x);
        let min_y = u32::try_from(clip_rect.min.y).unwrap_or(0).min(self.size.y);
        let max_x = u32::try_from(clip_rect.max.x).unwrap_or(0).min(self.size.x);
        let max_y = u32::try_from(clip_rect.max.y).unwrap_or(0).min(self.size.y);
        let mismatch_position = 'cmp: {
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let idx = y as usize * self.size.x as usize + x as usize;
                    if self.reference[idx].0 != self.scratch[idx].0 {
                        break 'cmp Some(Point2::new(x, y));
                    }
                }
            }

            None
        };

        let mut write_snapshot = || {
            let rgba_pixel_bytes = to_straight_rgba(&mut self.scratch);
            let display_path = format!(
                "{}.{}.png",
                self.display_base_path, self.secondary_draw_index
            );
            test_util::write_png(
                &self
                    .base_path
                    .with_extension(format!("{}.png", self.secondary_draw_index)),
                rgba_pixel_bytes,
                self.size.x,
                self.size.y,
            )
            .unwrap();
            eprintln!(
                "Draw {} snapshot written to {display_path}",
                self.secondary_draw_index
            );
        };

        if let Some(mismatch_position) = mismatch_position {
            eprintln!("Instanced draw output mismatch at {mismatch_position:?}!");

            write_snapshot();

            panic!()
        } else if force_snapshot {
            write_snapshot();
        }
    }
}

#[test]
fn simple_rectangles() {
    let scene = &[
        scene::SceneNode::FilledRect(scene::FilledRect {
            rect: Rect2 {
                min: Point2::new(FixedS::new(10), FixedS::new(10)),
                max: Point2::new(FixedS::new(90), FixedS::new(90)),
            },
            color: BGRA8::YELLOW,
        }),
        scene::SceneNode::FilledRect(scene::FilledRect {
            rect: Rect2 {
                min: Point2::new(FixedS::new(5), FixedS::new(5)),
                max: Point2::new(FixedS::new(50), FixedS::new(50)),
            },
            color: BGRA8::RED,
        }),
        scene::SceneNode::FilledRect(scene::FilledRect {
            rect: Rect2 {
                min: Point2::new(FixedS::new(50), FixedS::new(50)),
                max: Point2::new(FixedS::new(100), FixedS::new(100)),
            },
            color: BGRA8::BLUE,
        }),
        scene::SceneNode::FilledRect(scene::FilledRect {
            rect: Rect2 {
                min: Point2::new(FixedS::new(25), FixedS::new(25)),
                max: Point2::new(FixedS::new(75), FixedS::new(75)),
            },
            color: BGRA8::GREEN.mul_alpha(150),
        }),
    ];

    DrawChecker::check_defaults("simple_rectangles", Vec2::new(100, 100), scene);
}

#[test]
fn clipped_polyline() {
    let scene = &[scene::SceneNode::StrokedPolyline(scene::StrokedPolyline {
        polyline: vec![
            Point2::new(I16Dot16::new(50), I16Dot16::new(120)),
            Point2::new(I16Dot16::new(120), I16Dot16::new(50)),
            Point2::new(I16Dot16::new(-20), I16Dot16::new(50)),
            Point2::new(I16Dot16::new(50), I16Dot16::new(-20)),
            Point2::new(I16Dot16::new(50), I16Dot16::new(120)),
        ],
        width: I16Dot16::new(8),
        color: BGRA8::RED,
    })];

    let mut checker = DrawChecker::check_defaults("clipped_polyline", Vec2::new(100, 100), scene);
    checker.check_instanced(
        Rect2::new(Point2::new(20, -10), Point2::new(200, 80)),
        false,
    );
}

#[test]
fn translated_subscene_with_polyline() {
    let subscene = Arc::from([
        scene::SceneNode::StrokedPolyline(scene::StrokedPolyline {
            polyline: vec![
                Point2::new(I16Dot16::new(4), I16Dot16::new(4)),
                Point2::new(I16Dot16::new(60), I16Dot16::new(4)),
                Point2::new(I16Dot16::new(60), I16Dot16::new(60)),
                Point2::new(I16Dot16::new(4), I16Dot16::new(60)),
                Point2::new(I16Dot16::new(4), I16Dot16::new(4)),
                // Needs an extra segment to properly close
                Point2::new(I16Dot16::new(60), I16Dot16::new(4)),
            ],
            width: I16Dot16::new(8),
            color: BGRA8::CYAN,
        }),
        scene::SceneNode::StrokedPolyline(scene::StrokedPolyline {
            polyline: vec![
                Point2::new(I16Dot16::new(4), I16Dot16::new(4)),
                Point2::new(I16Dot16::new(60), I16Dot16::new(60)),
            ],
            width: I16Dot16::new(8),
            color: BGRA8::MAGENTA,
        }),
        scene::SceneNode::StrokedPolyline(scene::StrokedPolyline {
            polyline: vec![
                Point2::new(I16Dot16::new(60), I16Dot16::new(4)),
                Point2::new(I16Dot16::new(4), I16Dot16::new(60)),
            ],
            width: I16Dot16::new(8),
            color: BGRA8::MAGENTA,
        }),
    ]);
    let scene = &[
        scene::SceneNode::FilledRect(scene::FilledRect {
            rect: Rect2 {
                min: Point2::new(FixedS::new(10), FixedS::new(10)),
                max: Point2::new(FixedS::new(90), FixedS::new(90)),
            },
            color: BGRA8::YELLOW,
        }),
        scene::SceneNode::FilledRect(scene::FilledRect {
            rect: Rect2 {
                min: Point2::new(FixedS::new(25), FixedS::new(25)),
                max: Point2::new(FixedS::new(120), FixedS::new(120)),
            },
            color: BGRA8::BLUE,
        }),
        scene::SceneNode::Subscene(scene::Subscene {
            pos: Point2::new(FixedS::new(25), FixedS::new(25)),
            scene: subscene,
        }),
    ];

    let mut checker = DrawChecker::check_immediate(
        "translated_subscene_with_polyline",
        Vec2::new(100, 100),
        scene,
    );
    checker.check_instanced(Rect2::new(Point2::new(20, 43), Point2::new(91, 76)), false);
    checker.check_instanced(Rect2::new(Point2::new(37, 37), Point2::new(75, 89)), false);
}
