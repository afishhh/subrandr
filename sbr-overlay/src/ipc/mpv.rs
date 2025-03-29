use std::{
    io::{BufRead as _, BufReader, Write as _},
    os::unix::net::UnixStream,
    path::PathBuf,
    str::FromStr,
};

use anyhow::Result;

struct MpvSocket {
    stream: BufReader<UnixStream>,
}

impl MpvSocket {
    fn connect(path: PathBuf) -> std::io::Result<MpvSocket> {
        Ok(MpvSocket {
            stream: BufReader::new(UnixStream::connect(path)?),
        })
    }

    fn get_property<T: FromStr>(&mut self, key: &str) -> T
    where
        <T as std::str::FromStr>::Err: std::fmt::Debug,
    {
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
                self.stream.read_line(&mut line).unwrap();

                if line.contains("property unavailable") {
                    break;
                }

                if let Some(data_idx) = line.find(r#""data""#) {
                    let colon_idx = data_idx + line[data_idx..].find(":").unwrap() + 1;
                    let comma_idx = colon_idx + line[colon_idx..].find(',').unwrap();
                    return line[colon_idx..comma_idx].trim().parse::<T>().unwrap();
                    //     as u32;
                }
            }
        }
    }
}

impl super::PlayerConnection for MpvSocket {
    fn get_window_id(&mut self) -> Option<u32> {
        Some(dbg!(self.get_property("window-id")))
    }

    fn poll(&mut self) -> super::PlayerState {
        super::PlayerState {
            dimensions: None,
            viewport: None,
            current_time: (self.get_property::<f32>("playback-time") * 1000.) as u32,
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
