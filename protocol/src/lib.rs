//! The messages between the party controller, the Mac which runs the party, and the phones and browsers which play it
//! or control it, sent as JSON over a WebSocket. The controller is the authority: it holds the game, the clock and the
//! scores, and the clients only show what they are sent and send what is pressed.
//!
//! The iOS app's Swift copy of these types must stay in step: `docs/Protocol.md` describes them, and the JSON Schema in
//! `protocol/schema/`, generated from them by the tests, is what both sides are checked against.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The version of these messages. A client sends it when it joins, and the controller refuses one whose version is
/// not its own, telling the player to update, rather than misreading what it sends.
pub const PROTOCOL_VERSION: u32 = 1;

/// The Bonjour service the controller advertises, for the iOS app to find it on the local network.
pub const BONJOUR_SERVICE: &str = "_quizparty._tcp";

/// The path of the WebSocket on the controller's web server.
pub const WEBSOCKET_PATH: &str = "/ws";

/// The URL scheme which opens the iOS app, e.g. from the join QR code, `quizparty://join?host=…&port=…&code=…`.
pub const URL_SCHEME: &str = "quizparty";

/// The path of the party's [`PartyInfo`] on the controller's web server, which a client reads before joining.
pub const PARTY_INFO_PATH: &str = "/party.json";

/// What a client may know of the party before joining, served as JSON at [`PARTY_INFO_PATH`]: whether to ask for the
/// game code. The code itself is never given, as it is what shows a player is in the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartyInfo {
    /// Whether new players need the game code to join.
    pub code_required: bool,
}

/// A player, as numbered by the controller for this party.
pub type PlayerId = u32;

/// A message from a phone or browser to the controller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Joins the party as a player, or rejoins it as the same player, with the same score, after the connection was
    /// lost, by giving the token the controller sent when the player first joined.
    Join {
        /// The client's [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The party's game code, as shown on the big screen; empty, or left out, if the party does not need one.
        #[serde(default)]
        game_code: String,
        /// The name the player chose, shown on the big screen.
        name: String,
        /// The token from the [`ServerMessage::Welcome`] of an earlier connection, kept on the device, to rejoin.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
    /// Answers the question being asked. Only the first answer to a question counts.
    Answer {
        /// The question being answered, as numbered in its [`ServerMessage::Question`].
        question: QuestionRef,
        /// The index of the chosen answer, from 0.
        choice: u8,
    },
    /// Answers a [`ServerMessage::Ping`] at once, so that the controller can measure the connection's round trip.
    Pong {
        /// The ping's nonce.
        nonce: u64,
    },
    /// Pairs the iOS app as a host, from the one-time pairing code in the QR code shown in setup mode.
    PairHost {
        /// The client's [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The one-time code from the pairing QR code.
        pairing_code: String,
        /// The phone's name, to list the paired phones on the Mac.
        device_name: String,
    },
    /// Connects a paired host phone, with the token it was given when it was paired.
    HostConnect {
        /// The client's [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The token from [`ServerMessage::Paired`].
        token: String,
    },
    /// A command from a host phone.
    Host {
        /// What to do.
        command: HostCommand,
    },
}

/// What a host can do, from the Mac or a paired host phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum HostCommand {
    /// Starts one of the party's quizzes, by its index in [`HostState::quizzes`].
    StartQuiz { quiz: u32 },
    /// Goes on from the reveal or a leaderboard, the only way the party moves on.
    Next,
    /// Ends the question being asked at once, scoring nothing, or skips the next one.
    Skip,
    /// Stops the clock.
    Pause,
    /// Starts the clock again.
    Resume,
    /// Removes a player from the party.
    RemovePlayer { player: PlayerId },
    /// Ends the party.
    EndParty,
    /// Turns moving on by itself on or off, or changes how long it waits.
    SetAutoAdvance { auto_advance: AutoAdvance },
}

