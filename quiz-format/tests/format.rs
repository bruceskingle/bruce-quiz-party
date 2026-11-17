//! Tests of reading and writing quiz files, with the sample quiz and with files which are wrong in the ways a person
//! editing one by hand might make them.

use std::path::Path;

use bruce_quiz_format::{Curve, Quiz};

/// The sample quiz in the repository's quizzes folder.
fn sample_folder() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../quizzes/around-the-world"))
}

#[test]
fn reads_the_sample_quiz() {
    let quiz = Quiz::load(sample_folder()).unwrap();
    assert_eq!(quiz.title, "Around the World");
    assert_eq!(quiz.questions.len(), 5);
    assert_eq!(quiz.sources.len(), 5);

    let flag = &quiz.questions[2];
    assert_eq!(flag.answers.len(), 6);
    assert_eq!(flag.answers[flag.correct], "Japan");
    let image = flag.image.as_ref().unwrap();
    assert_eq!(image.file, "images/flag.svg");
    assert!(quiz.image_credit(&image.file).is_some());
    assert!(sample_folder().join(&image.file).is_file());

    let wall = quiz.question_settings(3);
    assert_eq!(wall.time_limit, 15);
    assert_eq!(wall.scoring.curve, Curve::Logarithmic);
    assert_eq!(wall.max_points, 1000);
    assert_eq!(quiz.question_settings(0).time_limit, 20);
}

#[test]
fn writes_what_it_reads() {
    let quiz = Quiz::load(sample_folder()).unwrap();
    let written = quiz.to_markdown();
    assert_eq!(Quiz::parse(&written).unwrap(), quiz);
    // Written again, it is the same text, so that saving a quiz unchanged does not change its file.
    assert_eq!(Quiz::parse(&written).unwrap().to_markdown(), written);
}

#[test]
fn defaults_the_settings() {
    let quiz = Quiz::parse("+++\ntitle = \"T\"\n+++\n## Q\n- [x] A\n- [ ] B\n").unwrap();
    assert_eq!(quiz.settings.time_limit, 20);
    assert_eq!(quiz.settings.max_points, 1000);
    assert_eq!(quiz.settings.scoring.floor, 0.5);
}

/// The line and message of the error a quiz file gives.
fn error_of(text: &str) -> (usize, String) {
    let error = Quiz::parse(text).unwrap_err();
    (error.line, error.message)
}

#[test]
fn reports_problems_with_their_lines() {
    let (line, message) = error_of("## Q\n");
    assert_eq!(line, 1);
    assert!(message.contains("+++"), "{message}");

    let (line, message) = error_of("+++\ntitle = \"T\"\ntime_limt = 5\n+++\n## Q\n- [x] A\n- [ ] B\n");
    assert_eq!(line, 3);
    assert!(message.contains("time_limt"), "{message}");

    let (line, message) = error_of("+++\ntitle = \"T\"\n+++\n\n## Q\n- [x] A\n");
    assert_eq!(line, 5);
    assert!(message.contains("1 answers"), "{message}");

    let (line, message) = error_of("+++\ntitle = \"T\"\n+++\n## Q\n- [x] A\n- [x] B\n");
    assert_eq!(line, 4);
    assert!(message.contains("2 answers marked"), "{message}");

    let (line, message) = error_of("+++\ntitle = \"T\"\n+++\n## Q\n- [x] A\n- [ ] B\nWhat?\n");
    assert_eq!(line, 7);
    assert!(message.contains("What?"), "{message}");

    let (line, message) = error_of("+++\ntitle = \"T\"\n+++\n## Q\n- [x] A\n- [ ] B\n```toml\nfloor = 2.0\n```\n");
    assert_eq!(line, 4);
    assert!(message.contains("floor"), "{message}");

    let (line, _) = error_of("+++\ntitle = \"T\"\n+++\n## Q\n- [x] A\n- [ ] B\n```toml\nmax_point = 2\n```\n");
    assert_eq!(line, 8);
}
