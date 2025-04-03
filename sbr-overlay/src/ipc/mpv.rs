use std::{
    io::{BufRead as _, BufReader, Write as _},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use anyhow::Result;
use serde::{Deserialize, de::DeserializeOwned};

struct MpvSocket {
    stream: BufReader<UnixStream>,
}

impl MpvSocket {
    fn connect(path: PathBuf) -> std::io::Result<MpvSocket> {
        Ok(Self {
            stream: BufReader::new(UnixStream::connect(path)?),
        })
    }

    fn get_property<T: DeserializeOwned>(
        &mut self,
        key: &str,
        mut track_switched: Option<&mut bool>,
    ) -> Result<T> {
        loop {
            self.stream
                .get_mut()
                .write_all(
                    format!(
                        concat!(r#"{{ "command": ["get_property", "{key}"] }}"#, "\n"),
                        key = key
                    )
                    .as_bytes(),
                )
                .unwrap();

            let mut line = String::new();
            loop {
                line.clear();
                self.stream.read_line(&mut line).unwrap();

                if line.contains("start-file") {
                    if let Some(flag) = track_switched.as_deref_mut() {
                        *flag = true;
                    }
                    continue;
                }

                if line.contains("property unavailable") {
                    break;
                }

                #[derive(Deserialize)]
                struct DataResponse<T> {
                    data: T,
                }

                if line.contains(r#""data""#) {
                    return serde_json::from_str::<DataResponse<T>>(&line)
                        .map(|r| r.data)
                        .map_err(Into::into);
                }
            }
        }
    }
}

struct MpvConnection {
    socket: MpvSocket,
    remote_working_dir: PathBuf,
}

impl MpvConnection {
    fn connect(path: PathBuf) -> Result<Self> {
        Ok({
            let mut socket = MpvSocket::connect(path)?;
            Self {
                remote_working_dir: socket.get_property("working-directory", None)?,
                socket,
            }
        })
    }
}

impl super::PlayerConnection for MpvConnection {
    fn get_window_id(&mut self) -> Option<u32> {
        Some(self.socket.get_property("window-id", None).unwrap())
    }

    fn get_stream_path(&mut self) -> Result<Option<PathBuf>> {
        Ok(Some(
            self.remote_working_dir.join(
                self.socket
                    .get_property::<PathBuf>("stream-path", None)
                    .unwrap(),
            ),
        ))
    }

    fn poll(&mut self, track_switched: &mut bool) -> super::PlayerState {
        super::PlayerState {
            dimensions: None,
            viewport: None,
            current_time: (self
                .socket
                .get_property::<f32>("playback-time", Some(track_switched))
                .unwrap()
                * 1000.) as u32,
        }
    }
}

pub struct Connector;

impl super::PlayerConnector for Connector {
    fn try_connect(&self, connection_string: &str) -> Result<Box<dyn super::PlayerConnection>> {
        Ok(
            Box::new(MpvConnection::connect(PathBuf::from(connection_string))?)
                as Box<dyn super::PlayerConnection>,
        )
    }
}
