use std::path::PathBuf;

use anyhow::Result;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "_serde", derive(serde::Deserialize))]
pub struct PlayerDimensions {
    pub video_width: f32,
    pub video_height: f32,
    pub player_width: f32,
    pub player_height: f32,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "_serde", derive(serde::Deserialize))]
pub struct PlayerViewport {
    pub offset_x: u32,
    pub offset_y: u32,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "_serde", derive(serde::Deserialize))]
pub struct PlayerState {
    #[cfg_attr(feature = "_serde", serde(flatten))]
    pub dimensions: Option<PlayerDimensions>,
    pub viewport: Option<PlayerViewport>,
    // in ms
    pub current_time: u32,
}

pub trait PlayerConnector {
    fn try_connect(&self, connection_string: &str) -> Result<Box<dyn PlayerConnection>>;
}

pub trait PlayerConnection {
    fn get_window_id(&mut self) -> Option<u32> {
        None
    }

    fn get_stream_path(&mut self) -> Result<Option<PathBuf>> {
        Ok(None)
    }

    // TODO: Make this fallible
    fn poll(&mut self, track_switched: &mut bool) -> PlayerState;
}

#[cfg(feature = "ipc-browser-cdp")]
mod cdp;
#[cfg(all(feature = "ipc-mpv", target_os = "linux"))]
mod mpv;

pub struct PlayerConnectorDescriptor {
    pub id: &'static str,
    pub description: &'static str,
    pub connector: &'static dyn PlayerConnector,
}

pub const AVAILABLE_CONNECTORS: &[PlayerConnectorDescriptor] = &[
    #[cfg(all(feature = "ipc-mpv", target_os = "linux"))]
    PlayerConnectorDescriptor {
        id: "mpv",
        description: "mpv player IPC socket",
        connector: &mpv::Connector,
    },
    #[cfg(feature = "ipc-browser-cdp")]
    PlayerConnectorDescriptor {
        id: "youtube-cdp",
        description: "browser YouTube tab via Chrome DevTools Protocol",
        connector: &cdp::Connector,
    },
];
