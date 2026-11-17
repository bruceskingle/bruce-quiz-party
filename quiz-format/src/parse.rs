//! Reads a quiz file, reporting the first problem with the line it is on, as the files are edited by hand.

use serde::{Deserialize, Serialize};

use crate::{
    Curve, Image, ImageCredit, MAX_ANSWERS, MIN_ANSWERS, Question, Quiz, ScoringSettings, Settings, SettingsOverride,
    Source,
};

/// The line which opens and closes the front matter.
pub(crate) const FRONT_MATTER_FENCE: &str = "+++";

/// The fence which opens a question's settings block.
pub(crate) const SETTINGS_FENCE: &str = "```toml";

/// The prefix of a question's source line.
pub(crate) const SOURCE_PREFIX: &str = "Source:";

/// A quiz file which is not valid, with the line of the problem, counting from 1.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ParseError {
    /// The line of the problem, counting from 1.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

fn error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

/// The quiz's front matter, as written in the file: every setting is optional, defaulting to [`Settings::default`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrontMatter {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_points: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoring: Option<Curve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steepness: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageCredit>,
}

impl FrontMatter {
    fn settings(&self) -> Settings {
        let default = Settings::default();
        Settings {
            time_limit: self.time_limit.unwrap_or(default.time_limit),
            max_points: self.max_points.unwrap_or(default.max_points),
            scoring: ScoringSettings {
                curve: self.scoring.unwrap_or(default.scoring.curve),
                floor: self.floor.unwrap_or(default.scoring.floor),
                steepness: self.steepness.unwrap_or(default.scoring.steepness),
            },
        }
    }
}

/// The line number, counting from 1, of a byte offset into a block of text which starts at line `first`.
fn line_of(text: &str, offset: usize, first: usize) -> usize {
    first + text[..offset.min(text.len())].matches('\n').count()
}

impl Quiz {
    /// Reads a quiz from the text of its `quiz.md`, see `docs/QuizFormat.md`.
    pub fn parse(text: &str) -> Result<Quiz, ParseError> {
        let lines: Vec<&str> = text.lines().collect();
        let mut next = 0;
        while next < lines.len() && lines[next].trim().is_empty() {
            next += 1;
        }
        if lines.get(next).map(|line| line.trim()) != Some(FRONT_MATTER_FENCE) {
            return Err(error(
                next + 1,
                "a quiz file starts with its settings between +++ lines",
            ));
        }
        let open = next;
        let close = (open + 1..lines.len())
            .find(|&i| lines[i].trim() == FRONT_MATTER_FENCE)
            .ok_or_else(|| error(open + 1, "the settings' +++ line is never closed"))?;
        let front_text = lines[open + 1..close].join("\n");
        let front: FrontMatter = toml::from_str(&front_text).map_err(|e| {
            let line = e
                .span()
                .map(|span| line_of(&front_text, span.start, open + 2))
                .unwrap_or(open + 1);
            error(line, e.message().trim().to_string())
        })?;
        if front.title.trim().is_empty() {
            return Err(error(open + 1, "the quiz has no title"));
        }
        let settings = front.settings();
        check_settings(&settings).map_err(|message| error(open + 1, message))?;

        let questions = parse_questions(&lines, close + 1, &settings)?;
        if questions.is_empty() {
            return Err(error(
                lines.len(),
                "the quiz has no questions; each starts with a ## heading",
            ));
        }

        Ok(Quiz {
            title: front.title,
            description: front.description,
            licence: front.licence,
            settings,
            sources: front.sources,
            images: front.images,
            questions,
        })
    }
}

/// Checks that settings are ones a question can be run with.
pub(crate) fn check_settings(settings: &Settings) -> Result<(), String> {
    if settings.time_limit == 0 {
        return Err("the time limit must be at least 1 second".into());
    }
    if !(0.0..=1.0).contains(&settings.scoring.floor) {
        return Err(format!(
            "the floor is {}, it must be from 0 to 1",
            settings.scoring.floor
        ));
    }
    if settings.scoring.steepness <= 0.0 {
        return Err(format!(
            "the steepness is {}, it must be more than 0",
            settings.scoring.steepness
        ));
    }
    Ok(())
}

