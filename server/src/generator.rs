//! Writing a quiz with Claude, on the Quizzes page: a subject, the Wikipedia articles to write from, found for it or
//! chosen, anything else for Claude in the user's own words, how many questions and how hard, and where to save it,
//! this app's quizzes, to play and edit it here, or a library checkout, to share it. Claude is asked through Claude
//! Code, on the user's own subscription, where it is installed and the app may run it, or through Claude's API, with
//! the user's own key and the model they choose, see the generator crate's claude.rs.

use std::path::{Path, PathBuf};

use dioxus::prelude::*;
use generator::{
    api_key,
    claude::{self, Choice, Claude, MODELS},
    draft::{Difficulty, Request},
    generate::{self, MAX_ARTICLES},
    settings::{self, Destination},
    wikipedia::category_name,
};
use quiz_format::Quiz;

/// The Claude Code command, if it can be found: the one given, else `claude` on the PATH or in its usual places, else
/// the copy that comes with the VS Code extension. Never in the Mac App Store's build, whose sandbox would not let it
/// run.
pub fn claude_command(given: Option<&Path>) -> Option<PathBuf> {
    if crate::STORE {
        return None;
    }
    generator::claude::find(given)
}

/// The library checkout suggested before one has been chosen.
fn suggested_checkout() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("git/bruce-quiz-library")
}

/// How the last quiz went.
#[derive(Clone, PartialEq)]
enum Outcome {
    /// Saved in a library checkout, shown here, as the Quizzes page lists only this app's quizzes.
    InLibrary(PathBuf, Quiz),
    Failed(String),
}

