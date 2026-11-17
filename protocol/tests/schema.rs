//! Tests that the JSON Schema in `schema/` is that of the messages, so that a change to them shows in the schema which
//! the iOS app is checked against, and that example messages read as documented.
//!
//! It also writes an example of every message to `schema/examples-*.json`, which the iOS app's tests read, so that
//! its Swift types are checked against what this side really sends and expects.

use bruce_quiz_protocol::{
    AutoAdvance, ClientMessage, HostCommand, HostPhase, HostState, Leaderboard, LeaderboardKind, LeaderboardRow,
    PlayerResult, PlayerSummary, Progress, QuestionRef, QuestionView, QuizResult, QuizSummary, RejectReason, Reveal,
    ServerMessage, Standing,
};
use serde::Serialize;

/// Checks a generated file against the saved one, or writes it when `UPDATE_SCHEMA=1` is set.
fn check(name: &str, generated: impl Serialize) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(name);
    let generated = serde_json::to_string_pretty(&generated).unwrap() + "\n";
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let saved = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        saved == generated,
        "{} is out of date, run: UPDATE_SCHEMA=1 cargo test -p bruce-quiz-protocol",
        path.display()
    );
}

#[test]
fn schema_files_are_up_to_date() {
    check("client.json", schemars::schema_for!(ClientMessage));
    check("server.json", schemars::schema_for!(ServerMessage));
}

/// An example of every message, with every optional field both present and absent somewhere.
#[test]
fn example_files_are_up_to_date() {
    let question = QuestionRef { quiz: 1, index: 2 };
    let view = QuestionView {
        question,
        quiz_title: "Around the World".into(),
        number: 3,
        total: 5,
        text: "Which flag is this?".into(),
        image: Some("/quiz/around-the-world/images/flag.svg".into()),
        image_alt: Some("A red disc on white".into()),
        answers: vec!["Japan".into(), "Bangladesh".into(), "Palau".into()],
        time_limit_ms: 20_000,
        remaining_ms: 12_345,
        max_points: 1000,
    };
    let progress = Progress {
        quiz_title: "Flags of Asia".into(),
        asked: 6,
        total: 20,
    };
    let reveal = Reveal {
        question,
        progress: progress.clone(),
        correct: 0,
        counts: vec![3, 1, 0],
        source: Some("https://en.wikipedia.org/wiki/Flag_of_Japan".into()),
        result: Some(PlayerResult {
            choice: Some(0),
            correct: true,
            points: 850,
            quiz_score: 2400,
            quiz_rank: 2,
        }),
    };
    let players = vec![
        PlayerSummary {
            player: 1,
            name: "Sam".into(),
            connected: true,
        },
        PlayerSummary {
            player: 2,
            name: "Jo".into(),
            connected: false,
        },
    ];
    let client = vec![
        ClientMessage::Join {
            protocol: 1,
            game_code: "ABCD".into(),
            name: "Sam".into(),
            token: None,
        },
        ClientMessage::Join {
            protocol: 1,
            game_code: "ABCD".into(),
            name: "".into(),
            token: Some("0123456789abcdef".into()),
        },
        ClientMessage::Answer { question, choice: 1 },
        ClientMessage::Pong { nonce: 42 },
        ClientMessage::PairHost {
            protocol: 1,
            pairing_code: "XYZW1234".into(),
            device_name: "Sam's iPhone".into(),
        },
        ClientMessage::HostConnect {
            protocol: 1,
            token: "fedcba9876543210".into(),
        },
        ClientMessage::Host {
            command: HostCommand::StartQuiz { quiz: 1 },
        },
        ClientMessage::Host {
            command: HostCommand::Next,
        },
        ClientMessage::Host {
            command: HostCommand::Skip,
        },
        ClientMessage::Host {
            command: HostCommand::Pause,
        },
        ClientMessage::Host {
            command: HostCommand::Resume,
        },
        ClientMessage::Host {
            command: HostCommand::RemovePlayer { player: 2 },
        },
        ClientMessage::Host {
            command: HostCommand::EndParty,
        },
        ClientMessage::Host {
            command: HostCommand::SetAutoAdvance {
                auto_advance: AutoAdvance {
                    enabled: true,
                    reveal_secs: 5,
                    standings_secs: 30,
                    countdown_secs: 3,
                },
            },
        },
    ];
    let server = vec![
        ServerMessage::Welcome {
            player: 1,
            token: "0123456789abcdef".into(),
            name: "Sam".into(),
        },
        ServerMessage::Rejected {
            reason: RejectReason::WrongGameCode,
            message: "That isn't this party's code.".into(),
        },
        ServerMessage::Rejected {
            reason: RejectReason::WrongProtocol,
            message: "Update the app.".into(),
        },
        ServerMessage::Rejected {
            reason: RejectReason::BadName,
            message: "Choose a name.".into(),
        },
        ServerMessage::Rejected {
            reason: RejectReason::Closed,
            message: "Closed.".into(),
        },
        ServerMessage::Rejected {
            reason: RejectReason::NotAuthorised,
            message: "Not paired.".into(),
        },
        ServerMessage::Ping { nonce: 42 },
        ServerMessage::Lobby {
            players: players.clone(),
        },
        ServerMessage::Question(view.clone()),
        ServerMessage::Question(QuestionView {
            image: None,
            image_alt: None,
            ..view.clone()
        }),
        ServerMessage::AnswerLocked { question, choice: 1 },
        ServerMessage::AnswerCount {
            question,
            answered: 1,
            players: 2,
        },
        ServerMessage::Reveal(reveal.clone()),
        ServerMessage::Reveal(Reveal {
            source: None,
            result: Some(PlayerResult {
                choice: None,
                correct: false,
                points: 0,
                quiz_score: 0,
                quiz_rank: 2,
            }),
            ..reveal.clone()
        }),
        ServerMessage::Reveal(Reveal {
            result: None,
            ..reveal.clone()
        }),
        ServerMessage::Standings {
            standings: vec![Standing {
                player: 1,
                name: "Sam".into(),
                score: 2400,
                rank: 1,
                last_points: 850,
            }],
            progress: Some(progress.clone()),
        },
        ServerMessage::Leaderboard(Leaderboard {
            sorted_by: LeaderboardKind::Quiz { quiz: 1 },
            quizzes: vec!["Science Basics".into(), "Around the World".into()],
            rows: vec![LeaderboardRow {
                player: 2,
                name: "Jo".into(),
                quizzes: vec![None, Some(QuizResult { score: 2400, rank: 1 })],
                total: 2400,
                rank: 2,
            }],
        }),
        ServerMessage::Leaderboard(Leaderboard {
            sorted_by: LeaderboardKind::Party,
            quizzes: vec!["Science Basics".into()],
            rows: Vec::new(),
        }),
        ServerMessage::Paused,
        ServerMessage::Resumed,
        ServerMessage::Removed,
        ServerMessage::PartyEnded,
        ServerMessage::Paired {
            token: "fedcba9876543210".into(),
        },
    ];
    let quizzes = vec![QuizSummary {
        title: "Around the World".into(),
        questions: 5,
        played: true,
    }];
    let phases = [
        HostPhase::Lobby,
        HostPhase::Question {
            view: view.clone(),
            correct: 0,
            answered: vec![1],
        },
        HostPhase::Reveal(reveal.clone()),
        HostPhase::Standings {
            standings: vec![Standing {
                player: 1,
                name: "Sam".into(),
                score: 2400,
                rank: 1,
                last_points: 850,
            }],
            progress: Some(progress.clone()),
        },
        HostPhase::Leaderboard(Leaderboard {
            sorted_by: LeaderboardKind::Party,
            quizzes: vec!["Science Basics".into()],
            rows: Vec::new(),
        }),
        HostPhase::Ended,
        HostPhase::Setup,
    ];
    let server: Vec<ServerMessage> = server
        .into_iter()
        .chain(phases.into_iter().enumerate().map(|(index, phase)| {
            // Every other one waiting to move on by itself, so that the optional fields are both there and not.
            let waiting = index % 2 == 1;
            ServerMessage::HostState(HostState {
                game_code: "ABCD".into(),
                code_required: !waiting,
                quizzes: quizzes.clone(),
                players: players.clone(),
                phase,
                paused: waiting,
                auto_advance: AutoAdvance {
                    enabled: waiting,
                    ..AutoAdvance::default()
                },
                advance_in_ms: waiting.then_some(12_345),
                next: waiting.then(|| "Show Standings".into()),
            })
        }))
        .chain([ServerMessage::Countdown {
            next: "Next question".into(),
            in_ms: 3000,
        }])
        .collect();
    check("examples-client.json", client);
    check("examples-server.json", server);
}

