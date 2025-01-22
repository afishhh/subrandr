#![allow(clippy::too_many_arguments)]

use std::{
    error::Error,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    os::unix::{ffi::OsStrExt, net::UnixStream},
    path::PathBuf,
    time::Instant,
};

use clap::Parser;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;
use subrandr::{Painter, Renderer, Subrandr, SubtitleContext, Subtitles};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};
use xcb::XidNew;

#[derive(clap::Parser)]
struct Args {
    file: Option<PathBuf>,
    #[clap(long = "dpi", default_value_t = 72)]
    dpi: u32,
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

struct MpvSocket {
    stream: BufReader<UnixStream>,
}

impl MpvSocket {
    fn connect(path: PathBuf) -> MpvSocket {
        MpvSocket {
            stream: BufReader::new(UnixStream::connect(path).unwrap()),
        }
    }

    fn get_playback_time(&mut self) -> u32 {
        loop {
            self.stream
                .get_mut()
                .write_all(
                    concat!(r#"{ "command": ["get_property", "playback-time"] }"#, "\n").as_bytes(),
                )
                .unwrap();

            let mut line = String::new();
            loop {
                self.stream.read_line(&mut line).unwrap();

                if line.contains("property unavailable") {
                    break;
                }

                if let Some(data_idx) = line.find(r#""data""#) {
                    let colon_idx = data_idx + line[data_idx..].find(":").unwrap() + 1;
                    let comma_idx = colon_idx + line[colon_idx..].find(',').unwrap();
                    return (line[colon_idx..comma_idx].trim().parse::<f32>().unwrap() * 1000.)
                        as u32;
                }
            }
        }
    }
}

struct ChromeDebugSocket {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    targets: Vec<ChromeDebugTarget>,
    next_id: u64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ChromeDebugTarget {
    #[serde(rename = "targetId")]
    id: String,
    #[serde(rename = "type")]
    kind: String,
    title: String,
    url: String,
    attached: bool,
    #[serde(rename = "browserContextId")]
    browser_context_id: Option<String>,
}

type JsonValue = serde_json::Value;

impl ChromeDebugSocket {
    pub fn connect(url: &str) -> Self {
        let req = tungstenite::ClientRequestBuilder::new(url.parse().unwrap())
            .with_header("Host", "127.0.0.1");
        let ws = tungstenite::connect(req).unwrap().0;
        Self {
            ws,
            targets: Vec::new(),
            next_id: 0,
        }
    }