/// The page: the topic and options, where to save, what is happening, and how it went. A quiz saved in this app's
/// quizzes is handed to `saved`, for the Quizzes page to open in the editor.
#[component]
pub fn QuizWriter(
    /// Claude Code, if it is installed.
    code: Option<PathBuf>,
    quizzes: PathBuf,
    saved: EventHandler<(PathBuf, Quiz)>,
    done: EventHandler<()>,
) -> Element {
    // Where the last quiz was saved: a library checkout, or else this app's quizzes.
    let mut destination = use_signal({
        let quizzes = quizzes.clone();
        move || match settings::load() {
            // The Mac App Store's build saves only into its own quizzes, see DestinationChooser.
            library @ Destination::Library { .. } if !crate::STORE => library,
            _ => Destination::Folder { path: quizzes },
        }
    });
    let mut topic = use_signal(String::new);
    // The articles to write from, one per line, titles or addresses; if none, those found for the subject.
    let mut articles = use_signal(String::new);
    let mut instructions = use_signal(String::new);
    // Whether the articles for the subject are being looked for, and why they could not be.
    let mut suggesting = use_signal(|| false);
    let mut suggest_error = use_signal(|| None::<String>);
    let mut count = use_signal(|| 10usize);
    let mut difficulty = use_signal(|| Difficulty::Medium);
    let mut steps = use_signal(Vec::<String>::new);
    let mut running = use_signal(|| false);
    let mut outcome = use_signal(|| None::<Outcome>);
    // How to ask Claude, as chosen below, or None until it can be asked, e.g. without an API key.
    let asker = use_signal(|| None::<Claude>);

    // What the page asks for, as it stands.
    let request = move || Request {
        topic: topic().trim().to_string(),
        articles: article_titles(&articles()),
        instructions: instructions().trim().to_string(),
        questions: count(),
        difficulty: difficulty(),
    };

    let write = {
        move |_| {
            let request = request();
            if request.topic.is_empty() && request.articles.is_empty() {
                return;
            }
            let Some(claude) = asker() else { return };
            let destination = destination();
            running.set(true);
            steps.set(Vec::new());
            outcome.set(None);
            suggest_error.set(None);
            spawn(async move {
                let in_library = matches!(destination, Destination::Library { .. });
                let written = with_steps(steps, move |progress| {
                    generate::generate(&request, &claude, &destination, progress)
                })
                .await;
                running.set(false);
                match written {
                    Ok((folder, quiz)) if in_library => outcome.set(Some(Outcome::InLibrary(folder, quiz))),
                    Ok((folder, quiz)) => saved.call((folder, quiz)),
                    Err(error) => outcome.set(Some(Outcome::Failed(format!("{error:#}")))),
                }
            });
        }
    };

    // Fills in the articles to write from, for the user to change before writing: the best of any category listed,
    // with the other articles listed; else those for the subject, as Claude chooses them.
    let suggest = {
        move |_| {
            let mut request = request();
            if !request.articles.iter().any(|line| category_name(line).is_some()) {
                request.articles.clear();
            }
            if request.topic.is_empty() && request.articles.is_empty() {
                return;
            }
            let Some(claude) = asker() else { return };
            suggesting.set(true);
            steps.set(Vec::new());
            outcome.set(None);
            suggest_error.set(None);
            spawn(async move {
                let found = with_steps(steps, move |progress| generate::suggest(&request, &claude, progress)).await;
                suggesting.set(false);
                match found {
                    Ok(titles) if titles.is_empty() => suggest_error.set(Some("No articles were found.".into())),
                    Ok(titles) => {
                        articles.set(titles.join("\n"));
                        steps.set(Vec::new());
                    }
                    Err(error) => suggest_error.set(Some(format!("Cannot suggest articles: {error:#}"))),
                }
            });
        }
    };
    let busy = running() || suggesting();
    let can_ask = asker.read().is_some();
    let can_write = can_ask && (!topic().trim().is_empty() || !article_titles(&articles()).is_empty());
    let can_suggest = can_ask
        && (!topic().trim().is_empty()
            || article_titles(&articles())
                .iter()
                .any(|line| category_name(line).is_some()));

    rsx! {
        div { class: "editor generator",
            div { class: "editor-bar",
                h1 { "Write a Quiz with Claude" }
                div { class: "spacer" }
                button { disabled: busy, onclick: move |_| done.call(()), "Done" }
            }
            p { class: "muted",
                "Writes a draft quiz from Wikipedia's articles on a topic, crediting them and its images, with Claude. "
                "Review it in the editor before playing or sharing it."
            }
            ClaudeChooser { code: code.clone(), disabled: busy, asker }
            // The Mac App Store's build saves only into this app's quizzes, as publishing to a library runs git, which
            // its sandbox would not let it.
            if !crate::STORE {
                DestinationChooser {
                    destination: destination(),
                    quizzes: quizzes.clone(),
                    disabled: running(),
                    onchange: move |chosen: Destination| {
                        if let Err(error) = settings::save(&chosen) {
                            eprintln!("Cannot remember where to save quizzes: {error:#}");
                        }
                        destination.set(chosen);
                    },
                }
            }
            label { class: "generator-label", "Subject" }
            div { class: "generator-form",
                input {
                    class: "topic",
                    placeholder: "Short, e.g. Famous buildings, The Moon, 1980s pop music; or a Wikipedia category, e.g. Category:Bridges in London",
                    value: "{topic}",
                    disabled: busy,
                    oninput: move |event| topic.set(event.value()),
                }
                button {
                    disabled: busy || !can_suggest,
                    title: "Ask Claude for the best-known articles on the subject, or the best of a category listed below, and list them here, to change before writing",
                    onclick: suggest,
                    if suggesting() {
                        "Suggesting…"
                    } else {
                        "Suggest Articles"
                    }
                }
            }
            label { class: "generator-label", "Wikipedia articles" }
            textarea {
                rows: 6,
                placeholder: "Titles or links, one per line, at most {MAX_ARTICLES}, or a category, e.g. Category:Bridges in London, for the best of its articles. Leave empty for Claude to choose for the subject.",
                value: "{articles}",
                disabled: busy,
                oninput: move |event| articles.set(event.value()),
            }
            if let Some(error) = suggest_error() {
                p { class: "invalid", "{error}" }
            }
            label { class: "generator-label", "Instructions for Claude" }
            textarea {
                rows: 4,
                placeholder: "Optional, e.g. “Every question is a picture question”, or “Avoid questions which only ask to name a landmark”",
                value: "{instructions}",
                disabled: busy,
                oninput: move |event| instructions.set(event.value()),
            }
            div { class: "generator-form",
                select {
                    disabled: busy,
                    value: "{count}",
                    onchange: move |event: FormEvent| count.set(event.value().parse().unwrap_or(10)),
                    for n in [5, 8, 10, 12, 15, 20] {
                        option { key: "{n}", value: "{n}", "{n} questions" }
                    }
                }
                select {
                    disabled: busy,
                    value: difficulty().name(),
                    onchange: move |event: FormEvent| difficulty.set(Difficulty::parse(&event.value())),
                    for level in Difficulty::ALL {
                        option { key: "{level.name()}", value: level.name(), "{capitalised(level.name())}" }
                    }
                }
                div { class: "spacer" }
                button {
                    class: "primary",
                    disabled: busy || !can_write,
                    onclick: write,
                    if running() {
                        "Writing…"
                    } else {
                        "Write the Quiz"
                    }
                }
            }
            if !steps.read().is_empty() {
                ol { class: "steps",
                    for (index , step) in steps.read().iter().enumerate() {
                        li { key: "{index}", "{step}" }
                    }
                }
            }
            match outcome() {
                Some(Outcome::Failed(error)) => rsx! {
                    p { class: "invalid", "The quiz could not be written: {error}" }
                },
                Some(Outcome::InLibrary(folder, quiz)) => rsx! {
                    DraftPreview { folder, quiz }
                },
                None => rsx! {},
            }
        }
    }
}

