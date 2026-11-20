//! A library of quizzes to share: a folder of quiz folders with an index, `library.json`, listing each quiz and its
//! files, so that it can be served from anywhere, e.g. a GitHub repository's raw files or a shared folder, and the
//! controller can list it and download quizzes from it. The index is rewritten whenever a quiz is added to a library,
//! from the Quizzes page's Add to Library or by the generator.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{QUIZ_FILE, Quiz};

/// The library's index, in its top folder.
pub const LIBRARY_INDEX: &str = "library.json";

/// The kinds of file a quiz in a library may have: its quiz file and its images, nothing which could run.
const EXTENSIONS: [&str; 7] = ["md", "svg", "png", "jpg", "jpeg", "gif", "webp"];

/// The index of a library.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryIndex {
    /// The quizzes, by folder name.
    pub quizzes: Vec<LibraryQuiz>,
}

/// A quiz in a library, as listed in its index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryQuiz {
    /// Its folder in the library, e.g. `around-the-world`.
    pub folder: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How many questions it has.
    pub questions: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// Its files, as paths from its folder, `quiz.md` first.
    pub files: Vec<String>,
}

impl LibraryIndex {
    /// Reads an index, keeping only the quizzes whose folder and files are safe to download: a downloaded index is
    /// not trusted to name only files inside the quiz's folder.
    pub fn parse(json: &str) -> serde_json::Result<LibraryIndex> {
        let mut index: LibraryIndex = serde_json::from_str(json)?;
        index.quizzes.retain(|quiz| {
            is_safe_name(&quiz.folder)
                && quiz.files.first().map(String::as_str) == Some(QUIZ_FILE)
                && quiz.files.iter().all(|file| is_safe_file(file))
        });
        Ok(index)
    }
}

/// Indexes a library's folder: every quiz folder in it which reads as a quiz, with its quiz file and images.
pub fn index_library(library: &Path) -> std::io::Result<LibraryIndex> {
    let mut quizzes = Vec::new();
    for (folder, quiz) in crate::load_library(library)? {
        let Ok(quiz) = quiz else { continue };
        let Some(name) = folder.file_name().map(|name| name.to_string_lossy().into_owned()) else {
            continue;
        };
        if !is_safe_name(&name) {
            continue;
        }
        quizzes.push(entry(&folder, &name, &quiz));
    }
    Ok(LibraryIndex { quizzes })
}

/// Writes a library's index, from its folders.
pub fn write_index(library: &Path) -> std::io::Result<()> {
    let index = index_library(library)?;
    let json = serde_json::to_string_pretty(&index).map_err(std::io::Error::other)? + "\n";
    std::fs::write(library.join(LIBRARY_INDEX), json)
}

/// Copies a quiz into a library, in a folder of the same name, replacing any quiz there, and rewrites the library's
/// index, making it if the library had none, returning the quiz's folder in the library. Only the files the index
/// lists are copied, the quiz file and the images it uses, so that nothing else in the quiz's folder is shared by
/// accident, and the copy is made beside the old one first, so that a copy which fails never replaces a whole one.
pub fn add_quiz(library: &Path, folder: &Path, quiz: &Quiz) -> std::io::Result<PathBuf> {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| is_safe_name(name))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} is not a name a library's quiz may have", folder.display()),
            )
        })?;
    let target = library.join(&name);
    let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b);
    if !same(&target, folder) {
        let staging = library.join(format!(".adding-{name}"));
        let _ = std::fs::remove_dir_all(&staging);
        let copied = (|| {
            for file in entry(folder, &name, quiz).files {
                let to = staging.join(&file);
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(folder.join(&file), to)?;
            }
            if target.exists() {
                std::fs::remove_dir_all(&target)?;
            }
            std::fs::rename(&staging, &target)
        })();
        if copied.is_err() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        copied?;
    }
    write_index(library)?;
    Ok(target)
}

/// A quiz's entry in the index: its files are its quiz file and the images it uses.
fn entry(folder: &Path, name: &str, quiz: &Quiz) -> LibraryQuiz {
    let mut files = vec![QUIZ_FILE.to_string()];
    let mut images: Vec<String> = quiz
        .questions
        .iter()
        .filter_map(|question| question.image.as_ref().map(|image| image.file.clone()))
        .filter(|file| is_safe_file(file) && folder.join(file).is_file())
        .collect();
    images.sort();
    images.dedup();
    files.extend(images);
    LibraryQuiz {
        folder: name.to_string(),
        title: quiz.title.clone(),
        description: quiz.description.clone(),
        questions: quiz.questions.len(),
        licence: quiz.licence.clone(),
        files,
    }
}

