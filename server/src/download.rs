//! Downloading quizzes from a library: a folder of quiz folders with an index, `library.json`, at a web address, e.g. a
//! GitHub repository's raw files, or in a folder. What is downloaded is not trusted: only the files the index lists,
//! which must be safe paths inside the quiz's folder and a quiz file or images, and only a quiz which reads; it
//! replaces a quiz of the same folder only then.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use dioxus::prelude::*;
use quiz_format::{
    Quiz,
    library::{LIBRARY_INDEX, LibraryIndex, LibraryQuiz},
};

/// Bruce's library, which the controller lists unless told otherwise.
pub const DEFAULT_LIBRARY: &str = "https://raw.githubusercontent.com/bruceskingle/bruce-quiz-library/main";

/// The controller's name to the library's server, with where to find out more.
const USER_AGENT: &str = "BrucesQuizParty/0.1 (https://github.com/bruceskingle/bruce-quiz-party)";

/// The most a file may be, so that a library cannot fill the disk.
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Where a library is: a web address, or a folder.
#[derive(Debug, Clone, PartialEq)]
pub enum Library {
    Web(String),
    Folder(PathBuf),
}

impl Library {
    /// A library from its address as typed: one starting `http://` or `https://` is on the web, anything else is a
    /// folder.
    pub fn new(address: &str) -> Library {
        let address = address.trim();
        if address.starts_with("https://") || address.starts_with("http://") {
            Library::Web(address.trim_end_matches('/').to_string())
        } else {
            Library::Folder(PathBuf::from(address))
        }
    }

    /// The library's index.
    pub fn index(&self) -> anyhow::Result<LibraryIndex> {
        let bytes = self.read(LIBRARY_INDEX)?;
        LibraryIndex::parse(&String::from_utf8_lossy(&bytes)).context("the library's index is not as expected")
    }

    /// Downloads a quiz into the quizzes folder, in a folder of its name, replacing any quiz there, returning the
    /// folder. A quiz which does not read is not kept.
    pub fn download(&self, quiz: &LibraryQuiz, quizzes: &Path) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(
            quiz_format::library::is_safe_name(&quiz.folder),
            "the library's quiz has an unsafe name"
        );
        let target = quizzes.join(&quiz.folder);
        // Into a folder of its own first, so that a quiz half downloaded never replaces a whole one.
        let staging = quizzes.join(format!(".download-{}", quiz.folder));
        let _ = std::fs::remove_dir_all(&staging);
        let result = (|| {
            for file in &quiz.files {
                anyhow::ensure!(
                    quiz_format::library::is_safe_file(file),
                    "the library lists an unsafe file {file}"
                );
                let bytes = self.read(&format!("{}/{file}", quiz.folder))?;
                let path = staging.join(file);
                if let Some(folder) = path.parent() {
                    std::fs::create_dir_all(folder)?;
                }
                std::fs::write(&path, bytes).with_context(|| format!("cannot write {}", path.display()))?;
            }
            Quiz::load(&staging)
                .map_err(|error| anyhow::Error::from(error).context("the downloaded quiz does not read"))?;
            let _ = std::fs::remove_dir_all(&target);
            std::fs::rename(&staging, &target).with_context(|| format!("cannot make {}", target.display()))
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        result.map(|_| target)
    }

    /// A file of the library, by its path from the library's top.
    fn read(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        match self {
            Library::Folder(folder) => {
                let path = folder.join(path);
                let size = std::fs::metadata(&path)
                    .with_context(|| format!("cannot read {}", path.display()))?
                    .len();
                anyhow::ensure!(size <= MAX_FILE_BYTES, "{} is too big", path.display());
                std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))
            }
            Library::Web(base) => {
                let url = format!("{base}/{}", path.split('/').map(encode).collect::<Vec<_>>().join("/"));
                let mut response = agent()
                    .get(&url)
                    .call()
                    .with_context(|| format!("cannot download {url}"))?;
                response
                    .body_mut()
                    .with_config()
                    .limit(MAX_FILE_BYTES)
                    .read_to_vec()
                    .with_context(|| format!("cannot download {url}"))
            }
        }
    }
}

/// A part of a path, encoded for a URL.
fn encode(part: &str) -> String {
    part.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent(USER_AGENT)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .new_agent()
}

/// The library address the host last chose, kept in the controller's data folder, or Bruce's library.
pub fn saved_library() -> String {
    std::fs::read_to_string(library_file())
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| DEFAULT_LIBRARY.to_string())
}

/// Remembers the library address chosen.
pub fn save_library(address: &str) {
    let path = library_file();
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(path, address.trim());
}

fn library_file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bruce-quiz-party")
        .join("library.txt")
}

