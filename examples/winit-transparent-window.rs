use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use pollster::FutureExt;
use subrandr::{Renderer, Subrandr, Subtitles};
use wgpu::TextureUsages;
use winit::{
    event::StartCause,
    event_loop::{self, ControlFlow, EventLoop},
    window::{Window, WindowAttributes},
};

#[derive(clap::Parser)]
struct Args {
    file: Option<PathBuf>,
    #[clap(long = "dpi")]
    dpi: Option<u32>,
    #[clap(long = "start", default_value_t = 0)]
    start_at: u32,
    #[clap(long = "speed", default_value_t = 1.0)]
    speed: f64,
    #[clap(long = "parse")]
    parse: bool,
    #[clap(long = "overlay")]
    overlay_window: Option<u32>,
    #[clap(long = "follow-mpv")]
    mpv_socket: Option<PathBuf>,
    #[clap(long = "cdp")]
    cdp_url: Option<String>,
    #[clap(long = "fps", default_value_t = 30.0)]
    target_fps: f32,
}

struct App<'a> {
    args: Args,
    start: std::time::Instant,
    wgpu: wgpu::Instance,
    renderer: Renderer<'a>,
    state: Option<WindowState>,
}

struct WindowState {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    alpha_mode: wgpu::CompositeAlphaMode,
    surface: wgpu::Surface<'static>,
}

impl WindowState {
    pub fn reconfigure(&self, size: winit::dpi::PhysicalSize<u32>) {
        self.surface.configure(
            &self.device,
            &wgpu::SurfaceConfiguration {
                usage: TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Bgra8Unorm,
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: self.alpha_mode,
                view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
            },
        );
    }
}

impl winit::application::ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_transparent(true)
                        .with_decorations(false),
                )
                .unwrap(),
        );

        let surface = self.wgpu.create_surface(window.clone()).unwrap();
        let adapter = self
            .wgpu
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .block_on()
            .unwrap();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .block_on()
            .unwrap();

        let cap = surface.get_capabilities(&adapter);

        let mut has_premultiplied = false;
        let mut has_opaque = false;
        for mode in cap.alpha_modes {
            match mode {
                wgpu::CompositeAlphaMode::Opaque => has_opaque = true,
                wgpu::CompositeAlphaMode::PreMultiplied => has_premultiplied = true,
                _ => (),
            }
        }

        let alpha_mode = if has_premultiplied {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if has_opaque {
            // While this does say that the alpha channel is ignored, this seems like
            // it's actually not the case on X11 with the nvidia driver.
            // I would not be surprised if it's because of X11 not truly supporting transparency
            // and the fact the support is just tacked on by the compositor.
            wgpu::CompositeAlphaMode::Opaque
        } else {
            // I guess it's better specify *something* than to crash?
            wgpu::CompositeAlphaMode::Inherit
        };

        unsafe { self.renderer.init_wgpwu(device.clone(), queue.clone()) };

        self.state = Some(WindowState {
            window,
            device,
            queue,
            surface,
            alpha_mode,
        });
    }

    fn new_events(
        &mut self,
        _event_loop: &event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            if let Some(state) = self.state.as_ref() {
                state.window.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            winit::event::WindowEvent::Resized(size) => {
                if let Some(ref mut state) = self.state {
                    state.reconfigure(size);
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                if let Some(WindowState {
                    window, surface, ..
                }) = self.state.as_ref()
                {
                    let size = window.inner_size();

                    let surface_texture = surface.get_current_texture().unwrap();
                    unsafe {
                        self.renderer.render_wgpu(
                            &subrandr::SubtitleContext {
                                dpi: self
                                    .args
                                    .dpi
                                    .unwrap_or_else(|| (72.0 * window.scale_factor()) as u32),
                                video_width: size.width as f32,
                                video_height: size.height as f32,
                                padding_left: 0.0,
                                padding_right: 0.0,
                                padding_top: 0.0,
                                padding_bottom: 0.0,
                            },
                            (std::time::Instant::now() - self.start).as_millis() as u32,
                            surface_texture.texture.clone(),
                        );
                    }

                    surface_texture.present();

                    if let Some(next_ms) = self.renderer.unchanged_until() {
                        let next = self.start + std::time::Duration::from_millis(next_ms.into());
                        event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                    } else {
                        event_loop.set_control_flow(ControlFlow::Poll);
                    }
                }
            }
            _ => (),
        }
    }
}

fn main() {
    let args = Args::parse();

    let sbr = Subrandr::init();
    let subs = if let Some(file) = args.file.as_ref() {
        match file.extension().and_then(|x| x.to_str()) {
            Some("srv3" | "ytt") => {
                let document =
                    subrandr::srv3::parse(&sbr, &std::fs::read_to_string(file).unwrap()).unwrap();
                subrandr::srv3::convert(document)
            }
            Some("ass") => {
                let script = subrandr::ass::parse(&std::fs::read_to_string(file).unwrap()).unwrap();
                subrandr::ass::convert(script)
            }
            _ => panic!("Unrecognised subtitle file extension"),
        }
    } else {
        Subtitles::test_new()
    };

    let renderer = Renderer::new(&sbr, &subs);

    let wgpu = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
        .run_app(&mut App {
            start: std::time::Instant::now()
                - std::time::Duration::from_millis(args.start_at.into()),
            args,
            wgpu,
            renderer,
            state: None,
        })
        .unwrap()
}
