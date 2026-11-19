//! Writing a draft quiz: the prompt which gives Claude the articles and images, the JSON Schema of its answer, and the
//! quiz built from the answer, with every article credited and each image's author and licence, under the licence of
//! Wikipedia's text.

use std::path::{Path, PathBuf};

use anyhow::Context;
use quiz_format::{Image, ImageCredit, MAX_ANSWERS, MIN_ANSWERS, Question, Quiz, Settings, SettingsOverride, Source};
use rand::seq::SliceRandom;
use serde::Deserialize;
use serde_json::json;

use crate::wikipedia::{Article, Picture};

/// The licence of quizzes drawn from Wikipedia, whose text is under it.
pub const LICENCE: &str = "CC BY-SA 4.0";

/// What the quiz is to be.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Request {
    /// The subject, short, e.g. "Flags of Europe": searched for on Wikipedia if no articles are given, and the quiz is
    /// about it. It may be empty if articles are given.
    pub topic: String,
    /// The Wikipedia articles to write from, by title, chosen by the user; if none, those found for the topic.
    pub articles: Vec<String>,
    /// Anything else for Claude, in the user's own words, e.g. "Every question is a picture question".
    pub instructions: String,
    pub questions: usize,
    pub difficulty: Difficulty,
}

impl Request {
    /// The subject, as it is to be written about: a category's name without `Category:`.
    pub fn topic_name(&self) -> &str {
        let topic = self.topic.trim();
        match topic.split_once(':') {
            Some((prefix, name)) if prefix.trim().eq_ignore_ascii_case("category") => name.trim(),
            _ => topic,
        }
    }
}

/// How hard the questions are to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Difficulty {
    Easy,
    #[default]
    Medium,
    Hard,
}

impl Difficulty {
    /// Every difficulty, easiest first, for choosing one.
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

    /// Reads a difficulty's name, e.g. from the command line, medium if it is none of them.
    pub fn parse(text: &str) -> Difficulty {
        match text.trim().to_ascii_lowercase().as_str() {
            "easy" => Difficulty::Easy,
            "hard" => Difficulty::Hard,
            _ => Difficulty::Medium,
        }
    }

    /// Its name, as [`Difficulty::parse`] reads it.
    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
        }
    }

    /// What it means, for the prompts.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy: questions most people at a party would know or could guess",
            Difficulty::Medium => "medium: a mix, some which most would know and some which only a keen few would",
            Difficulty::Hard => "hard: for enthusiasts, with plausible wrong answers",
        }
    }
}

/// The prompt: the articles and images to write from, and how to write the questions.
pub fn prompt(request: &Request, articles: &[Article], pictures: &[Picture]) -> String {
    let about = match request.topic_name() {
        "" => "Write a multiple-choice quiz for a party game, from the articles below.".to_string(),
        topic => format!("Write a multiple-choice quiz for a party game, about: {topic}"),
    };
    let mut prompt = format!(
        "{about}\n\n\
         Write {count} questions, {difficulty}.\n\n\
         Rules:\n\
         - Use only facts stated in the articles below, and give for each question the number of the article it comes \
           from.\n\
         - Each question has 4 answers, exactly one of them correct; 2 or 3 for a true-or-false or either-or question. \
           Wrong answers must be plausible but clearly wrong according to the article.\n\
         - Keep questions under 120 characters and answers under 40, as they are shown on a TV and on phones, and \
           answered against the clock.\n\
         - Put the correct answer first; the game shuffles them.\n\
         - Do not ask two questions with the same answer, or whose wording gives away another's answer.\n\
         - Spread the questions across the articles, so that the quiz covers the subject broadly: ask more than two \
           from one article only if there are few articles.\n\
         - A question may show one of the images below, if it makes a good question, e.g. \"Which country's flag is \
           this?\". Then give its number and a short description of what it shows for those who cannot see it, which \
           does not give away the answer. Choose only an image whose name or description makes clear what it shows. \
           Unless the instructions below say otherwise, most questions have no image.\n\
         - The title is short and inviting; the description is one sentence.\n\n",
        count = request.questions,
        difficulty = request.difficulty.describe(),
    );
    if !request.instructions.trim().is_empty() {
        prompt.push_str(&format!(
            "The quiz maker's own instructions. Follow them where they differ from the rules above, except that every \
             fact must still come from the articles, and only the images listed may be shown. If they cannot all be \
             followed, e.g. there are too few images, follow them as far as the articles and images allow:\n\
             <instructions>\n{}\n</instructions>\n\n",
            request.instructions.trim()
        ));
    }
    for (index, article) in articles.iter().enumerate() {
        prompt.push_str(&format!(
            "<article number=\"{}\" title=\"{}\">\n{}\n</article>\n\n",
            index + 1,
            article.title,
            article.text
        ));
    }
    if !pictures.is_empty() {
        prompt.push_str("Images:\n");
        for (index, picture) in pictures.iter().enumerate() {
            prompt.push_str(&format!(
                "{}. {}: {}\n",
                index + 1,
                picture.name,
                match shortened(&picture.description, MAX_DESCRIPTION_CHARS) {
                    "" => "no description",
                    description => description,
                }
            ));
        }
    }
    prompt
}

