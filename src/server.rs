use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use futures_util::sink::SinkExt;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, LinesCodec};

use crate::{ClientMessage, ServerMessage};

/// Implementation Note: Originally tried to use the tokio::sync::broadcast
/// channel type, but the requirement is not to send the message to the sender.
/// Similarly, trying to store the actual network handle inside this set
/// results in some bad borrowing failures.
struct ClientSet {
    inner: Mutex<HashMap<String, Sender<String>>>,
}

impl ClientSet {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Registers the nick if it is not already in use
    async fn register(&self, nick: &str, sender: Sender<String>) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if inner.contains_key(nick) {
            return Err(anyhow::Error::msg("nick already registered: {nick}"));
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
        let socket = socket?;
        let mut socket = Framed::new(socket, LinesCodec::new());
        if let Some(msg) = socket.next().await {
            let (tx, rx) = mpsc::channel(16);
            if let ClientMessage::Connect(nick) = serde_json::from_str(&msg?)? {
                let mut clt = ClientHandle {
                    nick,
                    nick_set: nick_set.clone(),
                    socket,
                    receiver: rx,
                };

                match nick_set.register(&clt.nick, tx).await {
                    Ok(()) => {
                        clt.send(ServerMessage::Success).await?;
                        let connect = format!("{} has joined the channel", clt.nick);
                        clt.publish(&connect).await?;
                        tokio::spawn(async move { clt.handle_client().await });
                    }
                    Err(err) => {
                        clt.send(ServerMessage::Error(err.to_string())).await?;
                        continue;
                    }
                }
            }

            return Err(anyhow::Error::msg(
                "expected nick registration, received: {:?}",
            ));
        }
    }

    Ok(())
}

struct ClientHandle {
    nick: String,
    nick_set: Arc<ClientSet>,
    socket: Framed<TcpStream, LinesCodec>,
    receiver: Receiver<String>,
}

impl ClientHandle {
    async fn send(&mut self, msg: ServerMessage) -> Result<()> {
        let msg_json = serde_json::to_string(&msg)?;
        self.socket.send(msg_json).await?;
        Ok(())
    }