/// Whether the party moves on by itself after each question, as if the host pressed Next: the reveal is shown for a
/// while, then the standings, then the next question is asked. At the end of a quiz, it shows its leaderboard, then
/// the party's, and waits there for the host to choose the next quiz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AutoAdvance {
    /// Whether it is on.
    pub enabled: bool,
    /// How long the reveal of each question's answer is shown, in seconds.
    pub reveal_secs: u32,
    /// How long the standings, or a quiz's leaderboard, are shown, in seconds.
    pub standings_secs: u32,
    /// How long to count down after the host presses Next on the reveal, the standings or a quiz's leaderboard, in
    /// seconds, so that the players see what is coming; 0 moves on at once. This applies whether or not the party
    /// moves on by itself.
    pub countdown_secs: u32,
}

impl Default for AutoAdvance {
    fn default() -> Self {
        Self {
            enabled: false,
            reveal_secs: 5,
            standings_secs: 5,
            countdown_secs: 3,
        }
    }
}

/// A question of the party: its quiz and its place in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QuestionRef {
    /// The quiz's index among the party's quizzes.
    pub quiz: u32,
    /// The question's index in its quiz, from 0.
    pub index: u32,
}

/// A message from the controller to a phone or browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The player has joined or rejoined.
    Welcome {
        /// The player's number in this party.
        player: PlayerId,
        /// The token to keep on the device and send with [`ClientMessage::Join`] to rejoin as the same player.
        token: String,
        /// The player's name, which the controller may have changed, e.g. to tell two players apart.
        name: String,
    },
    /// The join or pairing was refused, and the connection will be closed.
    Rejected {
        /// Why, for the client to act on.
        reason: RejectReason,
        /// Why, in words, to show the player.
        message: String,
    },
    /// Asks the client to answer with a [`ClientMessage::Pong`] at once.
    Ping {
        /// A number to send back.
        nonce: u64,
    },
    /// The party is waiting for the next quiz: who has joined.
    Lobby {
        /// The players, in the order they joined.
        players: Vec<PlayerSummary>,
    },
    /// A question, sent to every player at the same moment. The clock starts as it is sent.
    Question(QuestionView),
    /// The player's answer was taken. Any later answer to the same question is ignored.
    AnswerLocked {
        /// The question answered.
        question: QuestionRef,
        /// The answer taken.
        choice: u8,
    },
    /// How many players have answered the question so far.
    AnswerCount {
        /// The question being asked.
        question: QuestionRef,
        /// How many have answered.
        answered: u32,
        /// How many players are playing.
        players: u32,
    },
    /// The question is over: the correct answer, how many chose each, and how the player did.
    Reveal(Reveal),
    /// The standings after a question.
    Standings {
        /// Every player, in the order of their places.
        standings: Vec<Standing>,
        /// How far through its quiz the party is, for "After 6 of 20 Questions".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress: Option<Progress>,
    },
    /// A leaderboard, at the end of a quiz: first the quiz's, then the party's.
    Leaderboard(Leaderboard),
    /// The party will move on soon, by itself or as the host pressed Next: what comes next and when, for a countdown
    /// under the reveal or the standings. Whatever the party shows next ends it.
    Countdown {
        /// What comes next, e.g. "Next question" or "Standings".
        next: String,
        /// How long until then.
        in_ms: u32,
    },
    /// The clock has stopped.
    Paused,
    /// The clock has started again.
    Resumed,
    /// The host removed this player from the party. The connection will be closed.
    Removed,
    /// The party is over.
    PartyEnded,
    /// The phone is paired as a host: the token to keep, securely, and send with [`ClientMessage::HostConnect`].
    Paired {
        /// The host's token.
        token: String,
    },
    /// What a host phone shows: the whole state of the party, including the correct answer, sent whenever it changes.
    HostState(HostState),
}

