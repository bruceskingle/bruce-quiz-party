//! The party: its game code, its quizzes and its players. This is the game's state and rules alone, with no network
//! or window, so that it can be tested on its own; net.rs connects it to the phones, and the views show it.

use std::{collections::HashMap, path::PathBuf, time::Duration};

use protocol::{PlayerId, PlayerSummary, RejectReason};
use quiz_format::Quiz;

use crate::game::{Phase, Round};
use rand::Rng;

/// The letters a game code is made of: capitals without those easily mistaken for others, I, L, O and the like.
const CODE_LETTERS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ";

/// The number of letters in a game code.
const CODE_LENGTH: usize = 4;

/// The longest name a player may choose, in characters, to fit on the big screen.
pub const MAX_NAME_CHARS: usize = 20;

/// A party.
#[derive(Debug)]
pub struct Party {
    /// The code players type, or which the join QR code holds, to join.
    pub code: String,
    /// The quizzes which may be played.
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
    /// Counts the questions asked, so that a question's timer can tell whether it is still the one being asked.
    pub(crate) rounds_asked: u64,
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
    /// A new party with a random game code.
    pub fn new(quizzes: Vec<PartyQuiz>) -> Self {
        Self::with_code(new_game_code(), quizzes)
    }

    /// A new party with the given game code, for the tests.
    pub fn with_code(code: String, quizzes: Vec<PartyQuiz>) -> Self {
        Self {
            code,
            quizzes,
            players: Vec::new(),
            phase: Phase::Lobby,
            round: None,
            scores: HashMap::new(),
            played: Vec::new(),
            rounds_asked: 0,
        }
    }

    /// Joins a player, or rejoins one whose device gave the token it was given, who keeps their name and score.
    pub fn join(&mut self, game_code: &str, name: &str, token: Option<&str>) -> Result<Joined, Refused> {
        if !game_code.trim().eq_ignore_ascii_case(&self.code) {
            return Err(Refused::new(
                RejectReason::WrongGameCode,
                format!("There is no party with the code {}.", game_code.trim()),
            ));
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
    fn game_codes_use_clear_letters() {
        let code = new_game_code();
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|byte| CODE_LETTERS.contains(&byte)));
    }
}
