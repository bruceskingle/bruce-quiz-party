//! Tests of playing a quiz over the network, as the phones would: questions sent to everyone, answers timed and
//! scored, the reveal with each player's own result, and the standings.

use std::{sync::Arc, time::Duration};

use bruce_quiz_party::{
    game::Phase,
    net,
    party::{Party, PartyQuiz},
};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, LeaderboardKind, PROTOCOL_VERSION, ServerMessage};
use quiz_format::Quiz;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A party with one quiz of two questions, each with a time limit in seconds, served on a free port.
async fn start(time_limit: u32) -> (Arc<net::Shared>, u16) {
    let quiz = Quiz::parse(&format!(
        "+++\ntitle = \"T\"\ntime_limit = {time_limit}\n+++\n\
         ## One?\n- [x] A\n- [ ] B\n- [ ] C\n\
         ## Two?\n- [ ] A\n- [x] B\n"
    ))
    .unwrap();
    let quizzes = vec![PartyQuiz {
        folder: ".".into(),
        quiz,
    }];
    let shared = net::new_shared(Party::with_code("ABCD".into(), quizzes));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(net::serve(listener, shared.clone()));
    (shared, port)
}

async fn send(client: &mut Client, message: ClientMessage) {
    let text = serde_json::to_string(&message).unwrap();
    client.send(Message::Text(text.into())).await.unwrap();
}

/// The next message other than a ping or an answer count.
async fn receive(client: &mut Client) -> ServerMessage {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("a message in time");
        let Some(Ok(Message::Text(text))) = message else {
            panic!("closed")
        };
        let message: ServerMessage = serde_json::from_str(&text).unwrap();
        if !matches!(message, ServerMessage::Ping { .. } | ServerMessage::AnswerCount { .. }) {
            return message;
        }
    }
}

/// A player who has joined and been sent the lobby.
async fn player(port: u16, name: &str) -> Client {
    let mut client = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .unwrap()
        .0;
    send(
        &mut client,
        ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            game_code: "ABCD".into(),
            name: name.into(),
            token: None,
        },
    )
    .await;
    assert!(matches!(receive(&mut client).await, ServerMessage::Welcome { .. }));
    assert!(matches!(receive(&mut client).await, ServerMessage::Lobby { .. }));
    client
}

/// Reads messages until one which `matches` returns something for.
async fn until<T>(client: &mut Client, matches: impl Fn(ServerMessage) -> Option<T>) -> T {
    loop {
        if let Some(found) = matches(receive(client).await) {
            return found;
        }
    }
}

