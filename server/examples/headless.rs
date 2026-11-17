//! Runs a party without its window, for trying a phone's client, e.g. the iOS app in the simulator: the game code is
//! ABCD, the first quiz starts when the first player joins, and the party moves on by itself every few seconds, as if
//! the host were pressing Next. It plays the quizzes in the folder given, `quizzes` by default:
//!
//! ```sh
//! cargo run -p bruce-quiz-party --example headless -- quizzes
//! ```

use std::time::Duration;

use bruce_quiz_party::{
    game::Phase,
    net,
    party::{Party, PartyQuiz},
};

fn main() -> anyhow::Result<()> {
    let folder = std::env::args().nth(1).unwrap_or_else(|| "quizzes".into());
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
    let server = net::start(Party::with_code("ABCD".into(), quizzes))?;
    println!("Join at {}", server.join_url());
    let mut started = false;
    loop {
        std::thread::sleep(Duration::from_secs(4));
        let (phase, players) = {
            let hub = server.shared.hub();
            (hub.party.phase, hub.party.players.len())
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
            Phase::Lobby | Phase::Question => {}
            _ => server.shared.next(),
        }
    }
}
