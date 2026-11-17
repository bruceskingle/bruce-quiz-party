//! The big screen, shown on the Mac and on a TV or projector: setup, before the guests arrive; then the lobby, each
//! question with its countdown, the reveal of its answer, the standings and the leaderboards, with the host's controls
//! below; and the end of the party. Nothing on it lets anyone who only sees it control the party: the host pairing QR
//! code is shown only in setup.

use std::time::{Duration, Instant};

use dioxus::prelude::*;
use protocol::{AutoAdvance, Leaderboard, LeaderboardKind, PlayerSummary, Reveal, Standing};
use quiz_format::{ImageCredit, Source};

use crate::{PartyServer, answer_shape::AnswerShape, game::Phase, net::Server, party::MAX_QUIZZES};

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

    let (phase, counting, countdown) = {
        let hub = server.shared.hub();
        let party = &hub.party;
        // What comes next and how many seconds until it, while the party is counting down to moving on.
        let countdown = party.countdown_title().zip(party.advance_at()).map(|(next, at)| {
            let seconds = at.saturating_duration_since(Instant::now()).as_millis().div_ceil(1000);
            (next, seconds)
        });
        (
            party.phase,
            party.phase == Phase::Question || party.advance_at().is_some(),
            countdown,
        )
    };
    if counting {
        let _ = tick();
    }
    let body = match phase {
        Phase::Setup => rsx! { SetupScreen { server: server.clone() } },
        Phase::Ended => rsx! { EndedScreen { server: server.clone() } },
        Phase::Lobby => rsx! { Lobby { server: server.clone() } },
        Phase::Question => rsx! { QuestionScreen { server: server.clone() } },
        Phase::Reveal => rsx! { RevealScreen { server: server.clone() } },
        Phase::Standings => rsx! { StandingsScreen { server: server.clone() } },
        Phase::QuizLeaderboard | Phase::PartyLeaderboard => rsx! { LeaderboardScreen { server: server.clone() } },
    };
    rsx! {
        div { class: "big-screen",
            {body}
            if let Some((next, seconds)) = countdown {
                div { class: "next-countdown",
                    "{next} in "
                    span { class: "next-countdown-seconds", "{seconds}" }
                }
            }
            Controls { server: server.clone(), phase }
        }
    }
}

