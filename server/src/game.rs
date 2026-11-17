//! The game: asking a quiz's questions, timing and scoring the answers, revealing them and the standings, and moving on
//! when the host says so, or by itself if the host has turned that on. Like party.rs, it is rules alone: the time is
//! passed in, so that the tests can set it, and net.rs sends the players what changes, and calls [`Party::tick`] a few
//! times a second for whatever happens when time passes.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use protocol::{
    HostPhase, HostState, Leaderboard, LeaderboardKind, LeaderboardRow, PlayerId, PlayerResult, QuestionRef,
    QuestionView, QuizResult, QuizSummary, Reveal, Standing,
};

use crate::party::Party;

/// The most extra time a question stays open beyond its limit for answers still on their way from slow phones.
pub const MAX_GRACE: Duration = Duration::from_secs(2);

/// What the party is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Being set up on the Mac: choosing the quizzes and pairing host phones. No one can join yet.
    Setup,
    /// Waiting for the host to start a quiz.
    Lobby,
    /// A question is being asked.
    Question,
    /// The answer to the question is being shown.
    Reveal,
    /// The standings after the question are being shown.
    Standings,
    /// The leaderboard of the quiz just played, after its last question.
    QuizLeaderboard,
    /// The party's leaderboard, after the quiz's.
    PartyLeaderboard,
    /// The party is over.
    Ended,
}

/// A question being asked, or last asked, and its answers.
#[derive(Debug, Clone)]
pub struct Round {
    /// The quiz's index in the party.
    pub quiz: usize,
    /// The question's index in the quiz.
    pub index: usize,
    /// The number of the round, from [`Party::rounds_asked`], to tell one question's reveal from the next's.
    pub number: u64,
    /// When the question was sent to the players, all at once.
    pub asked_at: Instant,
    /// When the question closes unless everyone answers first: the time limit, and a grace for slow connections.
    pub deadline: Instant,
    /// The answers given, by player.
    pub answers: HashMap<PlayerId, Answer>,
    /// The points each player scored, once the question has closed.
    pub points: HashMap<PlayerId, u32>,
}

/// A player's answer to a question.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Answer {
    /// The index of the answer chosen.
    pub choice: u8,
    /// How long the player took, from the question reaching their phone to their answer leaving it: the time between
    /// sending and receiving, less their connection's round trip.
    pub elapsed: Duration,
}

/// Why a host's command could not be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotNow {
    /// A quiz is already being played, or the party is not waiting for one.
    Playing,
    /// There is no such quiz.
    NoSuchQuiz,
}

impl std::fmt::Display for NotNow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotNow::Playing => write!(f, "a quiz is already being played"),
            NotNow::NoSuchQuiz => write!(f, "there is no such quiz"),
        }
    }
}

impl std::error::Error for NotNow {}

impl Party {
    /// Starts one of the party's quizzes, from its first question, which is asked at once.
    pub fn start_quiz(&mut self, quiz: usize, now: Instant) -> Result<(), NotNow> {
        if self.phase != Phase::Lobby {
            return Err(NotNow::Playing);
        }
        self.resume(now);
        if quiz >= self.quizzes.len() {
            return Err(NotNow::NoSuchQuiz);
        }
        self.played.retain(|played| *played != quiz);
        self.played.push(quiz);
        self.scores.insert(quiz, HashMap::new());
        self.ask(quiz, 0, now);
        Ok(())
    }

    /// Asks a question of every player at once.
    fn ask(&mut self, quiz: usize, index: usize, now: Instant) {
        let settings = self.quizzes[quiz].quiz.question_settings(index);
        // The slowest connected phone's round trip, so that its answer, given in time, is not cut off on its way.
        let grace = self
            .players
            .iter()
            .filter(|player| player.connected)
            .filter_map(|player| player.round_trip)
            .max()
            .unwrap_or_default()
            .min(MAX_GRACE);
        // Everyone connected when a question is asked has played the quiz, with a score, if only 0, so that the
        // leaderboards can tell them from players who joined after it.
        let connected: Vec<PlayerId> = self
            .players
            .iter()
            .filter(|player| player.connected)
            .map(|player| player.id)
            .collect();
        let scores = self.scores.entry(quiz).or_default();
        for player in connected {
            scores.entry(player).or_insert(0);
        }
        self.rounds_asked += 1;
        self.round = Some(Round {
            quiz,
            index,
            number: self.rounds_asked,
            asked_at: now,
            deadline: now + Duration::from_secs(settings.time_limit.into()) + grace,
            answers: HashMap::new(),
            points: HashMap::new(),
        });
        self.phase = Phase::Question;
    }