/// How to ask Claude: Claude Code, if it is installed, on the user's subscription, or Claude's API with a model they
/// choose, and their API key, kept in the Keychain. Sets `asker` to the way chosen, or None while it cannot be used,
/// e.g. with no API key.
#[component]
fn ClaudeChooser(code: Option<PathBuf>, disabled: bool, mut asker: Signal<Option<Claude>>) -> Element {
    let mut choice = use_signal(|| match claude::saved_choice(code.is_some()) {
        Choice::Code if code.is_none() => Choice::default(),
        choice => choice,
    });
    // The key saved, or why it cannot be read.
    let mut key = use_signal(|| api_key::load().map_err(|error| format!("{error:#}")));
    // A key being typed, while there is none or it is being changed.
    let mut typed = use_signal(String::new);
    let mut changing = use_signal(|| false);
    let mut key_error = use_signal(|| None::<String>);

    use_effect({
        let code = code.clone();
        move || {
            let chosen = match choice() {
                Choice::Code => code.clone().map(Claude::Code),
                Choice::Api { model } => match key() {
                    Ok(Some(key)) => Some(Claude::Api { key, model }),
                    _ => None,
                },
            };
            asker.set(chosen);
        }
    });

    let selected = match choice() {
        Choice::Code => "code".to_string(),
        Choice::Api { model } => model,
    };
    let on_api = matches!(choice(), Choice::Api { .. });
    let saved = matches!(key(), Ok(Some(_)));
    let mut save_key = move || match api_key::save(&typed()) {
        Ok(()) => {
            key.set(Ok(Some(typed().trim().to_string())));
            typed.set(String::new());
            changing.set(false);
            key_error.set(None);
        }
        Err(error) => key_error.set(Some(format!("{error:#}"))),
    };

    rsx! {
        fieldset { class: "destination", disabled,
            legend { "Ask Claude with" }
            select {
                value: "{selected}",
                onchange: move |event: FormEvent| {
                    let chosen = if event.value() == "code" {
                        Choice::Code
                    } else {
                        Choice::Api { model: event.value() }
                    };
                    if let Err(error) = claude::save_choice(&chosen) {
                        eprintln!("Cannot remember how to ask Claude: {error:#}");
                    }
                    choice.set(chosen);
                },
                if code.is_some() {
                    option { value: "code", selected: selected == "code", "Claude Code, on your Claude subscription" }
                }
                for model in MODELS {
                    option {
                        key: "{model.id}",
                        value: model.id,
                        selected: selected == model.id,
                        "{model.name}, with your API key: {model.cost}"
                    }
                }
            }
            if on_api {
                if saved && !changing() {
                    p { class: "muted small",
                        "Your API key is saved in the Keychain; what Claude writes is billed to it. "
                        button { onclick: move |_| changing.set(true), "Change Key" }
                        " "
                        button {
                            onclick: move |_| match api_key::forget() {
                                Ok(()) => key.set(Ok(None)),
                                Err(error) => key_error.set(Some(format!("{error:#}"))),
                            },
                            "Forget Key"
                        }
                    }
                } else {
                    p { class: "muted small",
                        "Claude's API needs your own API key, which you can make in the Claude Console, "
                        "console.anthropic.com; what Claude writes is billed to it. It is kept in the Keychain."
                    }
                    div { class: "generator-form",
                        input {
                            r#type: "password",
                            placeholder: "Your API key, sk-ant-…",
                            value: "{typed}",
                            oninput: move |event| typed.set(event.value()),
                            onkeydown: move |event: KeyboardEvent| {
                                if event.key() == Key::Enter {
                                    save_key();
                                }
                            },
                        }
                        button {
                            disabled: typed().trim().is_empty(),
                            onclick: move |_| save_key(),
                            "Save Key"
                        }
                        if changing() {
                            button { onclick: move |_| changing.set(false), "Cancel" }
                        }
                    }
                }
                if let Err(error) = key() {
                    p { class: "invalid small", "{error}" }
                }
                if let Some(error) = key_error() {
                    p { class: "invalid small", "{error}" }
                }
            } else if let Some(code) = &code {
                p { class: "muted small", "Claude Code at {code.display()}." }
            }
        }
    }
}

