//! Tests of a host phone over the network: pairing in setup, controlling the party, and seeing what a host sees.

use std::{sync::Arc, time::Duration};

use bruce_quiz_party::{
    net,
    party::{Party, PartyQuiz},
};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, HostCommand, HostPhase, HostState, PROTOCOL_VERSION, RejectReason, ServerMessage};
use quiz_format::Quiz;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A party in setup, with a library of one quiz of two questions, served on a free port.
async fn start() -> (Arc<net::Shared>, u16) {
    let quiz = Quiz::parse(
        "+++\ntitle = \"T\"\ntime_limit = 30\n+++\n\
         ## One?\n- [x] A\n- [ ] B\n\
         ## Two?\n- [ ] A\n- [x] B\n",
    )
    .unwrap();
    let library = vec![PartyQuiz {
        folder: ".".into(),
        quiz,
    }];
    let shared = net::new_shared(Party::new(library));
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

/// The next message other than a ping or an answer count, or None if the connection closes.
async fn receive(client: &mut Client) -> Option<ServerMessage> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("a message in time");
        match message {
            Some(Ok(Message::Text(text))) => {
                let message: ServerMessage = serde_json::from_str(&text).unwrap();
                if !matches!(message, ServerMessage::Ping { .. } | ServerMessage::AnswerCount { .. }) {
                    return Some(message);
                }
            }
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
            Some(Ok(_)) => {}
        }
    }
}

/// Reads a host's messages until a host state passes `test`.
async fn host_state(client: &mut Client, test: impl Fn(&HostState) -> bool) -> HostState {
    loop {
        match receive(client).await {
            Some(ServerMessage::HostState(state)) if test(&state) => return state,
            Some(_) => {}
            None => panic!("closed"),
        }
    }
}

/// Reads a player's messages until one passes `test`.
async fn until(client: &mut Client, test: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
    loop {
        let message = receive(client).await.expect("still connected");
        if test(&message) {
            return message;
        }
    }
}

fn host(command: HostCommand) -> ClientMessage {
    ClientMessage::Host { command }
}

