//! The web server the phones join: it serves the browser client's files and the WebSocket, over which the phones and
//! the controller exchange the protocol's messages. It runs on its own thread with its own Tokio runtime, beside the
//! window, and tells the window when the party changes.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, PROTOCOL_VERSION, PlayerId, RejectReason, ServerMessage, WEBSOCKET_PATH};
use tokio::sync::{mpsc, watch};

use crate::{
    game::{NotNow, Phase},
    party::Party,
};

/// The port the server listens on if it is free, so that the join URL is the same from one party to the next; if it
/// is taken, any free port.
pub const DEFAULT_PORT: u16 = 8765;

/// How often each phone is pinged, to keep its round trip up to date.
const PING_INTERVAL: Duration = Duration::from_secs(2);

/// How much of each new round trip measured goes into the estimate: a moving average, so that one slow ping does not
/// change a player's timing much.
const ROUND_TRIP_WEIGHT: f64 = 0.3;

/// The browser client's files, built into the app.
const INDEX_HTML: &str = include_str!("../../web/index.html");
const APP_JS: &str = include_str!("../../web/app.js");
const STYLE_CSS: &str = include_str!("../../web/style.css");

/// A connection's number, for as long as it is open.
type ConnId = u64;

/// A phone's or browser's connection.
struct Conn {
    /// Sends a message to the connection's socket; dropping it closes the socket.
    sender: mpsc::UnboundedSender<ServerMessage>,
    /// The player the connection has joined as, once it has.
    player: Option<PlayerId>,
    /// The ping sent and not yet answered, and when it was sent.
    ping: Option<(u64, Instant)>,
}

/// The party and its connections, behind one lock, held only briefly and never across an await.
pub struct Hub {
    /// The party.
    pub party: Party,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
    next_nonce: u64,
}

impl Hub {
    /// Sends a message to a connection, if it is still open.
    fn send(&self, conn: ConnId, message: ServerMessage) {
        if let Some(conn) = self.conns.get(&conn) {
            let _ = conn.sender.send(message);
        }
    }

    /// Sends a message to every connection which has joined as a player.
    fn broadcast(&self, message: &ServerMessage) {
        for conn in self.conns.values().filter(|conn| conn.player.is_some()) {
            let _ = conn.sender.send(message.clone());
        }
    }

    /// Sends the lobby to every player.
    fn broadcast_lobby(&self) {
        self.broadcast(&ServerMessage::Lobby {
            players: self.party.lobby(),
        });
    }

    /// Sends how many have answered to every player.
    fn broadcast_answer_count(&self) {
        if let Some(message) = self.answer_count() {
            self.broadcast(&message);
        }
    }

    fn answer_count(&self) -> Option<ServerMessage> {
        let round = self.party.round.as_ref()?;
        let (answered, players) = self.party.answer_count();
        Some(ServerMessage::AnswerCount {
            question: protocol::QuestionRef {
                quiz: round.quiz as u32,
                index: round.index as u32,
            },
            answered,
            players,
        })
    }

    /// What a player's phone shows of the party's phase: sent to every player when the phase changes, and to a player
    /// who joins or rejoins.
    fn phase_messages(&self, player: PlayerId, now: Instant) -> Vec<ServerMessage> {
        let party = &self.party;
        match party.phase {
            Phase::Lobby => vec![ServerMessage::Lobby { players: party.lobby() }],
            Phase::Question => {
                let mut messages: Vec<ServerMessage> = party
                    .question_view(now)
                    .map(ServerMessage::Question)
                    .into_iter()
                    .collect();
                if let Some(round) = &party.round
                    && let Some(answer) = round.answers.get(&player)
                {
                    messages.push(ServerMessage::AnswerLocked {
                        question: protocol::QuestionRef {
                            quiz: round.quiz as u32,
                            index: round.index as u32,
                        },
                        choice: answer.choice,
                    });
                }
                messages.extend(self.answer_count());
                messages
            }
            Phase::Reveal => party
                .reveal()
                .map(|mut reveal| {
                    reveal.result = party.player_result(player);
                    ServerMessage::Reveal(reveal)
                })
                .into_iter()
                .collect(),
            Phase::Standings => vec![ServerMessage::Standings {
                standings: party.standings(),
            }],
            Phase::QuizLeaderboard | Phase::PartyLeaderboard => party
                .current_leaderboard()
                .map(ServerMessage::Leaderboard)
                .into_iter()
                .collect(),
        }
    }

