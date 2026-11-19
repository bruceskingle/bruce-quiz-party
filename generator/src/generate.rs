//! Generating a quiz from start to finish: reading Wikipedia, asking Claude, and saving the draft with its images,
//! telling the window how it is going.

use std::path::{Path, PathBuf};

use quiz_format::Quiz;

use crate::{
    claude,
    draft::{self, Request},
    publish,
    settings::Destination,
    suggest,
    wikipedia::{Article, Picture, Wikipedia, article_title, category_name},
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
    let titles = articles_with(&wikipedia, request, claude_path, progress)?;

    let mut articles: Vec<Article> = Vec::new();
    let titles_count = titles.len().max(1);
    for title in titles {
        progress(format!("Reading “{title}”…"));
        match wikipedia.article(&title) {
            Ok(mut article) if !article.text.trim().is_empty() => {
                // The more articles, the less of each, from its start, which says the most about it.
                article.cut_to(ARTICLES_CHARS / titles_count);
                articles.push(article);
            }
            Ok(_) => progress(format!("“{title}” has no text, so it is left out.")),
            Err(error) => progress(format!("Leaving out “{title}”: {error:#}")),
        }
    }
    anyhow::ensure!(!articles.is_empty(), "none of the articles could be read");

    // Each article's images, its main one first, for Claude to choose from, enough for every question to have one if
    // the instructions ask, but not so many that Claude's request is mostly images.
    let most = (request.questions * 3).clamp(20, 100);
    let mut names: Vec<String> = Vec::new();
    for name in articles
        .iter()
        .flat_map(|article| article.pictures(PICTURES_PER_ARTICLE))
    {
        if names.len() < most && !names.contains(name) {
            names.push(name.clone());
        }
    }
    let mut pictures: Vec<Picture> = Vec::new();
    if !names.is_empty() {
        progress(format!("Checking the licences of {} images…", names.len()));
        match wikipedia.pictures(&names) {
            Ok(found) => pictures = found,
            Err(error) => progress(format!("Leaving out the images: {error:#}")),
        }
        if pictures.len() < names.len() {
            progress(format!(
                "{} of them are not freely licensed, or are icons, so they are left out.",
                names.len() - pictures.len()
            ));
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

/// The most articles read for a quiz, one for each question of the longest quiz.
pub const MAX_ARTICLES: usize = 20;

/// The most text of all the articles given to Claude together, in characters, so that they fit in one request: all of
/// a few articles, or the start of many.
const ARTICLES_CHARS: usize = 72_000;

/// The most images offered to Claude from each article.
const PICTURES_PER_ARTICLE: usize = 15;

/// How many articles to read for a quiz: about one for each question, so that it covers its subject broadly.
fn articles_for(questions: usize) -> usize {
    questions.clamp(3, MAX_ARTICLES)
}

/// The Wikipedia articles to write a quiz from, for the user to look at and change before writing it: as
/// [`generate`] would choose them.
pub fn suggest(request: &Request, claude_path: &Path, progress: &dyn Fn(String)) -> anyhow::Result<Vec<String>> {
    articles_with(&Wikipedia::new(), request, claude_path, progress)
}

/// The articles to write from: those listed, with any category listed replaced by the best of its articles; or else
/// the best of the subject's category, if it is one; or else the best-known on the subject, chosen by Claude, or found
/// by searching Wikipedia if Claude cannot say.
fn articles_with(
    wikipedia: &Wikipedia,
    request: &Request,
    claude_path: &Path,
    progress: &dyn Fn(String),
) -> anyhow::Result<Vec<String>> {
    let wanted = articles_for(request.questions);
    let mut listed: Vec<String> = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    for given in &request.articles {
        match category_name(given) {
            Some(category) => categories.push(category),
            None => listed.push(article_title(given)),
        }
    }
    if request.articles.is_empty()
        && let Some(category) = category_name(&request.topic)
    {
        categories.push(category);
    }

    if categories.is_empty() && !listed.is_empty() {
        if listed.len() > MAX_ARTICLES {
            progress(format!(
                "Reading only the first {MAX_ARTICLES} articles, as Claude can read only so much at once."
            ));
            listed.truncate(MAX_ARTICLES);
        }
        return Ok(listed);
    }

    if !categories.is_empty() {
        let mut members: Vec<String> = Vec::new();
        for category in &categories {
            progress(format!("Listing the articles in the category “{category}”…"));
            for title in wikipedia.category(category)? {
                if !members.contains(&title) && !listed.contains(&title) && !is_list(&title) {
                    members.push(title);
                }
            }
        }
        let wanted = wanted.saturating_sub(listed.len()).max(1);
        let chosen = if members.len() <= wanted {
            members
        } else {
            progress(format!(
                "Asking Claude which {wanted} of the category's {} articles to read…",
                members.len()
            ));
            match ask_for_articles(wikipedia, request, claude_path, wanted, &members) {
                Ok(chosen) if !chosen.is_empty() => chosen,
                Ok(_) => members.into_iter().take(wanted).collect(),
                Err(error) => {
                    progress(format!(
                        "Claude could not choose, so reading the first of them: {error:#}"
                    ));
                    members.into_iter().take(wanted).collect()
                }
            }
        };
        listed.extend(chosen);
        anyhow::ensure!(!listed.is_empty(), "the category has no articles");
        listed.truncate(MAX_ARTICLES);
        return Ok(listed);
    }

    let topic = request.topic.trim();
    anyhow::ensure!(!topic.is_empty(), "there is no subject, nor any articles");
    progress(format!("Asking Claude which articles about “{topic}” to read…"));
    match ask_for_articles(wikipedia, request, claude_path, wanted, &[]) {
        // Too few is a sign that Claude did not know the subject's articles.
        Ok(chosen) if chosen.len() >= wanted.min(3) => return Ok(chosen),
        Ok(_) => progress("Claude found too few, so searching Wikipedia instead.".into()),
        Err(error) => progress(format!(
            "Claude could not choose, so searching Wikipedia instead: {error:#}"
        )),
    }
    progress(format!("Searching Wikipedia for “{topic}”…"));
    let found = choose(wikipedia.search(topic, wanted * 2)?, wanted);
    anyhow::ensure!(!found.is_empty(), "Wikipedia has no articles about “{topic}”");
    Ok(found)
}

/// Asks Claude for the articles to read, from those given if any, and keeps those which exist, at most `wanted`.
fn ask_for_articles(
    wikipedia: &Wikipedia,
    request: &Request,
    claude_path: &Path,
    wanted: usize,
    candidates: &[String],
) -> anyhow::Result<Vec<String>> {
    // A few more than wanted, as some may not be Wikipedia's titles.
    let asked = if candidates.is_empty() {
        wanted + wanted / 2
    } else {
        wanted
    };
    let answer = claude::ask(
        claude_path,
        &suggest::prompt(request, asked, candidates),
        &suggest::schema(),
    )?;
    let titles: Vec<String> = suggest::titles(answer)?
        .into_iter()
        .filter(|title| candidates.is_empty() || candidates.contains(title))
        .collect();
    let mut existing = if candidates.is_empty() {
        wikipedia.existing(&titles)?
    } else {
        titles
    };
    existing.truncate(wanted);
    Ok(existing)
}

/// Whether an article is a list, e.g. "List of flags of Asia", which has little to ask about beyond its entries.
fn is_list(title: &str) -> bool {
    title.starts_with("List of") || title.starts_with("Lists of")
}

/// The articles to read from those found, best first: lists, e.g. "List of flags of Asia", have little to ask about
/// beyond their entries, so they come only after the others.
fn choose(titles: Vec<String>, count: usize) -> Vec<String> {
    let (lists, others): (Vec<String>, Vec<String>) = titles.into_iter().partition(|title| is_list(title));
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
