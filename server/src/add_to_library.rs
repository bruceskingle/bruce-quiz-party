//! Adding a quiz to a library, from its page on the Quizzes page, to share it: the quiz is copied into the library's
//! folder, replacing any copy there, and the library's index, `library.json`, is rewritten, or made if it had none. A
//! library on the web, e.g. a GitHub repository, is added to through a checkout of it, which, outside the Mac App
//! Store's build, can be committed and pushed with the user's own git; the App Store's sandbox would not let it run
//! git, so there the host pushes it themselves.

use std::path::PathBuf;

use anyhow::Context;
use dioxus::prelude::*;
use quiz_format::Quiz;

/// The library folder last added to, kept in the controller's data folder, or else the library checkout the generator
/// last saved into, or nothing.
fn saved_library() -> String {
    std::fs::read_to_string(library_file())
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .or_else(|| match generator::settings::load() {
            generator::settings::Destination::Library { checkout, .. } => Some(checkout.display().to_string()),
            generator::settings::Destination::Folder { .. } => None,
        })
        .unwrap_or_default()
}

/// Remembers the library folder added to.
fn save_library(folder: &str) {
    let path = library_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, folder.trim());
}

fn library_file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bruce-quiz-party")
        .join("add-to-library.txt")
}

/// How adding went: the quiz's folder in the library, and whether it was pushed, or why it could not be added.
type Added = Result<(PathBuf, bool), String>;

/// The panel which adds a quiz, in `folder`, to a library the host chooses.
#[component]
pub fn AddToLibrary(folder: PathBuf, quiz: Quiz, done: EventHandler<()>) -> Element {
    let mut library = use_signal(saved_library);
    let mut publish = use_signal(|| false);
    let mut adding = use_signal(|| false);
    let mut added = use_signal(|| None::<Added>);
    // A folder chosen in an earlier run is opened again from its bookmark, so that a sandboxed app may write to it.
    use_hook(move || crate::folder_access::resume(&PathBuf::from(library.peek().trim())));

    let add = {
        let folder = folder.clone();
        let quiz = quiz.clone();
        move |_| {
            let target = PathBuf::from(library().trim());
            if target.as_os_str().is_empty() {
                return;
            }
            save_library(&library());
            crate::folder_access::resume(&target);
            let push = publish() && !crate::STORE;
            let folder = folder.clone();
            let quiz = quiz.clone();
            adding.set(true);
            added.set(None);
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || -> anyhow::Result<(PathBuf, bool)> {
                    anyhow::ensure!(target.is_dir(), "{} is not a folder", target.display());
                    let copy = quiz_format::library::add_quiz(&target, &folder, &quiz)
                        .with_context(|| format!("cannot add the quiz to {}", target.display()))?;
                    if push {
                        generator::publish::add_to_library(&target, &copy, &quiz.title, true, &|_| {})?;
                    }
                    Ok((copy, push))
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|result| result);
                added.set(Some(result.map_err(|error| format!("{error:#}"))));
                adding.set(false);
            });
        }
    };

    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let chosen = PathBuf::from(library().trim());
    let replaces = !chosen.as_os_str().is_empty() && chosen.join(&name).join(quiz_format::QUIZ_FILE).is_file();
    let finished = matches!(added(), Some(Ok(_)));

    rsx! {
        div { class: "question-card add-to-library",
            h2 { "Add “{quiz.title}” to a Library" }
            p { class: "muted",
                "Copies the quiz, its quiz.md and the images it uses, into a library's folder, and updates the library's "
                "index, library.json, making it if the folder has none. For a library on the web, e.g. a GitHub "
                "repository, choose your checkout of it."
            }
            div { class: "library-address",
                input {
                    value: "{library}",
                    placeholder: "The library's folder",
                    disabled: adding(),
                    oninput: move |event| {
                        library.set(event.value());
                        added.set(None);
                    },
                }
                button {
                    disabled: adding(),
                    title: "A library's folder, chosen here so that the app may write to it, and can again next time",
                    onclick: move |_| {
                        let start = PathBuf::from(library().trim());
                        spawn(async move {
                            if let Some(chosen) = crate::folder_access::choose(
                                    "Choose a library's folder",
                                    Some(start.as_path()),
                                )
                                .await
                            {
                                library.set(chosen.display().to_string());
                                added.set(None);
                            }
                        });
                    },
                    "Choose Folder…"
                }
            }
            if crate::STORE {
                p { class: "muted small",
                    "For a library in a git repository, commit and push it yourself afterwards, so that its readers can "
                    "download the quiz."
                }
            } else {
                label { class: "add-to-library-push",
                    input {
                        r#type: "checkbox",
                        checked: publish(),
                        disabled: adding(),
                        onchange: move |event: FormEvent| publish.set(event.checked()),
                    }
                    "Commit and push it with git, so that the library's readers can download it"
                }
            }
            if replaces && !finished {
                p { class: "muted small", "The library already has a quiz in {name}: adding replaces it." }
            }
            match added() {
                Some(Ok((copy, true))) => rsx! {
                    p { class: "ok", "Added to {copy.display()}, committed and pushed." }
                },
                Some(Ok((copy, false))) => rsx! {
                    p { class: "ok", "Added to {copy.display()}, and library.json updated." }
                },
                Some(Err(error)) => rsx! {
                    p { class: "invalid", "{error}" }
                },
                None => rsx! {},
            }
            div { class: "add-to-library-actions",
                button { disabled: adding(), onclick: move |_| done.call(()),
                    if finished {
                        "Close"
                    } else {
                        "Cancel"
                    }
                }
                if !finished {
                    button {
                        class: "primary",
                        disabled: adding() || chosen.as_os_str().is_empty(),
                        onclick: add,
                        if adding() {
                            "Adding…"
                        } else {
                            "Add"
                        }
                    }
                }
            }
        }
    }
}