    /// The question being asked, as the players see it, with the time left at `now`.
    pub fn question_view(&self, now: Instant) -> Option<QuestionView> {
        let round = self.round.as_ref()?;
        let party_quiz = &self.quizzes[round.quiz];
        let quiz = &party_quiz.quiz;
        let question = &quiz.questions[round.index];
        let settings = quiz.question_settings(round.index);
        let limit = Duration::from_secs(settings.time_limit.into());
        // While the clock is stopped, the time left is what it was when it stopped.
        let now = self.paused_at.unwrap_or(now);
        let remaining = limit.saturating_sub(now.saturating_duration_since(round.asked_at));
        Some(QuestionView {
            question: question_ref(round),
            quiz_title: quiz.title.clone(),
            number: round.index as u32 + 1,
            total: quiz.questions.len() as u32,
            text: question.text.clone(),
            image: question
                .image
                .as_ref()
                .map(|image| format!("/quiz/{}/{}", round.quiz, image.file)),
            image_alt: question.image.as_ref().map(|image| image.alt.clone()),
            answers: question.answers.clone(),
            time_limit_ms: limit.as_millis() as u32,
            remaining_ms: remaining.as_millis() as u32,
            max_points: settings.max_points,
        })
    }

    /// Takes a player's answer, returning whether it was taken: only the first answer to the question being asked,
    /// from a player in the party, choosing an answer it has, given within the time limit, counts.
    pub fn answer(&mut self, player: PlayerId, question: QuestionRef, choice: u8, now: Instant) -> bool {
        if self.phase != Phase::Question || self.paused_at.is_some() {
            return false;
        }
        let round_trip = match self.player(player) {
            Some(player) => player.round_trip.unwrap_or_default(),
            None => return false,
        };
        let Some(round) = self.round.as_mut() else { return false };
        if question_ref(round) != question || round.answers.contains_key(&player) {
            return false;
        }
        let quiz = &self.quizzes[round.quiz].quiz;
        if usize::from(choice) >= quiz.questions[round.index].answers.len() {
            return false;
        }
        let limit = Duration::from_secs(quiz.question_settings(round.index).time_limit.into());
        let elapsed = now.saturating_duration_since(round.asked_at).saturating_sub(round_trip);
        if elapsed > limit {
            return false;
        }
        round.answers.insert(player, Answer { choice, elapsed });
        true
    }

    /// Whether every connected player has answered the question being asked, so that it can close early.
    pub fn all_answered(&self) -> bool {
        let Some(round) = &self.round else { return false };
        let mut connected = self.players.iter().filter(|player| player.connected).peekable();
        connected.peek().is_some() && connected.all(|player| round.answers.contains_key(&player.id))
    }

    /// How many players have answered the question being asked, and how many are playing.
    pub fn answer_count(&self) -> (u32, u32) {
        let answered = self.round.as_ref().map_or(0, |round| round.answers.len());
        let playing = self.players.iter().filter(|player| player.connected).count();
        (answered as u32, playing.max(answered) as u32)
    }

    /// Ends the question being asked, scoring its answers, and reveals the answer.
    pub fn close_question(&mut self) {
        if self.phase != Phase::Question {
            return;
        }
        let Some(round) = self.round.as_mut() else { return };
        let quiz = &self.quizzes[round.quiz].quiz;
        let question = &quiz.questions[round.index];
        let settings = quiz.question_settings(round.index);
        let scores = self.scores.entry(round.quiz).or_default();
        for (player, answer) in &round.answers {
            let points = if usize::from(answer.choice) == question.correct {
                let elapsed_ms = answer.elapsed.as_millis() as u64;
                settings
                    .scoring
                    .points(settings.max_points, settings.time_limit, elapsed_ms)
            } else {
                0
            };
            round.points.insert(*player, points);
            *scores.entry(*player).or_default() += points;
        }
        self.phase = Phase::Reveal;
    }

    /// Moves on, when the host says so: from the question, ending it; from the reveal to the standings, or after the
    /// quiz's last question to its leaderboard; from the standings to the next question; from the quiz's leaderboard
    /// to the party's, and from that back to the lobby.
    pub fn next(&mut self, now: Instant) {
        self.resume(now);
        match self.phase {
            Phase::Setup | Phase::Lobby | Phase::Ended => {}
            Phase::Question => self.close_question(),
            Phase::Reveal if self.is_last_question() => self.phase = Phase::QuizLeaderboard,
            Phase::Reveal => self.phase = Phase::Standings,
            Phase::Standings => self.ask_next(now),
            Phase::QuizLeaderboard => self.phase = Phase::PartyLeaderboard,
            Phase::PartyLeaderboard => {
                self.round = None;
                self.phase = Phase::Lobby;
            }
        }
    }