#[tokio::test]
async fn a_paired_phone_runs_the_party() {
    let (shared, port) = start().await;
    let (pairing_code, game_code) = {
        let hub = shared.hub();
        (hub.party.pairing_code.clone(), hub.party.code.clone())
    };

    // Pairing in setup gives the phone a token, and the host's view.
    let mut phone = connect(port).await;
    send(
        &mut phone,
        ClientMessage::PairHost {
            protocol: PROTOCOL_VERSION,
            pairing_code: pairing_code.clone(),
            device_name: "Bruce's iPhone".into(),
        },
    )
    .await;
    let Some(ServerMessage::Paired { token }) = receive(&mut phone).await else {
        panic!("not paired")
    };
    host_state(&mut phone, |state| state.phase == HostPhase::Setup).await;

    // The code pairs only once.
    let mut other = connect(port).await;
    send(
        &mut other,
        ClientMessage::PairHost {
            protocol: PROTOCOL_VERSION,
            pairing_code,
            device_name: "Someone".into(),
        },
    )
    .await;
    assert!(matches!(
        receive(&mut other).await,
        Some(ServerMessage::Rejected {
            reason: RejectReason::NotAuthorised,
            ..
        })
    ));

    // The Mac starts the party; a player joins.
    shared.start_party(&[0]).unwrap();
    host_state(&mut phone, |state| state.phase == HostPhase::Lobby).await;
    let mut sam = connect(port).await;
    send(
        &mut sam,
        ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            game_code: game_code.clone(),
            name: "Sam".into(),
            token: None,
        },
    )
    .await;
    until(&mut sam, |message| matches!(message, ServerMessage::Lobby { .. })).await;
    let state = host_state(&mut phone, |state| state.players.len() == 1).await;
    let player = state.players[0].player;

    // The phone starts the quiz, and sees the question with its answer.
    send(&mut phone, host(HostCommand::StartQuiz { quiz: 0 })).await;
    let ServerMessage::Question(view) = until(&mut sam, |message| matches!(message, ServerMessage::Question(_))).await
    else {
        unreachable!()
    };
    let state = host_state(&mut phone, |state| matches!(state.phase, HostPhase::Question { .. })).await;
    assert!(matches!(state.phase, HostPhase::Question { correct: 0, .. }));
    assert_eq!(state.next.as_deref(), Some("End Question"));

    // Pausing stops the player's clock; resuming sends the question again.
    send(&mut phone, host(HostCommand::Pause)).await;
    until(&mut sam, |message| *message == ServerMessage::Paused).await;
    host_state(&mut phone, |state| state.paused).await;
    send(&mut phone, host(HostCommand::Resume)).await;
    until(&mut sam, |message| *message == ServerMessage::Resumed).await;
    let ServerMessage::Question(again) = until(&mut sam, |message| matches!(message, ServerMessage::Question(_))).await
    else {
        unreachable!()
    };
    assert_eq!(again.question, view.question);

    // A phone which reconnects with its token is a host again; one with a wrong token is refused.
    drop(phone);
    let mut phone = connect(port).await;
    send(
        &mut phone,
        ClientMessage::HostConnect {
            protocol: PROTOCOL_VERSION,
            token,
        },
    )
    .await;
    host_state(&mut phone, |state| !state.paused).await;
    let mut stranger = connect(port).await;
    send(
        &mut stranger,
        ClientMessage::HostConnect {
            protocol: PROTOCOL_VERSION,
            token: "nonsense".into(),
        },
    )
    .await;
    assert!(matches!(
        receive(&mut stranger).await,
        Some(ServerMessage::Rejected {
            reason: RejectReason::NotAuthorised,
            ..
        })
    ));

    // Removing the player tells them and closes their connection.
    send(&mut phone, host(HostCommand::RemovePlayer { player })).await;
    until(&mut sam, |message| *message == ServerMessage::Removed).await;
    assert_eq!(receive(&mut sam).await, None);
    host_state(&mut phone, |state| state.players.is_empty()).await;

    // Ending the party.
    send(&mut phone, host(HostCommand::EndParty)).await;
    host_state(&mut phone, |state| state.phase == HostPhase::Ended).await;
}

#[tokio::test]
async fn only_paired_phones_control_the_party() {
    let (shared, port) = start().await;
    shared.start_party(&[0]).unwrap();
    let game_code = shared.hub().party.code.clone();
    let mut sam = connect(port).await;
    send(
        &mut sam,
        ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            game_code,
            name: "Sam".into(),
            token: None,
        },
    )
    .await;
    until(&mut sam, |message| matches!(message, ServerMessage::Lobby { .. })).await;
    // A player's host command is ignored.
    send(&mut sam, host(HostCommand::StartQuiz { quiz: 0 })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(shared.hub().party.phase, bruce_quiz_party::game::Phase::Lobby);
    // And pairing after setup is refused.
    let pairing_code = shared.hub().party.pairing_code.clone();
    let mut phone = connect(port).await;
    send(
        &mut phone,
        ClientMessage::PairHost {
            protocol: PROTOCOL_VERSION,
            pairing_code,
            device_name: "Late".into(),
        },
    )
    .await;
    assert!(matches!(
        receive(&mut phone).await,
        Some(ServerMessage::Rejected {
            reason: RejectReason::NotAuthorised,
            ..
        })
    ));
}

#[tokio::test]
async fn players_cannot_join_during_setup() {
    let (shared, port) = start().await;
    let game_code = shared.hub().party.code.clone();
    let mut sam = connect(port).await;
    send(
        &mut sam,
        ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            game_code,
            name: "Sam".into(),
            token: None,
        },
    )
    .await;
    assert!(matches!(
        receive(&mut sam).await,
        Some(ServerMessage::Rejected {
            reason: RejectReason::Closed,
            ..
        })
    ));
}
