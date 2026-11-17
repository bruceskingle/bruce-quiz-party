//! A library of quizzes to share: a folder of quiz folders with an index, `library.json`, listing each quiz and its
//! files, so that it can be served from anywhere, e.g. a GitHub repository's raw files or a shared folder, and the
//! controller can list it and download quizzes from it. The generator writes the index when it saves a quiz into a
//! library.

use std::path::Path;

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
}
