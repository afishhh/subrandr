use std::{
    ops::Range,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, FromArgMatches};
#[cfg(feature = "wgpu")]
use pollster::FutureExt as _;
use subrandr::{Renderer, Subrandr, SubtitleContext, Subtitles};
use winit::{
    event::StartCause,
    event_loop::{self, ControlFlow, EventLoop},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::WindowAttributes,
};
#[cfg(target_os = "linux")]
use xcb::XidNew;

#[derive(clap::Parser)]
struct Args {
    file: Option<PathBuf>,

    #[clap(long = "dpi", value_name = "DPI_MODE", default_value = "auto")]
    dpi: DpiMode,

    #[clap(
        long = "start",
        default_value_t = 0,
        value_name = "MILLISECONDS",
        conflicts_with = "ipc_connection_string"
    )]
    start_at: u32,

    #[clap(
        long = "speed",
        default_value_t = 1.0,
        conflicts_with = "ipc_connection_string"
    )]
    speed: f32,

    #[clap(
        long = "overlay",
        verbatim_doc_comment,
        value_name = "OVERLAY_MODE",
        default_value = "auto"
    )]
    /// Overlay the subtitle window over another existing window
    ///
    /// Allowed values:
    /// - "player": Will attempt to get the player's window id via IPC. Requires `--connect` to be specified.
    /// - "auto": Works like "player" except does not trigger an error if the feature is unsupported or `--connect` was not specified.
    /// - "no": Disable overlaying entirely, useful to override the "auto" default.
    /// - anything else: Will be parsed as a platform specific window id (currently a 32-bit unsigned integer). On X11 this ID can be acquired through tools like `xdotool`.
    ///
    /// Currently this is only supported on X11.
    overlay: OverlayMode,

    #[clap(long = "connect")]
    /// Player IPC connection string
    ipc_connection_string: Option<String>,

    #[clap(long = "fps", value_name = "FPS", default_value_t = 30.0)]
    target_fps: f32,

    #[clap(long = "rasterizer", value_enum)]
    #[cfg_attr(feature = "wgpu", clap(default_value_t = Rasterizer::Wgpu))]
    #[cfg_attr(not(feature = "wgpu"), clap(default_value_t = Rasterizer::Software))]
    rasterizer: Rasterizer,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Rasterizer {
    #[value(name = "sw")]
    Software,
    #[cfg(feature = "wgpu")]
    #[value(name = "wgpu")]
    Wgpu,
}

mod ipc;
mod softpresent;

#[derive(Debug, Clone, Copy)]
enum DpiMode {
    Automatic,
    Override(u32),
}

impl FromStr for DpiMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Automatic),
            other => Ok(Self::Override(other.parse().map_err(
                |_| r#"must be either "auto" or a non-negative 64-bit integer"#,
            )?)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum OverlayMode {
    Auto,
    Player,
    Window(u32),
    Disable,
}

impl FromStr for OverlayMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "player" => Ok(Self::Player),
            "no" => Ok(Self::Disable),
            other => Ok(Self::Window(other.parse().map_err(
                |_| r#"must be either "auto", "player", "no", or a non-negative 32-bit integer"#,
            )?)),
        }
    }
}

struct App<'a> {
    args: Args,

    player_connection: Option<Box<dyn ipc::PlayerConnection>>,
    overlay_window_id: Option<u32>,

    start: std::time::Instant,
    subs: Option<Subtitles>,
    renderer: Renderer<'a>,
    frame_valid_inside: Range<u32>,

    display_handle: Option<DisplayHandle>,
    #[cfg(feature = "wgpu")]
    wgpu: Option<wgpu::Instance>,
    state: Option<WindowState>,
}

enum DisplayHandle {
    #[cfg(target_os = "linux")]
    X11(std::mem::ManuallyDrop<xcb::Connection>),
}

#[allow(clippy::large_enum_variant)] // this has only one instance that is never copied after construction
enum WindowState {
    Software(SoftwareWindowState),
    #[cfg(feature = "wgpu")]
    Wgpu(WgpuWindowState),
}

struct SoftwareWindowState {
    window: winit::window::Window,
    presenter: softpresent::Presenter,
    buffer: Vec<u32>,
}

#[cfg(feature = "wgpu")]
struct WgpuWindowState {
    window: std::sync::Arc<winit::window::Window>,
    surface: wgpu::Surface<'static>,
    rasterizer: subrandr::rasterize::wgpu::Rasterizer,
    alpha_mode: wgpu::CompositeAlphaMode,
}

