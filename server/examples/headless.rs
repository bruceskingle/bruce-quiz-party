//! Runs a party without its window, for trying a phone's client, e.g. the iOS app in the simulator: the game code is
//! ABCD, the first quiz starts when the first player joins, and the party moves on by itself every few seconds, as if
//! the host were pressing Next, pausing for a few seconds during the second question. It plays the quizzes in the
//! folder given, `quizzes` by default:
//!
//! ```sh
//! cargo run -p bruce-quiz-party --example headless -- quizzes
//! ```
//!
//! With `--setup` after the folder, it starts in setup and prints the host pairing link, then starts the party once a
//! phone has paired, for trying the iOS app's host mode. With `--no-code`, players need no game code to join.

use std::time::Duration;

use bruce_quiz_party::{
    game::Phase,
    net,
    party::{Party, PartyQuiz},
};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let folder = args.next().unwrap_or_else(|| "quizzes".into());
    let flags: Vec<String> = args.collect();
    let setup = flags.iter().any(|flag| flag == "--setup");
    let no_code = flags.iter().any(|flag| flag == "--no-code");
    let quizzes: Vec<PartyQuiz> = quiz_format::load_library(folder.as_ref())?
        .into_iter()
        .filter_map(|(folder, quiz)| {
            Some(PartyQuiz {
                folder,
                quiz: quiz.ok()?,
            })
        })
        .collect();
    anyhow::ensure!(!quizzes.is_empty(), "no quizzes in {folder}");
    let count = quizzes.len();
    let mut party = Party::with_code("ABCD".into(), quizzes);
    if setup {
        party.phase = Phase::Setup;
    }
    party.code_required = !no_code;
    let server = net::start(party)?;
    if setup {
        println!("Pair at {}", server.pairing_url());
        while server.shared.hub().party.hosts.is_empty() {
            std::thread::sleep(Duration::from_millis(200));
        }
        std::thread::sleep(Duration::from_secs(3));
        server
            .shared
            .start_party(&(0..count).collect::<Vec<_>>())
            .map_err(anyhow::Error::msg)?;
    }
    println!("Join at {}", server.join_url());
    let mut started = false;
    let mut paused = false;
    loop {
        std::thread::sleep(Duration::from_secs(4));
        let (phase, players, question) = {
            let hub = server.shared.hub();
            let party = &hub.party;
            (
                party.phase,
                party.players.len(),
                party.round.as_ref().map(|round| round.index),
            )
        };
        println!("{phase:?} with {players} players");
        match phase {
            Phase::Lobby if players > 0 && !started => {
                started = true;
                server
                    .shared
                    .start_quiz(0)
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
            }
            Phase::Question if question == Some(1) && !paused => {
                paused = true;
                server.shared.pause();
                println!("Paused");
                std::thread::sleep(Duration::from_secs(8));
                server.shared.resume();
                println!("Resumed");
            }
            Phase::Setup | Phase::Lobby | Phase::Question | Phase::Ended => {}
            _ => server.shared.next(),
        }
    }
}