/// Why a join or pairing was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The game code is not this party's.
    WrongGameCode,
    /// The client's protocol version is not the controller's: one of them needs updating.
    WrongProtocol,
    /// The name is empty or too long.
    BadName,
    /// The party is not taking players now.
    Closed,
    /// The pairing code or host token is not valid, e.g. as the party is not in setup mode.
    NotAuthorised,
}

/// A player, as listed in the lobby and on a host phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerSummary {
    /// The player's number.
    pub player: PlayerId,
    /// The player's name.
    pub name: String,
    /// Whether the player's phone is connected now.
    pub connected: bool,
}

/// A question as the players see it, without its answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QuestionView {
    /// Which question it is.
    pub question: QuestionRef,
    /// The quiz's title.
    pub quiz_title: String,
    /// The question's number in its quiz, from 1, for "Question 3 of 10".
    pub number: u32,
    /// How many questions the quiz has.
    pub total: u32,
    /// The question's text, or the prompt of an image question.
    pub text: String,
    /// The image's URL path on the controller's web server, if it is an image question, e.g. `/image/1/3?p=12345`:
    /// by the quiz and question, never the file's name, which might give the answer away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// A description of the image, for those who cannot see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_alt: Option<String>,
    /// The answers, from 2 to 6, each shown in the colour and shape of its position, see [`ANSWER_STYLES`].
    pub answers: Vec<String>,
    /// How long the players have to answer.
    pub time_limit_ms: u32,
    /// How long is left to answer, less than the time limit for a phone which rejoined during the question.
    pub remaining_ms: u32,
    /// The most a correct answer can score.
    pub max_points: u32,
}

/// The end of a question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Reveal {
    /// The question.
    pub question: QuestionRef,
    /// How far through its quiz the party is, for "After 6 of 20 Questions".
    pub progress: Progress,
    /// The index of the correct answer.
    pub correct: u8,
    /// How many players chose each answer.
    pub counts: Vec<u32>,
    /// A link to read more, e.g. the Wikipedia section the answer comes from, shown as a QR code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The next question's image, if it has one, for the phones to fetch now, so that it is there as the question is
    /// asked, rather than all of them fetching it then, with the clock running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_image: Option<String>,
    /// How this player did, sent only to players.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<PlayerResult>,
}

/// How far through its quiz the party is, after a question, shown on the reveal and the standings as "After 6 of 20
/// Questions".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Progress {
    /// The quiz's title.
    pub quiz_title: String,
    /// How many of its questions have been asked, counting the one just closed.
    pub asked: u32,
    /// How many questions the quiz has.
    pub total: u32,
}

impl Progress {
    /// The progress in words, e.g. "After 6 of 20 Questions", as every screen shows it.
    pub fn words(&self) -> String {
        let questions = if self.total == 1 { "Question" } else { "Questions" };
        format!("After {} of {} {questions}", self.asked, self.total)
    }
}

/// How a player did on a question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerResult {
    /// The player's answer, if they answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<u8>,
    /// Whether it was correct.
    pub correct: bool,
    /// The points it scored.
    pub points: u32,
    /// The player's score in this quiz so far.
    pub quiz_score: u32,
    /// The player's place in this quiz so far, from 1; players with the same score share a place.
    pub quiz_rank: u32,
}

/// A player's place after a question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Standing {
    /// The player.
    pub player: PlayerId,
    /// The player's name.
    pub name: String,
    /// The player's score in this quiz so far.
    pub score: u32,
    /// The player's place, from 1; players with the same score share a place.
    pub rank: u32,
    /// The points the player scored on the last question.
    pub last_points: u32,
}

/// A leaderboard, the same information sorted by a quiz's ranks or by the party's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Leaderboard {
    /// Which ranking the rows are sorted by.
    pub sorted_by: LeaderboardKind,
    /// The titles of the quizzes played so far, one column each.
    pub quizzes: Vec<String>,
    /// One row per player, in the order of `sorted_by`.
    pub rows: Vec<LeaderboardRow>,
}

