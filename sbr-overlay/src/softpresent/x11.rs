use std::mem::ManuallyDrop;

use anyhow::{Context, Result};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use xcb::XidNew;

pub struct Presenter {
    conn: ManuallyDrop<xcb::Connection>,
    gc: xcb::x::Gcontext,
    window: xcb::x::Window,
}

pub fn extract_window_handle_from_raw(handle: RawWindowHandle) -> Option<xcb::x::Window> {
    match handle {
        RawWindowHandle::Xlib(handle) => Some(unsafe { xcb::x::Window::new(handle.window as u32) }),
        RawWindowHandle::Xcb(handle) => Some(unsafe { xcb::x::Window::new(handle.window.get()) }),
        _ => None,
    }
}

pub fn extract_window_handle_from_window(window: &winit::window::Window) -> Result<xcb::x::Window> {
    extract_window_handle_from_raw(
        window
            .window_handle()
            .context("Failed to get system window handle")?
            .as_raw(),
    )
    .context("Window handle incompatible with X11 display")
}

impl Presenter {
    fn from_connection_and_window(
        conn: ManuallyDrop<xcb::Connection>,
        window: xcb::x::Window,
    ) -> xcb::Result<Self> {
        let gc = conn.generate_id();
        conn.send_and_check_request(&xcb::x::CreateGc {
            drawable: xcb::x::Drawable::Window(window),
            cid: gc,
            value_list: &[xcb::x::Gc::SubwindowMode(
                xcb::x::SubwindowMode::IncludeInferiors,
            )],
        })
        .unwrap();

        Ok(Self { conn, window, gc })
    }

    pub unsafe fn from_xlib(
        display: *mut std::ffi::c_void,
        window: xcb::x::Window,
    ) -> xcb::Result<Self> {
        Self::from_connection_and_window(
            ManuallyDrop::new(unsafe { xcb::Connection::from_xlib_display(display as *mut _) }),
            window,
        )
    }

    pub unsafe fn from_xcb(
        xcb_conn: *mut std::ffi::c_void,
        window: xcb::x::Window,
    ) -> xcb::Result<Self> {
        Self::from_connection_and_window(
            ManuallyDrop::new(unsafe { xcb::Connection::from_raw_conn(xcb_conn as *mut _) }),
            window,
        )
    }
}

#[allow(clippy::too_many_arguments)]
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

impl Presenter {
    pub fn present(
        &self,
        buffer: &[u8],
        offset: (i16, i16),
        (width, height): (u32, u32),
    ) -> xcb::Result<()> {
        large_zpixmap32_putimage(
            &self.conn,
            xcb::x::Drawable::Window(self.window),
            self.gc,
            buffer,
            (offset.0 as i16, offset.1 as i16),
            width as u16,
            width as usize * 4,
            height as u16,
        )
    }
}