    pub fn read_until_result(&mut self, id: u64) -> JsonValue {
        #[derive(Deserialize)]
        struct Response {
            id: u64,
            result: JsonValue,
        }

        #[derive(Deserialize)]
        struct Call {
            method: String,
            params: JsonValue,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ProtocolMessage {
            Response(Response),
            Call(Call),
        }

        loop {
            match self.ws.read().unwrap() {
                Message::Text(text) => {
                    let msg = serde_json::from_str::<ProtocolMessage>(text.as_str())
                        .map_err(|_| panic!("Failed to parse CDP message {:?}", text.as_str()))
                        .unwrap();
                    match msg {
                        ProtocolMessage::Response(response) => {
                            if response.id != id {
                                println!("unexpected response id {} ignored", response.id);
                                continue;
                            }
                            break response.result;
                        }
                        ProtocolMessage::Call(call) => {
                            self.handle(&call.method, call.params);
                        }
                    }
                }
                _ => panic!("unexpected websocket message received"),
            }
        }
    }

    pub fn handle(&mut self, method: &str, mut params: JsonValue) {
        match method {
            "Target.targetCreated" => self
                .targets
                .push(serde_json::from_value(params["targetInfo"].take()).unwrap()),
            // ignore
            "Target.receivedMessageFromTarget" => (),
            "Target.consoleAPICalled" => (),
            _ => println!("ignored unrecognized method {method:?} call from browser {params:?}"),
        }
    }

    fn call_impl<R: DeserializeOwned>(
        &mut self,
        session_id: Option<&str>,
        method: &str,
        params: JsonValue,
    ) -> R {
        let mut command = json!({
            "id": self.next_id,
            "method": method,
            "params": params
        });
        if let Some(id) = session_id {
            command
                .as_object_mut()
                .unwrap()
                .insert("sessionId".to_owned(), id.into());
        }
        self.ws
            .send(Message::Text(command.to_string().into()))
            .unwrap();

        let result = serde_json::from_value(self.read_until_result(self.next_id)).unwrap();

        self.next_id += 1;

        result
    }

    pub fn call<R: DeserializeOwned>(&mut self, method: &str, params: JsonValue) -> R {
        self.call_impl(None, method, params)
    }

    pub fn session_call<R: DeserializeOwned>(
        &mut self,
        session_id: &str,
        method: &str,
        params: JsonValue,
    ) -> R {
        self.call_impl(Some(session_id), method, params)
    }

    pub fn enable_target_discovery(&mut self) {
        self.call::<JsonValue>(
            "Target.setDiscoverTargets",
            json!({
                "discover": true
            }),
        );
    }
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct PlayerDimensions {
    video_width: f32,
    video_height: f32,
    player_width: f32,
    player_height: f32,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct PlayerViewport {
    offset_x: u32,
    offset_y: u32,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct PlayerState {
    #[serde(flatten)]
    dimensions: Option<PlayerDimensions>,
    viewport: Option<PlayerViewport>,
    // in ms
    current_time: u32,
}

trait PlayerIpc {
    fn get_state(&mut self) -> PlayerState;
}

impl PlayerIpc for MpvSocket {
    fn get_state(&mut self) -> PlayerState {
        PlayerState {
            dimensions: None,
            viewport: None,
            current_time: Self::get_playback_time(self),
        }
    }
}

struct ChromeYoutubeIpc {
    chrome: ChromeDebugSocket,
    session_id: String,
}

impl ChromeYoutubeIpc {
    pub fn connect(url: &str) -> Self {
        let mut chrome = ChromeDebugSocket::connect(url);
        chrome.enable_target_discovery();

        let mut target_id = None;
        for target in &chrome.targets {
            if target.kind == "page" && target.url.starts_with("https://www.youtube.com") {
                target_id = Some(target.id.clone());
                println!("Found YouTube debug target {:#?}", target);
                break;
            }
        }
        let Some(target_id) = target_id else {
            panic!("no youtube page found");
        };

        let session_id = chrome.call::<JsonValue>(
            "Target.attachToTarget",
            json!({
                "targetId": target_id,
                "flatten": true,
            }),
        )["sessionId"]
            .as_str()
            .unwrap()
            .to_owned();

        chrome.session_call::<JsonValue>(&session_id, "Runtime.enable", json!({}));

        Self { chrome, session_id }
    }

    pub fn run_js(&mut self, expr: &str) -> JsonValue {
        self.chrome.session_call::<JsonValue>(
            &self.session_id,
            "Runtime.evaluate",
            json!({
                "expression": expr,
                "returnByValue": true,
            }),
        )["result"]["value"]
            .take()
    }

    pub fn run_js_in_lambda(&mut self, expr: &str) -> JsonValue {
        self.run_js(&format!("(() => {{{expr}}})()"))
    }
}

impl PlayerIpc for ChromeYoutubeIpc {
    fn get_state(&mut self) -> PlayerState {
        // whole player = #movie_player
        // video part only = #movie_player > div:nth-child(1) > video:nth-child(1)
        serde_json::from_value(self.run_js_in_lambda(
            r##"
            let movie_player = document.querySelector("#movie_player");
            let video_player = document.querySelector("#movie_player > div:nth-child(1) > video:nth-child(1)")
            let movie_rect = movie_player.getBoundingClientRect()
            let video_rect = video_player.getBoundingClientRect()
            return {
                player_width: movie_rect.width * window.devicePixelRatio,
                player_height: movie_rect.height * window.devicePixelRatio,
                video_width: video_rect.width * window.devicePixelRatio,
                video_height: video_rect.height * window.devicePixelRatio,
                viewport: {
                    offset_x: Math.floor(movie_rect.x * window.devicePixelRatio),
                    offset_y: Math.max(Math.floor((movie_rect.y + window.outerHeight - window.innerHeight) * window.devicePixelRatio), 0)
                },
                current_time: Math.floor(video_player.currentTime * 1000)
            };
        "##
        )).unwrap()
    }
}

fn large_zpixmap32_putimage(
    conn: &xcb::Connection,
    drawable: xcb::x::Drawable,
    gc: xcb::x::Gcontext,
    image: &[u8],
    offset: (i16, i16),
    width: u16,
    pitch: usize,
    height: u16,
) -> xcb::Result<()> {
    // the PutImage request itself will naturally have some overhead we want to account for
    let max_length = (conn.get_maximum_request_length() as usize * 4) - 1024;
    let chunk_height = max_length / pitch;

    for y in (0..height).step_by(chunk_height).map(|x| x as usize) {
        let current_end_y = (y + chunk_height).min(height as usize);
        let current_height = current_end_y - y;
        let data = &image[y * pitch..current_end_y * pitch];

        conn.check_request(conn.send_request_checked(&xcb::x::PutImage {
            format: xcb::x::ImageFormat::ZPixmap,
            drawable,
            gc,
            width,
            height: current_height as u16,
            dst_x: offset.0,
            dst_y: offset.1 + y as i16,
            left_pad: 0,
            depth: 32,
            data,
        }))?;
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error + 'static>> {
    let args = Args::parse();

    let sbr = Subrandr::init();
    let subs = if let Some(file) = args.file {
        match file.extension().map(|x| x.as_bytes()) {
            Some(b"srv3" | b"ytt") => {
                let document =
                    subrandr::srv3::parse(&sbr, &std::fs::read_to_string(file).unwrap()).unwrap();
                subrandr::srv3::convert(document)
            }
            Some(b"ass") => {
                let script = subrandr::ass::parse(&std::fs::read_to_string(file).unwrap()).unwrap();
                subrandr::ass::convert(script)
            }
            _ => panic!("Unrecognised subtitle file extension"),
        }
    } else {
        Subtitles::test_new()
    };

    if args.parse {
        println!("{subs:?}");
        return Ok(());
    }

    let mut ipc_socket = args
        .mpv_socket
        .map(|s| Box::new(MpvSocket::connect(s)) as Box<dyn PlayerIpc>)
        .or_else(|| {
            args.cdp_url
                .as_ref()
                .map(|url| Box::new(ChromeYoutubeIpc::connect(url)) as Box<dyn PlayerIpc>)
        });

    let (conn, screen_number) = xcb::Connection::connect(None)?;

    conn.prefetch_maximum_request_length();

    let screen = conn
        .get_setup()
        .roots()
        .nth(screen_number as usize)
        .unwrap();
    let window = conn.generate_id();
    let colormap = conn.generate_id();

    let visuals = screen
        .allowed_depths()
        .find(|d| d.depth() == 32)
        .unwrap()
        .visuals();
    let visual = visuals
        .iter()
        .find(|x| {
            x.bits_per_rgb_value() == 8 && {
                x.red_mask() == 0xFF0000 && x.green_mask() == 0x00FF00 && x.blue_mask() == 0x0000FF
            }
        })
        .unwrap();

    let cookie = conn.send_request_checked(&xcb::x::CreateColormap {
        visual: visual.visual_id(),
        alloc: xcb::x::ColormapAlloc::None,
        mid: colormap,
        window: screen.root(),
    });
    conn.check_request(cookie)?;

    conn.send_and_check_request(&xcb::x::CreateWindow {
        depth: 32,
        wid: window,
        parent: screen.root(),
        x: 0,
        y: 0,
        width: 640,
        height: 480,
        border_width: 0,
        class: xcb::x::WindowClass::InputOutput,
        visual: visual.visual_id(),
        value_list: &[
            xcb::x::Cw::BackPixel(0),
            xcb::x::Cw::BorderPixel(0),
            xcb::x::Cw::Colormap(colormap),
        ],
    })?;

    if args.overlay_window.is_some() {
        conn.send_and_check_request(&xcb::x::ChangeWindowAttributes {
            window,
            value_list: &[xcb::x::Cw::OverrideRedirect(true)],
        })?;
        conn.send_and_check_request(&xcb::shape::Rectangles {
            operation: xcb::shape::So::Set,
            destination_kind: xcb::shape::Sk::Input,
            ordering: xcb::x::ClipOrdering::Unsorted,
            destination_window: window,
            x_offset: 0,
            y_offset: 0,
            rectangles: &[],
        })?;
    }

    conn.send_request(&xcb::x::MapWindow { window });

    let gc = conn.generate_id();
    conn.send_and_check_request(&xcb::x::CreateGc {
        drawable: xcb::x::Drawable::Window(window),
        cid: gc,
        value_list: &[xcb::x::Gc::SubwindowMode(
            xcb::x::SubwindowMode::IncludeInferiors,
        )],
    })?;

    // TODO: get and scale by dpi

    let mut render = Renderer::new(&sbr, &subs);
    let mut buffer = Vec::<u32>::new();

    let start = Instant::now();
    loop {
        let geometry = if let Some(id) = args.overlay_window {
            let geometry = conn.wait_for_reply(conn.send_request(&xcb::x::GetGeometry {
                drawable: xcb::x::Drawable::Window(unsafe { xcb::x::Window::new(id) }),
            }))?;

            conn.send_request(&xcb::x::ConfigureWindow {
                window,
                value_list: &[
                    xcb::x::ConfigWindow::X(geometry.x().into()),
                    xcb::x::ConfigWindow::Y(geometry.y().into()),
                    xcb::x::ConfigWindow::Width(geometry.width().into()),
                    xcb::x::ConfigWindow::Height(geometry.height().into()),
                    xcb::x::ConfigWindow::StackMode(xcb::x::StackMode::Above),
                ],
            });

            // TODO: Set clip shape to visible region of mpv window

            geometry
        } else {
            conn.wait_for_reply(conn.send_request(&xcb::x::GetGeometry {
                drawable: xcb::x::Drawable::Window(window),
            }))?
        };

        let mut ctx = SubtitleContext {
            dpi: args.dpi,
            video_width: geometry.width() as f32,
            video_height: geometry.height() as f32,
            padding_left: 0.0,
            padding_right: 0.0,
            padding_top: 0.0,
            padding_bottom: 0.0,
        };

        let (mut s_width, mut s_height) = (geometry.width(), geometry.height());
        let mut voffset_x = 0;
        let mut voffset_y = 0;

        let now = Instant::now();

        let t = if let Some(ref mut ipc) = ipc_socket {
            let state = ipc.get_state();

            if let Some(PlayerViewport { offset_x, offset_y }) = state.viewport {
                s_width -= offset_x as u16;
                s_height -= offset_y as u16;
                voffset_x = offset_x;
                voffset_y = offset_y;
            }

            if let Some(PlayerDimensions {
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
                s_width = s_width.min(player_width.ceil() as u16);
                s_height = s_height.min(player_height.ceil() as u16);
            }

            state.current_time
        } else {
            ((now - start).as_secs_f64() * args.speed * 1000.) as u32 + args.start_at
        };

        let (width, height) = (s_width as u32, s_height as u32);
        buffer.resize(width as usize * height as usize, 0);

        let render_start = Instant::now();

        let mut painter = Painter::new(width, height, buffer.as_mut_slice());
        println!("render t = {}ms to {}x{}", t, width, height);
        render.render(&ctx, t, &mut painter);
        let end = Instant::now();
        println!(
            "took {:.2}ms",
            (end - render_start).as_micros() as f64 / 1000.
        );

        large_zpixmap32_putimage(
            &conn,
            xcb::x::Drawable::Window(window),
            gc,
            unsafe {
                std::slice::from_raw_parts(
                    buffer.as_ptr() as *const u8,
                    std::mem::size_of_val(buffer.as_slice()),
                )
            },
            (voffset_x as i16, voffset_y as i16),
            s_width,
            s_width as usize * 4,
            s_height,
        )?;

        std::thread::sleep(std::time::Duration::from_secs_f32(
            (args.target_fps.recip() - (end - now).as_secs_f32()).max(0.0),
        ));
    }
}
