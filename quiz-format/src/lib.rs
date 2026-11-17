//! Reads and writes Bruce's Quiz Party quiz files, and scores answers by a quiz's settings.
//!
//! A quiz is a folder holding a hand-editable Markdown file, `quiz.md`, and the images it shows. The format is
//! documented in `docs/QuizFormat.md`; [`Quiz::parse`] reads it and [`Quiz::to_markdown`] writes it back, so that the
//! editor and the generator write files a person can still edit by hand.

mod check;
pub mod library;
mod parse;
mod scoring;
mod write;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use check::{folder_name, new_folder};
pub use parse::ParseError;
pub use scoring::{Curve, ScoringSettings};

/// The name of the Markdown file in a quiz's folder.
pub const QUIZ_FILE: &str = "quiz.md";

/// The fewest answers a question may have.
pub const MIN_ANSWERS: usize = 2;

/// The most answers a question may have, one for each colour and shape the screens show.
pub const MAX_ANSWERS: usize = 6;

/// A quiz: its settings, its credits, and its questions in the order they are asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Quiz {
    /// The quiz's title, shown on the big screen and in the lists of quizzes.
    pub title: String,
    /// A sentence or two about the quiz, for the lists of quizzes.
    pub description: Option<String>,
    /// The licence the quiz's content is published under, e.g. `CC BY-SA 4.0` for one drawn from Wikipedia.
    pub licence: Option<String>,
    /// How long each question runs and how it is scored, unless a question overrides them.
    pub settings: Settings,
    /// The articles the questions were drawn from, credited at the end of the quiz.
    pub sources: Vec<Source>,
    /// The authors and licences of the quiz's images, credited at the end of the quiz.
    pub images: Vec<ImageCredit>,
    /// The questions, in the order they are asked.
    pub questions: Vec<Question>,
}

/// How long a question runs and how its answers are scored.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The seconds players have to answer.
    pub time_limit: u32,
    /// The points for a correct answer given at once.
    pub max_points: u32,
    /// How the points for a correct answer fall as time passes.
    pub scoring: ScoringSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            time_limit: 20,
            max_points: 1000,
            scoring: ScoringSettings::default(),
        }
    }
}

/// The settings a question may change from its quiz's: any left out are the quiz's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsOverride {
    /// The seconds players have to answer this question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_limit: Option<u32>,
    /// The points for a correct answer to this question given at once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_points: Option<u32>,
    /// How the points for a correct answer to this question fall as time passes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoring: Option<Curve>,
    /// The fraction of the points a correct answer still scores at the time limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor: Option<f64>,
    /// How steeply a logarithmic curve falls at first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steepness: Option<f64>,
}

impl SettingsOverride {
    /// Whether this overrides nothing, so that a question needs no settings block.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The settings of a question, its quiz's with this question's changes.
    pub fn apply(&self, quiz: &Settings) -> Settings {
        Settings {
            time_limit: self.time_limit.unwrap_or(quiz.time_limit),
            max_points: self.max_points.unwrap_or(quiz.max_points),
            scoring: ScoringSettings {
                curve: self.scoring.unwrap_or(quiz.scoring.curve),
                floor: self.floor.unwrap_or(quiz.scoring.floor),
                steepness: self.steepness.unwrap_or(quiz.scoring.steepness),
            },
        }
    }
}

/// An article a quiz's questions were drawn from, e.g. a Wikipedia page, for its credits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The article's title.
    pub title: String,
    /// The article's URL.
    pub url: String,
    /// The date of the revision the questions were drawn from, e.g. `2026-10-01`, as Wikipedia's text changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// The author, licence and origin of one of a quiz's images, for its credits. Only freely licensed images are used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCredit {
    /// The image's path in the quiz's folder, as the questions name it.
    pub file: String,
    /// Who made the image.
    pub author: String,
    /// The image's licence, e.g. `CC BY-SA 4.0`.
    pub licence: String,
    /// Where the image came from, e.g. its Wikimedia Commons page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One multiple-choice question.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// The question's text, or for an image question its short prompt, e.g. "Where is this?".
    pub text: String,
    /// The image shown with the question, if it is an image question.
    pub image: Option<Image>,
    /// The answers, from 2 to 6, in the order they are shown.
    pub answers: Vec<String>,
    /// The index in `answers` of the correct one.
    pub correct: usize,
    /// Where the answer comes from, e.g. a Wikipedia link to the section, which the reveal shows as a QR code.
    pub source: Option<String>,
    /// This question's changes to its quiz's settings.
    pub settings: SettingsOverride,
}

/// An image shown with a question.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// The image's path, relative to the quiz's folder.
    pub file: String,
    /// A description of the image, for those who cannot see it.
    pub alt: String,
}

impl Quiz {
    /// Reads the quiz in a folder, from its `quiz.md`.
    pub fn load(folder: &Path) -> Result<Quiz, LoadError> {
        let path = folder.join(QUIZ_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| LoadError::Read {
            path: path.clone(),
            source,
        })?;
        Quiz::parse(&text).map_err(|source| LoadError::Parse { path, source })
    }

    /// Writes the quiz to a folder's `quiz.md`, creating the folder if need be. Its images are not copied.
    pub fn save(&self, folder: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(folder)?;
        std::fs::write(folder.join(QUIZ_FILE), self.to_markdown())
    }

    /// The settings of one of the questions, the quiz's with the question's changes.
    pub fn question_settings(&self, index: usize) -> Settings {
        self.questions[index].settings.apply(&self.settings)
    }

    /// The credit for one of the quiz's images, if it has one.
    pub fn image_credit(&self, file: &str) -> Option<&ImageCredit> {
        self.images.iter().find(|credit| credit.file == file)
    }
}

/// A quiz which could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The file could not be read.
    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The file was read but is not a valid quiz.
    #[error("{path} is not a valid quiz")]
    Parse {
        path: PathBuf,
        #[source]
        source: ParseError,
    },
}

/// The quizzes in the folders of a directory, each folder with a `quiz.md`, sorted by folder name, with those which
/// could not be loaded given as errors rather than stopping the others loading.
pub fn load_library(directory: &Path) -> std::io::Result<Vec<(PathBuf, Result<Quiz, LoadError>)>> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.join(QUIZ_FILE).is_file() {
            folders.push(path);
        }
    }
    folders.sort();
    Ok(folders
        .into_iter()
        .map(|folder| {
            let quiz = Quiz::load(&folder);
            (folder, quiz)
        })
        .collect())
}