    /// Sends every player what their phone shows of the party's phase, when it changes.
    fn broadcast_phase(&self, now: Instant) {
        for conn in self.conns.values() {
            if let Some(player) = conn.player {
                for message in self.phase_messages(player, now) {
                    let _ = conn.sender.send(message);
                }
            }
        }
    }

    /// Closes the question if every connected player has answered it, returning whether it did.
    fn close_if_all_answered(&mut self, now: Instant) -> bool {
        if self.party.phase == Phase::Question && self.party.all_answered() {
            self.party.close_question();
            self.broadcast_phase(now);
            true
        } else {
            false
        }
    }
}

/// What the server and the window share.
pub struct Shared {
    hub: Mutex<Hub>,
    /// Counts the party's changes, for the window to show them.
    changes: watch::Sender<u64>,
    /// The server's runtime, for the timers which close questions, set when it starts serving.
    runtime: OnceLock<tokio::runtime::Handle>,
}

impl Shared {
    /// The party and its connections, locked.
    pub fn hub(&self) -> MutexGuard<'_, Hub> {
        // A panic while the lock was held leaves the party as it was, which is still worth showing.
        self.hub.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Tells the window the party has changed.
    fn changed(&self) {
        self.changes.send_modify(|count| *count += 1);
    }

    /// Waits for the party to change, for the window.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// Starts one of the party's quizzes, for the host.
    pub fn start_quiz(self: &Arc<Self>, quiz: usize) -> Result<(), NotNow> {
        self.host(|party, now| party.start_quiz(quiz, now))
    }

    /// Moves the party on, for the host.
    pub fn next(self: &Arc<Self>) {
        let _ = self.host(|party, now| {
            party.next(now);
            Ok(())
        });
    }

    /// Skips the question being asked, for the host.
    pub fn skip(self: &Arc<Self>) {
        let _ = self.host(|party, now| {
            party.skip(now);
            Ok(())
        });
    }

    /// Does a host's command, then tells the players and the window what changed, and if it asked a question, starts
    /// the timer which closes it.
    fn host(self: &Arc<Self>, command: impl FnOnce(&mut Party, Instant) -> Result<(), NotNow>) -> Result<(), NotNow> {
        let now = Instant::now();
        let mut hub = self.hub();
        command(&mut hub.party, now)?;
        hub.broadcast_phase(now);
        let timer = match (&hub.party.phase, &hub.party.round) {
            (Phase::Question, Some(round)) => Some((round.number, round.deadline)),
            _ => None,
        };
        drop(hub);
        if let Some((number, deadline)) = timer {
            self.close_at(number, deadline);
        }
        self.changed();
        Ok(())
    }

    /// Closes a question at its deadline, if it is still being asked then.
    fn close_at(self: &Arc<Self>, number: u64, deadline: Instant) {
        let Some(runtime) = self.runtime.get() else { return };
        let shared = self.clone();
        runtime.spawn(async move {
            tokio::time::sleep_until(deadline.into()).await;
            let mut hub = shared.hub();
            let current = hub.party.round.as_ref().map(|round| round.number);
            if hub.party.phase == Phase::Question && current == Some(number) {
                hub.party.close_question();
                hub.broadcast_phase(Instant::now());
                drop(hub);
                shared.changed();
            }
        });
    }
}

/// The running server.
#[derive(Clone)]
pub struct Server {
    /// The party and its connections.
    pub shared: Arc<Shared>,
    /// The port it listens on.
    pub port: u16,
    /// The Bonjour advertisement, for the iOS app to find the party, kept for as long as the server runs.
    pub advert: Option<Arc<crate::bonjour::Advert>>,
}

impl Server {
    /// The URL the join QR code holds: the browser client, with the game code, at the Mac's address on the local
    /// network.
    pub fn join_url(&self) -> String {
        let code = self.shared.hub().party.code.clone();
        let host = local_address().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        format!("http://{host}:{}/?code={code}", self.port)
    }
}