impl WindowState {
    fn window(&self) -> &winit::window::Window {
        match self {
            WindowState::Software(state) => &state.window,
            #[cfg(feature = "wgpu")]
            WindowState::Wgpu(state) => &state.window,
        }
    }

    fn size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.window().inner_size()
    }

    fn reconfigure(&self, size: winit::dpi::PhysicalSize<u32>) {
        match self {
            WindowState::Software(_) => _ = size,
            #[cfg(feature = "wgpu")]
            WindowState::Wgpu(WgpuWindowState {
                rasterizer,
                surface,
                alpha_mode,
                ..
            }) => {
                surface.configure(
                    rasterizer.device(),
                    &wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format: wgpu::TextureFormat::Bgra8Unorm,
                        width: size.width,
                        height: size.height,
                        present_mode: wgpu::PresentMode::AutoVsync,
                        desired_maximum_frame_latency: 2,
                        alpha_mode: *alpha_mode,
                        view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
                    },
                );
            }
        }
    }
}

impl winit::application::ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_transparent(true)
                    .with_decorations(false)
                    .with_visible(false),
            )
            .expect("failed to create window");

        #[cfg(target_os = "linux")]
        if let Some(DisplayHandle::X11(conn)) = &self.display_handle {
            conn.send_and_check_request(&xcb::x::ChangeWindowAttributes {
                window: softpresent::x11::extract_window_handle_from_window(&window).unwrap(),
                value_list: &[xcb::x::Cw::BackPixel(0), xcb::x::Cw::BorderPixel(0)],
            })
            .unwrap();
        };

        #[cfg(target_os = "linux")]
        if self.overlay_window_id.is_some() {
            let xwindow = match window
                .window_handle()
                .expect("failed to get system window handle")
                .as_raw()
            {
                winit::raw_window_handle::RawWindowHandle::Xlib(handle) => unsafe {
                    xcb::x::Window::new(handle.window as u32)
                },
                winit::raw_window_handle::RawWindowHandle::Xcb(handle) => unsafe {
                    xcb::x::Window::new(handle.window.get())
                },
                _ => panic!("Window handle incompatible with SoftwareX11"),
            };

            let conn = match &self.display_handle {
                Some(DisplayHandle::X11(conn)) => &**conn,
                _ => unreachable!(),
            };

            conn.send_and_check_request(&xcb::x::ChangeWindowAttributes {
                window: xwindow,
                value_list: &[xcb::x::Cw::OverrideRedirect(true)],
            })
            .unwrap();
            conn.send_and_check_request(&xcb::shape::Rectangles {
                operation: xcb::shape::So::Set,
                destination_kind: xcb::shape::Sk::Input,
                ordering: xcb::x::ClipOrdering::Unsorted,
                destination_window: xwindow,
                x_offset: 0,
                y_offset: 0,
                rectangles: &[],
            })
            .unwrap();
        }

        window.set_visible(true);

        match self.args.rasterizer {
            Rasterizer::Software => {
                self.state = Some(WindowState::Software(SoftwareWindowState {
                    presenter: softpresent::Presenter::new(&window)
                        .expect("Failed to create software presenter"),
                    window,
                    buffer: Vec::new(),
                }));
            }
            #[cfg(feature = "wgpu")]
            Rasterizer::Wgpu => {
                let window = std::sync::Arc::new(window);
                let wgpu = self.wgpu.as_mut().unwrap();

                let surface = wgpu.create_surface(window.clone()).unwrap();
                let adapter = wgpu
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::from_env()
                            .unwrap_or(wgpu::PowerPreference::LowPower),
                        force_fallback_adapter: false,
                        compatible_surface: Some(&surface),
                    })
                    .block_on()
                    .unwrap();
                let (device, queue) = adapter
                    .request_device(&wgpu::DeviceDescriptor {
                        label: None,
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::default(),
                        memory_hints: wgpu::MemoryHints::Performance,
                        trace: wgpu::Trace::Off,
                    })
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

                let mut rasterizer = subrandr::rasterize::wgpu::Rasterizer::new(device, queue);
                rasterizer.set_adapter_info(adapter.get_info());
                self.state = Some(WindowState::Wgpu(WgpuWindowState {
                    window,
                    surface,
                    rasterizer,
                    alpha_mode,
                }))
            }
        }
    }

    fn new_events(
        &mut self,
        _event_loop: &event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if matches!(
            cause,
            StartCause::Poll | StartCause::ResumeTimeReached { .. }
        ) {
            if let Some(state) = self.state.as_ref() {
                state.window().request_redraw();
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
                if let Some(state) = self.state.as_ref() {
                    self.frame_valid_inside = 0..0;
                    state.reconfigure(size);
                }
            }
            winit::event::WindowEvent::RedrawRequested => match &mut self.state {
                Some(state) => {
                    let frame_start = Instant::now();

                    let geometry = if let Some(id) = self.overlay_window_id {
                        let size = match &self.display_handle {
                            #[cfg(target_os = "linux")]
                            Some(DisplayHandle::X11(conn)) => {
                                let geometry = conn
                                    .wait_for_reply(conn.send_request(&xcb::x::GetGeometry {
                                        drawable: xcb::x::Drawable::Window(unsafe {
                                            xcb::x::Window::new(id)
                                        }),
                                    }))
                                    .unwrap();

                                conn.send_request(&xcb::x::ConfigureWindow {
                                    window: softpresent::x11::extract_window_handle_from_window(
                                        state.window(),
                                    )
                                    .unwrap(),
                                    value_list: &[
                                        xcb::x::ConfigWindow::X(geometry.x().into()),
                                        xcb::x::ConfigWindow::Y(geometry.y().into()),
                                        xcb::x::ConfigWindow::Width(geometry.width().into()),
                                        xcb::x::ConfigWindow::Height(geometry.height().into()),
                                        xcb::x::ConfigWindow::StackMode(xcb::x::StackMode::Above),
                                    ],
                                });

                                winit::dpi::PhysicalSize::<u32>::new(
                                    geometry.width().into(),
                                    geometry.height().into(),
                                )
                            }
                            _ => unreachable!(),
                        };

                        // TODO: Set clip shape to visible region of mpv window

                        state.reconfigure(size);

                        size
                    } else {
                        state.size()
                    };

                    let mut ctx = SubtitleContext {
                        dpi: match self.args.dpi {
                            DpiMode::Automatic => (state.window().scale_factor() * 72.) as u32,
                            DpiMode::Override(dpi) => dpi,
                        },
                        video_width: subrandr::I26Dot6::new(geometry.width as i32),
                        video_height: subrandr::I26Dot6::new(geometry.height as i32),
                        padding_left: subrandr::I26Dot6::new(0),
                        padding_right: subrandr::I26Dot6::new(0),
                        padding_top: subrandr::I26Dot6::new(0),
                        padding_bottom: subrandr::I26Dot6::new(0),
                    };

                    let (mut s_width, mut s_height) = (geometry.width, geometry.height);
                    let mut voffset_x = 0;
                    let mut voffset_y = 0;

                    let t = if let Some(ref mut ipc) = self.player_connection {
                        let mut track_switched = false;

                        let state = ipc.poll(&mut track_switched);

                        if track_switched && self.args.file.is_none() {
                            if let Some(path) = self
                                .player_connection
                                .as_mut()
                                .and_then(|x| x.get_stream_path().transpose())
                                .transpose()
                                .context("Failed to get stream path from player")
                                .unwrap()
                            {
                                self.subs =
                                    find_subs_near_path(self.renderer.library(), &path).unwrap();
                            };
                        }

                        if let Some(ipc::PlayerViewport { offset_x, offset_y }) = state.viewport {
                            s_width -= offset_x;
                            s_height -= offset_y;
                            voffset_x = offset_x;
                            voffset_y = offset_y;
                        }

                        if let Some(ipc::PlayerDimensions {
                            video_width,
                            video_height,
                            player_width,
                            player_height,
                        }) = state.dimensions
                        {
                            let padding_x = (player_width - video_width) / 2.0;
                            let padding_y = (player_height - video_height) / 2.0;
                            ctx.video_width = subrandr::I26Dot6::from_f32(video_width);
                            ctx.video_height = subrandr::I26Dot6::from_f32(video_height);
                            ctx.padding_left = subrandr::I26Dot6::from_f32(padding_x);
                            ctx.padding_right = subrandr::I26Dot6::from_f32(padding_x);
                            ctx.padding_top = subrandr::I26Dot6::from_f32(padding_y);
                            ctx.padding_bottom = subrandr::I26Dot6::from_f32(padding_y);
                            s_width = s_width.min(player_width.ceil() as u32);
                            s_height = s_height.min(player_height.ceil() as u32);
                        }

                        state.current_time
                    } else {
                        self.args.start_at
                            + (((frame_start - self.start).as_secs_f32() * self.args.speed)
                                * 1000.0) as u32
                    };

                    if !self.frame_valid_inside.contains(&t) {
                        match state {
                            WindowState::Software(soft) => {
                                soft.buffer.resize(s_width as usize * s_height as usize, 0);
                                if let Some(subs) = self.subs.as_ref() {
                                    // TODO: Do this properly instead
                                    self.renderer.set_subtitles(Some(subs));
                                    self.renderer
                                        .render(
                                            &ctx,
                                            t,
                                            unsafe {
                                                std::mem::transmute::<&mut [u32], &mut [_]>(
                                                    soft.buffer.as_mut_slice(),
                                                )
                                            },
                                            s_width,
                                            s_height,
                                            s_width,
                                        )
                                        .unwrap();

                                    soft.presenter
                                        .present(
                                            unsafe {
                                                std::slice::from_raw_parts(
                                                    soft.buffer.as_ptr() as *const u8,
                                                    std::mem::size_of_val(soft.buffer.as_slice()),
                                                )
                                            },
                                            (voffset_x as i16, voffset_y as i16),
                                            (s_width, s_height),
                                        )
                                        .expect("Failed to present buffer");
                                } else {
                                    soft.buffer.fill(0);
                                }
                            }
                            #[cfg(feature = "wgpu")]
                            WindowState::Wgpu(wgpu) => {
                                if let Some(subs) = self.subs.as_ref() {
                                    let surface_texture =
                                        wgpu.surface.get_current_texture().unwrap();
                                    let target = wgpu
                                        .rasterizer
                                        .target_from_texture(surface_texture.texture.clone());
                                    self.renderer.set_subtitles(Some(subs));
                                    self.renderer
                                        .render_to_wgpu(&mut wgpu.rasterizer, target, &ctx, t)
                                        .unwrap();

                                    surface_texture.present();
                                }
                            }
                        }

                        self.frame_valid_inside = self.renderer.unchanged_inside();
                    }

                    let next_min_wait =
                        frame_start + Duration::from_secs_f32(self.args.target_fps.recip());
                    let deadline = if self.player_connection.is_none() {
                        let next_change = self.start
                            + Duration::from_secs_f32(
                                (self.renderer.unchanged_inside().end - self.args.start_at) as f32
                                    / 1000.
                                    / self.args.speed,
                            );
                        next_min_wait.max(next_change)
                    } else {
                        next_min_wait
                    };

                    #[cfg(feature = "wgpu")]
                    let use_poll = matches!(state, WindowState::Wgpu(_));
                    #[cfg(not(feature = "wgpu"))]
                    let use_poll = false;

                    if use_poll && deadline == next_min_wait {
                        event_loop.set_control_flow(ControlFlow::Poll);
                    } else {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                    }
                }
                None => event_loop.set_control_flow(ControlFlow::Wait),
            },
            _ => (),
        }
    }
}

