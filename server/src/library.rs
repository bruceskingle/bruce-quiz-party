//! The quiz library: the quizzes in the quizzes folder, listed in the sidebar, with the chosen one's questions
//! previewed beside it, to edit, or to start a new one.

use std::path::{Path, PathBuf};

use dioxus::desktop::wry::RequestAsyncResponder;
use dioxus::desktop::wry::http::{Request, Response, StatusCode};
use dioxus::prelude::*;
use quiz_format::{LoadError, Quiz};

use crate::{Args, PartyServer, answer_shape::AnswerShape, editor::QuizEditor, party::PartyQuiz};

/// A quiz in the library, or why it could not be read.
#[derive(Clone)]
struct Entry {
    folder: PathBuf,
    quiz: Result<Quiz, String>,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.folder == other.folder
    }
}

/// Reads the library, giving an error if the folder itself cannot be read.
fn read_library(folder: &Path) -> Result<Vec<Entry>, String> {
    let entries = quiz_format::load_library(folder).map_err(|error| {
        format!(
            "{:#}",
            anyhow::Error::from(error).context(format!("cannot read {}", folder.display()))
        )
    })?;
    Ok(entries
        .into_iter()
        .map(|(folder, quiz)| Entry {
            folder,
            quiz: quiz.map_err(|error: LoadError| format!("{:#}", anyhow::Error::from(error))),
        })
        .collect())
}

/// The library's sidebar and the chosen quiz, or the editor.
#[component]
pub fn Library() -> Element {
    let args = use_context::<Args>();
    let PartyServer(server) = use_context::<PartyServer>();
    let folder = args.quizzes_folder();
    let mut library = use_signal({
        let folder = folder.clone();
        move || read_library(&folder)
    });
    let mut chosen = use_signal(|| 0usize);
    // The library is read each time the page is opened, so that it shows quizzes added since, e.g. by the generator;
    // the next party can be set up with them too.
    use_hook({
        let server = server.clone();
        move || {
            if let (Ok(entries), Ok(server)) = (&*library.peek(), &server) {
                server.shared.set_library(party_library(entries));
            }
        }
    });
    // The quiz being edited, in its folder, or a new one without; None when not editing.
    let mut editing = use_signal(|| None::<(Option<PathBuf>, Quiz)>);
    // Whether the host is looking at an online library to download quizzes from.
    let mut browsing = use_signal(|| false);
    // Reads the library again after a quiz was saved or downloaded, shows that quiz, and lets the next party choose it.
    let reload = use_callback({
        let folder = folder.clone();
        let server = server.clone();
        move |saved: PathBuf| {
            let read = read_library(&folder);
            if let Ok(entries) = &read {
                if let Some(index) = entries.iter().position(|entry| entry.folder == saved) {
                    chosen.set(index);
                }
                if let Ok(server) = &server {
                    server.shared.set_library(party_library(entries));
                }
            }
            library.set(read);
        }
    });

    if let Some((quiz_folder, quiz)) = editing() {
        return rsx! {
            div { class: "container",
                div { class: "content",
                    QuizEditor {
                        folder: quiz_folder,
                        quiz,
                        quizzes: folder.clone(),
                        done: move |saved: Option<PathBuf>| {
                            editing.set(None);
                            if let Some(saved) = saved {
                                reload(saved);
                            }
                        },
                    }
                }
            }
        };
    }

    if browsing() {
        return rsx! {
            div { class: "container",
                div { class: "content",
                    crate::download::LibraryBrowser {
                        quizzes: folder.clone(),
                        downloaded: move |saved: PathBuf| reload(saved),
                        done: move |_| browsing.set(false),
                    }
                }
            }
        };
    }

    let entries = match &*library.read() {
        Ok(entries) => entries.clone(),
        Err(error) => {
            return rsx! {
                div { class: "container",
                    div { class: "content",
                        h1 { "No quizzes" }
                        p { class: "invalid", "{error}" }
                        p { class: "muted", "Start the app with --quizzes <folder> to use another folder." }
                    }
                }
            };
        }
    };

    rsx! {
        div { class: "container",
            div { class: "sidebar",
                h2 { "Quizzes" }
                for (index , entry) in entries.iter().enumerate() {
                    div {
                        key: "{entry.folder.display()}",
                        class: match (&entry.quiz, index == chosen()) {
                            (Err(_), _) => "sidebar-item invalid",
                            (Ok(_), true) => "sidebar-item active",
                            (Ok(_), false) => "sidebar-item",
                        },
                        onclick: move |_| chosen.set(index),
                        match &entry.quiz {
                            Ok(quiz) => quiz.title.clone(),
                            Err(_) => folder_name(&entry.folder),
                        }
                    }
                }
                if entries.is_empty() {
                    p { class: "muted", "No quizzes in {folder.display()}" }
                }
                button {
                    class: "new-quiz",
                    onclick: move |_| editing.set(Some((None, crate::editor::new_quiz()))),
                    "New Quiz"
                }
                button {
                    class: "new-quiz",
                    onclick: move |_| browsing.set(true),
                    "Get Quizzes from a Library"
                }
            }
            div { class: "content",
                if let Some(entry) = entries.get(chosen()) {
                    div { class: "preview-actions",
                        if let Ok(quiz) = &entry.quiz {
                            button {
                                class: "primary",
                                onclick: {
                                    let entry = entry.clone();
                                    let quiz = quiz.clone();
                                    move |_| editing.set(Some((Some(entry.folder.clone()), quiz.clone())))
                                },
                                "Edit"
                            }
                        }
                        button {
                            title: "Open the quiz's folder in the Finder, e.g. to add its images",
                            onclick: {
                                let folder = entry.folder.clone();
                                move |_| {
                                    let _ = std::process::Command::new("open").arg(&folder).spawn();
                                }
                            },
                            "Show in Finder"
                        }
                    }
                    QuizPreview { entry: entry.clone() }
                }
            }
        }
    }
}

