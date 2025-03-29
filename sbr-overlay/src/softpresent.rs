use anyhow::{Context, Result, bail};
use winit::raw_window_handle::HasDisplayHandle;

pub mod x11;

pub enum Presenter {
    X11(x11::Presenter),
}

impl Presenter {
    pub fn new(window: &winit::window::Window) -> Result<Presenter> {
        match window
            .display_handle()
            .context("Failed to get system display handle")?
            .as_raw()
        {
            winit::raw_window_handle::RawDisplayHandle::Xlib(handle) => {
                Ok(Presenter::X11(unsafe {
                    x11::Presenter::from_xlib(
                        handle.display.unwrap().as_ptr(),
                        x11::extract_window_handle_from_window(window)?,
                    )
                    .context("Failed to create X11 software presenter")?
                }))
            }
            winit::raw_window_handle::RawDisplayHandle::Xcb(handle) => Ok(Presenter::X11(unsafe {
                x11::Presenter::from_xcb(
                    handle.connection.unwrap().as_ptr(),
                    x11::extract_window_handle_from_window(window)?,
                )
                .context("Failed to create X11 software presenter")?
            })),
            handle => bail!("Software presentation is not supported for {handle:?}"),
        }
    }

    pub fn present(&self, buffer: &[u8], offset: (i16, i16), size: (u32, u32)) -> Result<()> {
        match self {
            Presenter::X11(x11) => x11.present(buffer, offset, size).map_err(Into::into),
        }
    }
}