/// A server is never equal to itself as a component's prop: the party behind it changes without the prop changing, and
/// Dioxus would otherwise skip drawing a screen again whose props are equal, e.g. leaving setup's list of paired phones
/// or the lobby's players as they were. The big screen is drawn again only when the party changes or a countdown ticks,
/// so drawing its screens with it costs little.
impl PartialEq for Server {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

/// The host's controls, on the Mac: moving on by itself, ending the party, stopping the clock, and moving on, which
/// depends on what is being shown.
#[component]
fn Controls(server: Server, phase: Phase) -> Element {
    let mut confirm_end = use_signal(|| false);
    if matches!(phase, Phase::Setup | Phase::Ended) {
        return rsx! {};
    }
    let (next_label, paused, auto) = {
        let hub = server.shared.hub();
        let party = &hub.party;
        (party.next_label(), party.paused_at.is_some(), party.auto_advance)
    };
    let shared = server.shared.clone();
    let can_pause = phase != Phase::Lobby;
    rsx! {
        div { class: "controls",
            label { class: "auto-advance",
                input {
                    r#type: "checkbox",
                    checked: auto.enabled,
                    onchange: {
                        let shared = shared.clone();
                        move |event: FormEvent| {
                            shared.set_auto_advance(AutoAdvance { enabled: event.checked(), ..auto })
                        }
                    },
                }
                "Move on by itself"
            }
            if paused {
                span { class: "paused-note", "Paused" }
            }
            div { class: "spacer" }
            if confirm_end() {
                span { "End the party for everyone?" }
                button {
                    class: "danger",
                    onclick: {
                        let shared = shared.clone();
                        move |_| {
                            confirm_end.set(false);
                            shared.end_party();
                        }
                    },
                    "Yes, End It"
                }
                button { onclick: move |_| confirm_end.set(false), "Cancel" }
            } else {
                button { onclick: move |_| confirm_end.set(true), "End the Party" }
            }
            if can_pause {
                button {
                    onclick: {
                        let shared = shared.clone();
                        move |_| if paused { shared.resume() } else { shared.pause() }
                    },
                    if paused {
                        "Resume"
                    } else {
                        "Pause"
                    }
                }
            }
            if phase == Phase::Question {
                button {
                    onclick: {
                        let shared = shared.clone();
                        move |_| shared.skip()
                    },
                    "Skip Question"
                }
            }
            if let Some(label) = next_label {
                button {
                    class: "primary",
                    onclick: {
                        let shared = shared.clone();
                        move |_| shared.next()
                    },
                    "{label}"
                }
            }
        }
    }
}

/// Setup, before the guests arrive: choosing the party's quizzes, whether it moves on by itself, and pairing host
/// phones with a one-time QR code, which is never shown once the party starts.
#[component]
fn SetupScreen(server: Server) -> Element {
    let (library, hosts, auto, code, code_required): (Vec<(String, usize)>, Vec<String>, AutoAdvance, String, bool) = {
        let hub = server.shared.hub();
        let party = &hub.party;
        (
            party
                .library
                .iter()
                .map(|quiz| (quiz.quiz.title.clone(), quiz.quiz.questions.len()))
                .collect(),
            party.hosts.iter().map(|host| host.name.clone()).collect(),
            party.auto_advance,
            party.code.clone(),
            party.code_required,
        )
    };
    let count = library.len();
    let mut chosen = use_signal(move || (0..count.min(MAX_QUIZZES)).collect::<Vec<usize>>());
    let mut error = use_signal(|| None::<String>);
    let qr = crate::qr::svg(&server.pairing_url()).unwrap_or_default();
    let shared = server.shared.clone();

    rsx! {
        div { class: "setup",
            div { class: "setup-main",
                h1 { "Set Up the Party" }
                p { class: "muted",
                    "Choose the party's quizzes, then start the party: players can join once it has started."
                }
                h2 { "Quizzes" }
                if library.is_empty() {
                    p { class: "muted", "No quizzes could be read: see the Quizzes page." }
                }
                ul { class: "setup-quizzes",
                    for (index , (title , questions)) in library.into_iter().enumerate() {
                        li { key: "{index}",
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: chosen.read().contains(&index),
                                    onchange: move |event: FormEvent| {
                                        let mut chosen = chosen.write();
                                        chosen.retain(|other| *other != index);
                                        if event.checked() {
                                            chosen.push(index);
                                        }
                                    },
                                }
                                "{title} "
                                span { class: "muted", "({questions} questions)" }
                            }
                        }
                    }
                }
                h2 { "Joining" }
                CodeSettings { server: server.clone(), code, code_required }
                h2 { "Moving On" }
                AutoAdvanceSettings { server: server.clone(), auto }
                div { class: "setup-start",
                    button {
                        class: "primary big-button",
                        onclick: move |_| error.set(shared.start_party(&chosen.read()).err()),
                        "Start the Party"
                    }
                    if let Some(error) = error() {
                        span { class: "invalid", "{error}" }
                    }
                }
            }
            div { class: "setup-pair",
                h2 { "Host Phones" }
                div { class: "qr", dangerous_inner_html: "{qr}" }
                p {
                    "To run the party from your phone too, scan this with the iPhone's camera and open it in Quiz Party. "
                    "Each code pairs one phone."
                }
                if hosts.is_empty() {
                    p { class: "muted", "No phones are paired yet." }
                } else {
                    p { "Paired: {hosts.join(\", \")}" }
                }
                p { class: "muted small",
                    "This code is shown only now, before the party starts, so that no one at the party can scan it."
                }
            }
        }
    }
}

/// Whether players need the game code to join, and a button for a new code. The code is made as the app starts and
/// kept from party to party, so that phones which remember it can rejoin, until the host asks for a new one.
#[component]
fn CodeSettings(server: Server, code: String, code_required: bool) -> Element {
    let shared = server.shared.clone();
    rsx! {
        div { class: "code-settings",
            label {
                input {
                    r#type: "checkbox",
                    checked: code_required,
                    onchange: move |event: FormEvent| shared.set_code_required(event.checked()),
                }
                "Players need the game code to join"
            }
            div { class: "code-row",
                span { class: if code_required { "setup-code" } else { "setup-code unused" }, "{code}" }
                NewCodeButton { server: server.clone() }
            }
            p { class: "muted small",
                if code_required {
                    "The code stays the same from party to party until you ask for a new one. Players already in the party stay in."
                } else {
                    "Anyone on this network who can reach the Mac can join, without a code."
                }
            }
        }
    }
}

/// Asks for a new game code. The players already in the party stay, as their phones rejoin without it.
#[component]
fn NewCodeButton(server: Server) -> Element {
    let shared = server.shared.clone();
    rsx! {
        button { onclick: move |_| shared.new_code(), "New Code" }
    }
}