#[test]
fn messages_read_as_documented() {
    let join: ClientMessage =
        serde_json::from_str(r#"{"type":"join","protocol":1,"game_code":"ABCD","name":"Sam"}"#).unwrap();
    assert_eq!(
        join,
        ClientMessage::Join {
            protocol: 1,
            game_code: "ABCD".into(),
            name: "Sam".into(),
            token: None
        }
    );
    let answer: ClientMessage =
        serde_json::from_str(r#"{"type":"answer","question":{"quiz":0,"index":2},"choice":1}"#).unwrap();
    assert_eq!(
        answer,
        ClientMessage::Answer {
            question: QuestionRef { quiz: 0, index: 2 },
            choice: 1
        }
    );
    let next: ClientMessage = serde_json::from_str(r#"{"type":"host","command":{"command":"next"}}"#).unwrap();
    assert_eq!(
        next,
        ClientMessage::Host {
            command: HostCommand::Next
        }
    );
    assert_eq!(
        serde_json::to_string(&ServerMessage::Paused).unwrap(),
        r#"{"type":"paused"}"#
    );
}

/// The progress reads the same everywhere, in the plural except for a quiz of one question.
#[test]
fn progress_in_words() {
    let progress = |asked, total| Progress {
        quiz_title: "Flags".into(),
        asked,
        total,
    };
    assert_eq!(progress(6, 20).words(), "After 6 of 20 Questions");
    assert_eq!(progress(1, 1).words(), "After 1 of 1 Question");
}
