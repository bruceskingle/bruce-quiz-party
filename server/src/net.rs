//! The web server the phones join: it serves the browser client's files and the WebSocket, over which the phones and
//! the controller exchange the protocol's messages. It runs on its own thread with its own Tokio runtime, beside the
//! window, and tells the window when the party changes.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{Arc, Mutex, MutexGuard},
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
use protocol::{
    AutoAdvance, ClientMessage, HostCommand, PARTY_INFO_PATH, PROTOCOL_VERSION, PartyInfo, PlayerId, RejectReason,
    ServerMessage, WEBSOCKET_PATH,
};
use tokio::sync::{mpsc, watch};

use crate::{
    game::{NotNow, Phase},
    party::Party,
};

/// The port the server listens on if it is free, so that the join URL is the same from one party to the next; if it
/// is taken, any free port.
pub const DEFAULT_PORT: u16 = 8765;

/// How often each phone is pinged, to keep its round trip up to date: often, so that it follows the network as it gets
/// busier, e.g. as everyone answers, as answers are timed less their phone's round trip.
const PING_INTERVAL: Duration = Duration::from_secs(1);

/// The least time between sending the count of answers, see [`Hub::answer_count_sent`].
const ANSWER_COUNT_GAP: Duration = Duration::from_millis(250);

/// The least time between sending the lobby's list of players, see [`Hub::lobby_sent`].
const LOBBY_GAP: Duration = Duration::from_millis(500);

/// How often the party's clock is checked, for closing questions at their deadline and moving on by itself.
const TICK_INTERVAL: Duration = Duration::from_millis(100);

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
    /// Whether the connection is a paired host phone's, which is sent the host's view and may control the party.
    host: bool,
    /// The ping sent and not yet answered, and when it was sent.
    ping: Option<(u64, Instant)>,
    /// Whether the lobby's list of players has changed since this player was last sent it, see [`Hub::lobby_sent`].
    lobby_stale: bool,
}

/// The party and its connections, behind one lock, held only briefly and never across an await.
pub struct Hub {
    /// The party.
    pub party: Party,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
    next_nonce: u64,
    /// When the count of answers was last sent, and whether a newer one is waiting to be: it is sent at most every
    /// [`ANSWER_COUNT_GAP`], so that a hundred players answering do not send each phone a hundred updates.
    answer_count_sent: Option<Instant>,
    answer_count_due: bool,
    /// When the lobby's list of players was last sent, to those whose list was out of date: at most every
    /// [`LOBBY_GAP`], as when a hundred phones scan the QR code at once, each would otherwise be sent the list a
    /// hundred times, each time longer.
    lobby_sent: Option<Instant>,
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
    fn broadcast_lobby(&mut self) {
        self.lobby_changed(None);
    }

    /// Notes that the lobby's list of players has changed for every player but one, e.g. one just sent it, and sends
    /// it now, or once [`LOBBY_GAP`] has passed since it was last sent.
    fn lobby_changed(&mut self, except: Option<ConnId>) {
        for (id, conn) in &mut self.conns {
            if conn.player.is_some() && Some(*id) != except {
                conn.lobby_stale = true;
            }
        }
        self.send_lobby_if_due();
    }

    /// Sends the lobby's list of players to those whose list is out of date, unless it was sent too lately.
    fn send_lobby_if_due(&mut self) {
        let now = Instant::now();
        let stale = self.conns.values().any(|conn| conn.lobby_stale);
        let lately = self
            .lobby_sent
            .is_some_and(|sent| now.saturating_duration_since(sent) < LOBBY_GAP);
        if !stale || lately || self.party.phase != Phase::Lobby {
            return;
        }
        self.lobby_sent = Some(now);
        let message = ServerMessage::Lobby {
            players: self.party.lobby(),
        };
        for conn in self.conns.values_mut().filter(|conn| conn.lobby_stale) {
            conn.lobby_stale = false;
            let _ = conn.sender.send(message.clone());
        }
    }

