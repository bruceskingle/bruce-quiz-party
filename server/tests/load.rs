//! A hundred phones playing a question at once, over real connections on this Mac: they all join, the question is
//! sent to all of them, they all answer at once, and every answer counts. It measures how far apart the phones are sent
//! the question, and checks that the count of answers is not sent to each phone for every answer. It tests the
//! controller, not the Wi-Fi, which with a hundred phones matters more.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bruce_quiz_party::{
    net,
    party::{Party, PartyQuiz},
};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage};
use quiz_format::Quiz;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How many phones play, unless `LOAD_PLAYERS` says, e.g. to see how far it goes.
const PLAYERS: usize = 100;

fn players() -> usize {
    std::env::var("LOAD_PLAYERS")
        .ok()
        .and_then(|players| players.parse().ok())
        .unwrap_or(PLAYERS)
}

/// How far apart the phones may be sent the question, here on one Mac, in a debug build.
const MAX_SPREAD: Duration = Duration::from_millis(500);

/// What one phone saw of the question.
struct Seen {
    /// When the question came.
    asked: Instant,
    /// How many counts of answers came while it was asked.
    counts: usize,
    /// How many lists of the lobby's players came, as the others joined.
    lobbies: usize,
    /// How many chose each answer, from the reveal.
    reveal_counts: Vec<u32>,
    /// The points this phone scored.
    points: u32,
}

/// Lets this process have a socket for each phone and the controller's end of it, as macOS allows only 256 files open
/// by default.
fn allow_enough_sockets() {
    // SAFETY: getrlimit and setrlimit are given a valid rlimit to read and write.
    unsafe {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 && limit.rlim_cur < 4096 {
            limit.rlim_cur = limit.rlim_max.min(4096);
            libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
        }
    }
}

async fn send(client: &mut Client, message: &ClientMessage) {
    let text = serde_json::to_string(message).unwrap();
    client.send(Message::Text(text.into())).await.unwrap();
}

async fn receive(client: &mut Client) -> ServerMessage {
    let message = tokio::time::timeout(Duration::from_secs(10), client.next())
        .await
        .expect("a message in time");
    let Some(Ok(Message::Text(text))) = message else {
        panic!("closed")
    };
    serde_json::from_str(&text).unwrap()
}

/// A phone which has joined the party.
async fn join(port: u16, number: usize) -> Client {
    let mut client = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .unwrap()
        .0;
    let join = ClientMessage::Join {
        protocol: PROTOCOL_VERSION,
        game_code: "ABCD".into(),
        name: format!("Player {number}"),
        token: None,
    };
    send(&mut client, &join).await;
    loop {
        match receive(&mut client).await {
            ServerMessage::Welcome { .. } => return client,
            ServerMessage::Ping { nonce } => send(&mut client, &ClientMessage::Pong { nonce }).await,
            other => panic!("not welcomed: {other:?}"),
        }
    }
}

/// Plays a question as a phone does: answers pings all along, and the question as soon as it comes, with the first
/// answer, the correct one, then reports what it saw up to the reveal.
async fn play(mut client: Client) -> Seen {
    let mut asked = None;
    let mut counts = 0;
    let mut lobbies = 0;
    loop {
        match receive(&mut client).await {
            ServerMessage::Ping { nonce } => send(&mut client, &ClientMessage::Pong { nonce }).await,
            ServerMessage::Lobby { .. } => lobbies += 1,
            ServerMessage::Question(view) => {
                asked = Some(Instant::now());
                let answer = ClientMessage::Answer {
                    question: view.question,
                    choice: 0,
                };
                send(&mut client, &answer).await;
            }
            ServerMessage::AnswerCount { .. } if asked.is_some() => counts += 1,
            ServerMessage::Reveal(reveal) => {
                return Seen {
                    asked: asked.expect("the question before the reveal"),
                    counts,
                    lobbies,
                    reveal_counts: reveal.counts,
                    points: reveal.result.map(|result| result.points).unwrap_or(0),
                };
            }
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hundred_phones_play_a_question() {
    allow_enough_sockets();
    let players = players();
    let quiz = Quiz::parse("+++\ntitle = \"T\"\ntime_limit = 20\n+++\n## One?\n- [x] A\n- [ ] B\n").unwrap();
    let quizzes = vec![PartyQuiz {
        folder: ".".into(),
        quiz,
    }];
    let shared: Arc<net::Shared> = net::new_shared(Party::with_code("ABCD".into(), quizzes));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(net::serve(listener, shared.clone()));

    let mut clients = Vec::new();
    for number in 1..=players {
        clients.push(join(port, number).await);
    }
    let phones: Vec<_> = clients.into_iter().map(|client| tokio::spawn(play(client))).collect();
    // A moment for the pings to measure the round trips, as at a party.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let started = Instant::now();
    shared.start_quiz(0).unwrap();
    let mut seen = Vec::new();
    for phone in phones {
        seen.push(phone.await.unwrap());
    }
    let finished = started.elapsed();

    let first = seen.iter().map(|seen| seen.asked).min().unwrap();
    let last = seen.iter().map(|seen| seen.asked).max().unwrap();
    let spread = last - first;
    let most_counts = seen.iter().map(|seen| seen.counts).max().unwrap();
    let most_lobbies = seen.iter().map(|seen| seen.lobbies).max().unwrap();
    println!(
        "{players} phones: the question reached the first {:?} after it was asked, the last {spread:?} after the \
         first; all answered and saw the reveal in {finished:?}; at most {most_counts} counts of answers and \
         {most_lobbies} lists of players each",
        first - started,
    );
    assert!(spread < MAX_SPREAD, "the question reached the phones {spread:?} apart");
    for seen in &seen {
        assert_eq!(seen.reveal_counts, [players as u32, 0], "every answer counts");
        assert!(seen.points > 900, "answered at once, scored only {}", seen.points);
    }
    // Without a limit, each phone would be sent a count for every answer, a hundred of them.
    assert!(
        most_counts < players / 4,
        "{most_counts} counts of answers to one phone"
    );
    // Nor, as they join, a list of the players for every one who joins after it.
    assert!(
        most_lobbies < players / 4,
        "{most_lobbies} lists of players to one phone"
    );
}