fn load_subs_from_file(sbr: &Subrandr, path: &Path) -> Result<subrandr::Subtitles> {
    Ok(match path.extension().and_then(|x| x.to_str()) {
        Some("srv3" | "ytt") => {
            let document = subrandr::srv3::parse(sbr, &std::fs::read_to_string(path).unwrap())?;
            Subtitles::Srv3(util::rc::Rc::new(subrandr::srv3::convert(sbr, document)))
        }
        Some("vtt") => {
            let text = std::fs::read_to_string(path).unwrap();
            let captions = subrandr::vtt::parse(&text).unwrap();
            Subtitles::Vtt(util::rc::Rc::new(subrandr::vtt::convert(sbr, captions)))
        }
        _ => bail!("Unrecognised subtitle file extension"),
    })
}

fn find_subs_near_path(sbr: &Subrandr, path: &Path) -> Result<Option<subrandr::Subtitles>> {
    const ATTEMPTED_EXTENSIONS: &[&[u8]] = &[b"srv3".as_slice(), b"ytt", b"vtt"];

    println!("Looking for subtitles files near {}", path.display());

    let mut candidates = Vec::new();
    for entry in path.parent().unwrap().read_dir().unwrap() {
        let entry = entry.unwrap();
        let filename = entry.file_name();

        if filename
            .as_encoded_bytes()
            .starts_with(path.file_stem().unwrap().as_encoded_bytes())
        {
            for (i, &ext) in ATTEMPTED_EXTENSIONS.iter().enumerate() {
                if filename.as_encoded_bytes().ends_with(ext) {
                    candidates.push((entry.path(), i));
                }
            }
        }
    }

    candidates.sort_by_key(|(_, index)| *index);

    if let Some((found, _)) = candidates.first() {
        println!("Using {}", found.display());
        Ok(Some(load_subs_from_file(sbr, found)?))
    } else {
        println!("No subtitles found");
        Ok(None)
    }
}