/// Whether the party moves on by itself, and how long it shows the reveal and the standings.
#[component]
fn AutoAdvanceSettings(server: Server, auto: AutoAdvance) -> Element {
    let shared = server.shared.clone();
    let set = move |auto: AutoAdvance| shared.set_auto_advance(auto);
    let seconds =
        |event: &FormEvent, default: u32| event.value().trim().parse::<u32>().unwrap_or(default).clamp(1, 600);
    rsx! {
        div { class: "auto-settings",
            label {
                input {
                    r#type: "checkbox",
                    checked: auto.enabled,
                    onchange: {
                        let set = set.clone();
                        move |event: FormEvent| set(AutoAdvance { enabled: event.checked(), ..auto })
                    },
                }
                "Move on by itself once everyone has answered or the time is up"
            }
            label {
                "Show the answer for "
                input {
                    r#type: "number",
                    min: "1",
                    max: "600",
                    value: "{auto.reveal_secs}",
                    onchange: {
                        let set = set.clone();
                        move |event: FormEvent| {
                            set(AutoAdvance { reveal_secs: seconds(&event, auto.reveal_secs), ..auto })
                        }
                    },
                }
                " seconds"
            }
            label {
                "Then the standings for "
                input {
                    r#type: "number",
                    min: "1",
                    max: "600",
                    value: "{auto.standings_secs}",
                    onchange: {
                        let set = set.clone();
                        move |event: FormEvent| {
                            set(AutoAdvance { standings_secs: seconds(&event, auto.standings_secs), ..auto })
                        }
                    },
                }
                " seconds"
            }
            label {
                "When Next is pressed on the answer or the standings, count down for "
                input {
                    r#type: "number",
                    min: "0",
                    max: "60",
                    value: "{auto.countdown_secs}",
                    onchange: move |event: FormEvent| {
                        let countdown = event.value().trim().parse::<u32>().unwrap_or(auto.countdown_secs).min(60);
                        set(AutoAdvance { countdown_secs: countdown, ..auto })
                    },
                }
                " seconds"
            }
        }
    }
}

/// The end of the party: the party's leaderboard, and starting a new one.
#[component]
fn EndedScreen(server: Server) -> Element {
    let board = {
        let hub = server.shared.hub();
        let party = &hub.party;
        (!party.played.is_empty()).then(|| party.leaderboard(LeaderboardKind::Party))
    };
    let shared = server.shared.clone();
    rsx! {
        div { class: "standings-screen",
            h1 { "Thanks for Playing!" }
            if let Some(board) = board {
                PartyBoard { board }
            }
            div { class: "setup-start",
                button {
                    class: "primary big-button",
                    onclick: move |_| shared.new_party(),
                    "New Party"
                }
            }
        }
    }
}

/// A quiz as the lobby lists it: its title, how many questions it has, and whether it has been played.
type LobbyQuiz = (String, usize, bool);

/// The lobby: the join QR code and URL, the game code, the players, and the quizzes to start.
#[component]
fn Lobby(server: Server) -> Element {
    let url = server.join_url();
    let (code, players, quizzes): (Option<String>, Vec<PlayerSummary>, Vec<LobbyQuiz>) = {
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
        (party.code_required.then(|| party.code.clone()), party.lobby(), quizzes)
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
                if let Some(code) = code {
                    p { class: "muted", "Scan the code to join, or go to the address and enter" }
                    div { class: "code-row",
                        p { class: "game-code", "{code}" }
                        NewCodeButton { server: server.clone() }
                    }
                } else {
                    p { class: "muted", "Scan the code to join, or go to the address: no game code needed." }
                }
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
                            button {
                                class: "remove",
                                title: "Remove {player.name} from the party",
                                onclick: {
                                    let shared = server.shared.clone();
                                    move |_| shared.remove_player(player.player)
                                },
                                "×"
                            }
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
    let heading = format!("{} · {}", reveal.progress.quiz_title, reveal.progress.words());
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
    let (standings, heading): (Vec<Standing>, Option<String>) = {
        let hub = server.shared.hub();
        let party = &hub.party;
        let heading = party
            .progress()
            .map(|progress| format!("{} · {}", progress.quiz_title, progress.words()));
        (party.standings(), heading)
    };
    rsx! {
        div { class: "standings-screen",
            h1 { "Standings" }
            if let Some(heading) = heading {
                p { class: "muted standings-progress", "{heading}" }
            }
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
                PartyBoard { board }
            }
        },
    }
}

/// The party's leaderboard: a column for each quiz played, and the totals.
#[component]
fn PartyBoard(board: Leaderboard) -> Element {
    rsx! {
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