    /// Loops through the list of other clients to send them the message
    async fn publish(&mut self, msg: &str) -> Result<()> {
        let mut inner = self.nick_set.inner.lock().await;
        for (nick, sender) in inner.iter_mut() {
            if *nick == self.nick {
                continue;
            }

            sender.send(msg.to_string()).await?;
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
                                self.publish(&incoming).await?;
                            }
                            ClientMessage::Leave => {
                                self.nick_set.deregister(&self.nick).await;
                                self.send(ServerMessage::Success).await?;
                                let disconnect = format!("{} has disconnected", self.nick);
                                self.publish(&disconnect).await?;
                                break;
                            }
                            _ => todo!(),
                        }
                    }
                },
                msg = self.receiver.recv() => {
                    if let Some(msg) = msg {
                        self.send(ServerMessage::Message(msg)).await?;
                    }
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    //use futures::sink::SinkExt;
    //use tokio::net::TcpStream;
    //use tokio_stream::StreamExt;
    //use tokio_util::codec::{Framed, LinesCodec};
    //
    //use simple_chat::ClientMessage;

    #[tokio::test]
    #[should_panic]
    async fn register_twice() {
        use tokio::sync::mpsc;
        let (tx, _) = mpsc::channel(16);
        let nick_set = std::sync::Arc::new(crate::server::ClientSet::new());
        let nick = "Nick1";
        nick_set.register(nick, tx.clone()).await.unwrap();

        let nick = "Nick1";
        nick_set.register(nick, tx.clone()).await.unwrap();
    }

    //#[tokio::test]
    //async fn client_connect() {
    //    tokio::spawn(async move { crate::run("127.0.0.1:8080").await });
    //    // Just waiting for the server to start, this should be swapped out
    //    // since it slows down the tests
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //    let sock = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    //    let mut sock = Framed::new(sock, LinesCodec::new());
    //    let register = ClientMessage::Connect("Nick1".to_string());
    //    let register_json = serde_json::to_string(&register).unwrap();
    //    sock.send(register_json).await.unwrap();
    //    let reply = sock.next().await.unwrap().unwrap();
    //    eprintln!("{reply:?}");
    //
    //    let send_msg = ClientMessage::SendMsg("TestMsg".to_string());
    //    let send_msg_json = serde_json::to_string(&send_msg).unwrap();
    //    sock.send(send_msg_json).await.unwrap();
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //}
    //
    //#[tokio::test]
    //async fn register_same_nick() {
    //    tokio::spawn(async move { crate::run("127.0.0.1:8080").await });
    //    // Just waiting for the server to start, this should be swapped out
    //    // since it slows down the tests
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //    let sock = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    //    let mut sock = Framed::new(sock, LinesCodec::new());
    //    let register = ClientMessage::Connect("Nick1".to_string());
    //    let register_json = serde_json::to_string(&register).unwrap();
    //    sock.send(register_json).await.unwrap();
    //    let reply = sock.next().await.unwrap().unwrap();
    //    eprintln!("{reply:?}");
    //
    //    let sock = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    //    let mut sock = Framed::new(sock, LinesCodec::new());
    //    let register = ClientMessage::Connect("Nick1".to_string());
    //    let register_json = serde_json::to_string(&register).unwrap();
    //    sock.send(register_json).await.unwrap();
    //    let reply = sock.next().await.unwrap().unwrap();
    //    eprintln!("{reply:?}");
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //}
    //
    //#[tokio::test]
    //async fn register_same_nick_deregister() {
    //    tokio::spawn(async move { crate::run("127.0.0.1:8080").await });
    //    // Just waiting for the server to start, this should be swapped out
    //    // since it slows down the tests
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //    let sock = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    //    let mut sock = Framed::new(sock, LinesCodec::new());
    //    let register = ClientMessage::Connect("Nick1".to_string());
    //    let register_json = serde_json::to_string(&register).unwrap();
    //    sock.send(register_json).await.unwrap();
    //    let reply = sock.next().await.unwrap().unwrap();
    //    eprintln!("{reply:?}");
    //
    //    let leave = ClientMessage::Leave;
    //    let leave_json = serde_json::to_string(&leave).unwrap();
    //    sock.send(leave_json).await.unwrap();
    //    let reply = sock.next().await.unwrap().unwrap();
    //    eprintln!("{reply:?}");
    //
    //    let sock = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    //    let mut sock = Framed::new(sock, LinesCodec::new());
    //    let register = ClientMessage::Connect("Nick1".to_string());
    //    let register_json = serde_json::to_string(&register).unwrap();
    //    sock.send(register_json).await.unwrap();
    //    let reply = sock.next().await.unwrap().unwrap();
    //    eprintln!("{reply:?}");
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //}
    //
    //#[tokio::test]
    //async fn send_multiple() {
    //    tokio::spawn(async move { crate::run("127.0.0.1:8080").await });
    //    // Just waiting for the server to start, this should be swapped out
    //    // since it slows down the tests
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //    tokio::spawn(async move {
    //        let sock = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    //        let mut sock = Framed::new(sock, LinesCodec::new());
    //        let register = ClientMessage::Connect("Nick1".to_string());
    //        let register_json = serde_json::to_string(&register).unwrap();
    //        sock.send(register_json).await.unwrap();
    //        let reply = sock.next().await.unwrap().unwrap();
    //        eprintln!("Nick1: {reply:?}");
    //
    //        let send_msg = ClientMessage::SendMsg("TestMsg".to_string());
    //        let send_msg_json = serde_json::to_string(&send_msg).unwrap();
    //        sock.send(send_msg_json).await.unwrap();
    //    });
    //
    //    tokio::spawn(async move {
    //        let sock = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    //        let mut sock = Framed::new(sock, LinesCodec::new());
    //        let register = ClientMessage::Connect("Nick2".to_string());
    //        let register_json = serde_json::to_string(&register).unwrap();
    //        sock.send(register_json).await.unwrap();
    //        let reply = sock.next().await.unwrap().unwrap();
    //        eprintln!("Nick2: {reply:?}");
    //
    //        let reply = sock.next().await.unwrap().unwrap();
    //        eprintln!("Nick2: {reply:?}");
    //    });
    //
    //    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //}
}
