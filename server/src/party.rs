//! The party: its game code, its quizzes and its players. This is the game's state and rules alone, with no network
//! or window, so that it can be tested on its own; net.rs connects it to the phones, and the views show it.

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use protocol::{AutoAdvance, PlayerId, PlayerSummary, RejectReason};
use quiz_format::Quiz;

use crate::game::{Phase, Round};
use rand::Rng;

/// The letters a game code is made of: capitals without those easily mistaken for others, I, L, O and the like.
const CODE_LETTERS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ";

/// The number of letters in a game code.
const CODE_LENGTH: usize = 4;

/// The number of letters in a host pairing code, more than a game code's, as it lets a phone control the party.
const PAIRING_CODE_LENGTH: usize = 10;

/// The most quizzes a party may have.
pub const MAX_QUIZZES: usize = 9;

/// The longest name a player may choose, in characters, to fit on the big screen.
pub const MAX_NAME_CHARS: usize = 20;

/// A party.
#[derive(Debug)]
pub struct Party {
    /// The code players type, or which the join QR code holds, to join.
    pub code: String,
    /// Every quiz which could be read, to choose the party's from in setup.
    pub library: Vec<PartyQuiz>,
    /// The party's quizzes, chosen from the library in setup, which may be played.
    pub quizzes: Vec<PartyQuiz>,
    /// The players, in the order they joined.
    pub players: Vec<Player>,
    /// What the party is doing.
    pub phase: Phase,
    /// The question being asked or last asked, while a quiz is being played.
    pub round: Option<Round>,
    /// Each player's score in each quiz played, by the quiz's index.
    pub scores: HashMap<usize, HashMap<PlayerId, u32>>,
    /// The quizzes played, in the order they were played, for the leaderboards' columns.
    pub played: Vec<usize>,
    /// Counts the questions asked, so that a phase's clock can tell one question's reveal from the next's.
    pub(crate) rounds_asked: u64,
    /// The one-time code in the host pairing QR code, shown only in setup, changed each time a phone pairs.
    pub pairing_code: String,
    /// The phones paired as hosts.
    pub hosts: Vec<HostPhone>,
    /// Whether the party moves on by itself after each question.
    pub auto_advance: AutoAdvance,
    /// When the clock was stopped, while it is.
    pub paused_at: Option<Instant>,
    /// The phase being shown, the question it is about, and when it began to be shown, for moving on by itself.
    pub(crate) shown: Option<(Phase, u64, Instant)>,
    /// When the party moves on after the host pressed Next, counting down, and the phase and question it was pressed
    /// on, so that it lapses if the party has moved on some other way.
    pub(crate) next_at: Option<(Phase, u64, Instant)>,
}

/// A phone paired as a host, which may control the party.
#[derive(Debug, Clone)]
pub struct HostPhone {
    /// The phone's name, as it gave it.
    pub name: String,
    /// The secret the phone keeps to connect as a host.
    pub token: String,
}

/// One of the party's quizzes, and the folder its images are in.
#[derive(Debug, Clone)]
pub struct PartyQuiz {
    /// The quiz's folder.
    pub folder: PathBuf,
    /// The quiz.
    pub quiz: Quiz,
}

/// A player in the party.
#[derive(Debug, Clone)]
pub struct Player {
    /// The player's number.
    pub id: PlayerId,
    /// The player's name, unique in the party.
    pub name: String,
    /// The secret the player's device keeps to rejoin as this player.
    pub token: String,
    /// Whether the player's phone is connected now.
    pub connected: bool,
    /// The connection's round trip, as last measured, to correct the timing of the player's answers.
    pub round_trip: Option<Duration>,
}

/// A player who has joined, as told to their phone.
#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    pub player: PlayerId,
    pub name: String,
    pub token: String,
}

/// A join which was refused, as told to the phone.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub reason: RejectReason,
    pub message: String,
}

impl Refused {
    fn new(reason: RejectReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }
}

impl Party {
    /// A new party in setup, with a random game code, its quizzes to be chosen from the library.
    pub fn new(library: Vec<PartyQuiz>) -> Self {
        let mut party = Self::with_code(new_game_code(), library);
        party.phase = Phase::Setup;
        party
    }