    /// Sends how many have answered to every player.
    fn broadcast_answer_count(&mut self) {
        let now = Instant::now();
        if self
            .answer_count_sent
            .is_some_and(|sent| now.saturating_duration_since(sent) < ANSWER_COUNT_GAP)
        {
            self.answer_count_due = true;
            return;
        }
        self.answer_count_sent = Some(now);
        self.answer_count_due = false;
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
        let mut messages = match party.phase {
            Phase::Setup => Vec::new(),
            Phase::Ended => vec![ServerMessage::PartyEnded {
                leaderboard: party.final_leaderboard(),
            }],
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
                progress: party.progress(),
            }],
            Phase::QuizLeaderboard | Phase::PartyLeaderboard => party
                .current_leaderboard()
                .map(ServerMessage::Leaderboard)
                .into_iter()
                .collect(),
        };
        if party.paused_at.is_some() {
            messages.push(ServerMessage::Paused);
        }
        messages.extend(self.countdown(now));
        messages
    }

    /// Sends every host phone the party as a host sees it.
    fn broadcast_hosts(&self, now: Instant) {
        let mut state = None;
        for conn in self.conns.values().filter(|conn| conn.host) {
            let state = state.get_or_insert_with(|| ServerMessage::HostState(self.party.host_state(now)));
            let _ = conn.sender.send(state.clone());
        }
    }

    /// Closes every connection which has joined as a player, after a message, e.g. when the party is over.
    fn close_players(&mut self, message: Option<ServerMessage>) {
        self.conns.retain(|_, conn| {
            if conn.player.is_none() {
                return true;
            }
            if let Some(message) = &message {
                let _ = conn.sender.send(message.clone());
            }
            false
        });
    }

    /// The countdown to the party moving on, if it is counting down.
    fn countdown(&self, now: Instant) -> Option<ServerMessage> {
        let at = self.party.advance_at()?;
        Some(ServerMessage::Countdown {
            next: self.party.countdown_title()?.to_string(),
            in_ms: at.saturating_duration_since(now).as_millis() as u32,
        })
    }

    /// Sends every player what their phone shows of the party's phase, when it changes.
    fn broadcast_phase(&mut self, now: Instant) {
        self.party.note_shown(now);
        // A question's own messages carry its count, and the lobby's its players.
        self.answer_count_due = false;
        for conn in self.conns.values_mut() {
            conn.lobby_stale = false;
        }
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
}

impl Shared {
    /// The party and its connections, locked.
    pub fn hub(&self) -> MutexGuard<'_, Hub> {
        // A panic while the lock was held leaves the party as it was, which is still worth showing.
        self.hub.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Tells the window and the host phones the party has changed. The hub must not be locked.
    fn changed(&self) {
        self.hub().broadcast_hosts(Instant::now());
        self.changes.send_modify(|count| *count += 1);
    }

    /// Waits for the party to change, for the window.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// Changes the quizzes the next party can be set up with, after the library has changed, e.g. a quiz was edited.
    /// The quizzes of a party already set up are its own, and do not change.
    pub fn set_library(&self, library: Vec<crate::party::PartyQuiz>) {
        self.hub().party.library = library;
        self.changed();
    }

    /// Ends setup with the quizzes chosen from the library, so that players can join.
    pub fn start_party(&self, chosen: &[usize]) -> Result<(), String> {
        let images = {
            let mut hub = self.hub();
            hub.party.start_party(chosen)?;
            (0..hub.party.quizzes.len())
                .flat_map(|quiz| hub.party.image_files(quiz))
                .collect()
        };
        // The phones' copies of big photos are made now, so that they are ready before the questions are asked.
        crate::phone_image::prepare(images);
        self.changed();
        Ok(())
    }

    /// Starts one of the party's quizzes, for the host.
    pub fn start_quiz(&self, quiz: usize) -> Result<(), NotNow> {
        // In case the party did not start through setup, e.g. the headless one, its photos' copies are made now.
        let images = self.hub().party.image_files(quiz);
        crate::phone_image::prepare(images);
        self.host(|party, now| party.start_quiz(quiz, now))
    }

    /// Moves the party on, for the host.
    pub fn next(&self) {
        let _ = self.host(|party, now| {
            party.request_next(now);
            Ok(())
        });
    }

    /// Skips the question being asked, for the host.
    pub fn skip(&self) {
        let _ = self.host(|party, now| {
            party.skip(now);
            Ok(())
        });
    }

    /// Stops the clock, for the host.
    pub fn pause(&self) {
        let _ = self.host(|party, now| {
            party.pause(now);
            Ok(())
        });
    }

    /// Starts the clock again, for the host.
    pub fn resume(&self) {
        let _ = self.host(|party, now| {
            party.resume(now);
            Ok(())
        });
    }

