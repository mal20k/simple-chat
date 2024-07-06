use serde::{Deserialize, Serialize};
use tokio_util::codec::{Framed, LinesCodec};

pub mod client;
pub mod server;

type ChatFrame<T> = Framed<T, LinesCodec>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Message(String, String),
    Error(String),
    Join(String),
    Leave(String),
    Connected,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    Connect(String),
    SendMsg(String),
    Leave,
}
