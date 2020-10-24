//! Connects to and allows communication with the Twitch Messaging Interface
//!
//! Basic usage:
//! ```no_run
//! use madhouse_steve_tmi::Tmi;
//!
//! let oauth_token = String::from("oauth:some_token_here");
//! let nick = String::from("MadSteveBot");
//! let rooms = vec![String::from("MadhouseSteve")];
//! let mut tmi = Tmi::new(oauth_token, nick, rooms);
//! let (join_handle, receiver) = tmi.start();
//! loop {
//!     let msg = receiver.recv();
//!     if msg.is_err() {
//!         break;
//!     }
//!
//!     let msg = msg.unwrap();
//!     // Do something with the message here
//! }
//!
//! join_handle.join().unwrap();
//! ```
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver};
use std::thread::{spawn, JoinHandle};

/// The structure to handle the Twitch Messaging Interface
///
/// Examples are available in the top level Crate documentation
pub struct Tmi {
    oauth: String,
    nick: String,
    rooms: Vec<String>,
    sock: TcpStream,
    writer: BufWriter<TcpStream>,
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
        let sock = TcpStream::connect("irc.chat.twitch.tv:6667").expect("Cannot connect");
        let writer = BufWriter::new(sock.try_clone().unwrap());

        let mut tmi = Tmi {
            oauth,
            nick,
            rooms,
            sock,
            writer,
        };

        tmi.authenticate();
        tmi.join_all();

        tmi
    }

    fn authenticate(&mut self) {
        self.send(String::from(
            "CAP REQ :twitch.tv/tags twitch.tv/commands twitch.tv/membership",
        ));
        self.send(format!("PASS {}", self.oauth));
        self.send(format!("NICK {}", self.nick));
    }

    fn join_all(&mut self) {
        if self.rooms.len() == 0 {
            return;
        }
        let iter = self.rooms.clone();
        for channel in iter {
            self.send(format!("JOIN {}", channel));
        }
    }

    /// Sends a raw message to the IRC server
    pub fn send(&mut self, message: String) {
        let message = message + "\r\n";
        self.writer.write(message.as_bytes()).unwrap();
        self.writer.flush().unwrap();
    }

    /// Sends a message in to the specified channel
    pub fn send_to_channel(&mut self, message: String, channel: String) {
        self.writer
            .write(format!("PRIVMSG {} :{}", channel, message).as_bytes())
            .unwrap();
        self.writer.flush().unwrap();
    }

    /// Starts the polling thread, returning a receiver channel and a join handle
    pub fn start(&mut self) -> (JoinHandle<()>, Receiver<DecodedMessage>) {
        let (tx, rx) = channel();
        let mut local_reader = BufReader::new(self.sock.try_clone().unwrap());
        let mut local_writer = BufWriter::new(self.sock.try_clone().unwrap());
        let t = spawn(move || loop {
            let mut message = String::new();
            let read_result = local_reader.read_line(&mut message);
            if read_result.is_err() {
                break;
            }
            let message = message.trim();

            let lines = message.split("\r\n");
            for line in lines {
                if line.starts_with("PING ") {
                    local_writer
                        .write(line.replace("PING ", "PONG ").as_bytes())
                        .unwrap();
                    local_writer.flush().unwrap();
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
