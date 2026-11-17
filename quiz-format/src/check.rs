//! Checking a quiz made or changed in the editor before it is saved: what would stop it being read back, in words for
//! the person editing it, and tidying what they typed into what the file can hold.

use crate::{MAX_ANSWERS, MIN_ANSWERS, Quiz, parse::check_settings};

impl Quiz {
    /// What is wrong with the quiz, in words, e.g. "Question 3 has only one answer"; none if it can be saved.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.title.trim().is_empty() {
            problems.push("The quiz has no title.".to_string());
        }
        if let Err(message) = check_settings(&self.settings) {
            problems.push(format!("The quiz's settings: {message}."));
        }
        if self.questions.is_empty() {
            problems.push("The quiz has no questions.".to_string());
        }
        for (index, question) in self.questions.iter().enumerate() {
            let name = format!("Question {}", index + 1);
            if question.text.trim().is_empty() {
                problems.push(format!("{name} has no text."));
            }
            let count = question.answers.len();
            if !(MIN_ANSWERS..=MAX_ANSWERS).contains(&count) {
                problems.push(format!(
                    "{name} has {count} answers: it needs from {MIN_ANSWERS} to {MAX_ANSWERS}."
                ));
            }
            for (position, answer) in question.answers.iter().enumerate() {
                if answer.trim().is_empty() {
                    problems.push(format!("{name}'s answer {} is empty.", position + 1));
                }
            }
            if question.correct >= count {
                problems.push(format!("{name} has no correct answer chosen."));
            }
            if let Some(image) = &question.image
                && image.file.trim().is_empty()
            {
                problems.push(format!("{name}'s image has no file."));
            }
            if let Err(message) = check_settings(&self.question_settings(index)) {
                problems.push(format!("{name}'s settings: {message}."));
            }
        }
        problems
    }

    /// The quiz as the file can hold it: text on one line where the file has it on one line, without the spaces around
    /// it, and optional text left out when it is empty.
    pub fn tidied(&self) -> Quiz {
        let mut quiz = self.clone();
        quiz.title = line(&quiz.title);
        quiz.description = optional(quiz.description.as_deref().map(str::trim));
        quiz.licence = optional(quiz.licence.as_deref().map(line).as_deref());
        for source in &mut quiz.sources {
            source.title = line(&source.title);
            source.url = line(&source.url);
            source.revision = optional(source.revision.as_deref().map(line).as_deref());
        }
        quiz.sources
            .retain(|source| !source.title.is_empty() || !source.url.is_empty());
        for credit in &mut quiz.images {
            credit.file = line(&credit.file);
            credit.author = line(&credit.author);
            credit.licence = line(&credit.licence);
            credit.source = optional(credit.source.as_deref().map(line).as_deref());
        }
        quiz.images.retain(|credit| !credit.file.is_empty());
        for question in &mut quiz.questions {
            question.text = line(&question.text);
            question.answers = question.answers.iter().map(|answer| line(answer)).collect();
            question.source = optional(question.source.as_deref().map(line).as_deref());
            if let Some(image) = &mut question.image {
                image.file = line(&image.file);
                image.alt = line(&image.alt);
            }
        }
        quiz
    }
}

/// Text on one line, its runs of spaces and line breaks made single spaces.
fn line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// None for empty text.
fn optional(text: Option<&str>) -> Option<String> {
    text.filter(|text| !text.is_empty()).map(str::to_string)
}

/// A folder name for a new quiz, from its title: lower case letters and digits, with hyphens between words, e.g.
/// "around-the-world" for "Around the World!".
pub fn folder_name(title: &str) -> String {
    let name = title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if name.is_empty() { "quiz".into() } else { name }
}

/// A folder for a new quiz in a folder of quizzes, named after its title, which is not already taken, e.g.
/// `my-quiz`, or `my-quiz-2` if that is.
pub fn new_folder(quizzes: &std::path::Path, title: &str) -> std::path::PathBuf {
    let name = folder_name(title);
    (1..)
        .map(|n| quizzes.join(if n == 1 { name.clone() } else { format!("{name}-{n}") }))
        .find(|folder| !folder.exists())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Image, Question, Settings};

    fn question(text: &str, answers: &[&str]) -> Question {
        Question {
            text: text.into(),
            image: None,
            answers: answers.iter().map(|answer| answer.to_string()).collect(),
            correct: 0,
            source: None,
            settings: Default::default(),
        }
    }

    fn quiz(questions: Vec<Question>) -> Quiz {
        Quiz {
            title: "T".into(),
            description: None,
            licence: None,
            settings: Settings::default(),
            sources: Vec::new(),
            images: Vec::new(),
            questions,
        }
    }

    #[test]
    fn a_good_quiz_has_no_problems_and_reads_back_the_same() {
        let quiz = quiz(vec![question("Q?", &["A", "B"])]);
        assert!(quiz.problems().is_empty());
        assert_eq!(Quiz::parse(&quiz.to_markdown()).unwrap(), quiz);
    }

    #[test]
    fn says_what_is_wrong() {
        let mut quiz = quiz(vec![question(" ", &["A"]), question("Q?", &["A", ""])]);
        quiz.title = String::new();
        quiz.questions[1].settings.time_limit = Some(0);
        quiz.questions[1].correct = 5;
        assert_eq!(
            quiz.problems(),
            [
                "The quiz has no title.",
                "Question 1 has no text.",
                "Question 1 has 1 answers: it needs from 2 to 6.",
                "Question 2's answer 2 is empty.",
                "Question 2 has no correct answer chosen.",
                "Question 2's settings: the time limit must be at least 1 second.",
            ]
        );
        assert_eq!(
            Quiz {
                questions: Vec::new(),
                ..quiz
            }
            .problems()[1],
            "The quiz has no questions."
        );
    }

    #[test]
    fn tidies_what_was_typed() {
        let mut quiz = quiz(vec![question("  What is\nthis? ", &[" A ", "B\n"])]);
        quiz.description = Some("  ".into());
        quiz.licence = Some(" CC BY-SA 4.0 ".into());
        quiz.questions[0].source = Some("".into());
        quiz.questions[0].image = Some(Image {
            file: " images/a.png".into(),
            alt: "An\nimage".into(),
        });
        let tidied = quiz.tidied();
        assert_eq!(tidied.questions[0].text, "What is this?");
        assert_eq!(tidied.questions[0].answers, ["A", "B"]);
        assert_eq!(tidied.description, None);
        assert_eq!(tidied.licence.as_deref(), Some("CC BY-SA 4.0"));
        assert_eq!(tidied.questions[0].source, None);
        assert_eq!(tidied.questions[0].image.as_ref().unwrap().alt, "An image");
        assert_eq!(Quiz::parse(&tidied.to_markdown()).unwrap(), tidied);
    }

    #[test]
    fn folder_names_from_titles() {
        assert_eq!(folder_name("Around the World!"), "around-the-world");
        assert_eq!(folder_name("Ça va? 2026"), "a-va-2026");
        assert_eq!(folder_name("???"), "quiz");
    }
}
