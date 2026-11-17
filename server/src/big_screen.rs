//! The big screen, shown on the Mac and on a TV or projector: the lobby, each question with its countdown, the reveal
//! of its answer, and the standings, with the host's controls below. Nothing on it lets anyone who sees it control
//! the party.

use std::time::{Duration, Instant};

use dioxus::prelude::*;
use protocol::{Leaderboard, LeaderboardKind, PlayerSummary, Reveal, Standing};
use quiz_format::{ImageCredit, Source};

use crate::{PartyServer, answer_shape::AnswerShape, game::Phase, net::Server};

/// The big screen, kept up to date as the party changes, and each second of a countdown.
#[component]
pub fn BigScreen() -> Element {
    let PartyServer(server) = use_context::<PartyServer>();
    let server = match server {
        Ok(server) => server,
        Err(error) => {
            return rsx! {
                div { class: "content",
                    h1 { "The party cannot start" }
                    p { class: "invalid", "{error}" }
                }
            };
        }
    };

    // Counts the party's changes, read below so that the screen is drawn again when it changes.
    let mut changes = use_signal(|| 0u64);
    let shared = server.shared.clone();
    use_future(move || {
        let mut receiver = shared.subscribe();
        async move {
            while receiver.changed().await.is_ok() {
                let count = *receiver.borrow_and_update();
                changes.set(count);
            }
        }
    });
    let _ = changes();
    // Ticks a few times a second, for the countdown.
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            tick += 1;
        }
    });

    let phase = server.shared.hub().party.phase;
    if phase == Phase::Question {
        let _ = tick();
    }
    let body = match phase {
        Phase::Lobby => rsx! { Lobby { server: server.clone() } },
        Phase::Question => rsx! { QuestionScreen { server: server.clone() } },
        Phase::Reveal => rsx! { RevealScreen { server: server.clone() } },
        Phase::Standings => rsx! { StandingsScreen { server: server.clone() } },
        Phase::QuizLeaderboard | Phase::PartyLeaderboard => rsx! { LeaderboardScreen { server: server.clone() } },
    };
    rsx! {
        div { class: "big-screen",
            {body}
            Controls { server: server.clone(), phase }
        }
    }
}

impl PartialEq for Server {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.shared, &other.shared)
    }
}

/// The host's controls, on the Mac: what moves the party on depends on what is being shown.
#[component]
fn Controls(server: Server, phase: Phase) -> Element {
    if phase == Phase::Lobby {
        return rsx! {};
    }
    let last = server.shared.hub().party.is_last_question();
    let next_label = match phase {
        Phase::Question => "End Question",
        Phase::Reveal if last => "Show the Quiz's Leaderboard",
        Phase::Reveal => "Show Standings",
        Phase::QuizLeaderboard => "Show the Party's Leaderboard",
        Phase::PartyLeaderboard => "Back to the Lobby",
        _ => "Next Question",
    };
    let next = server.shared.clone();
    let skip = server.shared.clone();
    rsx! {
        div { class: "controls",
            if phase == Phase::Question {
                button { onclick: move |_| skip.skip(), "Skip Question" }
            }
            button { class: "primary", onclick: move |_| next.next(), "{next_label}" }
        }
    }
}

/// The lobby: the join QR code and URL, the game code, the players, and the quizzes to start.
#[component]
fn Lobby(server: Server) -> Element {
    let url = server.join_url();
    let (code, players, quizzes): (String, Vec<PlayerSummary>, Vec<(String, usize, bool)>) = {
        let hub = server.shared.hub();
        let party = &hub.party;
        let quizzes = party
            .quizzes
            .iter()
            .enumerate()
            .map(|(index, quiz)| {
                (
                    quiz.quiz.title.clone(),
                    quiz.quiz.questions.len(),
                    party.played.contains(&index),
                )
            })
            .collect();
        (party.code.clone(), party.lobby(), quizzes)
    };
    let qr = crate::qr::svg(&url).unwrap_or_default();
    let mut error = use_signal(|| None::<String>);

    rsx! {
        div { class: "lobby",
            div { class: "lobby-join",
                div { class: "qr", dangerous_inner_html: "{qr}" }
                p { class: "join-url", "{url}" }
            }
            div { class: "lobby-main",
                p { class: "muted", "Scan the code to join, or go to the address and enter" }
                p { class: "game-code", "{code}" }
                h2 { "Players ({players.len()})" }
                if players.is_empty() {
                    p { class: "muted", "Waiting for players…" }
                }
                ul { class: "lobby-players",
                    for player in players {
                        li {
                            key: "{player.player}",
                            class: if player.connected { "" } else { "away" },
                            "{player.name}"
                        }
                    }
                }
                h2 { "Quizzes" }
                if quizzes.is_empty() {
                    p { class: "muted", "No quizzes could be read: see the Quizzes page." }
                }
                div { class: "start-quizzes",
                    for (index , (title , questions , played)) in quizzes.into_iter().enumerate() {
                        button {
                            key: "{index}",
                            class: "primary",
                            onclick: {
                                let shared = server.shared.clone();
                                move |_| error.set(shared.start_quiz(index).err().map(|e| e.to_string()))
                            },
                            "Start {title} ({questions} questions)"
                            if played {
                                " again"
                            }
                        }
                    }
                }
                if let Some(error) = error() {
                    p { class: "invalid", "Cannot start the quiz: {error}" }
                }
                p { class: "muted small",
                    "If macOS asks whether Bruce's Quiz Party may accept incoming network connections, choose Allow, "
                    "or the phones cannot join."
                }
            }
        }
    }
}