    /// A new party with the given game code, already set up with every quiz in the library and waiting for players,
    /// for the tests and the headless party.
    pub fn with_code(code: String, library: Vec<PartyQuiz>) -> Self {
        Self {
            code,
            quizzes: library.clone(),
            library,
            players: Vec::new(),
            phase: Phase::Lobby,
            round: None,
            scores: HashMap::new(),
            played: Vec::new(),
            rounds_asked: 0,
            pairing_code: new_pairing_code(),
            hosts: Vec::new(),
            auto_advance: AutoAdvance::default(),
            paused_at: None,
            shown: None,
            next_at: None,
        }
    }

    /// Ends setup: the party has the chosen quizzes from the library, in the library's order, and players may join.
    pub fn start_party(&mut self, chosen: &[usize]) -> Result<(), String> {
        if self.phase != Phase::Setup {
            return Err("The party has already started.".into());
        }
        let quizzes: Vec<PartyQuiz> = self
            .library
            .iter()
            .enumerate()
            .filter(|(index, _)| chosen.contains(index))
            .map(|(_, quiz)| quiz.clone())
            .collect();
        if quizzes.is_empty() {
            return Err("Choose at least one quiz.".into());
        }
        if quizzes.len() > MAX_QUIZZES {
            return Err(format!("Choose at most {MAX_QUIZZES} quizzes."));
        }
        self.quizzes = quizzes;
        self.phase = Phase::Lobby;
        Ok(())
    }

    /// Starts a new party after one has ended: a new game code, no players or scores, back in setup. The paired host
    /// phones stay paired.
    pub fn new_party(&mut self) {
        let library = std::mem::take(&mut self.library);
        let hosts = std::mem::take(&mut self.hosts);
        let auto_advance = self.auto_advance;
        let rounds_asked = self.rounds_asked;
        *self = Self::new(library);
        self.hosts = hosts;
        self.auto_advance = auto_advance;
        self.rounds_asked = rounds_asked;
    }

    /// Pairs a phone as a host, in setup, with the pairing code from the QR code, returning the token it is to keep.
    /// The code is then changed, so that it pairs only one phone.
    pub fn pair(&mut self, pairing_code: &str, name: &str) -> Result<String, Refused> {
        if self.phase != Phase::Setup {
            return Err(Refused::new(
                RejectReason::NotAuthorised,
                "Phones can only be paired while the party is being set up on the Mac.",
            ));
        }
        if !pairing_code.trim().eq_ignore_ascii_case(&self.pairing_code) {
            return Err(Refused::new(
                RejectReason::NotAuthorised,
                "That pairing code has been used. Scan the one on the Mac now.",
            ));
        }
        let token = new_token();
        let name: String = name.trim().chars().take(40).collect();
        self.hosts.push(HostPhone {
            name: if name.is_empty() { "A phone".into() } else { name },
            token: token.clone(),
        });
        self.pairing_code = new_pairing_code();
        Ok(token)
    }

    /// Whether a token is a paired host phone's.
    pub fn is_host(&self, token: &str) -> bool {
        self.hosts.iter().any(|host| host.token == token)
    }

    /// Removes a player from the party, with their scores and any answer; their token no longer rejoins.
    pub fn remove_player(&mut self, id: PlayerId) -> bool {
        let Some(index) = self.players.iter().position(|player| player.id == id) else {
            return false;
        };
        self.players.remove(index);
        for scores in self.scores.values_mut() {
            scores.remove(&id);
        }
        if let Some(round) = &mut self.round {
            round.answers.remove(&id);
            round.points.remove(&id);
        }
        true
    }

