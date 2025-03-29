use std::net::TcpStream;

use anyhow::Result;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

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

impl super::PlayerConnection for ChromeYoutubeIpc {
    fn poll(&mut self) -> super::PlayerState {
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

pub struct Connector;

impl super::PlayerConnector for Connector {
    fn try_connect(&self, connection_string: &str) -> Result<Box<dyn super::PlayerConnection>> {
        // TODO: Proper error propagation here
        Ok(Box::new(ChromeYoutubeIpc::connect(connection_string))
            as Box<dyn super::PlayerConnection>)
    }
}