/// The most of an image's description given to Claude, in characters, as some are long, and a quiz may have many.
const MAX_DESCRIPTION_CHARS: usize = 300;

/// Text cut to at most `count` characters.
fn shortened(text: &str, count: usize) -> &str {
    match text.char_indices().nth(count) {
        Some((cut, _)) => &text[..cut],
        None => text,
    }
}

/// The JSON Schema of Claude's answer.
pub fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "description": { "type": "string" },
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "answers": { "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 6 },
                        "article": { "type": "integer", "description": "The number of the article it comes from" },
                        "image": { "type": "integer", "description": "The number of the image it shows, if any" },
                        "image_description": { "type": "string" }
                    },
                    "required": ["text", "answers", "article"]
                }
            }
        },
        "required": ["title", "questions"]
    })
}

/// Claude's answer, as the schema says.
#[derive(Debug, Deserialize)]
struct Answer {
    title: String,
    #[serde(default)]
    description: String,
    questions: Vec<AnswerQuestion>,
}

#[derive(Debug, Deserialize)]
struct AnswerQuestion {
    text: String,
    answers: Vec<String>,
    article: Option<usize>,
    image: Option<usize>,
    #[serde(default)]
    image_description: String,
}

/// A quiz drafted from Claude's answer, and the images it uses: each picture to download and the file in the quiz's
/// folder to download it to.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub quiz: Quiz,
    pub downloads: Vec<(Picture, String)>,
}

/// Builds the quiz from Claude's answer: the answers shuffled, each question's source the article it came from, every
/// article credited, and each image used credited with its author and licence. Questions which could not be asked,
/// e.g. with one answer, are left out.
pub fn build(answer: serde_json::Value, articles: &[Article], pictures: &[Picture]) -> anyhow::Result<Draft> {
    let answer: Answer = serde_json::from_value(answer).context("Claude's answer is not a quiz as asked")?;
    let mut rng = rand::rng();
    let mut downloads: Vec<(Picture, String)> = Vec::new();
    let mut questions = Vec::new();
    for asked in answer.questions {
        let mut answers: Vec<String> = asked
            .answers
            .iter()
            .map(|answer| answer.trim().to_string())
            .filter(|answer| !answer.is_empty())
            .collect();
        answers.dedup();
        if asked.text.trim().is_empty() || !(MIN_ANSWERS..=MAX_ANSWERS).contains(&answers.len()) {
            continue;
        }
        // The correct answer is first: shuffle, and follow it.
        let correct_text = answers[0].clone();
        answers.shuffle(&mut rng);
        let correct = answers.iter().position(|answer| *answer == correct_text).unwrap_or(0);
        let source = asked
            .article
            .and_then(|number| articles.get(number.checked_sub(1)?))
            .map(|article| article.url.clone());
        let image = asked
            .image
            .and_then(|number| pictures.get(number.checked_sub(1)?))
            .map(|picture| {
                let file = match downloads.iter().find(|(used, _)| used == picture) {
                    Some((_, file)) => file.clone(),
                    None => {
                        let file = image_file(picture);
                        downloads.push((picture.clone(), file.clone()));
                        file
                    }
                };
                Image {
                    file,
                    alt: asked.image_description.trim().to_string(),
                }
            });
        questions.push(Question {
            text: asked.text.trim().to_string(),
            image,
            answers,
            correct,
            source,
            settings: SettingsOverride::default(),
        });
    }
    anyhow::ensure!(
        !questions.is_empty(),
        "Claude's answer has no questions which could be asked"
    );
    let quiz = Quiz {
        title: answer.title.trim().to_string(),
        description: Some(answer.description.trim().to_string()).filter(|text| !text.is_empty()),
        licence: Some(LICENCE.to_string()),
        settings: Settings::default(),
        // Only the articles the questions come from, as only they are adapted, in the order they were read.
        sources: articles
            .iter()
            .filter(|article| {
                questions
                    .iter()
                    .any(|question| question.source.as_deref() == Some(article.url.as_str()))
            })
            .map(|article| Source {
                title: article.title.clone(),
                url: article.url.clone(),
                revision: Some(article.revision.clone()).filter(|revision| !revision.is_empty()),
            })
            .collect(),
        images: downloads
            .iter()
            .map(|(picture, file)| ImageCredit {
                file: file.clone(),
                author: picture.author.clone(),
                licence: picture.licence.clone(),
                source: Some(picture.page.clone()),
            })
            .collect(),
        questions,
    }
    .tidied();
    Ok(Draft { quiz, downloads })
}