    /// Joins a player, or rejoins one whose device gave the token it was given, who keeps their name and score.
    pub fn join(&mut self, game_code: &str, name: &str, token: Option<&str>) -> Result<Joined, Refused> {
        if !game_code.trim().eq_ignore_ascii_case(&self.code) {
            return Err(Refused::new(
                RejectReason::WrongGameCode,
                format!("There is no party with the code {}.", game_code.trim()),
            ));
        }
        match self.phase {
            Phase::Setup => {
                return Err(Refused::new(
                    RejectReason::Closed,
                    "The party hasn't started yet. Try again when the host says so.",
                ));
            }
            Phase::Ended => return Err(Refused::new(RejectReason::Closed, "The party is over.")),
            _ => {}
        }
        if let Some(token) = token
            && let Some(player) = self.players.iter_mut().find(|player| player.token == token)
        {
            player.connected = true;
            return Ok(Joined {
                player: player.id,
                name: player.name.clone(),
                token: player.token.clone(),
            });
        }
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err(Refused::new(RejectReason::BadName, "Please enter your name."));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(Refused::new(
                RejectReason::BadName,
                format!("Please use a name of at most {MAX_NAME_CHARS} letters."),
            ));
        }
        let name = self.unique_name(&name);
        let player = Player {
            id: self.players.iter().map(|player| player.id + 1).max().unwrap_or(1),
            name,
            token: new_token(),
            connected: true,
            round_trip: None,
        };
        let joined = Joined {
            player: player.id,
            name: player.name.clone(),
            token: player.token.clone(),
        };
        self.players.push(player);
        Ok(joined)
    }

    /// A name no other player has, the name itself or with a number after it, e.g. "Sam 2", so that the big screen
    /// can tell two players with the same name apart.
    fn unique_name(&self, name: &str) -> String {
        let taken = |candidate: &str| {
            self.players
                .iter()
                .any(|player| player.name.eq_ignore_ascii_case(candidate))
        };
        if !taken(name) {
            return name.to_string();
        }
        (2..)
            .map(|n| format!("{name} {n}"))
            .find(|candidate| !taken(candidate))
            .unwrap()
    }

    /// The player with a number, if they are in the party.
    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|player| player.id == id)
    }

    /// The player with a number, if they are in the party.
    pub fn player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.iter_mut().find(|player| player.id == id)
    }

    /// Marks a player's phone as disconnected; they stay in the party, to rejoin.
    pub fn disconnected(&mut self, id: PlayerId) {
        if let Some(player) = self.player_mut(id) {
            player.connected = false;
            player.round_trip = None;
        }
    }

    /// The players, as listed in the lobby.
    pub fn lobby(&self) -> Vec<PlayerSummary> {
        self.players
            .iter()
            .map(|player| PlayerSummary {
                player: player.id,
                name: player.name.clone(),
                connected: player.connected,
            })
            .collect()
    }
}

/// A random game code.
fn new_game_code() -> String {
    let mut rng = rand::rng();
    (0..CODE_LENGTH)
        .map(|_| CODE_LETTERS[rng.random_range(0..CODE_LETTERS.len())] as char)
        .collect()
}

/// A random host pairing code: enough letters that it cannot be guessed while it is shown.
fn new_pairing_code() -> String {
    let mut rng = rand::rng();
    (0..PAIRING_CODE_LENGTH)
        .map(|_| CODE_LETTERS[rng.random_range(0..CODE_LETTERS.len())] as char)
        .collect()
}