fn main() {
    let args = Args::from_arg_matches_mut(
        &mut Args::command()
            .mut_arg("ipc_connection_string", |arg| {
                let mut help = String::from("Connection string to use for communicating with a video player.\nThe argument should start with `<IPC TYPE>:` and a player-specific connection string follows.\n\nAvailable connectors:");
                for connector in ipc::AVAILABLE_CONNECTORS {
                    help.push_str("\n- ");
                    help.push_str(connector.id);
                    help.push_str(": ");
                    help.push_str(connector.description);
                }
                arg.long_help(help)
            })
            .get_matches(),
    ).unwrap();

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let handle = event_loop
        .display_handle()
        .expect("failed to get system display handle");
    let display_handle = match handle.as_raw() {
        #[cfg(target_os = "linux")]
        winit::raw_window_handle::RawDisplayHandle::Xlib(handle) => {
            Some(DisplayHandle::X11(std::mem::ManuallyDrop::new(unsafe {
                xcb::Connection::from_xlib_display(handle.display.unwrap().as_ptr() as *mut _)
            })))
        }
        #[cfg(target_os = "linux")]
        winit::raw_window_handle::RawDisplayHandle::Xcb(handle) => {
            Some(DisplayHandle::X11(std::mem::ManuallyDrop::new(unsafe {
                xcb::Connection::from_raw_conn(handle.connection.unwrap().as_ptr() as *mut _)
            })))
        }
        _ => None,
    };

    match args.rasterizer {
        Rasterizer::Software => match &display_handle {
            #[cfg(target_os = "linux")]
            Some(DisplayHandle::X11(conn)) => {
                conn.prefetch_maximum_request_length();
            }
            _ => panic!("SoftwareX11 rasterizer requires an X11 display handle"),
        },
        #[cfg(feature = "wgpu")]
        Rasterizer::Wgpu => {}
    }

    let mut player_connection = if let Some(connection_string) = &args.ipc_connection_string {
        let mut result = None;
        for desc in ipc::AVAILABLE_CONNECTORS {
            if let Some(suffix) = connection_string
                .strip_prefix(desc.id)
                .and_then(|s| s.strip_prefix(':'))
            {
                result = Some(
                    desc.connector
                        .try_connect(suffix)
                        .expect("failed to connect to player"),
                )
            }
        }
        result
    } else {
        None
    };

    let sbr = Subrandr::init();
    let subs = if let Some(file) = args.file.as_ref() {
        Some(load_subs_from_file(&sbr, file).unwrap())
    } else if let Some(path) = player_connection
        .as_mut()
        .and_then(|x| x.get_stream_path().transpose())
        .transpose()
        .context("Failed to get stream path from player")
        .unwrap()
    {
        find_subs_near_path(&sbr, &path).unwrap()
    } else {
        println!(
            "No subtitle file was provided and one couldn't be acquired via player connection"
        );
        None
    };

    #[cfg(target_os = "linux")]
    let display_supports_overlay = matches!(display_handle, Some(DisplayHandle::X11(_)));
    #[cfg(not(target_os = "linux"))]
    let display_supports_overlay = false;

    let overlay_window_id = match (args.overlay, display_supports_overlay) {
        (OverlayMode::Auto, false) => None,
        (OverlayMode::Disable, _) => None,
        (_, false) => panic!("Window overlay is only supported on X11"),
        (OverlayMode::Auto | OverlayMode::Player, true) => {
            if let Some(player) = player_connection.as_mut() {
                player.get_window_id()
            } else {
                if matches!(args.overlay, OverlayMode::Player) {
                    panic!("Overlay mode is `player` but no player connection string specified");
                }

                None
            }
        }
        (OverlayMode::Window(window), true) => Some(window),
    };

    event_loop
        .run_app(&mut App {
            start: std::time::Instant::now(),

            player_connection,
            overlay_window_id,

            display_handle,

            #[cfg(feature = "wgpu")]
            wgpu: if matches!(args.rasterizer, Rasterizer::Wgpu) {
                Some(wgpu::Instance::new(
                    &wgpu::InstanceDescriptor::from_env_or_default(),
                ))
            } else {
                None
            },

            args,
            subs,
            renderer: Renderer::new(&sbr),
            frame_valid_inside: 0..0,
            state: None,
        })
        .unwrap()
}