/// An online library: its address, which the host may change, and its quizzes, each to download, or download again
/// if the host has it already.
#[component]
pub fn LibraryBrowser(quizzes: PathBuf, downloaded: EventHandler<PathBuf>, done: EventHandler<()>) -> Element {
    let mut address = use_signal(saved_library);
    // The library's index, once read, or why it could not be.
    let mut index = use_signal(|| None::<Result<LibraryIndex, String>>);
    let mut looking = use_signal(|| false);
    // What happened to each quiz downloaded, by folder: done, or why not.
    let mut results = use_signal(std::collections::HashMap::<String, Result<(), String>>::new);
    let mut busy = use_signal(|| None::<String>);

    let mut look = move || {
        let library = Library::new(&address());
        save_library(&address());
        looking.set(true);
        spawn(async move {
            let read = tokio::task::spawn_blocking(move || library.index())
                .await
                .map_err(anyhow::Error::from)
                .and_then(|read| read);
            index.set(Some(read.map_err(|error| format!("{error:#}"))));
            looking.set(false);
        });
    };
    // The library is read as the page opens.
    use_hook(look);

    let get = {
        let quizzes = quizzes.clone();
        move |quiz: LibraryQuiz| {
            let library = Library::new(&address());
            let quizzes = quizzes.clone();
            let name = quiz.folder.clone();
            busy.set(Some(name.clone()));
            spawn(async move {
                let saved = tokio::task::spawn_blocking(move || library.download(&quiz, &quizzes))
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|saved| saved);
                match saved {
                    Ok(folder) => {
                        results.write().insert(name, Ok(()));
                        downloaded.call(folder);
                    }
                    Err(error) => {
                        results.write().insert(name, Err(format!("{error:#}")));
                    }
                }
                busy.set(None);
            });
        }
    };

    rsx! {
        div { class: "editor",
            div { class: "editor-bar",
                h1 { "Get Quizzes from a Library" }
                div { class: "spacer" }
                button { onclick: move |_| done.call(()), "Done" }
            }
            p { class: "muted",
                "A library is a folder of quizzes with an index, library.json, on the web or in a folder: Bruce's is "
                "the default. Quizzes drawn from Wikipedia are under CC BY-SA, with their credits in them."
            }
            div { class: "library-address",
                input {
                    value: "{address}",
                    oninput: move |event| address.set(event.value()),
                }
                button { disabled: looking(), onclick: move |_| look(), "Look" }
                button {
                    onclick: move |_| {
                        address.set(DEFAULT_LIBRARY.to_string());
                        look();
                    },
                    "Bruce's Library"
                }
            }
            match index() {
                None => rsx! {
                    p { class: "muted", "Reading the library…" }
                },
                Some(Err(error)) => rsx! {
                    p { class: "invalid", "The library cannot be read: {error}" }
                },
                Some(Ok(index)) if index.quizzes.is_empty() => rsx! {
                    p { class: "muted", "The library has no quizzes." }
                },
                Some(Ok(index)) => rsx! {
                    for quiz in index.quizzes {
                        div { key: "{quiz.folder}", class: "question-card library-quiz",
                            div { class: "library-quiz-text",
                                h3 { "{quiz.title}" }
                                if let Some(description) = &quiz.description {
                                    p { class: "muted", "{description}" }
                                }
                                p { class: "muted small",
                                    "{quiz.questions} questions"
                                    if let Some(licence) = &quiz.licence {
                                        ", {licence}"
                                    }
                                }
                                match results.read().get(&quiz.folder) {
                                    Some(Ok(())) => rsx! {
                                        p { class: "ok small", "Downloaded: it is on the Quizzes page." }
                                    },
                                    Some(Err(error)) => rsx! {
                                        p { class: "invalid small", "{error}" }
                                    },
                                    None => rsx! {},
                                }
                            }
                            button {
                                class: "primary",
                                disabled: busy().is_some(),
                                onclick: {
                                    let mut get = get.clone();
                                    let quiz = quiz.clone();
                                    move |_| get(quiz.clone())
                                },
                                if busy() == Some(quiz.folder.clone()) {
                                    "Downloading…"
                                } else if quizzes.join(&quiz.folder).exists() {
                                    "Download Again"
                                } else {
                                    "Download"
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("quiz-download-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tells_web_libraries_from_folders() {
        assert_eq!(
            Library::new(" https://example.org/library/ "),
            Library::Web("https://example.org/library".into())
        );
        assert_eq!(
            Library::new("/Users/someone/library"),
            Library::Folder("/Users/someone/library".into())
        );
        assert_eq!(encode("my flag.png"), "my%20flag.png");
    }

    #[test]
    fn downloads_a_quiz_from_a_library() {
        let library = TempFolder::new("library");
        let quizzes = TempFolder::new("quizzes");
        Quiz::parse("+++\ntitle = \"Flags\"\n+++\n## Whose?\n![A flag](images/flag.png)\n- [x] Japan\n- [ ] Laos\n")
            .unwrap()
            .save(&library.0.join("flags"))
            .unwrap();
        std::fs::create_dir_all(library.0.join("flags/images")).unwrap();
        std::fs::write(library.0.join("flags/images/flag.png"), "png").unwrap();
        quiz_format::library::write_index(&library.0).unwrap();

        let source = Library::Folder(library.0.clone());
        let index = source.index().unwrap();
        assert_eq!(index.quizzes.len(), 1);
        let folder = source.download(&index.quizzes[0], &quizzes.0).unwrap();
        assert_eq!(folder, quizzes.0.join("flags"));
        assert_eq!(Quiz::load(&folder).unwrap().title, "Flags");
        assert!(folder.join("images/flag.png").is_file());

        // A quiz which no longer reads does not replace the one there.
        std::fs::write(library.0.join("flags/quiz.md"), "broken").unwrap();
        assert!(source.download(&index.quizzes[0], &quizzes.0).is_err());
        assert_eq!(Quiz::load(&folder).unwrap().title, "Flags");
        assert!(!quizzes.0.join(".download-flags").exists());
    }
}
