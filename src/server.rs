use std::collections::HashMap;
use std::marker::Unpin;
use std::sync::Arc;

use anyhow::Result;
use futures_util::sink::SinkExt;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, ToSocketAddrs},
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
};
use tokio_stream::{wrappers::TcpListenerStream, StreamExt};
use tokio_util::codec::{Framed, LinesCodec};

use crate::{ChatFrame, ClientMessage, ServerMessage};

/// Implementation Note: Originally tried to use the tokio::sync::broadcast
/// channel type, but the requirement is not to send the message to the sender.
/// Similarly, trying to store the actual network handle inside this set
/// results in some bad borrowing failures.
struct ClientSet {
    inner: Mutex<HashMap<String, Sender<ServerMessage>>>,
}

impl ClientSet {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Registers the nick if it is not already in use
    async fn register(&self, nick: &str, sender: Sender<ServerMessage>) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if inner.contains_key(nick) {
            return Err(anyhow::Error::msg(format!(
                "nick already registered: {nick}"
            )));
        }

        inner.insert(nick.to_string(), sender);

        Ok(())
    }

    /// De-registers the nick, a nick that is not registered is not remov
    async fn deregister(&self, nick: &str) {
        let mut inner = self.inner.lock().await;
        inner.remove(nick);
    }
}

/// Start the server to listen for incoming connections
pub async fn run<A: ToSocketAddrs>(addr: A) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let mut listener = TcpListenerStream::new(listener);

    let nick_set = Arc::new(ClientSet::new());

    while let Some(socket) = listener.next().await {
        if let Ok(socket) = socket {
            handle_incoming(socket, nick_set.clone()).await?;
        }
    }

    Ok(())
}

async fn handle_incoming<S>(socket: S, nick_set: Arc<ClientSet>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut socket = Framed::new(socket, LinesCodec::new());
    if let Some(msg) = socket.next().await {
        match serde_json::from_str(&msg?)? {
            ClientMessage::Connect(nick) => {
                let (mut clt, tx) = ClientHandle::new(&nick, nick_set, socket);
                match clt.nick_set.register(&clt.nick, tx).await {
                    Ok(()) => {
                        clt.send(ServerMessage::Connected).await?;
                        clt.publish(ServerMessage::Join(clt.nick.clone())).await?;
                        eprintln!("User has joined the channel: {}", clt.nick);
                        tokio::spawn(async move { clt.handle_client().await });
                    }
                    Err(err) => {
                        clt.send(ServerMessage::Error(err.to_string())).await?;
                    }
                }
            }
            msg => {
                return Err(anyhow::Error::msg(format!(
                    "expected nick registration, received: {msg:?}"
                )));
            }
        }
    }

    Ok(())
}

struct ClientHandle<T> {
    nick: String,
    nick_set: Arc<ClientSet>,
    socket: ChatFrame<T>,
    receiver: Receiver<ServerMessage>,
}

impl<T> ClientHandle<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn new(
        nick: &str,
        nick_set: Arc<ClientSet>,
        socket: ChatFrame<T>,
    ) -> (Self, Sender<ServerMessage>) {
        let (tx, rx) = mpsc::channel::<ServerMessage>(16);
        let clt = ClientHandle {
            nick: nick.to_string(),
            nick_set,
            socket,
            receiver: rx,
        };

        (clt, tx)
    }

    async fn send(&mut self, msg: ServerMessage) -> Result<()> {
        let msg_json = serde_json::to_string(&msg)?;
        self.socket.send(msg_json).await?;
        Ok(())
    }

    /// Loops through the list of other clients to send them the message
    async fn publish(&mut self, msg: ServerMessage) -> Result<()> {
        let mut inner = self.nick_set.inner.lock().await;
        for (nick, sender) in inner.iter_mut() {
            if *nick == self.nick {
                continue;
            }
            sender.send(msg.clone()).await?;
        }
        Ok(())
    }

    async fn handle_client(mut self) -> Result<()> {
        loop {
            tokio::select!(
                incoming = self.socket.next() => {
                    if let Some(incoming) = incoming {
                        match serde_json::from_str(&incoming?)? {
                            ClientMessage::SendMsg(incoming) => {
                                eprintln!("Message received - {}: {}", self.nick, incoming);
                                let msg = ServerMessage::Message(self.nick.clone(), incoming);
                                self.publish(msg).await?;
                            }
                            ClientMessage::Leave => {
                                eprintln!("User has left the channel: {}", self.nick);
                                self.publish(ServerMessage::Leave(self.nick.clone())).await?;
                                self.nick_set.deregister(&self.nick).await;
                                break;
                            }
                            _ => self.send(ServerMessage::Error("Unrecognized message".to_string())).await?,
                        }
                    }
                },
                msg = self.receiver.recv() => {
                    if let Some(msg) = msg {
                        self.send(msg).await?;
                    }
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn register_twice() {
        use tokio::sync::mpsc;
        let (tx, _) = mpsc::channel(16);
        let nick_set = std::sync::Arc::new(crate::server::ClientSet::new());
        let nick = "Nick1";
        nick_set.register(nick, tx.clone()).await.unwrap();

        let nick = "Nick1";
        nick_set.register(nick, tx.clone()).await.unwrap_err();
    }

    #[tokio::test]
    async fn register_deregister() {
        use tokio::sync::mpsc;
        let (tx, _) = mpsc::channel(16);
        let nick_set = std::sync::Arc::new(crate::server::ClientSet::new());
        let nick = "Nick1";
        nick_set.register(nick, tx.clone()).await.unwrap();

        let nick = "Nick1";
        nick_set.register(nick, tx.clone()).await.unwrap_err();
        nick_set.deregister(nick).await;
        nick_set.register(nick, tx.clone()).await.unwrap();
    }
}