/// Whether a folder's name is safe to make: letters, digits, hyphens, underscores and dots, not starting with a dot.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.len() <= 100
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Whether a file's path in a quiz's folder is safe to write: relative, made of safe names, a few folders deep at most,
/// and a quiz file or an image.
pub fn is_safe_file(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    parts.len() <= 3
        && parts.iter().all(|part| is_safe_name(part))
        && extension.is_some_and(|extension| EXTENSIONS.contains(&extension.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_a_library() {
        let library = std::env::temp_dir().join(format!("quiz-library-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&library);
        let quiz = Quiz::parse(
            "+++\ntitle = \"Flags\"\nlicence = \"CC BY-SA 4.0\"\n+++\n\
             ## Whose?\n![A flag](images/flag.png)\n- [x] Japan\n- [ ] Laos\n",
        )
        .unwrap();
        quiz.save(&library.join("flags")).unwrap();
        std::fs::create_dir_all(library.join("flags/images")).unwrap();
        std::fs::write(library.join("flags/images/flag.png"), "png").unwrap();
        std::fs::write(library.join("flags/notes.txt"), "not listed").unwrap();
        std::fs::create_dir_all(library.join("broken")).unwrap();
        std::fs::write(library.join("broken/quiz.md"), "not a quiz").unwrap();

        write_index(&library).unwrap();
        let index = LibraryIndex::parse(&std::fs::read_to_string(library.join(LIBRARY_INDEX)).unwrap()).unwrap();
        assert_eq!(
            index.quizzes,
            [LibraryQuiz {
                folder: "flags".into(),
                title: "Flags".into(),
                description: None,
                questions: 1,
                licence: Some("CC BY-SA 4.0".into()),
                files: vec!["quiz.md".into(), "images/flag.png".into()],
            }]
        );
        let _ = std::fs::remove_dir_all(&library);
    }

    #[test]
    fn leaves_out_quizzes_with_unsafe_files() {
        let json = r#"{"quizzes": [
            {"folder": "good", "title": "G", "questions": 1, "files": ["quiz.md", "images/a.png"]},
            {"folder": "../escape", "title": "E", "questions": 1, "files": ["quiz.md"]},
            {"folder": "up", "title": "U", "questions": 1, "files": ["quiz.md", "../../x.png"]},
            {"folder": "script", "title": "S", "questions": 1, "files": ["quiz.md", "run.sh"]},
            {"folder": "absolute", "title": "A", "questions": 1, "files": ["quiz.md", "/etc/passwd.md"]},
            {"folder": "noquiz", "title": "N", "questions": 1, "files": ["images/a.png"]}
        ]}"#;
        let folders: Vec<String> = LibraryIndex::parse(json)
            .unwrap()
            .quizzes
            .into_iter()
            .map(|quiz| quiz.folder)
            .collect();
        assert_eq!(folders, ["good"]);
    }

    #[test]
    fn adds_a_quiz_to_a_library() {
        let root = std::env::temp_dir().join(format!("quiz-library-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = root.join("library");
        std::fs::create_dir_all(&library).unwrap();
        let quiz = Quiz::parse(
            "+++\ntitle = \"Flags\"\n+++\n## Whose?\n![A flag](images/flag.png)\n- [x] Japan\n- [ ] Laos\n",
        )
        .unwrap();
        let folder = root.join("quizzes/flags");
        quiz.save(&folder).unwrap();
        std::fs::create_dir_all(folder.join("images")).unwrap();
        std::fs::write(folder.join("images/flag.png"), "png").unwrap();
        std::fs::write(folder.join("notes.txt"), "not shared").unwrap();
        // An older copy, with a file the new one does not have, which goes.
        std::fs::create_dir_all(library.join("flags")).unwrap();
        std::fs::write(library.join("flags/old.png"), "old").unwrap();

        let added = add_quiz(&library, &folder, &quiz).unwrap();
        assert_eq!(added, library.join("flags"));
        assert!(added.join("quiz.md").is_file() && added.join("images/flag.png").is_file());
        assert!(!added.join("notes.txt").exists() && !added.join("old.png").exists());
        let index = LibraryIndex::parse(&std::fs::read_to_string(library.join(LIBRARY_INDEX)).unwrap()).unwrap();
        assert_eq!(index.quizzes.len(), 1);
        assert_eq!(index.quizzes[0].files, ["quiz.md", "images/flag.png"]);

        // Adding a library's own quiz to it only rewrites the index.
        add_quiz(&library, &added, &quiz).unwrap();
        assert!(added.join("quiz.md").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