    /// Turns moving on by itself on or off, for the host.
    pub fn set_auto_advance(&self, auto_advance: AutoAdvance) {
        let now = Instant::now();
        let mut hub = self.hub();
        hub.party.auto_advance = auto_advance;
        // The players' countdown starts, stops or changes with it.
        if hub.party.countdown_title().is_some() {
            hub.broadcast_phase(now);
        }
        drop(hub);
        self.changed();
    }

    /// Sets whether new players need the game code to join, for the host.
    pub fn set_code_required(&self, required: bool) {
        self.hub().party.code_required = required;
        self.changed();
    }

    /// Changes the game code, for the host: the players already in the party stay, and rejoin with their tokens.
    pub fn new_code(&self) {
        self.hub().party.new_code();
        self.changed();
    }

    /// Removes a player, for the host: their phone is told and its connection closed.
    pub fn remove_player(&self, player: PlayerId) {
        let now = Instant::now();
        let mut hub = self.hub();
        if !hub.party.remove_player(player) {
            return;
        }
        let removed: Vec<ConnId> = hub
            .conns
            .iter()
            .filter(|(_, conn)| conn.player == Some(player))
            .map(|(id, _)| *id)
            .collect();
        for id in removed {
            hub.send(id, ServerMessage::Removed);
            hub.conns.remove(&id);
        }
        match hub.party.phase {
            Phase::Lobby => hub.broadcast_lobby(),
            Phase::Question => {
                if !hub.close_if_all_answered(now) {
                    hub.broadcast_answer_count();
                }
            }
            _ => {}
        }
        drop(hub);
        self.changed();
    }

    /// Ends the party, for the host: the players are told.
    pub fn end_party(&self) {
        let _ = self.host(|party, _| {
            party.end_party();
            Ok(())
        });
    }

    /// Starts a new party after one has ended, in setup with a new game code: the last party's players are closed, still
    /// showing its final leaderboard.
    pub fn new_party(&self) {
        let mut hub = self.hub();
        let leaderboard = hub.party.final_leaderboard();
        hub.party.new_party();
        hub.close_players(Some(ServerMessage::PartyEnded { leaderboard }));
        drop(hub);
        self.changed();
    }

    /// Does a command from a host phone.
    pub fn command(&self, command: HostCommand) {
        match command {
            HostCommand::StartQuiz { quiz } => {
                let _ = self.start_quiz(quiz as usize);
            }
            HostCommand::Next => self.next(),
            HostCommand::Skip => self.skip(),
            HostCommand::Pause => self.pause(),
            HostCommand::Resume => self.resume(),
            HostCommand::RemovePlayer { player } => self.remove_player(player),
            HostCommand::EndParty => self.end_party(),
            HostCommand::SetAutoAdvance { auto_advance } => self.set_auto_advance(auto_advance),
        }
    }

    /// Does a host's command, then tells the players, the host phones and the window what changed; the phones are
    /// told when the clock stops or starts too.
    fn host(&self, command: impl FnOnce(&mut Party, Instant) -> Result<(), NotNow>) -> Result<(), NotNow> {
        let now = Instant::now();
        let mut hub = self.hub();
        let was_paused = hub.party.paused_at.is_some();
        let was_showing = (hub.party.phase, hub.party.rounds_asked);
        command(&mut hub.party, now)?;
        let paused = hub.party.paused_at.is_some();
        if paused && !was_paused {
            hub.broadcast(&ServerMessage::Paused);
        } else if was_paused && !paused {
            // The question, if one is being asked, is sent again with the time it has left.
            hub.broadcast(&ServerMessage::Resumed);
            hub.broadcast_phase(now);
        } else if (hub.party.phase, hub.party.rounds_asked) != was_showing {
            hub.broadcast_phase(now);
        } else if let Some(countdown) = hub.countdown(now) {
            // Next was pressed and the party is counting down: what is shown stays.
            hub.broadcast(&countdown);
        }
        drop(hub);
        self.changed();
        Ok(())
    }