#[tokio::test]
async fn plays_a_quiz() {
    let (shared, port) = start(20).await;
    let mut sam = player(port, "Sam").await;
    let mut jo = player(port, "Jo").await;
    // Sam also sees Jo join.
    assert!(matches!(receive(&mut sam).await, ServerMessage::Lobby { .. }));

    shared.start_quiz(0).unwrap();
    let ServerMessage::Question(view) = receive(&mut sam).await else {
        panic!("no question")
    };
    assert_eq!((view.number, view.total, view.answers.len()), (1, 2, 3));
    assert!(matches!(receive(&mut jo).await, ServerMessage::Question(_)));

    // Sam answers correctly at once, Jo wrongly; Sam's second answer is ignored.
    send(
        &mut sam,
        ClientMessage::Answer {
            question: view.question,
            choice: 0,
        },
    )
    .await;
    assert!(matches!(
        receive(&mut sam).await,
        ServerMessage::AnswerLocked { choice: 0, .. }
    ));
    send(
        &mut sam,
        ClientMessage::Answer {
            question: view.question,
            choice: 1,
        },
    )
    .await;
    send(
        &mut jo,
        ClientMessage::Answer {
            question: view.question,
            choice: 2,
        },
    )
    .await;
    assert!(matches!(
        receive(&mut jo).await,
        ServerMessage::AnswerLocked { choice: 2, .. }
    ));

    // Everyone has answered, so the question closes at once.
    let ServerMessage::Reveal(reveal) = receive(&mut sam).await else {
        panic!("no reveal")
    };
    assert_eq!((reveal.correct, reveal.counts), (0, vec![1, 0, 1]));
    let result = reveal.result.unwrap();
    assert!(result.correct);
    assert!(result.points > 990, "answered at once, scored {}", result.points);
    assert_eq!(result.quiz_rank, 1);
    let ServerMessage::Reveal(reveal) = receive(&mut jo).await else {
        panic!("no reveal")
    };
    let result = reveal.result.unwrap();
    assert_eq!(
        (result.correct, result.points, result.quiz_rank, result.choice),
        (false, 0, 2, Some(2))
    );

    // Next counts down first, so that the players see what is coming; pressed again, it moves on at once.
    shared.next();
    let ServerMessage::Countdown { next, in_ms } = receive(&mut jo).await else {
        panic!("no countdown")
    };
    assert_eq!(next, "Standings");
    assert!((2000..=3000).contains(&in_ms), "{in_ms}");
    shared.next();
    let ServerMessage::Standings { standings, progress } = receive(&mut jo).await else {
        panic!("no standings")
    };
    assert_eq!(standings[0].name, "Sam");
    assert_eq!(
        progress.map(|progress| progress.words()).as_deref(),
        Some("After 1 of 2 Questions")
    );
    // From here, without the countdown.
    shared.hub().party.auto_advance.countdown_secs = 0;

    shared.next();
    let ServerMessage::Question(view) = receive(&mut jo).await else {
        panic!("no second question")
    };
    assert_eq!(view.number, 2);

    // Skipping the last question scores nothing and goes to the quiz's leaderboard, then the party's.
    shared.skip();
    let board = until(&mut sam, |message| match message {
        ServerMessage::Leaderboard(board) => Some(board),
        _ => None,
    })
    .await;
    assert_eq!(board.sorted_by, LeaderboardKind::Quiz { quiz: 0 });
    assert_eq!(board.rows[0].name, "Sam");
    assert_eq!(board.rows[1].quizzes[0].unwrap().score, 0);
    shared.next();
    let board = until(&mut jo, |message| match message {
        ServerMessage::Leaderboard(board) if board.sorted_by == LeaderboardKind::Party => Some(board),
        _ => None,
    })
    .await;
    assert_eq!(board.rows.len(), 2);
    shared.next();
    until(&mut sam, |message| {
        matches!(message, ServerMessage::Lobby { .. }).then_some(())
    })
    .await;
    assert_eq!(shared.hub().party.phase, Phase::Lobby);
    assert_eq!(shared.hub().party.played, vec![0]);
}

#[tokio::test]
async fn closes_the_question_when_time_runs_out() {
    let (shared, port) = start(1).await;
    let mut sam = player(port, "Sam").await;
    shared.start_quiz(0).unwrap();
    assert!(matches!(receive(&mut sam).await, ServerMessage::Question(_)));
    // Nobody answers; the reveal comes after the second.
    let started = std::time::Instant::now();
    let ServerMessage::Reveal(reveal) = receive(&mut sam).await else {
        panic!("no reveal")
    };
    assert!(started.elapsed() >= Duration::from_millis(900));
    let result = reveal.result.unwrap();
    assert_eq!((result.correct, result.points, result.choice), (false, 0, None));
}

#[tokio::test]
async fn a_player_who_rejoins_mid_question_gets_it_again() {
    let (shared, port) = start(20).await;
    let mut sam = player(port, "Sam").await;
    let mut jo = player(port, "Jo").await;
    shared.start_quiz(0).unwrap();
    let ServerMessage::Question(view) = receive(&mut jo).await else {
        panic!("no question")
    };
    send(
        &mut jo,
        ClientMessage::Answer {
            question: view.question,
            choice: 1,
        },
    )
    .await;
    assert!(matches!(receive(&mut jo).await, ServerMessage::AnswerLocked { .. }));
    let token = shared.hub().party.players[1].token.clone();
    drop(jo);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut jo = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .unwrap()
        .0;
    send(
        &mut jo,
        ClientMessage::Join {
            protocol: PROTOCOL_VERSION,
            game_code: "ABCD".into(),
            name: String::new(),
            token: Some(token),
        },
    )
    .await;
    assert!(matches!(receive(&mut jo).await, ServerMessage::Welcome { .. }));
    let ServerMessage::Question(again) = receive(&mut jo).await else {
        panic!("no question")
    };
    assert_eq!(again.question, view.question);
    assert!(again.remaining_ms < again.time_limit_ms);
    // Her answer was kept.
    assert!(matches!(
        receive(&mut jo).await,
        ServerMessage::AnswerLocked { choice: 1, .. }
    ));
    drop(sam.close(None).await);
}