/// Runs work on a thread of its own, showing each step it says in `steps` as it goes, and gives its result.
async fn with_steps<T: Send + 'static>(
    mut steps: Signal<Vec<String>>,
    work: impl FnOnce(&dyn Fn(String)) -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
    let work = tokio::task::spawn_blocking(move || {
        work(&|step| {
            let _ = sender.send(step);
        })
    });
    while let Some(step) = receiver.recv().await {
        steps.write().push(step);
    }
    work.await.map_err(anyhow::Error::from).and_then(|result| result)
}

/// The titles of the articles listed, one per line, as titles or addresses, less blank lines and repeats.
fn article_titles(text: &str) -> Vec<String> {
    let mut titles: Vec<String> = Vec::new();
    for title in text.lines().map(generator::wikipedia::article_title) {
        if !title.is_empty() && !titles.contains(&title) {
            titles.push(title);
        }
    }
    titles
}

/// A word with its first letter a capital, e.g. "Easy".
fn capitalised(word: &str) -> String {
    let mut letters = word.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

/// Where to save: this app's quizzes, to play and edit the quiz here, or a library checkout, to share it. Each choice
/// shows its folder; the checkout's is kept while this app's is chosen, so that switching back and forth keeps it.
#[component]
fn DestinationChooser(
    destination: Destination,
    quizzes: PathBuf,
    disabled: bool,
    onchange: EventHandler<Destination>,
) -> Element {
    let mut checkout = use_signal(|| match &destination {
        Destination::Library { checkout, .. } => checkout.clone(),
        Destination::Folder { .. } => suggested_checkout(),
    });
    let is_library = matches!(destination, Destination::Library { .. });
    let publish = matches!(destination, Destination::Library { publish: true, .. });
    let to_library = move |publish: bool| Destination::Library {
        checkout: checkout(),
        publish,
    };
    rsx! {
        fieldset { class: "destination", disabled,
            legend { "Save the quiz in" }
            label {
                input {
                    r#type: "radio",
                    name: "destination",
                    checked: !is_library,
                    onchange: {
                        let quizzes = quizzes.clone();
                        move |_| onchange.call(Destination::Folder { path: quizzes.clone() })
                    },
                }
                "This app's quizzes, to play it here"
            }
            p { class: "destination-path muted small", "{quizzes.display()}" }
            label {
                input {
                    r#type: "radio",
                    name: "destination",
                    checked: is_library,
                    onchange: move |_| onchange.call(to_library(false)),
                }
                "A library, to share it: a local checkout of the library's git repository"
            }
            input {
                class: "destination-path",
                value: "{checkout.read().display()}",
                disabled: !is_library,
                oninput: move |event: FormEvent| {
                    checkout.set(PathBuf::from(event.value()));
                    onchange.call(to_library(publish));
                },
            }
            if is_library {
                label {
                    input {
                        r#type: "checkbox",
                        checked: publish,
                        onchange: move |event: FormEvent| onchange.call(to_library(event.checked())),
                    }
                    "Commit and push it with git, so that the library's readers can download it"
                }
            }
        }
    }
}

/// A quiz saved in a library checkout: where it is, and its questions with their correct answers, to check at a
/// glance, as it is not in this app's quizzes to open in the editor.
#[component]
fn DraftPreview(folder: PathBuf, quiz: Quiz) -> Element {
    rsx! {
        div { class: "draft",
            h2 { "{quiz.title}" }
            if let Some(description) = &quiz.description {
                p { class: "muted", "{description}" }
            }
            p {
                "Saved in {folder.display()}. "
                button {
                    onclick: {
                        let folder = folder.clone();
                        move |_| {
                            let _ = std::process::Command::new("open").arg(&folder).spawn();
                        }
                    },
                    "Show in Finder"
                }
            }
            for (index , question) in quiz.questions.iter().enumerate() {
                div { key: "{index}", class: "question-card",
                    h3 { "{index + 1}. {question.text}" }
                    if let Some(image) = &question.image {
                        p { class: "muted small", "Image: {image.file}, “{image.alt}”" }
                    }
                    ul { class: "draft-answers",
                        for (position , answer) in question.answers.iter().enumerate() {
                            li {
                                key: "{position}",
                                class: if position == question.correct { "correct" } else { "" },
                                "{answer}"
                            }
                        }
                    }
                    if let Some(source) = &question.source {
                        p { class: "muted small", "From {source}" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn capitalises_the_difficulties() {
        assert_eq!(super::capitalised("medium"), "Medium");
        assert_eq!(super::capitalised(""), "");
    }

    #[test]
    fn reads_the_articles_listed() {
        let listed =
            "Flag of Japan\n\n  https://en.wikipedia.org/wiki/Flag_of_Japan\nhttps://en.wikipedia.org/wiki/Hinomaru\n";
        assert_eq!(super::article_titles(listed), ["Flag of Japan", "Hinomaru"]);
    }
}
