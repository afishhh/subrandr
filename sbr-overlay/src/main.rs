use std::{
    mem::ManuallyDrop,
    path::PathBuf,
    str::FromStr,
    time::{Duration, Instant},
};

use clap::{CommandFactory, FromArgMatches};
use subrandr::{Painter, Renderer, Subrandr, SubtitleContext, Subtitles};
use winit::{
    event::StartCause,
    event_loop::{self, ControlFlow, EventLoop},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::WindowAttributes,
};
use xcb::XidNew;

#[derive(clap::Parser)]
struct Args {
    file: Option<PathBuf>,
    #[clap(long = "dpi", default_value = "auto")]
    dpi: DpiMode,
    #[clap(
        long = "start",
        default_value_t = 0,
        conflicts_with = "ipc_connection_string"
    )]
    start_at: u32,
    #[clap(
        long = "speed",
        default_value_t = 1.0,
        conflicts_with = "ipc_connection_string"
    )]
    speed: f32,
    #[clap(long = "parse")]
    parse: bool,
    #[clap(long = "overlay")]
    /// X11 window ID to stay on top of.
    x11_overlay_window: Option<u32>,
    #[clap(long = "connect")]
    /// Player IPC connection string.
    ipc_connection_string: Option<String>,

    #[clap(long = "fps", default_value_t = 30.0)]
    target_fps: f32,
    #[clap(long = "rasterizer", value_enum, default_value_t = Rasterizer::Software)]
    rasterizer: Rasterizer,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Rasterizer {
    #[value(name = "sw")]
    Software,
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

struct App<'a> {
    args: Args,

    player_connection: Option<Box<dyn ipc::PlayerConnection>>,

    start: std::time::Instant,
    renderer: Renderer<'a>,

    display_handle: Option<DisplayHandle>,
    state: Option<WindowState>,
}

enum DisplayHandle {
    X11(ManuallyDrop<xcb::Connection>),
}

enum WindowState {
    Software(SoftwareWindowState),
}

struct SoftwareWindowState {
    window: winit::window::Window,
    presenter: softpresent::Presenter,
    buffer: Vec<u32>,
}

impl WindowState {
    fn window(&self) -> &winit::window::Window {
        match self {
            WindowState::Software(state) => &state.window,
        }
    }

    fn size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.window().inner_size()
    }
}

impl winit::application::ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_transparent(true)
                    .with_decorations(false),
            )
            .expect("failed to create window");

        match self.args.rasterizer {
            Rasterizer::Software => {
                if self.args.x11_overlay_window.is_some() {
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

                self.state = Some(WindowState::Software(SoftwareWindowState {
                    presenter: softpresent::Presenter::new(&window)
                        .expect("Failed to create software presenter"),
                    window,
                    buffer: Vec::new(),
                }));
            }
        }
    }

    fn new_events(
        &mut self,
        _event_loop: &event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
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
            winit::event::WindowEvent::RedrawRequested => match &mut self.state {
                Some(state) => {
                    let frame_start = Instant::now();

                    let geometry = if let Some(id) = self.args.x11_overlay_window {
                        let conn = match &self.display_handle {
                            Some(DisplayHandle::X11(conn)) => conn,
                            _ => unreachable!(),
                        };

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

                        // TODO: Set clip shape to visible region of mpv window

                        winit::dpi::PhysicalSize::<u32>::new(
                            geometry.width().into(),
                            geometry.height().into(),
                        )
                    } else {
                        state.size()
                    };

                    let mut ctx = SubtitleContext {
                        dpi: match self.args.dpi {
                            DpiMode::Automatic => (state.window().scale_factor() * 72.) as u32,
                            DpiMode::Override(dpi) => dpi,
                        },
                        video_width: geometry.width as f32,
                        video_height: geometry.height as f32,
                        padding_left: 0.0,
                        padding_right: 0.0,
                        padding_top: 0.0,
                        padding_bottom: 0.0,
                    };

                    let (mut s_width, mut s_height) = (geometry.width, geometry.height);
                    let mut voffset_x = 0;
                    let mut voffset_y = 0;

                    let t = if let Some(ref mut ipc) = self.player_connection {
                        let state = ipc.poll();

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
                            ctx.video_width = video_width;
                            ctx.video_height = video_height;
                            ctx.padding_left = padding_x;
                            ctx.padding_right = padding_x;
                            ctx.padding_top = padding_y;
                            ctx.padding_bottom = padding_y;
                            s_width = s_width.min(player_width.ceil() as u32);
                            s_height = s_height.min(player_height.ceil() as u32);
                        }

                        state.current_time
                    } else {
                        self.args.start_at
                            + (((frame_start - self.start).as_secs_f32() * self.args.speed)
                                * 1000.0) as u32
                    };

                    let render_start = Instant::now();

                    match state {
                        WindowState::Software(x11) => {
                            x11.buffer.resize(s_width as usize * s_height as usize, 0);
                            let mut painter =
                                Painter::new(s_width, s_height, x11.buffer.as_mut_slice());
                            println!(
                                "render t = {}ms to {}x{}",
                                t, geometry.width, geometry.height
                            );
                            self.renderer.render(&ctx, t, &mut painter);
                        }
                    }

                    let render_end = Instant::now();
                    println!(
                        "took {:.2}ms",
                        (render_end - render_start).as_micros() as f64 / 1000.
                    );

                    match state {
                        WindowState::Software(soft) => {
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
                        }
                    }

                    event_loop.set_control_flow(ControlFlow::WaitUntil(
                        frame_start + Duration::from_secs_f32(self.args.target_fps.recip()),
                    ));
                }
                None => event_loop.set_control_flow(ControlFlow::Wait),
            },
            _ => (),
        }
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

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let handle = event_loop
        .display_handle()
        .expect("failed to get system display handle");
    let display_handle = match handle.as_raw() {
        winit::raw_window_handle::RawDisplayHandle::Xlib(handle) => {
            Some(DisplayHandle::X11(ManuallyDrop::new(unsafe {
                xcb::Connection::from_xlib_display(handle.display.unwrap().as_ptr() as *mut _)
            })))
        }
        winit::raw_window_handle::RawDisplayHandle::Xcb(handle) => {
            Some(DisplayHandle::X11(ManuallyDrop::new(unsafe {
                xcb::Connection::from_raw_conn(handle.connection.unwrap().as_ptr() as *mut _)
            })))
        }
        _ => None,
    };

    match args.rasterizer {
        Rasterizer::Software => match &display_handle {
            Some(DisplayHandle::X11(conn)) => {
                conn.prefetch_maximum_request_length();
            }
            _ => panic!("SoftwareX11 rasterizer requires an X11 display handle"),
        },
    }

    if args.x11_overlay_window.is_some() && !matches!(display_handle, Some(DisplayHandle::X11(_))) {
        panic!("x11_overlay_window is only supported on X11")
    }

    let player_connection = if let Some(connection_string) = &args.ipc_connection_string {
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

    event_loop
        .run_app(&mut App {
            start: std::time::Instant::now(),

            player_connection,

            display_handle,

            args,
            renderer,
            state: None,
        })
        .unwrap()
}
