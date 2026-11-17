//! Tests of joining a party over the network, as a phone would: the server on a port of its own, and WebSocket clients
//! speaking the protocol's JSON.

use std::{sync::Arc, time::Duration};

use bruce_quiz_party::{net, party::Party};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, PROTOCOL_VERSION, RejectReason, ServerMessage};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A party with the code ABCD served on a free port, and the port.
async fn start() -> (Arc<net::Shared>, u16) {
    let shared = net::new_shared(Party::with_code("ABCD".into(), Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(net::serve(listener, shared.clone()));
    (shared, port)
}

async fn connect(port: u16) -> Client {
    tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .unwrap()
        .0
}

async fn send(client: &mut Client, message: ClientMessage) {
    let text = serde_json::to_string(&message).unwrap();
    client.send(Message::Text(text.into())).await.unwrap();
}

/// The next message other than a ping, or None if the connection closes.
async fn receive(client: &mut Client) -> Option<ServerMessage> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("a message in time");
        match message {
            Some(Ok(Message::Text(text))) => {
                let message: ServerMessage = serde_json::from_str(&text).unwrap();
                if !matches!(message, ServerMessage::Ping { .. }) {
                    return Some(message);
                }
            }
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
            Some(Ok(_)) => {}
        }
    }
}

fn join(name: &str, token: Option<String>) -> ClientMessage {
    ClientMessage::Join {
        protocol: PROTOCOL_VERSION,
        game_code: "abcd".into(),
        name: name.into(),
        token,
    }
}

#[tokio::test]
async fn players_join_and_see_each_other() {
    let (_, port) = start().await;
    let mut sam = connect(port).await;
    send(&mut sam, join("Sam", None)).await;
    let Some(ServerMessage::Welcome { player, name, .. }) = receive(&mut sam).await else {
        panic!("no welcome")
    };
    assert_eq!((player, name.as_str()), (1, "Sam"));
    let Some(ServerMessage::Lobby { players }) = receive(&mut sam).await else {
        panic!("no lobby")
    };
    assert_eq!(players.len(), 1);

    let mut jo = connect(port).await;
    send(&mut jo, join("Jo", None)).await;
    assert!(matches!(
        receive(&mut jo).await,
        Some(ServerMessage::Welcome { player: 2, .. })
    ));
    // Sam sees Jo join.
    let Some(ServerMessage::Lobby { players }) = receive(&mut sam).await else {
        panic!("no lobby")
    };
    let names: Vec<_> = players.iter().map(|player| player.name.as_str()).collect();
    assert_eq!(names, ["Sam", "Jo"]);
}

#[tokio::test]
async fn a_wrong_code_is_refused_and_closed() {
    let (_, port) = start().await;
    let mut client = connect(port).await;
    send(
        &mut client,
        ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            game_code: "WXYZ".into(),
            name: "Sam".into(),
            token: None,
        },
    )
    .await;
    let Some(ServerMessage::Rejected { reason, .. }) = receive(&mut client).await else {
        panic!("not refused")
    };
    assert_eq!(reason, RejectReason::WrongGameCode);
    assert_eq!(receive(&mut client).await, None);
}

#[tokio::test]
async fn an_old_protocol_is_refused() {
    let (_, port) = start().await;
    let mut client = connect(port).await;
    send(
        &mut client,
        ClientMessage::Join {
            protocol: PROTOCOL_VERSION + 1,
            game_code: "ABCD".into(),
            name: "Sam".into(),
            token: None,
        },
    )
    .await;
    let Some(ServerMessage::Rejected { reason, .. }) = receive(&mut client).await else {
        panic!("not refused")
    };
    assert_eq!(reason, RejectReason::WrongProtocol);
}

#[tokio::test]
async fn a_dropped_phone_rejoins_as_the_same_player() {
    let (shared, port) = start().await;
    let mut sam = connect(port).await;
    send(&mut sam, join("Sam", None)).await;
    let Some(ServerMessage::Welcome { player, token, .. }) = receive(&mut sam).await else {
        panic!("no welcome")
    };
    drop(sam);

    // The server notices the drop.
    for _ in 0..50 {
        if !shared.hub().party.lobby()[0].connected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!shared.hub().party.lobby()[0].connected);

    let mut again = connect(port).await;
    send(&mut again, join("", Some(token.clone()))).await;
    let Some(ServerMessage::Welcome {
        player: rejoined, name, ..
    }) = receive(&mut again).await
    else {
        panic!("no welcome")
    };
    assert_eq!((rejoined, name.as_str()), (player, "Sam"));
    assert_eq!(shared.hub().party.players.len(), 1);
    assert!(shared.hub().party.lobby()[0].connected);
}

#[tokio::test]
async fn pongs_measure_the_round_trip() {
    let (shared, port) = start().await;
    let mut sam = connect(port).await;
    send(&mut sam, join("Sam", None)).await;
    // The first ping is sent at once; answer it.
    loop {
        let Some(Ok(Message::Text(text))) = sam.next().await else {
            panic!("closed")
        };
        if let ServerMessage::Ping { nonce } = serde_json::from_str(&text).unwrap() {
            send(&mut sam, ClientMessage::Pong { nonce }).await;
            break;
        }
    }
    for _ in 0..50 {
        if shared
            .hub()
            .party
            .players
            .first()
            .and_then(|player| player.round_trip)
            .is_some()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no round trip measured");
}

#[tokio::test]
async fn serves_the_browser_client() {
    let (_, port) = start().await;
    for (path, expected) in [
        ("/", "<title>Bruce's Quiz Party</title>"),
        ("/app.js", "PROTOCOL_VERSION = 1"),
    ] {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{path}: {response}");
        assert!(response.contains(expected), "{path} lacks {expected}");
    }
}

#[tokio::test]
async fn says_whether_the_party_needs_its_code() {
    let (shared, port) = start().await;
    let get = || async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let request = "GET /party.json HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    };
    let response = get().await;
    assert!(response.ends_with(r#"{"code_required":true}"#), "{response}");
    assert!(!response.contains("ABCD"), "the code itself is never given");
    shared.set_code_required(false);
    assert!(get().await.ends_with(r#"{"code_required":false}"#));
}

/// The browser client's protocol version is the protocol crate's.
#[test]
fn the_browser_client_speaks_this_protocol() {
    let app = include_str!("../../web/app.js");
    assert!(app.contains(&format!("const PROTOCOL_VERSION = {PROTOCOL_VERSION};")));
}