/// A question being read, with the line it started on, for errors about it as a whole.
struct Draft {
    line: usize,
    text: String,
    image: Option<Image>,
    answers: Vec<(String, bool)>,
    source: Option<String>,
    settings: SettingsOverride,
}

impl Draft {
    fn finish(self, quiz: &Settings) -> Result<Question, ParseError> {
        let count = self.answers.len();
        if !(MIN_ANSWERS..=MAX_ANSWERS).contains(&count) {
            return Err(error(
                self.line,
                format!("the question has {count} answers, it needs from {MIN_ANSWERS} to {MAX_ANSWERS}"),
            ));
        }
        let correct: Vec<usize> = (0..count).filter(|&i| self.answers[i].1).collect();
        if correct.len() != 1 {
            return Err(error(
                self.line,
                format!(
                    "the question has {} answers marked [x], it needs exactly one",
                    correct.len()
                ),
            ));
        }
        check_settings(&self.settings.apply(quiz)).map_err(|message| error(self.line, message))?;
        Ok(Question {
            text: self.text,
            image: self.image,
            answers: self.answers.into_iter().map(|(text, _)| text).collect(),
            correct: correct[0],
            source: self.source,
            settings: self.settings,
        })
    }
}

/// Reads the questions, from line index `start` to the end.
fn parse_questions(lines: &[&str], start: usize, quiz: &Settings) -> Result<Vec<Question>, ParseError> {
    let mut questions = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut i = start;
    while i < lines.len() {
        let number = i + 1;
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() {
            continue;
        }
        if let Some(text) = line.strip_prefix("## ") {
            if let Some(done) = draft.take() {
                questions.push(done.finish(quiz)?);
            }
            let text = text.trim();
            if text.is_empty() {
                return Err(error(number, "the question has no text"));
            }
            draft = Some(Draft {
                line: number,
                text: text.to_string(),
                image: None,
                answers: Vec::new(),
                source: None,
                settings: SettingsOverride::default(),
            });
            continue;
        }
        let Some(question) = draft.as_mut() else {
            return Err(error(number, "expected a question, starting with a ## heading"));
        };
        if let Some((alt, file)) = parse_image(line) {
            if question.image.is_some() {
                return Err(error(number, "a question can have only one image"));
            }
            if !question.answers.is_empty() {
                return Err(error(number, "the image goes before the answers"));
            }
            question.image = Some(Image { file, alt });
        } else if let Some((text, correct)) = parse_answer(line) {
            if text.is_empty() {
                return Err(error(number, "the answer has no text"));
            }
            question.answers.push((text, correct));
        } else if let Some(url) = line.strip_prefix(SOURCE_PREFIX) {
            question.source = Some(url.trim().to_string());
        } else if line == SETTINGS_FENCE {
            let close = (i..lines.len())
                .find(|&j| lines[j].trim() == "```")
                .ok_or_else(|| error(number, "the settings block is never closed with ```"))?;
            let block = lines[i..close].join("\n");
            question.settings = toml::from_str(&block).map_err(|e| {
                let line = e
                    .span()
                    .map(|span| line_of(&block, span.start, i + 1))
                    .unwrap_or(number);
                error(line, e.message().trim().to_string())
            })?;
            i = close + 1;
        } else {
            return Err(error(
                number,
                format!("expected an answer (- [ ] or - [x]), an image, a Source: line or a settings block: {line}"),
            ));
        }
    }
    if let Some(done) = draft.take() {
        questions.push(done.finish(quiz)?);
    }
    Ok(questions)
}

/// An image line, `![alt](file)`, as its alt text and file.
fn parse_image(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let file = rest.strip_suffix(')')?;
    Some((alt.trim().to_string(), file.trim().to_string()))
}

/// An answer line, `- [ ] text` or `- [x] text`, as its text and whether it is the correct one.
fn parse_answer(line: &str) -> Option<(String, bool)> {
    let rest = line.strip_prefix("- [").or_else(|| line.strip_prefix("* ["))?;
    let (mark, text) = rest.split_once(']')?;
    let correct = match mark {
        " " | "" => false,
        "x" | "X" => true,
        _ => return None,
    };
    Some((text.trim().to_string(), correct))
}
