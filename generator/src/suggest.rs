//! Asking Claude which Wikipedia articles to write a quiz from: the best-known examples of a subject, for the
//! difficulty and the quiz maker's instructions, which Wikipedia's search, matching words, does not find; or the best
//! of a category's articles. Claude only names them: they are checked and read from Wikipedia by the generator, so
//! that the credits are exact.

use serde::Deserialize;
use serde_json::json;

use crate::draft::Request;

/// The prompt: the subject, or the articles to choose from, and what makes a good article for the quiz.
pub fn prompt(request: &Request, count: usize, candidates: &[String]) -> String {
    let mut prompt = format!(
        "Choose English Wikipedia articles to write a multiple-choice quiz for a party game from{about}.\n\n\
         The quiz will have {questions} questions, {difficulty}, about one from each article.\n\n\
         Rules:\n\
         - Give {count} articles' exact titles, as on English Wikipedia, best first.\n\
         - Each is about one distinct thing, e.g. one building, not a list, a disambiguation page, or the subject in \
           general, so that the quiz covers the subject broadly.\n\
         - Choose what suits the difficulty: for an easy quiz, things most people at a party have heard of.\n",
        about = match request.topic_name() {
            "" => String::new(),
            topic => format!(" about: {topic}"),
        },
        questions = request.questions,
        difficulty = request.difficulty.describe(),
    );
    if !candidates.is_empty() {
        prompt.push_str("- Choose only from the articles listed below.\n");
    }
    if !request.instructions.trim().is_empty() {
        prompt.push_str(&format!(
            "\nThe quiz maker's own instructions for the quiz, to bear in mind when choosing, e.g. articles with good \
             pictures if they ask for picture questions:\n<instructions>\n{}\n</instructions>\n",
            request.instructions.trim()
        ));
    }
    if !candidates.is_empty() {
        prompt.push_str("\nArticles to choose from:\n");
        for title in candidates {
            prompt.push_str(&format!("- {title}\n"));
        }
    }
    prompt
}

/// The JSON Schema of Claude's answer.
pub fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "articles": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["articles"]
    })
}

/// The titles in Claude's answer.
pub fn titles(answer: serde_json::Value) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Answer {
        articles: Vec<String>,
    }
    let answer: Answer = serde_json::from_value(answer)?;
    Ok(answer
        .articles
        .into_iter()
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::Difficulty;

    #[test]
    fn asks_for_well_known_articles_on_the_subject() {
        let request = Request {
            topic: "Famous buildings".into(),
            instructions: "Every question is a picture question".into(),
            questions: 10,
            difficulty: Difficulty::Easy,
            ..Request::default()
        };
        let prompt = prompt(&request, 12, &[]);
        assert!(prompt.contains("from about: Famous buildings."));
        assert!(prompt.contains("Give 12 articles"));
        assert!(prompt.contains("<instructions>\nEvery question is a picture question\n</instructions>"));
        assert!(!prompt.contains("Choose only from"));
    }

    #[test]
    fn chooses_from_a_category() {
        let request = Request {
            topic: "Category:Bridges in London".into(),
            questions: 5,
            ..Request::default()
        };
        let prompt = prompt(&request, 5, &["Tower Bridge".into(), "London Bridge".into()]);
        assert!(prompt.contains("from about: Bridges in London."));
        assert!(prompt.contains("Choose only from"));
        assert!(prompt.ends_with("- Tower Bridge\n- London Bridge\n"));
    }

    #[test]
    fn reads_the_titles() {
        let answer = json!({"articles": [" Eiffel Tower ", "", "Taj Mahal"]});
        assert_eq!(titles(answer).unwrap(), ["Eiffel Tower", "Taj Mahal"]);
    }
}
