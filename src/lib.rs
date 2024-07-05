use serde::{Deserialize, Serialize};

pub mod server;

#[derive(Serialize, Deserialize)]
pub enum ServerMessage {
    Message(String),
    Error(String),
    Success,
}

#[derive(Serialize, Deserialize)]
pub enum ClientMessage {
    Connect(String),
    SendMsg(String),
    Leave,
}