    /// Does what is due as time passes, see [`Party::tick`], and tells everyone if anything changed.
    fn tick(&self) {
        let now = Instant::now();
        let mut hub = self.hub();
        if hub.party.tick(now) {
            hub.broadcast_phase(now);
            drop(hub);
            self.changed();
        } else if hub.answer_count_due && hub.party.phase == Phase::Question {
            hub.broadcast_answer_count();
        } else {
            hub.send_lobby_if_due();
        }
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
    /// The URL the join QR code holds: the browser client at the Mac's address on the local network, with the game
    /// code if the party needs it.
    pub fn join_url(&self) -> String {
        let (code, required) = {
            let hub = self.shared.hub();
            (hub.party.code.clone(), hub.party.code_required)
        };
        let host = local_address().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        if required {
            format!("http://{host}:{}/?code={code}", self.port)
        } else {
            format!("http://{host}:{}/", self.port)
        }
    }

    /// The URL the host pairing QR code holds, shown only in setup: it opens the iOS app, which pairs as a host with
    /// the one-time pairing code.
    pub fn pairing_url(&self) -> String {
        let code = self.shared.hub().party.pairing_code.clone();
        let host = local_address().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        format!(
            "{}://host?host={host}&port={}&pair={code}",
            protocol::URL_SCHEME,
            self.port
        )
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
            answer_count_sent: None,
            answer_count_due: false,
            lobby_sent: None,
        }),
        changes: watch::channel(0).0,
    })
}

/// Serves a party on a listener until it fails, e.g. in the tests on a port of their own.
pub async fn serve(listener: tokio::net::TcpListener, shared: Arc<Shared>) -> std::io::Result<()> {
    let ticker = shared.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TICK_INTERVAL);
        loop {
            interval.tick().await;
            ticker.tick();
        }
    });
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
        .route(PARTY_INFO_PATH, get(party_info))
        .route("/image/{quiz}/{index}", get(quiz_image))
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

/// Whether the party needs a game code, for the join screens to ask for it only if so; never cached, as the host may
/// change it.
async fn party_info(State(shared): State<Arc<Shared>>) -> Response {
    let info = PartyInfo {
        code_required: shared.hub().party.code_required,
    };
    ([(header::CACHE_CONTROL, "no-store")], axum::Json(info)).into_response()
}

/// A question's image, for the phones: only the question being asked or the next one's, and a smaller copy of a big
/// photo. Phones may keep it, as its address names the party, see [`crate::party::Party::images_key`].
async fn quiz_image(Path((quiz, index)): Path<(usize, usize)>, State(shared): State<Arc<Shared>>) -> Response {
    let Some(original) = shared.hub().party.phone_image(quiz, index) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = tokio::task::spawn_blocking(move || crate::phone_image::for_phone(&original))
        .await
        .unwrap_or_default();
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, crate::library::content_type(&path)),
                (header::CACHE_CONTROL, "private, max-age=3600"),
            ],
            bytes,
        )
            .into_response(),
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
                host: false,
                ping: None,
                lobby_stale: false,
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
                        let lobby = ServerMessage::Lobby {
                            players: hub.party.lobby(),
                        };
                        hub.send(id, lobby);
                        hub.lobby_changed(Some(id));
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
        ClientMessage::PairHost {
            protocol,
            pairing_code,
            device_name,
        } => {
            if protocol != PROTOCOL_VERSION {
                reject(
                    &mut hub,
                    id,
                    RejectReason::WrongProtocol,
                    "Please update the app to control this party.",
                );
                return false;
            }
            match hub.party.pair(&pairing_code, &device_name) {
                Ok(token) => {
                    hub.send(id, ServerMessage::Paired { token });
                    become_host(shared, hub, id);
                    true
                }
                Err(refused) => {
                    reject(&mut hub, id, refused.reason, &refused.message);
                    false
                }
            }
        }
        ClientMessage::HostConnect { protocol, token } => {
            if protocol != PROTOCOL_VERSION {
                reject(
                    &mut hub,
                    id,
                    RejectReason::WrongProtocol,
                    "Please update the app to control this party.",
                );
                return false;
            }
            if !hub.party.is_host(&token) {
                reject(
                    &mut hub,
                    id,
                    RejectReason::NotAuthorised,
                    "This phone isn't paired with this party. Pair it again while the party is set up on the Mac.",
                );
                return false;
            }
            become_host(shared, hub, id);
            true
        }
        ClientMessage::Host { command } => {
            // Only a paired host phone's commands count.
            if hub.conns.get(&id).is_some_and(|conn| conn.host) {
                drop(hub);
                shared.command(command);
            }
            true
        }
    }
}

/// Makes a connection a host phone's, and sends it the party as a host sees it.
fn become_host(shared: &Shared, mut hub: MutexGuard<'_, Hub>, id: ConnId) {
    if let Some(conn) = hub.conns.get_mut(&id) {
        conn.host = true;
    }
    drop(hub);
    shared.changed();
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