/// A random token, 128 bits as hex, which cannot be guessed.
fn new_token() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn party() -> Party {
        Party::with_code("ABCD".into(), Vec::new())
    }

    #[test]
    fn joins_with_the_code_in_any_case() {
        let mut party = party();
        let joined = party.join(" abcd ", "  Sam   Smith ", None).unwrap();
        assert_eq!(joined.player, 1);
        assert_eq!(joined.name, "Sam Smith");
        assert_eq!(joined.token.len(), 32);
        let refused = party.join("ABCE", "Jo", None).unwrap_err();
        assert_eq!(refused.reason, RejectReason::WrongGameCode);
    }

    #[test]
    fn refuses_bad_names() {
        let mut party = party();
        assert_eq!(
            party.join("ABCD", "  ", None).unwrap_err().reason,
            RejectReason::BadName
        );
        let long = "x".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(
            party.join("ABCD", &long, None).unwrap_err().reason,
            RejectReason::BadName
        );
        assert!(party.join("ABCD", &"é".repeat(MAX_NAME_CHARS), None).is_ok());
    }

    #[test]
    fn tells_players_with_the_same_name_apart() {
        let mut party = party();
        party.join("ABCD", "Sam", None).unwrap();
        assert_eq!(party.join("ABCD", "sam", None).unwrap().name, "sam 2");
        assert_eq!(party.join("ABCD", "Sam", None).unwrap().name, "Sam 3");
    }

    #[test]
    fn rejoins_as_the_same_player() {
        let mut party = party();
        let first = party.join("ABCD", "Sam", None).unwrap();
        party.join("ABCD", "Jo", None).unwrap();
        party.disconnected(first.player);
        assert!(!party.lobby()[0].connected);
        let again = party.join("ABCD", "Someone Else", Some(&first.token)).unwrap();
        assert_eq!(again, first);
        assert!(party.lobby()[0].connected);
        assert_eq!(party.players.len(), 2);
        // An unknown token joins as a new player.
        assert_eq!(party.join("ABCD", "Al", Some("nonsense")).unwrap().player, 3);
    }

    #[test]
    fn sets_up_then_lets_players_join() {
        let quiz = quiz_format::Quiz::parse("+++\ntitle = \"T\"\n+++\n## Q?\n- [x] A\n- [ ] B\n").unwrap();
        let library: Vec<PartyQuiz> = ["One", "Two", "Three"]
            .into_iter()
            .map(|title| PartyQuiz {
                folder: PathBuf::from(title),
                quiz: quiz_format::Quiz {
                    title: title.into(),
                    ..quiz.clone()
                },
            })
            .collect();
        let mut party = Party::new(library);
        assert_eq!(party.phase, Phase::Setup);
        let code = party.code.clone();
        assert_eq!(party.join(&code, "Sam", None).unwrap_err().reason, RejectReason::Closed);
        assert!(party.start_party(&[]).is_err());
        party.start_party(&[2, 0]).unwrap();
        let titles: Vec<&str> = party.quizzes.iter().map(|quiz| quiz.quiz.title.as_str()).collect();
        assert_eq!(titles, ["One", "Three"]);
        assert!(party.join(&code, "Sam", None).is_ok());
        assert!(party.start_party(&[1]).is_err(), "only once");
    }

    #[test]
    fn pairs_each_code_once_and_only_in_setup() {
        let mut party = Party::new(Vec::new());
        let code = party.pairing_code.clone();
        assert_eq!(code.len(), PAIRING_CODE_LENGTH);
        let token = party.pair(&code.to_lowercase(), "Bruce's iPhone").unwrap();
        assert!(party.is_host(&token));
        assert!(!party.is_host("nonsense"));
        assert_eq!(
            party.pair(&code, "Another").unwrap_err().reason,
            RejectReason::NotAuthorised,
            "the code is used up"
        );
        let next = party.pairing_code.clone();
        assert_ne!(next, code);
        party.library.push(PartyQuiz {
            folder: PathBuf::from("."),
            quiz: quiz_format::Quiz::parse("+++\ntitle = \"T\"\n+++\n## Q?\n- [x] A\n- [ ] B\n").unwrap(),
        });
        party.start_party(&[0]).unwrap();
        assert_eq!(
            party.pair(&next, "Late").unwrap_err().reason,
            RejectReason::NotAuthorised
        );
        // A new party keeps the phones paired, and is set up afresh.
        party.join(&party.code.clone(), "Sam", None).unwrap();
        party.new_party();
        assert_eq!(party.phase, Phase::Setup);
        assert!(party.players.is_empty());
        assert!(party.is_host(&token));
    }

    #[test]
    fn removes_players_for_good() {
        let mut party = party();
        let sam = party.join("ABCD", "Sam", None).unwrap();
        party.join("ABCD", "Jo", None).unwrap();
        assert!(party.remove_player(sam.player));
        assert!(!party.remove_player(sam.player));
        assert_eq!(party.players.len(), 1);
        // The removed player's token joins as someone new.
        assert_ne!(party.join("ABCD", "Sam", Some(&sam.token)).unwrap().player, sam.player);
    }

    #[test]
    fn game_codes_use_clear_letters() {
        let code = new_game_code();
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|byte| CODE_LETTERS.contains(&byte)));
    }
}
