use std::{
    error::Error, fmt::Debug, ops::IndexMut, os::unix::ffi::OsStrExt, path::PathBuf, time::Instant,
};

use clap::Parser;
use subrandr::{Renderer, Subtitles};
use xcb::{Xid, XidNew};

#[derive(clap::Parser)]
struct Args {
    file: Option<PathBuf>,
    #[clap(long = "dpi", default_value_t = 72)]
    dpi: u32,
    #[clap(long = "start", default_value_t = 0)]
    start_at: u32,
    #[clap(long = "parse")]
    parse: bool,
    #[clap(long = "overlay")]
    overlay_window: Option<u32>,
}

fn main() -> Result<(), Box<dyn Error + 'static>> {
    let args = Args::parse();

    let subs = if let Some(file) = args.file {
        match file.extension().map(|x| x.as_bytes()) {
            Some(b"srv3" | b"ytt") => {
                let document =
                    subrandr::srv3::parse(&std::fs::read_to_string(file).unwrap()).unwrap();
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

    let (conn, screen_number) = xcb::Connection::connect(None)?;

    let screen = conn
        .get_setup()
        .roots()
        .nth(screen_number as usize)
        .unwrap();
    let window = conn.generate_id();
    let colormap = conn.generate_id();

    // let r = conn.wait_for_reply(conn.send_request(&xcb::bigreq::Enable {}))?;
    // println!("{} {} {:?}", r.length(), r.maximum_request_length(), r);

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

    if let Some(_) = args.overlay_window {
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

    let mut render = Renderer::new(0, 0, &subs, args.dpi);
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

        let (mut s_width, mut s_height) = (geometry.width(), geometry.height());
        // s_width = 1280;
        // s_height = 720;

        let (width, height) = (s_width as u32, s_height as u32);
        render.resize(width, height);
        let now = Instant::now();
        let t = (now - start).as_millis() as u32 + args.start_at;
        println!("render t = {}ms to {}x{}", t, width, height);
        render.render(t);
        let end = Instant::now();
        println!("took {:.2}ms", (end - now).as_micros() as f64 / 1000.);

        // FIXME: X11 expects ARGB, maybe everything should be switched to ARGB?
        let bitmap = render.bitmap();
        let mut new_bitmap = vec![0u8; s_width as usize * s_height as usize * 4];
        for i in (0..new_bitmap.len()).step_by(4) {
            new_bitmap[i] = bitmap[i + 2];
            new_bitmap[i + 1] = bitmap[i + 1];
            new_bitmap[i + 2] = bitmap[i];
            new_bitmap[i + 3] = bitmap[i + 3];
        }

        conn.check_request(conn.send_request_checked(&xcb::x::PutImage {
            format: xcb::x::ImageFormat::ZPixmap,
            drawable: xcb::x::Drawable::Window(window),
            gc,
            width: s_width,
            height: s_height,
            dst_x: 0,
            dst_y: 0,
            left_pad: 0,
            depth: 32,
            data: &new_bitmap,
        }))?;
    }

    // Ok(())
}
