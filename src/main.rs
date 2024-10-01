use core::f32;
use std::{error::Error, time::Instant};

// use macroquad::{
//     camera::{set_camera, Camera2D},
//     color::WHITE,
//     math::vec2,
//     miniquad::window::screen_size,
//     texture::Texture2D,
//     window::next_frame,
// };
// use miniquad::window::dpi_scale;
use subrandr::{Renderer, Subtitles};
use xcb::Xid;

// fn window_conf() -> miniquad::conf::Conf {
//     miniquad::conf::Conf {
//         window_title: "subrandr test".to_owned(),
//         high_dpi: true,
//         platform: miniquad::conf::Platform {
//             framebuffer_alpha: true,
//             linux_x11_gl: miniquad::conf::LinuxX11Gl::EGLOnly,
//             linux_backend: miniquad::conf::LinuxBackend::X11Only,
//             ..Default::default()
//         },
//         ..Default::default()
//     }
// }

fn main() -> Result<(), Box<dyn Error + 'static>> {
    let mut render = Renderer::new(0, 0, &Subtitles::empty(), 72 /* (dpi_scale() * 72.0) */ as u32);
    // dbg!(subrandr::ass::split_into_sections(
    //     &std::fs::read_to_string("/home/fishhh/sync/downloads/m2/test/test.ass").unwrap()
    // )
    // .unwrap());
    let script = subrandr::ass::parse_ass(
        &std::fs::read_to_string("/home/fishhh/sync/downloads/m2/test/test.ass").unwrap(),
    )
    .unwrap();
    // dbg!(&script);
    let script_subs = subrandr::ass_to_subs(script);

    let (conn, screen_number) = xcb::Connection::connect(None)?;

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
        .find(|x| x.bits_per_rgb_value() == 8)
        .unwrap();

    let cookie = conn.send_request_checked(&xcb::x::CreateColormap {
        visual: visual.visual_id(),
        alloc: xcb::x::ColormapAlloc::None,
        mid: colormap,
        window: screen.root(),
    });
    conn.check_request(cookie)?;

    let cookie = conn.send_request_checked(&xcb::x::CreateWindow {
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
    });
    conn.check_request(cookie)?;

    conn.send_request(&xcb::x::MapWindow { window });

    let gc = conn.generate_id();
    conn.send_and_check_request(&xcb::x::CreateGc {
        drawable: xcb::x::Drawable::Window(window),
        cid: gc,
        value_list: &[],
    })?;

    // TODO: get and scale by dpi

    let aaa = Subtitles::test_new();
    let mut render = Renderer::new(0, 0, &script_subs, 72 /* (dpi_scale() * 72.0) */ as u32);
    let start = Instant::now();
    loop {
        let geometry = conn.wait_for_reply(conn.send_request(&xcb::x::GetGeometry {
            drawable: xcb::x::Drawable::Window(window),
        }))?;

        let (s_width, s_height) = (geometry.width(), geometry.height());

        let (width, height) = (s_width as u32, s_height as u32);
        render.resize(width, height);
        let now = Instant::now();
        let t = (now - start).as_millis() as u32;
        println!("render t = {}ms to {}x{}", t, width, height);
        render.render(t);
        let end = Instant::now();
        println!("took {:.2}ms", (end - now).as_micros() as f64 / 1000.);

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
            data: render.bitmap(),
        }))?;
    }

    Ok(())
}