    /// Moves on when the host presses Next: from the reveal, the standings or a quiz's leaderboard, after a short
    /// countdown, so that the players see what is coming, or at once if Next is pressed again during it; anywhere
    /// else, at once.
    pub fn request_next(&mut self, now: Instant) {
        self.resume(now);
        let countdown = Duration::from_secs(self.auto_advance.countdown_secs.into());
        let counting = self.pending_next().is_some();
        if self.countdown_title().is_some() && !countdown.is_zero() && !counting {
            self.next_at = Some((self.phase, self.rounds_asked, now + countdown));
        } else {
            self.next(now);
        }
    }

    /// When the party moves on after the host pressed Next, if it is counting down to that.
    fn pending_next(&self) -> Option<Instant> {
        let (phase, round, at) = self.next_at?;
        ((phase, round) == (self.phase, self.rounds_asked)).then_some(at)
    }

    /// What comes after the reveal, the standings or a quiz's leaderboard, for the countdown to it, e.g. "Standings";
    /// None where there is no countdown.
    pub fn countdown_title(&self) -> Option<&'static str> {
        Some(match self.phase {
            Phase::Reveal if self.is_last_question() => "The leaderboard",
            Phase::Reveal => "Standings",
            Phase::Standings => "Next question",
            Phase::QuizLeaderboard => "The party's leaderboard",
            _ => return None,
        })
    }

    /// Skips the question being asked, scoring nothing, and asks the next; anywhere else, the same as [`Party::next`].
    pub fn skip(&mut self, now: Instant) {
        self.resume(now);
        if self.phase == Phase::Question {
            if self.is_last_question() {
                self.phase = Phase::QuizLeaderboard;
            } else {
                self.ask_next(now);
            }
        } else {
            self.next(now);
        }
    }

    /// What Next does now, for its button on the Mac and the host phones, or None when there is nothing to move on to.
    pub fn next_label(&self) -> Option<&'static str> {
        Some(match self.phase {
            Phase::Setup | Phase::Lobby | Phase::Ended => return None,
            Phase::Question => "End Question",
            Phase::Reveal if self.is_last_question() => "Show the Quiz's Leaderboard",
            Phase::Reveal => "Show Standings",
            Phase::Standings => "Next Question",
            Phase::QuizLeaderboard => "Show the Party's Leaderboard",
            Phase::PartyLeaderboard => "Back to the Lobby",
        })
    }

    /// Stops the clock: the question's time stops running, nothing moves on by itself, and no answers are taken
    /// until it starts again. Returns whether it stopped it.
    pub fn pause(&mut self, now: Instant) -> bool {
        let running = matches!(
            self.phase,
            Phase::Question | Phase::Reveal | Phase::Standings | Phase::QuizLeaderboard | Phase::PartyLeaderboard
        );
        if !running || self.paused_at.is_some() {
            return false;
        }
        self.paused_at = Some(now);
        true
    }

    /// Starts the clock again, as if the time it was stopped had not passed. Returns whether it was stopped.
    pub fn resume(&mut self, now: Instant) -> bool {
        let Some(paused_at) = self.paused_at.take() else {
            return false;
        };
        let stopped = now.saturating_duration_since(paused_at);
        if let Some(round) = &mut self.round {
            round.asked_at += stopped;
            round.deadline += stopped;
        }
        if let Some((_, _, since)) = &mut self.shown {
            *since += stopped;
        }
        if let Some((_, _, at)) = &mut self.next_at {
            *at += stopped;
        }
        true
    }

    /// Ends the party: the players are told, and no one can join until the host starts a new one.
    pub fn end_party(&mut self) {
        self.paused_at = None;
        self.phase = Phase::Ended;
    }

    /// Does what is due as time passes: closes the question at its deadline, and, if the party moves on by itself,
    /// moves on once the reveal or standings have been shown for their while. Returns whether anything changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.paused_at.is_some() {
            return false;
        }
        self.note_shown(now);
        if self.phase == Phase::Question {
            if self.round.as_ref().is_some_and(|round| now >= round.deadline) {
                self.close_question();
                return true;
            }
            return false;
        }
        if let Some(at) = self.advance_at()
            && now >= at
        {
            self.next(now);
            return true;
        }
        false
    }

    /// Notes when what is being shown began to be shown, if it is new, for the time it is shown before moving on by
    /// itself. Called as the players are sent each change, and as time passes.
    pub fn note_shown(&mut self, now: Instant) {
        let showing = (self.phase, self.rounds_asked);
        if self.shown.is_none_or(|(phase, round, _)| (phase, round) != showing) {
            self.shown = Some((showing.0, showing.1, now));
        }
    }

    /// When the party will move on by itself, if it will: counting down after the host pressed Next, or if the party
    /// moves on by itself; never while the clock is stopped, nor from a question, the lobby or the party's
    /// leaderboard, where the host chooses what comes next.
    pub fn advance_at(&self) -> Option<Instant> {
        if self.paused_at.is_some() {
            return None;
        }
        // Whichever comes first, if the host pressed Next with less time to go by itself than the countdown.
        match (self.pending_next(), self.auto_advance_at()) {
            (Some(next), Some(auto)) => Some(next.min(auto)),
            (next, auto) => next.or(auto),
        }
    }

    /// When the party will move on by itself, if it moves on by itself and is showing a phase it moves on from.
    fn auto_advance_at(&self) -> Option<Instant> {
        let auto = self.auto_advance;
        if !auto.enabled {
            return None;
        }
        let wait = match self.phase {
            Phase::Reveal => auto.reveal_secs,
            Phase::Standings | Phase::QuizLeaderboard => auto.standings_secs,
            _ => return None,
        };
        let (phase, round, since) = self.shown?;
        ((phase, round) == (self.phase, self.rounds_asked)).then(|| since + Duration::from_secs(wait.into()))
    }

    /// Everything a host phone shows.
    pub fn host_state(&self, now: Instant) -> HostState {
        let phase = match self.phase {
            Phase::Setup => HostPhase::Setup,
            Phase::Lobby => HostPhase::Lobby,
            Phase::Question => match (self.question_view(now), &self.round) {
                (Some(view), Some(round)) => {
                    let mut answered: Vec<PlayerId> = round.answers.keys().copied().collect();
                    answered.sort();
                    HostPhase::Question {
                        view,
                        correct: self.quizzes[round.quiz].quiz.questions[round.index].correct as u8,
                        answered,
                    }
                }
                _ => HostPhase::Lobby,
            },
            Phase::Reveal => self.reveal().map_or(HostPhase::Lobby, HostPhase::Reveal),
            Phase::Standings => HostPhase::Standings {
                standings: self.standings(),
            },
            Phase::QuizLeaderboard | Phase::PartyLeaderboard => self
                .current_leaderboard()
                .map_or(HostPhase::Lobby, HostPhase::Leaderboard),
            Phase::Ended => HostPhase::Ended,
        };
        HostState {
            game_code: self.code.clone(),
            quizzes: self
                .quizzes
                .iter()
                .enumerate()
                .map(|(index, quiz)| QuizSummary {
                    title: quiz.quiz.title.clone(),
                    questions: quiz.quiz.questions.len() as u32,
                    played: self.played.contains(&index),
                })
                .collect(),
            players: self.lobby(),
            phase,
            paused: self.paused_at.is_some(),
            auto_advance: self.auto_advance,
            advance_in_ms: self
                .advance_at()
                .map(|at| at.saturating_duration_since(now).as_millis() as u32),
            next: self.next_label().map(str::to_string),
        }
    }

    /// Asks the quiz's next question, or after its last shows its leaderboard.
    fn ask_next(&mut self, now: Instant) {
        let Some(round) = &self.round else { return };
        let (quiz, index) = (round.quiz, round.index);
        if self.is_last_question() {
            self.phase = Phase::QuizLeaderboard;
        } else {
            self.ask(quiz, index + 1, now);
        }
    }

    /// A leaderboard: the quizzes played so far as its columns, a row for every player, sorted by one quiz's places
    /// or by the party's. A quiz's leaderboard lists only the players who played it; the party's lists everyone.
    pub fn leaderboard(&self, sorted_by: LeaderboardKind) -> Leaderboard {
        // Each played quiz's scores, and the places they give among those who played it.
        let columns: Vec<(&HashMap<PlayerId, u32>, Vec<u32>)> = self
            .played
            .iter()
            .map(|quiz| {
                let scores = &self.scores[quiz];
                (scores, scores.values().copied().collect())
            })
            .collect();
        let totals: Vec<u32> = self
            .players
            .iter()
            .map(|player| columns.iter().filter_map(|(scores, _)| scores.get(&player.id)).sum())
            .collect();
        let mut rows: Vec<LeaderboardRow> = self
            .players
            .iter()
            .zip(&totals)
            .map(|(player, total)| LeaderboardRow {
                player: player.id,
                name: player.name.clone(),
                quizzes: columns
                    .iter()
                    .map(|(scores, all)| {
                        scores.get(&player.id).map(|score| QuizResult {
                            score: *score,
                            rank: rank(*score, all),
                        })
                    })
                    .collect(),
                total: *total,
                rank: rank(*total, &totals),
            })
            .collect();
        match sorted_by {
            LeaderboardKind::Quiz { quiz } => {
                let column = quiz as usize;
                rows.retain(|row| row.quizzes.get(column).is_some_and(Option::is_some));
                rows.sort_by_key(|row| row.quizzes[column].map(|result| result.rank));
            }
            LeaderboardKind::Party => rows.sort_by_key(|row| row.rank),
        }
        Leaderboard {
            sorted_by,
            quizzes: self
                .played
                .iter()
                .map(|quiz| self.quizzes[*quiz].quiz.title.clone())
                .collect(),
            rows,
        }
    }

    /// The leaderboard being shown, if one is: the quiz just played is the last column.
    pub fn current_leaderboard(&self) -> Option<Leaderboard> {
        let sorted_by = match self.phase {
            Phase::QuizLeaderboard => LeaderboardKind::Quiz {
                quiz: self.played.len().checked_sub(1)? as u32,
            },
            Phase::PartyLeaderboard => LeaderboardKind::Party,
            _ => return None,
        };
        Some(self.leaderboard(sorted_by))
    }

    /// Whether the question being asked, or last asked, is its quiz's last.
    pub fn is_last_question(&self) -> bool {
        self.round
            .as_ref()
            .is_some_and(|round| round.index + 1 >= self.quizzes[round.quiz].quiz.questions.len())
    }

    /// The reveal of the question just closed: the correct answer and how many chose each, without any player's
    /// own result.
    pub fn reveal(&self) -> Option<Reveal> {
        let round = self.round.as_ref()?;
        let question = &self.quizzes[round.quiz].quiz.questions[round.index];
        let mut counts = vec![0u32; question.answers.len()];
        for answer in round.answers.values() {
            counts[usize::from(answer.choice)] += 1;
        }
        Some(Reveal {
            question: question_ref(round),
            correct: question.correct as u8,
            counts,
            source: question.source.clone(),
            result: None,
        })
    }

    /// How a player did on the question just closed, and where it leaves them in the quiz.
    pub fn player_result(&self, player: PlayerId) -> Option<PlayerResult> {
        let round = self.round.as_ref()?;
        let question = &self.quizzes[round.quiz].quiz.questions[round.index];
        let answer = round.answers.get(&player);
        let standing = self
            .standings()
            .into_iter()
            .find(|standing| standing.player == player)?;
        Some(PlayerResult {
            choice: answer.map(|answer| answer.choice),
            correct: answer.is_some_and(|answer| usize::from(answer.choice) == question.correct),
            points: round.points.get(&player).copied().unwrap_or(0),
            quiz_score: standing.score,
            quiz_rank: standing.rank,
        })
    }

    /// Every player's score and place in the quiz being played, best first, players with the same score sharing a
    /// place, and those with the same place in the order they joined.
    pub fn standings(&self) -> Vec<Standing> {
        let Some(round) = &self.round else { return Vec::new() };
        let scores = self.scores.get(&round.quiz);
        let score = |player: PlayerId| scores.and_then(|scores| scores.get(&player)).copied().unwrap_or(0);
        let mut standings: Vec<Standing> = self
            .players
            .iter()
            .map(|player| Standing {
                player: player.id,
                name: player.name.clone(),
                score: score(player.id),
                rank: 0,
                last_points: round.points.get(&player.id).copied().unwrap_or(0),
            })
            .collect();
        let all: Vec<u32> = standings.iter().map(|standing| standing.score).collect();
        for standing in &mut standings {
            standing.rank = rank(standing.score, &all);
        }
        standings.sort_by_key(|standing| standing.rank);
        standings
    }
}