/// A question as it is asked: its text or image, its answers, the countdown and how many have answered.
#[component]
fn QuestionScreen(server: Server) -> Element {
    let now = Instant::now();
    let hub = server.shared.hub();
    let party = &hub.party;
    let (Some(view), Some(round)) = (party.question_view(now), party.round.as_ref()) else {
        return rsx! {};
    };
    let folder = party.quizzes[round.quiz].folder.clone();
    let image = party.quizzes[round.quiz].quiz.questions[round.index].image.clone();
    let (answered, playing) = party.answer_count();
    drop(hub);
    let seconds = view.remaining_ms.div_ceil(1000);
    let urgent = if seconds <= 5 { "countdown urgent" } else { "countdown" };

    rsx! {
        div { class: "question-screen",
            div { class: "question-header",
                span { class: "muted", "{view.quiz_title} · Question {view.number} of {view.total}" }
            }
            h1 { class: "question-text", "{view.text}" }
            div { class: "question-middle",
                div { class: "{urgent}", "{seconds}" }
                if let Some(image) = image {
                    img {
                        class: "big-image",
                        src: crate::library::image_url(&folder, &image.file),
                        alt: "{image.alt}",
                    }
                }
                div { class: "answered",
                    span { class: "answered-count", "{answered}" }
                    span { class: "muted", "of {playing} answered" }
                }
            }
            div { class: "big-answers",
                for (position , answer) in view.answers.iter().enumerate() {
                    div { key: "{position}", class: "answer answer-{position} big",
                        AnswerShape { position }
                        "{answer}"
                    }
                }
            }
        }
    }
}

/// The answer revealed: the correct one highlighted, how many chose each, and a QR code to read more.
#[component]
fn RevealScreen(server: Server) -> Element {
    let hub = server.shared.hub();
    let party = &hub.party;
    let (Some(reveal), Some(round)) = (party.reveal(), party.round.as_ref()) else {
        return rsx! {};
    };
    let quiz = &party.quizzes[round.quiz].quiz;
    let question = quiz.questions[round.index].clone();
    let heading = format!(
        "{} · Question {} of {}",
        quiz.title,
        round.index + 1,
        quiz.questions.len()
    );
    drop(hub);
    let Reveal {
        correct,
        counts,
        source,
        ..
    } = reveal;
    let most = counts.iter().copied().max().unwrap_or(0).max(1);
    let source_qr = source.as_deref().and_then(crate::qr::svg);

    rsx! {
        div { class: "question-screen",
            div { class: "question-header",
                span { class: "muted", "{heading}" }
            }
            h1 { class: "question-text", "{question.text}" }
            div { class: "reveal-middle",
                div { class: "reveal-bars",
                    for (position , count) in counts.iter().enumerate() {
                        div { key: "{position}", class: "reveal-bar-column",
                            span { class: "reveal-count", "{count}" }
                            div {
                                class: "reveal-bar answer-{position}",
                                style: "height: {(*count as f64 / most as f64 * 100.0).max(2.0)}%",
                            }
                            AnswerShape { position }
                        }
                    }
                }
                if let (Some(source), Some(qr)) = (source, source_qr) {
                    div { class: "source",
                        div { class: "qr small-qr", dangerous_inner_html: "{qr}" }
                        p { class: "muted small", "Read more: {source}" }
                    }
                }
            }
            div { class: "big-answers",
                for (position , answer) in question.answers.iter().enumerate() {
                    div {
                        key: "{position}",
                        class: if position == correct as usize { "answer answer-{position} big correct" } else { "answer answer-{position} big wrong" },
                        AnswerShape { position }
                        "{answer}"
                    }
                }
            }
        }
    }
}