/// Starts the server for a party on its own thread, on [`DEFAULT_PORT`] or, if that is taken, any free port.
pub fn start(party: Party) -> std::io::Result<Server> {
    let listener = std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT))
        .or_else(|_| std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let shared = new_shared(party);
    let advert = crate::bonjour::Advert::new(port)
        .inspect_err(|error| eprintln!("The party is not advertised with Bonjour: {error:#}"))
        .ok()
        .map(Arc::new);
    let server = Server {
        shared: shared.clone(),
        port,
        advert,
    };
    std::thread::Builder::new().name("party-server".into()).spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build();
        let result = runtime.and_then(|runtime| {
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::from_std(listener)?;
                serve(listener, shared).await
            })
        });
        if let Err(error) = result {
            eprintln!("The party's web server stopped: {:#}", anyhow::Error::from(error));
        }
    })?;
    Ok(server)
}

/// The shared state of a new party.
pub fn new_shared(party: Party) -> Arc<Shared> {
    Arc::new(Shared {
        hub: Mutex::new(Hub {
            party,
            conns: HashMap::new(),
            next_conn: 1,
            next_nonce: 1,
        }),
        changes: watch::channel(0).0,
        runtime: OnceLock::new(),
    })
}

/// Serves a party on a listener until it fails, e.g. in the tests on a port of their own.
pub async fn serve(listener: tokio::net::TcpListener, shared: Arc<Shared>) -> std::io::Result<()> {
    let _ = shared.runtime.set(tokio::runtime::Handle::current());
    let app = Router::new()
        .route("/", get(|| async { asset("text/html; charset=utf-8", INDEX_HTML) }))
        .route(
            "/app.js",
            get(|| async { asset("text/javascript; charset=utf-8", APP_JS) }),
        )
        .route(
            "/style.css",
            get(|| async { asset("text/css; charset=utf-8", STYLE_CSS) }),
        )
        .route(WEBSOCKET_PATH, get(websocket))
        .route("/quiz/{quiz}/{*file}", get(quiz_image))
        .with_state(shared);
    axum::serve(listener, app).await
}

/// One of the browser client's files, never cached, so that a phone always has the controller's own version.
fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

/// One of a quiz's images, for the phones, only from inside the quiz's folder.
async fn quiz_image(Path((quiz, file)): Path<(usize, String)>, State(shared): State<Arc<Shared>>) -> Response {
    let folder: Option<PathBuf> = shared.hub().party.quizzes.get(quiz).map(|quiz| quiz.folder.clone());
    let found = folder.and_then(|folder| {
        let folder = folder.canonicalize().ok()?;
        let path = folder.join(&file).canonicalize().ok()?;
        path.starts_with(&folder).then_some(path)
    });
    let Some(path) = found else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, crate::library::content_type(&path))], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn websocket(upgrade: WebSocketUpgrade, State(shared): State<Arc<Shared>>) -> Response {
    upgrade.on_upgrade(move |socket| connection(socket, shared))
}

/// Runs one connection: its messages out, its messages in, and its pings, until it closes.
async fn connection(socket: WebSocket, shared: Arc<Shared>) {
    let (mut socket_out, mut socket_in) = socket.split();
    let (sender, mut outgoing) = mpsc::unbounded_channel::<ServerMessage>();
    let id = {
        let mut hub = shared.hub();
        let id = hub.next_conn;
        hub.next_conn += 1;
        hub.conns.insert(
            id,
            Conn {
                sender,
                player: None,
                ping: None,
            },
        );
        id
    };

    // Writes the messages sent to the connection, until its sender is dropped, which closes the socket.
    let writer = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                continue;
            };
            if socket_out.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = socket_out.close().await;
    });

    // Pings the connection, for its round trip.
    let pinger = {
        let shared = shared.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(PING_INTERVAL);
            loop {
                interval.tick().await;
                let mut hub = shared.hub();
                let nonce = hub.next_nonce;
                hub.next_nonce += 1;
                let Some(conn) = hub.conns.get_mut(&id) else { break };
                conn.ping = Some((nonce, Instant::now()));
                let _ = conn.sender.send(ServerMessage::Ping { nonce });
            }
        })
    };

    while let Some(Ok(message)) = socket_in.next().await {
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        // A message which cannot be read is ignored, rather than ending the connection, so that a newer client's
        // additions do not break it.
        let Ok(message) = serde_json::from_str::<ClientMessage>(&text) else {
            continue;
        };
        if !handle(&shared, id, message, Instant::now()) {
            break;
        }
    }

    pinger.abort();
    let removed = shared.hub().conns.remove(&id);
    if let Some(Conn {
        player: Some(player), ..
    }) = removed
    {
        let mut hub = shared.hub();
        // Only if no newer connection has rejoined as the same player.
        if !hub.conns.values().any(|conn| conn.player == Some(player)) {
            hub.party.disconnected(player);
            match hub.party.phase {
                Phase::Lobby => hub.broadcast_lobby(),
                Phase::Question => {
                    // The others may all have answered, and been waiting only for this player.
                    if !hub.close_if_all_answered(Instant::now()) {
                        hub.broadcast_answer_count();
                    }
                }
                _ => {}
            }
        }
        drop(hub);
        shared.changed();
    }
    let _ = writer.await;
}