/// Which ranking a leaderboard is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LeaderboardKind {
    /// A quiz's ranking, by its index in [`Leaderboard::quizzes`], shown at the end of that quiz.
    Quiz { quiz: u32 },
    /// The party's ranking, by the total of every quiz.
    Party,
}

/// A player's row in a leaderboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LeaderboardRow {
    /// The player.
    pub player: PlayerId,
    /// The player's name.
    pub name: String,
    /// The player's score and place in each quiz played so far, none for a quiz they did not play.
    pub quizzes: Vec<Option<QuizResult>>,
    /// The player's total over the party.
    pub total: u32,
    /// The player's place in the party, from 1; players with the same total share a place.
    pub rank: u32,
}

/// A player's score and place in a quiz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QuizResult {
    /// The player's score.
    pub score: u32,
    /// The player's place, from 1; players with the same score share a place.
    pub rank: u32,
}

/// Everything a host phone shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HostState {
    /// The party's game code, for the host to tell the players.
    pub game_code: String,
    /// Whether new players need the game code to join.
    pub code_required: bool,
    /// The party's quizzes, to choose the next one from.
    pub quizzes: Vec<QuizSummary>,
    /// The players.
    pub players: Vec<PlayerSummary>,
    /// What the party is doing.
    pub phase: HostPhase,
    /// Whether the clock is stopped.
    pub paused: bool,
    /// Whether the party moves on by itself, and how long it waits.
    pub auto_advance: AutoAdvance,
    /// How long until the party moves on by itself, if it will, as it was when this was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_in_ms: Option<u32>,
    /// What Next does now, e.g. "Show Standings", or none when there is nothing to move on to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// One of the party's quizzes, as listed to choose from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QuizSummary {
    /// The quiz's title.
    pub title: String,
    /// How many questions it has.
    pub questions: u32,
    /// Whether it has been played in this party.
    pub played: bool,
}

/// What the party is doing, as a host phone sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum HostPhase {
    /// The party is being set up on the Mac: no one can join yet.
    Setup,
    /// Waiting for the host to choose the next quiz.
    Lobby,
    /// A question is being asked.
    Question {
        /// The question, as the players see it.
        view: QuestionView,
        /// The index of the correct answer, which only hosts see while it is asked.
        correct: u8,
        /// The players who have answered so far.
        answered: Vec<PlayerId>,
    },
    /// The answer to a question is being shown.
    Reveal(Reveal),
    /// The standings after a question are being shown.
    Standings {
        /// Every player, in the order of their places.
        standings: Vec<Standing>,
        /// How far through its quiz the party is, for "After 6 of 20 Questions".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress: Option<Progress>,
    },
    /// A leaderboard is being shown.
    Leaderboard(Leaderboard),
    /// The party is over.
    Ended,
}

/// The colour and shape of an answer's position, the same on the big screen and the phones, so that players who cannot
/// tell the colours apart can tell the shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnswerStyle {
    /// The colour's name.
    pub name: &'static str,
    /// The colour, as CSS hex.
    pub colour: &'static str,
    /// The shape's name.
    pub shape: &'static str,
}

/// The style of each answer position, from the first to the sixth.
pub const ANSWER_STYLES: [AnswerStyle; 6] = [
    AnswerStyle {
        name: "red",
        colour: "#d64545",
        shape: "triangle",
    },
    AnswerStyle {
        name: "blue",
        colour: "#2f6fde",
        shape: "diamond",
    },
    AnswerStyle {
        name: "yellow",
        colour: "#e0b400",
        shape: "circle",
    },
    AnswerStyle {
        name: "green",
        colour: "#2e9d4f",
        shape: "square",
    },
    AnswerStyle {
        name: "purple",
        colour: "#8a55d6",
        shape: "star",
    },
    AnswerStyle {
        name: "orange",
        colour: "#e8772e",
        shape: "hexagon",
    },
];
