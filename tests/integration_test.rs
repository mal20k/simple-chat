#[tokio::test]
async fn connect_send() {
    let addr = "127.0.0.1:8080";

    // Start the server
    tokio::spawn(async move { simple_chat::server::run(addr).await });
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let messages = std::sync::Arc::new(simple_chat::client::Messages::default());
    let inner_messages = messages.clone();

    // Using a separate thread to prevent runtime-in-runtime issues, connect to the server
    std::thread::spawn(move || {
        simple_chat::client::Connection::connect("test", addr, inner_messages).unwrap();
    });

    let messages2 = std::sync::Arc::new(simple_chat::client::Messages::default());
    let inner_messages2 = messages2.clone();
    std::thread::spawn(move || {
        let mut conn =
            simple_chat::client::Connection::connect("test2", addr, inner_messages2).unwrap();
        conn.send(simple_chat::ClientMessage::SendMsg(
            "Test message".to_string(),
        ))
        .unwrap();
        conn.send(simple_chat::ClientMessage::Leave).unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(dbg!(messages2.get()).len(), 2);
    assert_eq!(dbg!(messages.get()).len(), 4);
}