/// Handles a message from a connection, returning whether to keep the connection open.
fn handle(shared: &Shared, id: ConnId, message: ClientMessage, now: Instant) -> bool {
    let mut hub = shared.hub();
    match message {
        ClientMessage::Join {
            protocol,
            game_code,
            name,
            token,
        } => {
            if protocol != PROTOCOL_VERSION {
                reject(
                    &mut hub,
                    id,
                    RejectReason::WrongProtocol,
                    "Please update the app to join this party.",
                );
                return false;
            }
            match hub.party.join(&game_code, &name, token.as_deref()) {
                Ok(joined) => {
                    // A connection which had joined as this player before is replaced by this one, and closed.
                    let stale: Vec<ConnId> = hub
                        .conns
                        .iter()
                        .filter(|(other, conn)| **other != id && conn.player == Some(joined.player))
                        .map(|(other, _)| *other)
                        .collect();
                    for other in stale {
                        hub.conns.remove(&other);
                    }
                    if let Some(conn) = hub.conns.get_mut(&id) {
                        conn.player = Some(joined.player);
                    }
                    hub.send(
                        id,
                        ServerMessage::Welcome {
                            player: joined.player,
                            token: joined.token,
                            name: joined.name,
                        },
                    );
                    // In the lobby, everyone, the new player too, is sent the lobby with them in it; otherwise the
                    // new player is sent what the others are shown.
                    if hub.party.phase == Phase::Lobby {
                        hub.broadcast_lobby();
                    } else {
                        for message in hub.phase_messages(joined.player, now) {
                            hub.send(id, message);
                        }
                        if hub.party.phase == Phase::Question {
                            hub.broadcast_answer_count();
                        }
                    }
                    drop(hub);
                    shared.changed();
                    true
                }
                Err(refused) => {
                    reject(&mut hub, id, refused.reason, &refused.message);
                    false
                }
            }
        }
        ClientMessage::Pong { nonce } => {
            let Some(conn) = hub.conns.get_mut(&id) else {
                return false;
            };
            let Some((sent_nonce, sent)) = conn.ping else {
                return true;
            };
            if sent_nonce != nonce {
                return true;
            }
            conn.ping = None;
            let sample = sent.elapsed();
            if let Some(player) = conn.player
                && let Some(player) = hub.party.player_mut(player)
            {
                player.round_trip = Some(match player.round_trip {
                    Some(old) => old.mul_f64(1.0 - ROUND_TRIP_WEIGHT) + sample.mul_f64(ROUND_TRIP_WEIGHT),
                    None => sample,
                });
            }
            true
        }
        ClientMessage::Answer { question, choice } => {
            let Some(player) = hub.conns.get(&id).and_then(|conn| conn.player) else {
                return true;
            };
            if hub.party.answer(player, question, choice, now) {
                hub.send(id, ServerMessage::AnswerLocked { question, choice });
                if !hub.close_if_all_answered(now) {
                    hub.broadcast_answer_count();
                }
                drop(hub);
                shared.changed();
            }
            true
        }
        // Pairing and controlling as a host come in a later milestone.
        ClientMessage::PairHost { .. } | ClientMessage::HostConnect { .. } | ClientMessage::Host { .. } => true,
    }
}

/// Refuses a connection: tells it why, then closes it once that is sent, by dropping its sender.
fn reject(hub: &mut Hub, id: ConnId, reason: RejectReason, message: &str) {
    hub.send(
        id,
        ServerMessage::Rejected {
            reason,
            message: message.to_string(),
        },
    );
    hub.conns.remove(&id);
}

/// The Mac's address on the local network, for the join URL: that of the interface a packet to the internet would
/// leave by. No packet is sent; connecting a UDP socket only chooses the route.
pub fn local_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(SocketAddr::from(([192, 0, 2, 1], 9))).ok()?;
    let address = socket.local_addr().ok()?.ip();
    (!address.is_unspecified()).then_some(address)
}
