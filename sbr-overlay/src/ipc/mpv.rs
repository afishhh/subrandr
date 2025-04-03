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
        Ok(MpvSocket {
            stream: BufReader::new(UnixStream::connect(path)?),
        })
    }

    fn get_property<T: DeserializeOwned>(&mut self, key: &str) -> Result<T> {
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

impl super::PlayerConnection for MpvSocket {
    fn get_window_id(&mut self) -> Option<u32> {
        Some(self.get_property("window-id").unwrap())
    }

    fn get_stream_path(&mut self) -> Result<Option<PathBuf>> {
        Ok(Some(
            self.get_property::<PathBuf>("working-directory")
                .unwrap()
                .join(self.get_property::<PathBuf>("stream-path").unwrap()),
        ))
    }

    fn poll(&mut self) -> super::PlayerState {
        super::PlayerState {
            dimensions: None,
            viewport: None,
            current_time: (self.get_property::<f32>("playback-time").unwrap() * 1000.) as u32,
        }
    }
}

pub struct Connector;

impl super::PlayerConnector for Connector {
    fn try_connect(&self, connection_string: &str) -> Result<Box<dyn super::PlayerConnection>> {
        Ok(
            Box::new(MpvSocket::connect(PathBuf::from(connection_string))?)
                as Box<dyn super::PlayerConnection>,
        )
    }
}
