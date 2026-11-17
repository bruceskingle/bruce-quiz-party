//! Generating a quiz from start to finish: reading Wikipedia, asking Claude, and saving the draft with its images,
//! telling the window how it is going.

use std::path::{Path, PathBuf};

use quiz_format::Quiz;

use crate::{
    claude,
    draft::{self, Request},
    publish,
    settings::Destination,
    wikipedia::{Article, Picture, Wikipedia},
};

/// Generates a quiz and saves it in a new folder where chosen, adding it to the library if it is one, returning the
/// folder and the quiz, and saying each step as it starts.
pub fn generate(
    request: &Request,
    claude_path: &Path,
    destination: &Destination,
    progress: &dyn Fn(String),
) -> anyhow::Result<(PathBuf, Quiz)> {
    let quizzes = destination.folder();
    match destination {
        // The controller's quizzes folder may not have been made yet.
        Destination::Folder { path } => std::fs::create_dir_all(path)
            .map_err(anyhow::Error::from)
            .map_err(|error| error.context(format!("cannot make {}", path.display())))?,
        Destination::Library { checkout, .. } => anyhow::ensure!(
            checkout.is_dir(),
            "there is no library checkout at {}",
            checkout.display()
        ),
    }
    let wikipedia = Wikipedia::new();
    // Enough articles for the questions to vary, but few enough for Claude to read them all.
    let count = (request.questions / 2).clamp(3, 6);
    progress(format!("Searching Wikipedia for “{}”…", request.topic));
    let titles = choose(wikipedia.search(&request.topic, count * 2)?, count);
    anyhow::ensure!(
        !titles.is_empty(),
        "Wikipedia has no articles about “{}”",
        request.topic
    );

    let mut articles: Vec<Article> = Vec::new();
    for title in titles {
        progress(format!("Reading “{title}”…"));
        match wikipedia.article(&title) {
            Ok(article) if !article.text.trim().is_empty() => articles.push(article),
            Ok(_) => progress(format!("“{title}” has no text, so it is left out.")),
            Err(error) => progress(format!("Leaving out “{title}”: {error:#}")),
        }
    }
    anyhow::ensure!(
        !articles.is_empty(),
        "none of Wikipedia's articles about “{}” could be read",
        request.topic
    );

    let mut pictures: Vec<Picture> = Vec::new();
    for name in articles.iter().filter_map(|article| article.image.clone()) {
        progress(format!("Checking the licence of {name}…"));
        match wikipedia.picture(&name) {
            Ok(Some(picture)) if !pictures.iter().any(|other| other.name == picture.name) => pictures.push(picture),
            Ok(Some(_)) => {}
            Ok(None) => progress(format!("{name} is not freely licensed, so it is left out.")),
            Err(error) => progress(format!("Leaving out {name}: {error:#}")),
        }
    }

    progress(format!(
        "Asking Claude to write {} questions from {} articles and {} images: this takes a minute or two…",
        request.questions,
        articles.len(),
        pictures.len()
    ));
    let prompt = draft::prompt(request, &articles, &pictures);
    let answer = claude::ask(claude_path, &prompt, &draft::schema())?;
    let draft = draft::build(answer, &articles, &pictures)?;

    progress(format!(
        "Saving “{}”, with {} questions and {} images…",
        draft.quiz.title,
        draft.quiz.questions.len(),
        draft.downloads.len()
    ));
    let folder = draft::save(&draft, quizzes, |picture, path| wikipedia.download(picture, path))?;
    if let Destination::Library { checkout, publish } = destination
        && let Err(error) = publish::add_to_library(checkout, &folder, &draft.quiz.title, *publish, progress)
    {
        // The quiz is saved, so this is told rather than failing it.
        progress(format!("The quiz is saved, but {error:#}"));
    }
    Ok((folder, draft.quiz))
}

/// The articles to read from those found, best first: lists, e.g. "List of flags of Asia", have little to ask about
/// beyond their entries, so they come only after the others.
fn choose(titles: Vec<String>, count: usize) -> Vec<String> {
    let (lists, others): (Vec<String>, Vec<String>) = titles
        .into_iter()
        .partition(|title| title.starts_with("List of") || title.starts_with("Lists of"));
    others.into_iter().chain(lists).take(count).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn prefers_articles_to_lists() {
        let titles = [
            "Lists of flags",
            "Flag of Japan",
            "List of Asian flags",
            "Flag of India",
            "Vexillology",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            super::choose(titles, 4),
            ["Flag of Japan", "Flag of India", "Vexillology", "Lists of flags"]
        );
    }
}