/// The file in a quiz's folder for a picture, e.g. `images/flag-of-japan.png`, the type of its download.
fn image_file(picture: &Picture) -> String {
    let stem = picture
        .name
        .rsplit_once('.')
        .map_or(picture.name.as_str(), |(stem, _)| stem);
    // The address may end with tracking parameters, e.g. `?utm_source=…`, after the file's name.
    let address = picture.download.split(['?', '#']).next().unwrap_or_default();
    let extension = address
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .filter(|extension| matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp"))
        .unwrap_or_else(|| "png".into());
    format!("images/{}.{extension}", quiz_format::folder_name(stem))
}

/// Saves a draft in a new folder in the quizzes folder, with its images, returning the folder.
pub fn save(
    draft: &Draft,
    quizzes: &Path,
    download: impl Fn(&Picture, &Path) -> anyhow::Result<()>,
) -> anyhow::Result<PathBuf> {
    let problems = draft.quiz.problems();
    anyhow::ensure!(problems.is_empty(), "the draft cannot be saved: {}", problems.join(" "));
    let folder = quiz_format::new_folder(quizzes, &draft.quiz.title);
    for (picture, file) in &draft.downloads {
        download(picture, &folder.join(file))?;
    }
    draft
        .quiz
        .save(&folder)
        .with_context(|| format!("cannot save {}", folder.display()))?;
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn articles() -> Vec<Article> {
        vec![Article {
            title: "Flag of Japan".into(),
            url: "https://en.wikipedia.org/wiki/Flag_of_Japan".into(),
            revision: "2026-11-02".into(),
            text: "The flag of Japan is white with a red disc.".into(),
            image: Some("Flag of Japan.svg".into()),
            images: Vec::new(),
        }]
    }

    fn pictures() -> Vec<Picture> {
        vec![Picture {
            name: "Flag of Japan.svg".into(),
            description: "The flag of Japan".into(),
            download: "https://upload.wikimedia.org/thumb/800px-Flag_of_Japan.svg.png".into(),
            page: "https://commons.wikimedia.org/wiki/File:Flag_of_Japan.svg".into(),
            author: "Various".into(),
            licence: "Public domain".into(),
        }]
    }

    #[test]
    fn the_prompt_has_the_articles_and_images() {
        let request = Request {
            topic: "Flags".into(),
            questions: 5,
            difficulty: Difficulty::Easy,
            ..Request::default()
        };
        let prompt = prompt(&request, &articles(), &pictures());
        assert!(prompt.contains("about: Flags"));
        assert!(!prompt.contains("<instructions>"));
        assert!(prompt.contains("Write 5 questions, easy"));
        assert!(prompt.contains("<article number=\"1\" title=\"Flag of Japan\">"));
        assert!(prompt.contains("1. Flag of Japan.svg: The flag of Japan"));
    }

    #[test]
    fn the_prompt_has_the_quiz_makers_instructions() {
        let request = Request {
            articles: vec!["Flag of Japan".into()],
            instructions: " Every question is a picture question.\n".into(),
            questions: 5,
            ..Request::default()
        };
        let prompt = prompt(&request, &articles(), &pictures());
        assert!(prompt.starts_with("Write a multiple-choice quiz for a party game, from the articles below."));
        assert!(prompt.contains("<instructions>\nEvery question is a picture question.\n</instructions>"));
    }

    #[test]
    fn builds_a_quiz_from_the_answer() {
        let answer = json!({
            "title": " Flags ",
            "description": "Know your flags.",
            "questions": [
                {"text": "Which country's flag is this?", "answers": ["Japan", "Bangladesh", "Palau", "Laos"],
                 "article": 1, "image": 1, "image_description": "A red disc on white"},
                {"text": "What colour is the disc?", "answers": ["Red", "Blue"], "article": 1},
                {"text": "One answer only?", "answers": ["Yes"], "article": 1},
                {"text": "Seven answers?", "answers": ["1", "2", "3", "4", "5", "6", "7"], "article": 1}
            ]
        });
        let draft = build(answer, &articles(), &pictures()).unwrap();
        let quiz = &draft.quiz;
        assert_eq!(quiz.title, "Flags");
        assert_eq!(quiz.licence.as_deref(), Some(LICENCE));
        assert_eq!(
            quiz.questions.len(),
            2,
            "the questions which cannot be asked are left out"
        );
        let first = &quiz.questions[0];
        assert_eq!(
            first.answers[first.correct], "Japan",
            "the correct answer is followed through the shuffle"
        );
        assert_eq!(
            first.source.as_deref(),
            Some("https://en.wikipedia.org/wiki/Flag_of_Japan")
        );
        let image = first.image.as_ref().unwrap();
        assert_eq!(image.file, "images/flag-of-japan.png");
        assert_eq!(image.alt, "A red disc on white");
        assert_eq!(quiz.questions[1].answers[quiz.questions[1].correct], "Red");
        assert_eq!(quiz.sources[0].revision.as_deref(), Some("2026-11-02"));
        assert_eq!(quiz.images[0].licence, "Public domain");
        assert_eq!(
            quiz.images[0].source.as_deref(),
            Some("https://commons.wikimedia.org/wiki/File:Flag_of_Japan.svg")
        );
        assert_eq!(draft.downloads.len(), 1);
        assert!(quiz.problems().is_empty(), "{:?}", quiz.problems());
        assert_eq!(Quiz::parse(&quiz.to_markdown()).unwrap(), *quiz);
    }

    #[test]
    fn credits_only_the_articles_used() {
        let mut read = articles();
        read.push(Article {
            title: "Moon Moon Sen".into(),
            url: "https://en.wikipedia.org/wiki/Moon_Moon_Sen".into(),
            ..read[0].clone()
        });
        let answer = json!({"title": "T", "questions": [{"text": "Q?", "answers": ["A", "B"], "article": 1}]});
        let quiz = build(answer, &read, &[]).unwrap().quiz;
        let credited: Vec<&str> = quiz.sources.iter().map(|source| source.title.as_str()).collect();
        assert_eq!(credited, ["Flag of Japan"]);
    }

    #[test]
    fn names_images_by_the_type_downloaded() {
        let picture = Picture {
            name: "Marshal Foch banner.jpg".into(),
            download: "https://thumb.wikimedia.org/thumb/960px-Marshal_Foch_banner.jpg?utm_source=en.wikipedia.org"
                .into(),
            ..pictures()[0].clone()
        };
        assert_eq!(image_file(&picture), "images/marshal-foch-banner.jpg");
        assert_eq!(
            image_file(&pictures()[0]),
            "images/flag-of-japan.png",
            "an SVG is downloaded as PNG"
        );
    }

    #[test]
    fn shuffles_the_correct_answer_around() {
        let answer =
            || json!({"title": "T", "questions": [{"text": "Q?", "answers": ["A", "B", "C", "D"], "article": 1}]});
        let places: std::collections::HashSet<usize> = (0..50)
            .map(|_| build(answer(), &articles(), &[]).unwrap().quiz.questions[0].correct)
            .collect();
        assert!(places.len() > 1, "always at {places:?}");
    }

    #[test]
    fn saves_with_its_images() {
        let quizzes = std::env::temp_dir().join(format!("quiz-generator-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&quizzes);
        let answer = json!({"title": "Flags", "questions": [
            {"text": "Whose flag?", "answers": ["Japan", "Laos"], "article": 1, "image": 1, "image_description": "A flag"}
        ]});
        let draft = build(answer, &articles(), &pictures()).unwrap();
        let folder = save(&draft, &quizzes, |_, path| {
            std::fs::create_dir_all(path.parent().unwrap())?;
            Ok(std::fs::write(path, "png")?)
        })
        .unwrap();
        assert_eq!(folder, quizzes.join("flags"));
        assert!(folder.join("images/flag-of-japan.png").is_file());
        assert_eq!(Quiz::load(&folder).unwrap(), draft.quiz);
        let _ = std::fs::remove_dir_all(&quizzes);
    }
}