/// The reference of a round's question, as the players are told it.
fn question_ref(round: &Round) -> QuestionRef {
    QuestionRef {
        quiz: round.quiz as u32,
        index: round.index as u32,
    }
}

/// The place of a score among scores, from 1: one more than the number of higher scores, so that equal scores share a
/// place and the next is skipped, e.g. 900, 800, 800, 700 are 1st, 2nd, 2nd and 4th.
pub fn rank(score: u32, scores: &[u32]) -> u32 {
    1 + scores.iter().filter(|other| **other > score).count() as u32
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use quiz_format::Quiz;

    use protocol::AutoAdvance;

    use super::*;
    use crate::party::PartyQuiz;

    /// A party playing a two-question quiz, with 10 seconds a question, linear scoring to half points, and three
    /// players, Sam, Jo and Al, numbered 1, 2 and 3.
    fn party() -> Party {
        let quiz = Quiz::parse(
            "+++\ntitle = \"T\"\ntime_limit = 10\n+++\n\
             ## One?\n- [x] A\n- [ ] B\n- [ ] C\nSource: https://example.org/one\n\
             ## Two?\n- [ ] A\n- [x] B\n",
        )
        .unwrap();
        let mut party = Party::with_code(
            "ABCD".into(),
            vec![PartyQuiz {
                folder: PathBuf::from("."),
                quiz,
            }],
        );
        for name in ["Sam", "Jo", "Al"] {
            party.join("ABCD", name, None).unwrap();
        }
        party
    }

    fn at(start: Instant, seconds: f64) -> Instant {
        start + Duration::from_secs_f64(seconds)
    }

    const FIRST: QuestionRef = QuestionRef { quiz: 0, index: 0 };

    #[test]
    fn scores_by_speed_and_ranks_ties_together() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        assert_eq!(party.phase, Phase::Question);
        assert!(party.answer(1, FIRST, 0, at(start, 0.0)));
        assert!(party.answer(2, FIRST, 0, at(start, 5.0)));
        assert!(!party.all_answered());
        assert!(party.answer(3, FIRST, 1, at(start, 1.0)));
        assert!(party.all_answered());
        party.close_question();
        assert_eq!(party.phase, Phase::Reveal);

        let reveal = party.reveal().unwrap();
        assert_eq!((reveal.correct, reveal.counts.clone()), (0, vec![2, 1, 0]));
        assert_eq!(reveal.source.as_deref(), Some("https://example.org/one"));

        let sam = party.player_result(1).unwrap();
        assert_eq!((sam.correct, sam.points, sam.quiz_rank), (true, 1000, 1));
        let jo = party.player_result(2).unwrap();
        assert_eq!((jo.points, jo.quiz_rank), (750, 2));
        let al = party.player_result(3).unwrap();
        assert_eq!((al.correct, al.points, al.quiz_rank), (false, 0, 3));

        assert_eq!(rank(800, &[900, 800, 800, 700]), 2);
        assert_eq!(rank(700, &[900, 800, 800, 700]), 4);
    }

    #[test]
    fn takes_only_the_first_answer_in_time() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        assert!(party.answer(1, FIRST, 1, at(start, 1.0)));
        assert!(!party.answer(1, FIRST, 0, at(start, 2.0)), "a second answer");
        assert!(
            !party.answer(2, QuestionRef { quiz: 0, index: 1 }, 0, at(start, 2.0)),
            "another question"
        );
        assert!(!party.answer(2, FIRST, 3, at(start, 2.0)), "an answer it does not have");
        assert!(!party.answer(9, FIRST, 0, at(start, 2.0)), "not a player");
        assert!(!party.answer(2, FIRST, 0, at(start, 10.5)), "too late");
    }

    #[test]
    fn allows_for_the_round_trip() {
        let mut party = party();
        party.player_mut(2).unwrap().round_trip = Some(Duration::from_millis(400));
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        // The slowest phone's round trip is added to the time the question stays open.
        assert_eq!(party.round.as_ref().unwrap().deadline, at(start, 10.4));
        // Jo's answer left her phone at 5 seconds, and arrived 0.4 seconds later.
        assert!(party.answer(2, FIRST, 0, at(start, 5.4)));
        // Within the limit once the round trip is taken off.
        assert!(party.answer(1, FIRST, 0, at(start, 10.0)));
        party.close_question();
        assert_eq!(party.player_result(2).unwrap().points, 750);
    }

    #[test]
    fn runs_the_quiz_through_to_the_lobby() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        assert_eq!(party.start_quiz(0, start), Err(NotNow::Playing));
        party.answer(1, FIRST, 0, start);
        party.next(start);
        assert_eq!(party.phase, Phase::Reveal);
        party.next(start);
        assert_eq!(party.phase, Phase::Standings);
        assert_eq!(party.standings()[0].name, "Sam");
        assert!(!party.is_last_question());
        party.next(start);
        assert_eq!(party.phase, Phase::Question);
        assert_eq!(party.question_view(start).unwrap().number, 2);
        // Skipping the last question scores nothing and goes to the quiz's leaderboard, then the party's.
        party.skip(start);
        assert_eq!(party.phase, Phase::QuizLeaderboard);
        assert_eq!(party.current_leaderboard().unwrap().rows[0].name, "Sam");
        party.next(start);
        assert_eq!(party.phase, Phase::PartyLeaderboard);
        party.next(start);
        assert_eq!(party.phase, Phase::Lobby);
        assert!(party.round.is_none());
        assert_eq!(party.scores[&0][&1], 1000);
        assert_eq!(party.played, vec![0]);
    }

    #[test]
    fn a_rejoining_phone_sees_the_time_left() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        let view = party.question_view(at(start, 3.0)).unwrap();
        assert_eq!((view.time_limit_ms, view.remaining_ms), (10_000, 7_000));
        assert_eq!(view.answers.len(), 3);
    }

    #[test]
    fn closes_early_only_when_every_connected_player_has_answered() {
        let mut party = party();
        party.disconnected(3);
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        party.answer(1, FIRST, 0, start);
        party.answer(2, FIRST, 0, start);
        assert!(party.all_answered());
        assert_eq!(party.answer_count(), (2, 2));
    }

    #[test]
    fn closes_the_question_at_its_deadline() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        assert!(!party.tick(at(start, 9.9)));
        assert_eq!(party.phase, Phase::Question);
        assert!(party.tick(at(start, 10.0)));
        assert_eq!(party.phase, Phase::Reveal);
        // Without auto-advance, it waits there for the host.
        assert!(!party.tick(at(start, 100.0)));
        assert_eq!(party.next_label(), Some("Show Standings"));
    }

    #[test]
    fn a_pause_stops_the_clock() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        assert!(party.pause(at(start, 4.0)));
        assert!(!party.pause(at(start, 5.0)), "already paused");
        // No time passes, and no answers are taken, while it is stopped.
        assert_eq!(party.question_view(at(start, 30.0)).unwrap().remaining_ms, 6_000);
        assert!(!party.answer(1, FIRST, 0, at(start, 30.0)));
        assert!(!party.tick(at(start, 30.0)));
        assert_eq!(party.phase, Phase::Question);
        // After a minute's pause, the question has its 6 seconds left, and an answer 1 second later took 5.
        assert!(party.resume(at(start, 64.0)));
        assert_eq!(party.question_view(at(start, 64.0)).unwrap().remaining_ms, 6_000);
        assert!(party.answer(1, FIRST, 0, at(start, 65.0)));
        assert_eq!(
            party.round.as_ref().unwrap().answers[&1].elapsed,
            Duration::from_secs(5)
        );
        assert!(!party.tick(at(start, 69.9)));
        assert!(party.tick(at(start, 70.0)));
        assert_eq!(party.phase, Phase::Reveal);
    }

    #[test]
    fn moves_on_by_itself_when_asked_to() {
        let mut party = party();
        party.auto_advance = AutoAdvance {
            enabled: true,
            reveal_secs: 5,
            standings_secs: 30,
            countdown_secs: 3,
        };
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        for player in 1..=3 {
            party.answer(player, FIRST, 0, start);
        }
        party.close_question();
        // The reveal for 5 seconds, from when the clock first sees it.
        assert!(!party.tick(at(start, 1.0)));
        assert_eq!(party.advance_at(), Some(at(start, 6.0)));
        assert!(!party.tick(at(start, 5.9)));
        assert!(party.tick(at(start, 6.0)));
        assert_eq!(party.phase, Phase::Standings);
        // The standings for 30 seconds, less a 10-second pause, then the next question.
        assert!(!party.tick(at(start, 6.0)));
        party.pause(at(start, 10.0));
        assert_eq!(party.advance_at(), None);
        party.resume(at(start, 20.0));
        assert!(!party.tick(at(start, 45.9)));
        assert!(party.tick(at(start, 46.0)));
        assert_eq!(party.phase, Phase::Question);
        // The last question's reveal goes on to the quiz's leaderboard, then the party's, and waits there.
        party.close_question();
        party.tick(at(start, 50.0));
        assert!(party.tick(at(start, 55.0)));
        assert_eq!(party.phase, Phase::QuizLeaderboard);
        party.tick(at(start, 55.0));
        assert!(party.tick(at(start, 85.0)));
        assert_eq!(party.phase, Phase::PartyLeaderboard);
        assert!(!party.tick(at(start, 1000.0)));
        assert_eq!(party.phase, Phase::PartyLeaderboard);
    }

    #[test]
    fn next_counts_down_from_the_reveal_and_standings() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        // Ending a question is at once.
        party.request_next(start);
        assert_eq!(party.phase, Phase::Reveal);
        assert_eq!(party.countdown_title(), Some("Standings"));
        // From the reveal, 3 seconds' countdown, which a pause stops.
        party.request_next(at(start, 1.0));
        assert_eq!(party.phase, Phase::Reveal);
        assert_eq!(party.advance_at(), Some(at(start, 4.0)));
        party.pause(at(start, 2.0));
        party.resume(at(start, 12.0));
        assert!(!party.tick(at(start, 13.9)));
        assert!(party.tick(at(start, 14.0)));
        assert_eq!(party.phase, Phase::Standings);
        assert_eq!(party.advance_at(), None, "the countdown lapses once the party moves on");
        // Pressing Next again during the countdown moves on at once.
        party.request_next(at(start, 15.0));
        party.request_next(at(start, 15.5));
        assert_eq!(party.phase, Phase::Question);
        // With no countdown, Next is at once.
        party.auto_advance.countdown_secs = 0;
        party.close_question();
        party.request_next(at(start, 20.0));
        assert_eq!(party.phase, Phase::QuizLeaderboard);
    }

    #[test]
    fn next_with_auto_advance_moves_on_at_the_sooner_time() {
        let mut party = party();
        party.auto_advance.enabled = true;
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        party.close_question();
        party.note_shown(start);
        // 5 seconds' reveal: Next at 4 seconds would count down to 7, so it still moves on at 5.
        party.request_next(at(start, 4.0));
        assert_eq!(party.advance_at(), Some(at(start, 5.0)));
    }

    #[test]
    fn hosts_see_the_answer_and_who_has_answered() {
        let mut party = party();
        let start = Instant::now();
        party.start_quiz(0, start).unwrap();
        party.answer(2, FIRST, 1, start);
        let state = party.host_state(at(start, 1.0));
        assert_eq!(state.game_code, "ABCD");
        assert_eq!(state.next.as_deref(), Some("End Question"));
        let HostPhase::Question {
            correct,
            answered,
            view,
        } = state.phase
        else {
            panic!("not a question: {:?}", state.phase)
        };
        assert_eq!((correct, answered, view.remaining_ms), (0, vec![2], 9_000));
        party.end_party();
        assert_eq!(party.host_state(start).phase, HostPhase::Ended);
        assert_eq!(party.next_label(), None);
    }

    /// Plays a one-question quiz in which each named player answers correctly at once or wrongly, by number.
    fn play(party: &mut Party, quiz: usize, right: &[PlayerId], wrong: &[PlayerId]) {
        let start = Instant::now();
        party.start_quiz(quiz, start).unwrap();
        let question = QuestionRef {
            quiz: quiz as u32,
            index: 0,
        };
        for player in right {
            assert!(party.answer(*player, question, 0, start));
        }
        for player in wrong {
            assert!(party.answer(*player, question, 1, start));
        }
        party.close_question();
        party.next(start);
        assert_eq!(party.phase, Phase::QuizLeaderboard);
        party.next(start);
        party.next(start);
        assert_eq!(party.phase, Phase::Lobby);
    }

    #[test]
    fn leaderboards_rank_each_quiz_and_the_party() {
        let one = Quiz::parse("+++\ntitle = \"One\"\n+++\n## Q?\n- [x] A\n- [ ] B\n").unwrap();
        let two = Quiz::parse("+++\ntitle = \"Two\"\n+++\n## Q?\n- [x] A\n- [ ] B\n").unwrap();
        let quizzes = [one, two].map(|quiz| PartyQuiz {
            folder: PathBuf::from("."),
            quiz,
        });
        let mut party = Party::with_code("ABCD".into(), quizzes.into());
        for name in ["Sam", "Jo"] {
            party.join("ABCD", name, None).unwrap();
        }
        // Quiz One: Sam right, Jo wrong.
        play(&mut party, 0, &[1], &[2]);
        // Al joins after it, and in quiz Two he and Jo are right, Sam wrong.
        party.join("ABCD", "Al", None).unwrap();
        play(&mut party, 1, &[2, 3], &[1]);

        let board = party.leaderboard(LeaderboardKind::Quiz { quiz: 1 });
        assert_eq!(board.quizzes, ["One", "Two"]);
        let order: Vec<_> = board.rows.iter().map(|row| row.name.as_str()).collect();
        // Jo and Al tie in quiz Two, in the order they joined.
        assert_eq!(order, ["Jo", "Al", "Sam"]);
        assert_eq!(board.rows[0].quizzes[1], Some(QuizResult { score: 1000, rank: 1 }));
        assert_eq!(board.rows[1].quizzes[1], Some(QuizResult { score: 1000, rank: 1 }));
        // Al missed quiz One.
        assert_eq!(board.rows[1].quizzes[0], None);

        let board = party.leaderboard(LeaderboardKind::Party);
        let rows: Vec<_> = board
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.total, row.rank))
            .collect();
        assert_eq!(rows, [("Sam", 1000, 1), ("Jo", 1000, 1), ("Al", 1000, 1)]);

        // Quiz One's leaderboard lists only those who played it.
        let board = party.leaderboard(LeaderboardKind::Quiz { quiz: 0 });
        let order: Vec<_> = board.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(order, ["Sam", "Jo"]);
        assert_eq!(board.rows[1].quizzes[0], Some(QuizResult { score: 0, rank: 2 }));
    }
}
