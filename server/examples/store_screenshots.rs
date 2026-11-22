//! Plays one question of a party, without its window, for the App Store's screenshots of the iOS app: phones in the
//! simulator join it, launched with a join link, and are photographed with `xcrun simctl io` at the join screen's
//! lobby, the question, the reveal, the standings and the final leaderboard. A few made-up players join as well, and
//! everyone's answers are given here, so that the reveal's chart and the standings have something in them.
//!
//! ```sh
//! cargo run -p bruce-quiz-party --example store_screenshots -- <quizzes> <quiz title> <out folder> <udid>=<name>...
//! ```
//!
//! It prints the join link, then waits until each simulator has joined under its name, so that they can be launched
//! with `-joinLink <link> -autoJoin YES -quizparty:name <name>`, which a debug build of the app follows at once.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use bruce_quiz_party::{
    game::Phase,
    net::{self, Server},
    party::{Party, PartyQuiz},
};
use protocol::QuestionRef;

/// The made-up players, and whether each answers correctly, in the order they answer.
const OTHERS: [(&str, bool); 6] = [
    ("Priya", true),
    ("Tom", true),
    ("Mei", false),
    ("Olu", true),
    ("Hannah", false),
    ("Diego", true),
];

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [folder, title, out, devices @ ..] = args.as_slice() else {
        bail!("usage: store_screenshots <quizzes> <quiz title> <out folder> <udid>=<name>...");
    };
    let devices: Vec<(&str, &str)> = devices
        .iter()
        .map(|device| device.split_once('=').context("a device is <udid>=<name>"))
        .collect::<anyhow::Result<_>>()?;
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let quizzes: Vec<PartyQuiz> = quiz_format::load_library(folder.as_ref())?
        .into_iter()
        .filter_map(|(folder, quiz)| {
            Some(PartyQuiz {
                folder,
                quiz: quiz.ok()?,
            })
        })
        .collect();
    let quiz = quizzes
        .iter()
        .position(|party_quiz| party_quiz.quiz.title == *title)
        .with_context(|| format!("no quiz called {title} in {folder}"))?;
    let server = net::start(Party::with_code("ABCD".into(), quizzes))?;
    println!("Join at {}", server.join_url());

    // The others join first, so that the lobby lists them.
    let mut others = Vec::new();
    for (name, correct) in OTHERS {
        let joined = server
            .shared
            .hub()
            .party
            .join("ABCD", name, None)
            .map_err(|refused| anyhow::anyhow!("{} could not join: {}", name, refused.message))?;
        others.push((joined.player, correct));
    }
    wait_until(Duration::from_secs(120), "the simulators to join", || {
        let hub = server.shared.hub();
        devices
            .iter()
            .all(|(_, name)| hub.party.players.iter().any(|player| player.name == *name))
    })?;
    std::thread::sleep(Duration::from_secs(3));
    shoot(&devices, &out, "1-lobby")?;

    server
        .shared
        .start_quiz(quiz)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    wait_for(&server, Phase::Question)?;
    // The others answer over the first few seconds, so that their scores differ, and the simulators' players answer
    // correctly, the first quickest, after the question and its image have been photographed.
    std::thread::sleep(Duration::from_secs(5));
    shoot(&devices, &out, "2-question")?;
    let players: Vec<_> = {
        let hub = server.shared.hub();
        devices
            .iter()
            .filter_map(|(_, name)| hub.party.players.iter().find(|player| player.name == *name))
            .map(|player| (player.id, true))
            .collect()
    };
    for (player, correct) in players.into_iter().chain(others) {
        answer(&server, player, correct);
        std::thread::sleep(Duration::from_millis(700));
    }
    wait_for(&server, Phase::Reveal)?;
    std::thread::sleep(Duration::from_secs(3));
    shoot(&devices, &out, "3-reveal")?;

    server.shared.next();
    wait_for(&server, Phase::Standings)?;
    std::thread::sleep(Duration::from_secs(3));
    shoot(&devices, &out, "4-standings")?;

    server.shared.end_party();
    std::thread::sleep(Duration::from_secs(3));
    shoot(&devices, &out, "5-final")?;
    Ok(())
}

/// Answers the question being asked for a player, correctly or with the first wrong answer.
fn answer(server: &Server, player: u32, correct: bool) {
    let mut hub = server.shared.hub();
    let party = &mut hub.party;
    let Some(round) = party.round.as_ref() else { return };
    let (quiz, index) = (round.quiz, round.index);
    let question = &party.quizzes[quiz].quiz.questions[index];
    let choice = if correct {
        question.correct
    } else {
        (0..question.answers.len())
            .find(|&choice| choice != question.correct)
            .unwrap_or(0)
    };
    let question = QuestionRef {
        quiz: quiz as u32,
        index: index as u32,
    };
    party.answer(player, question, choice as u8, Instant::now());
}

/// Waits for the party to reach a phase, at most a minute, which is longer than any question's time limit.
fn wait_for(server: &Server, phase: Phase) -> anyhow::Result<()> {
    wait_until(Duration::from_secs(60), &format!("{phase:?}"), || {
        server.shared.hub().party.phase == phase
    })
}

fn wait_until(limit: Duration, what: &str, mut done: impl FnMut() -> bool) -> anyhow::Result<()> {
    let start = Instant::now();
    while !done() {
        if start.elapsed() > limit {
            bail!("gave up waiting for {what}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

/// Photographs each simulator's screen, as `<out>/<name>-<shot>.png`.
fn shoot(devices: &[(&str, &str)], out: &Path, shot: &str) -> anyhow::Result<()> {
    for (udid, name) in devices {
        let file = out.join(format!("{name}-{shot}.png"));
        let status = Command::new("xcrun")
            .args(["simctl", "io", udid, "screenshot"])
            .arg(&file)
            .status()?;
        anyhow::ensure!(status.success(), "could not photograph {name}'s simulator");
        println!("Saved {}", file.display());
    }
    Ok(())
}
