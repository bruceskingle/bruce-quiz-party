//! Where the generator saves its quizzes, remembered between runs: a quizzes folder, to play them at once, or a local
//! checkout of a library's git repository, to share them, optionally committed and pushed with the user's own git.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Where to save quizzes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Destination {
    /// A quizzes folder, e.g. the controller's, to play the quiz there.
    Folder { path: PathBuf },
    /// A local checkout of a library's git repository: the quiz is saved in it and the library's index rewritten, and
    /// if `publish`, they are committed and pushed.
    Library { checkout: PathBuf, publish: bool },
}

impl Destination {
    /// The controller's quizzes folder, in its data folder, which it plays from unless told otherwise.
    pub fn default_folder() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("bruce-quiz-party")
            .join("quizzes")
    }

    /// The folder quizzes are saved into.
    pub fn folder(&self) -> &Path {
        match self {
            Destination::Folder { path } => path,
            Destination::Library { checkout, .. } => checkout,
        }
    }
}

impl Default for Destination {
    fn default() -> Self {
        Destination::Folder {
            path: Self::default_folder(),
        }
    }
}

/// The settings file, in the generator's data folder.
fn file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bruce-quiz-generator")
        .join("settings.json")
}

/// The destination last chosen, or the controller's quizzes folder.
pub fn load() -> Destination {
    std::fs::read_to_string(file())
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Remembers the destination chosen.
pub fn save(destination: &Destination) -> anyhow::Result<()> {
    let path = file();
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let json = serde_json::to_string_pretty(destination)?;
    std::fs::write(&path, json).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_it_writes() {
        let library = Destination::Library {
            checkout: PathBuf::from("/Users/someone/git/quiz-library"),
            publish: true,
        };
        let json = serde_json::to_string(&library).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"library","checkout":"/Users/someone/git/quiz-library","publish":true}"#
        );
        assert_eq!(serde_json::from_str::<Destination>(&json).unwrap(), library);
    }
}
