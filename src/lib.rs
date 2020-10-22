//! Connects to and allows communication with the Twitch Messaging Interface
//!
//! Basic usage:
//! ```
//! let oauth_token = String::from("oauth:some_token_here");
//! let nick = String::from("MadSteveBot");
//! let rooms = String::from(vec!["MadhouseSteve"]);
//! let tmi = Tmi::new(oauth_token, nick, rooms);
//! let (join_handle, receiver) = tmi.start();
//! loop {
//!     let msg = receiver.recv();
//!     if msg.is_err {
//!         break;
//!     }
//!
//!     // Do something with the message here
//! }
//!
//! t.join().unwrap();
//! ```
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{spawn, JoinHandle};
use tungstenite::client::AutoStream;
use tungstenite::http::{HeaderValue, Request};
use tungstenite::{Message, WebSocket};

/// The structure to handle the Twitch Messaging Interface
///
/// Examples are available in the top level Crate documentation
pub struct Tmi {
    ws: Arc<Mutex<WebSocket<AutoStream>>>,

    oauth: String,
    nick: String,
    rooms: Vec<String>,
}

/// The parsed content of a TMI message
#[derive(Debug)]
pub struct DecodedMessage {
    /// Contains all the metadata in a map from the TMI message
    pub metadata: HashMap<String, String>,

    /// The server or user name from which the message originated
    pub from: String,

    /// The command that the IRC server sent. These are IRC commands as per section 3 of https://tools.ietf.org/html/rfc2821
    pub command: String,

    /// Where the message was sent (e.g. channel, or direct to user)
    pub target: Option<String>,

    /// The parameters of the command, for example the message content for PRIVMSG
    pub params: String,
}

impl Tmi {
    /// Creates a new Twitch Messaging Interface structure
    pub fn new(oauth: String, nick: String, rooms: Vec<String>) -> Tmi {
        let mut request = Request::get("wss://irc-ws.chat.twitch.tv")
            .body(())
            .unwrap();

        request.headers_mut().insert(
            "Sec-Websocket-Protocol",
            HeaderValue::from_str("tmi".into()).unwrap(),
        );
        let (ws, _response) =
            tungstenite::client::connect(request).expect("Unable to connect to TMI");
        let ws = Arc::new(Mutex::new(ws));

        let mut tmi = Tmi {
            oauth,
            nick,
            rooms,
            ws,
        };

        tmi.authenticate();
        tmi.join_all();

        tmi
    }

    fn authenticate(&mut self) {
        self.send(String::from(
            "CAP REQ :twitch.tv/tags twitch.tv/commands twitch.tv/membership",
        ))
        .unwrap();
        self.send(format!("PASS {}", self.oauth)).unwrap();
        self.send(format!("NICK {}", self.nick)).unwrap();
    }

    fn join_all(&mut self) {
        if self.rooms.len() == 0 {
            return;
        }
        let iter = self.rooms.clone();
        for channel in iter {
            self.send(format!("JOIN {}", channel)).unwrap();
        }
    }

    /// Sends a raw message to the IRC server
    pub fn send(&mut self, message: String) -> Result<(), tungstenite::Error> {
        self.ws
            .lock()
            .unwrap()
            .write_message(Message::from(message))
    }

    /// Sends a message in to the specified channel
    pub fn send_to_channel(
        &mut self,
        message: String,
        channel: String,
    ) -> Result<(), tungstenite::Error> {
        self.send(format!("PRIVMSG {} :{}", channel, message))
    }

    /// Starts the polling thread, returning a receiver channel and a join handle
    pub fn start(&mut self) -> (JoinHandle<()>, Receiver<DecodedMessage>) {
        let (tx, rx) = channel();
        let local_ws = self.ws.clone();
        let t = spawn(move || loop {
            // TODO - Make sure this breaks if the read fails
            let mut l_ws = local_ws.lock().unwrap();
            let message = l_ws.read_message().unwrap().to_string();
            let message = message.trim();

            let lines = message.split("\r\n");
            for line in lines {
                if line.starts_with("PING ") {
                    l_ws.write_message(Message::from(line.replace("PING ", "PONG ")))
                        .unwrap();
                } else {
                    tx.send(parse_message(line.into())).unwrap();
                }
            }
        });

        (t, rx)
    }
}

fn parse_message(message: String) -> DecodedMessage {
    let mut metadata = HashMap::new();

    let mut chunks: Vec<String> = message.split(" ").map(|s| s.to_string()).collect();
    if chunks[0].starts_with("@") == true {
        // Parse metadata here
        for entry in chunks[0].split(";").into_iter() {
            let parts: Vec<&str> = entry.split("=").collect();
            metadata.insert(parts[0].to_string(), parts[1..].join("=").to_string());
        }
        chunks.drain(0..1);
    }

    // Parse from
    let from: String = chunks.drain(0..1).next().unwrap()[1..]
        .split("@")
        .next()
        .unwrap()
        .split("!")
        .next()
        .unwrap()
        .into();

    // Parse command
    let command: String = chunks.drain(0..1).next().unwrap().into();

    // Get target and params if they exist
    let mut target = None;
    let mut params = String::new();
    if chunks.len() != 0 {
        target = Some(chunks.drain(0..1).next().unwrap().into());
        params = chunks.join(" ").into();

        if params.starts_with(":") {
            params = params[1..].into();
        }
    }

    DecodedMessage {
        metadata,
        from,
        command,
        target,
        params,
    }
}