/// The quizzes of the library which could be read, for setting up a party.
fn party_library(entries: &[Entry]) -> Vec<PartyQuiz> {
    entries
        .iter()
        .filter_map(|entry| {
            Some(PartyQuiz {
                folder: entry.folder.clone(),
                quiz: entry.quiz.clone().ok()?,
            })
        })
        .collect()
}

/// The last part of a folder's path, to name a quiz which could not be read.
fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A quiz's settings and questions, or why it could not be read.
#[component]
fn QuizPreview(entry: Entry) -> Element {
    let quiz = match &entry.quiz {
        Ok(quiz) => quiz,
        Err(error) => {
            return rsx! {
                h1 { "{folder_name(&entry.folder)}" }
                p { class: "invalid", "{error}" }
            };
        }
    };
    let settings = &quiz.settings;
    rsx! {
        h1 { "{quiz.title}" }
        if let Some(description) = &quiz.description {
            p { class: "muted", "{description}" }
        }
        p { class: "muted",
            "{quiz.questions.len()} questions, {settings.time_limit} seconds each, up to {settings.max_points} points, "
            "{settings.scoring.curve:?} scoring down to {settings.scoring.floor * 100.0:.0}%"
        }
        for (index , question) in quiz.questions.iter().enumerate() {
            div { class: "question-card", key: "{index}",
                h3 { "{index + 1}. {question.text}" }
                if let Some(image) = &question.image {
                    img {
                        class: "question-image",
                        src: image_url(&entry.folder, &image.file),
                        alt: "{image.alt}",
                    }
                }
                div { class: "answers",
                    for (position , answer) in question.answers.iter().enumerate() {
                        div {
                            key: "{position}",
                            class: if position == question.correct { "answer answer-{position} correct" } else { "answer answer-{position}" },
                            AnswerShape { position }
                            "{answer}"
                        }
                    }
                }
                if !question.settings.is_empty() {
                    div { class: "meta",
                        "{quiz.question_settings(index).time_limit} seconds, "
                        "{quiz.question_settings(index).scoring.curve:?} scoring"
                    }
                }
                if let Some(source) = &question.source {
                    div { class: "meta", "Source: {source}" }
                }
            }
        }
    }
}

/// The URL the webview loads one of a quiz's images from, served by [`serve_image`].
pub fn image_url(folder: &Path, file: &str) -> String {
    let path = folder.join(file);
    format!("quizimg://localhost/{}", percent_encode(&path.to_string_lossy()))
}

/// Encodes a path for a URL, keeping its slashes.
fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Decodes a URL's path back to the file's path.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |byte: u8| (byte as char).to_digit(16).map(|digit| digit as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(high * 16 + low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The quizzes folder, which [`serve_image`] serves only from, set as the app starts.
pub static QUIZZES_FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The file a request is for, if it is inside the quizzes folder, so that the webview cannot be made to read others.
fn requested_file(request: &Request<Vec<u8>>) -> Option<PathBuf> {
    let folder = QUIZZES_FOLDER.get()?.canonicalize().ok()?;
    let path = PathBuf::from(percent_decode(request.uri().path()))
        .canonicalize()
        .ok()?;
    path.starts_with(&folder).then_some(path)
}

/// Serves a quiz's image to the webview, only from inside the quizzes folder.
pub fn serve_image(_id: dioxus::desktop::wry::WebViewId, request: Request<Vec<u8>>, responder: RequestAsyncResponder) {
    let response = match requested_file(&request).and_then(|path| Some((std::fs::read(&path).ok()?, path))) {
        Some((bytes, path)) => Response::builder()
            .header("Content-Type", content_type(&path))
            .body(bytes)
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap(),
    };
    responder.respond(response);
}

/// The MIME type of an image, from its extension.
pub fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}
