use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::sink::SinkExt;
use tokio::{
    net::{tcp::OwnedWriteHalf, TcpStream, ToSocketAddrs},
    runtime::Runtime,
};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use crate::{ClientMessage, ServerMessage};

#[derive(Debug, Default)]
pub struct Messages {
    messages: std::sync::Mutex<Vec<(String, String)>>,
}

impl Messages {
    pub fn push(&self, nick: &str, msg: &str) {
        self.messages
            .lock()
            .unwrap()
            .push((nick.to_string(), msg.to_string()))
    }

    pub fn get(&self) -> Vec<(String, String)> {
        self.messages.lock().unwrap().clone()
    }
}

#[derive(Debug)]
pub struct Connection {
    pub nick: String,
    sender: FramedWrite<OwnedWriteHalf, LinesCodec>,
    rt: Arc<Runtime>,
}

impl Connection {
    pub fn connect<A: ToSocketAddrs>(nick: &str, addr: A, messages: Arc<Messages>) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let socket = rt.block_on(async { TcpStream::connect(addr).await })?;
        let (receiver, sender) = socket.into_split();
        let mut receiver = FramedRead::new(receiver, LinesCodec::new());
        let sender = FramedWrite::new(sender, LinesCodec::new());
        let mut conn = Connection {
            nick: nick.to_string(),
            sender,
            rt: Arc::new(rt),
        };

        conn.send(ClientMessage::Connect(conn.nick.clone()))?;
        let reply = conn
            .rt
            .block_on(async { receiver.next().await })
            .ok_or(anyhow::Error::msg("error"))?;
        match serde_json::from_str(&reply?)? {
            ServerMessage::Connected => {
                messages.push("INFO", "Connected to server.");
            }
            ServerMessage::Error(err) => {
                return Err(anyhow::Error::msg(format!(
                    "failed to connect to server: {err}"
                )));
            }
            _ => {
                return Err(anyhow::Error::msg(
                    "received unexpected message".to_string(),
                ));
            }
        }

        let rt_inner = conn.rt.clone();
        std::thread::spawn(move || {
            let listener_task = rt_inner.spawn(async move {
                while let Some(incoming) = receiver.next().await {
                    let message = serde_json::from_str(&incoming?)?;
                    match message {
                        ServerMessage::Message(nick, msg) => {
                            messages.push(&nick, &msg);
                        }
                        ServerMessage::Error(err) => {
                            messages.push("ERROR", &err);
                        }
                        ServerMessage::Join(nick) => {
                            messages.push("SERVER", &format!("{nick} has joined the channel."));
                        }
                        ServerMessage::Leave(nick) => {
                            messages.push("SERVER", &format!("{nick} has left the channel."));
                        }
                        _ => {
                            // We don't care about any other message types in here
                            continue;
                        }
                    }
                }
                messages.push("INFO", "Server has closed the connection");
                anyhow::Ok(())
            });

            rt_inner.block_on(listener_task)??;
            anyhow::Ok(())
        });

        Ok(conn)
    }

    pub fn send(&mut self, msg: ClientMessage) -> Result<()> {
        let msg_json = serde_json::to_string(&msg)?;
        self.rt
            .block_on(async { self.sender.send(msg_json).await })
            .context("sending message to server")
    }
}