/// The standings after a question.
#[component]
fn StandingsScreen(server: Server) -> Element {
    let standings: Vec<Standing> = server.shared.hub().party.standings();
    rsx! {
        div { class: "standings-screen",
            h1 { "Standings" }
            table { class: "standings",
                tbody {
                    for standing in standings {
                        tr { key: "{standing.player}",
                            td { class: "rank", "{ordinal(standing.rank)}" }
                            td { class: "name", "{standing.name}" }
                            td { class: "gained",
                                if standing.last_points > 0 {
                                    "+{standing.last_points}"
                                }
                            }
                            td { class: "score", "{standing.score}" }
                        }
                    }
                }
            }
        }
    }
}

/// The credits of a quiz, shown at its end: the articles its questions came from and its images' authors and licences.
#[derive(Clone, PartialEq)]
struct Credits {
    licence: Option<String>,
    sources: Vec<Source>,
    images: Vec<ImageCredit>,
}

/// A leaderboard, at the end of a quiz: first the quiz's, with its credits, then the party's.
#[component]
fn LeaderboardScreen(server: Server) -> Element {
    let (board, credits): (Option<Leaderboard>, Option<Credits>) = {
        let hub = server.shared.hub();
        let party = &hub.party;
        let credits = party.played.last().map(|quiz| {
            let quiz = &party.quizzes[*quiz].quiz;
            Credits {
                licence: quiz.licence.clone(),
                sources: quiz.sources.clone(),
                images: quiz.images.clone(),
            }
        });
        (party.current_leaderboard(), credits)
    };
    let Some(board) = board else { return rsx! {} };
    match board.sorted_by {
        LeaderboardKind::Quiz { quiz } => {
            let column = quiz as usize;
            let title = board.quizzes.get(column).cloned().unwrap_or_default();
            rsx! {
                div { class: "standings-screen",
                    h1 { "{title}: Leaderboard" }
                    table { class: "standings",
                        thead {
                            tr {
                                th {}
                                th {}
                                th { class: "score", "Score" }
                                th { class: "party-rank", "In the party" }
                            }
                        }
                        tbody {
                            for row in board.rows {
                                tr { key: "{row.player}",
                                    td { class: "rank",
                                        {row.quizzes[column].map(|result| ordinal(result.rank)).unwrap_or_default()}
                                    }
                                    td { class: "name", "{row.name}" }
                                    td { class: "score",
                                        {row.quizzes[column].map(|result| result.score.to_string()).unwrap_or_default()}
                                    }
                                    td { class: "party-rank", "{ordinal(row.rank)}" }
                                }
                            }
                        }
                    }
                    if let Some(credits) = credits {
                        CreditsBlock { credits }
                    }
                }
            }
        }
        LeaderboardKind::Party => rsx! {
            div { class: "standings-screen",
                h1 { "Party Leaderboard" }
                table { class: "standings",
                    thead {
                        tr {
                            th {}
                            th {}
                            for (index , title) in board.quizzes.iter().enumerate() {
                                th { key: "{index}", class: "quiz-column", "{title}" }
                            }
                            th { class: "score", "Total" }
                        }
                    }
                    tbody {
                        for row in board.rows {
                            tr { key: "{row.player}",
                                td { class: "rank", "{ordinal(row.rank)}" }
                                td { class: "name", "{row.name}" }
                                for (index , result) in row.quizzes.iter().enumerate() {
                                    td { key: "{index}", class: "quiz-column",
                                        if let Some(result) = result {
                                            "{result.score} "
                                            span { class: "muted", "({ordinal(result.rank)})" }
                                        } else {
                                            span { class: "muted", "–" }
                                        }
                                    }
                                }
                                td { class: "score", "{row.total}" }
                            }
                        }
                    }
                }
            }
        },
    }
}

/// A quiz's credits, as the licences of its content require.
#[component]
fn CreditsBlock(credits: Credits) -> Element {
    if credits.sources.is_empty() && credits.images.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "credits",
            if !credits.sources.is_empty() {
                p {
                    "Questions from "
                    {credits.sources.iter().map(|source| source.title.clone()).collect::<Vec<_>>().join(", ")}
                    if let Some(licence) = &credits.licence {
                        ", under {licence}"
                    }
                    "."
                }
            }
            for image in credits.images {
                p { key: "{image.file}",
                    "Image {image.file}: {image.author}, {image.licence}"
                    if let Some(source) = &image.source {
                        ", {source}"
                    }
                    "."
                }
            }
        }
    }
}

/// A place as an ordinal, e.g. 1st, 2nd, 3rd, 11th.
pub fn ordinal(rank: u32) -> String {
    let suffix = match (rank % 10, rank % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{rank}{suffix}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinals() {
        let ordinals: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111]
            .into_iter()
            .map(super::ordinal)
            .collect();
        assert_eq!(
            ordinals,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st", "111th"
            ]
        );
    }
}
