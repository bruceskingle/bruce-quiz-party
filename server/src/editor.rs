//! The quiz editor: a quiz's title, description and licence, its scoring with a graph of the points a correct answer
//! scores as time passes, its questions with their answers, images, sources and own settings, and its credits. It
//! saves the quiz as a person would write it, and only a quiz which reads back the same.

use std::path::{Path, PathBuf};

use dioxus::prelude::*;
use quiz_format::{
    Curve, Image, ImageCredit, MAX_ANSWERS, MIN_ANSWERS, Question, Quiz, ScoringSettings, Settings, SettingsOverride,
    Source,
};

use crate::{answer_shape::AnswerShape, library::image_url};

/// The image files a quiz's folder has, as paths from the folder, e.g. `images/flag.svg`, in its folder and the
/// folders inside it, for choosing a question's image.
pub fn images_in(folder: &Path) -> Vec<String> {
    let mut images = Vec::new();
    let mut folders = vec![(folder.to_path_buf(), 0)];
    while let Some((current, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth < 2 {
                folders.push((path, depth + 1));
            } else if is_image(&path)
                && let Ok(relative) = path.strip_prefix(folder)
            {
                images.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    images.sort();
    images
}

fn is_image(path: &Path) -> bool {
    let extension = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    matches!(
        extension.as_deref(),
        Some("svg" | "png" | "jpg" | "jpeg" | "gif" | "webp")
    )
}

/// Saves a quiz, tidied, in its folder, or for a new quiz in a new folder named after its title in the quizzes folder,
/// returning the folder and the quiz as saved; or what is wrong with it, in words.
pub fn save(quizzes: &Path, folder: Option<&Path>, quiz: &Quiz) -> Result<(PathBuf, Quiz), Vec<String>> {
    let quiz = quiz.tidied();
    let problems = quiz.problems();
    if !problems.is_empty() {
        return Err(problems);
    }
    // Only what reads back as it is, so that saving never loses or changes anything.
    let text = quiz.to_markdown();
    match Quiz::parse(&text) {
        Ok(read) if read == quiz => {}
        Ok(_) => return Err(vec!["The quiz would not read back as it is: please tell Bruce.".into()]),
        Err(error) => return Err(vec![format!("The quiz would not read back: {error}")]),
    }
    let folder = match folder {
        Some(folder) => folder.to_path_buf(),
        None => quiz_format::new_folder(quizzes, &quiz.title),
    };
    quiz.save(&folder).map_err(|error| {
        vec![format!(
            "{:#}",
            anyhow::Error::from(error).context(format!("cannot save {}", folder.display()))
        )]
    })?;
    Ok((folder, quiz))
}

/// A new quiz to start from: one question with two answers.
pub fn new_quiz() -> Quiz {
    Quiz {
        title: "New Quiz".into(),
        description: None,
        licence: None,
        settings: Settings::default(),
        sources: Vec::new(),
        images: Vec::new(),
        questions: vec![new_question()],
    }
}

fn new_question() -> Question {
    Question {
        text: String::new(),
        image: None,
        answers: vec![String::new(), String::new()],
        correct: 0,
        source: None,
        settings: SettingsOverride::default(),
    }
}

/// The settings a question changes from its quiz's, only those it changes, so that the file says no more than a
/// person would write.
fn changes(quiz: &Settings, question: &Settings) -> SettingsOverride {
    let differs = |a: f64, b: f64| (a - b).abs() > 1e-9;
    SettingsOverride {
        time_limit: (question.time_limit != quiz.time_limit).then_some(question.time_limit),
        max_points: (question.max_points != quiz.max_points).then_some(question.max_points),
        scoring: (question.scoring.curve != quiz.scoring.curve).then_some(question.scoring.curve),
        floor: differs(question.scoring.floor, quiz.scoring.floor).then_some(question.scoring.floor),
        steepness: differs(question.scoring.steepness, quiz.scoring.steepness).then_some(question.scoring.steepness),
    }
}

/// The editor, for a quiz in a folder, or a new one without; `done` is called with the folder it was saved in, or
/// None if the changes were discarded.
#[component]
pub fn QuizEditor(
    folder: Option<PathBuf>,
    quiz: Quiz,
    quizzes: PathBuf,
    done: EventHandler<Option<PathBuf>>,
) -> Element {
    let original = quiz.clone();
    let mut draft = use_signal(move || quiz);
    let mut problems = use_signal(Vec::<String>::new);
    let mut discarding = use_signal(|| false);
    // Counts the times the host asked to look for images again, e.g. after adding some in the Finder.
    let mut rescan = use_signal(|| 0u32);
    let _ = rescan();
    let images = folder.as_deref().map(images_in).unwrap_or_default();
    let changed = *draft.read() != original;

    let save_folder = folder.clone();
    let save = move |_| match save(&quizzes, save_folder.as_deref(), &draft.read()) {
        Ok((folder, _)) => done.call(Some(folder)),
        Err(found) => problems.set(found),
    };
    let questions = draft.read().questions.len();

    rsx! {
        div { class: "editor",
            div { class: "editor-bar",
                h1 { if folder.is_some() { "Edit the Quiz" } else { "New Quiz" } }
                div { class: "spacer" }
                if discarding() {
                    span { "Discard your changes?" }
                    button { class: "danger", onclick: move |_| done.call(None), "Discard" }
                    button { onclick: move |_| discarding.set(false), "Keep Editing" }
                } else {
                    button {
                        onclick: move |_| if changed { discarding.set(true) } else { done.call(None) },
                        "Cancel"
                    }
                    button { class: "primary", disabled: !changed && folder.is_some(), onclick: save, "Save" }
                }
            }
            if !problems.read().is_empty() {
                div { class: "problems",
                    p { "The quiz cannot be saved yet:" }
                    ul {
                        for problem in problems.read().iter() {
                            li { "{problem}" }
                        }
                    }
                }
            }
            section { class: "editor-section",
                h2 { "The Quiz" }
                Field { label: "Title",
                    input {
                        value: "{draft.read().title}",
                        oninput: move |event| draft.write().title = event.value(),
                    }
                }
                Field { label: "Description",
                    textarea {
                        rows: "2",
                        value: "{draft.read().description.clone().unwrap_or_default()}",
                        oninput: move |event| draft.write().description = Some(event.value()),
                    }
                }
                Field { label: "Licence",
                    input {
                        placeholder: "e.g. CC BY-SA 4.0, for a quiz drawn from Wikipedia",
                        value: "{draft.read().licence.clone().unwrap_or_default()}",
                        oninput: move |event| draft.write().licence = Some(event.value()),
                    }
                }
            }
            section { class: "editor-section",
                h2 { "Timing and Scoring" }
                p { class: "muted", "For every question, unless a question has its own." }
                SettingsFields {
                    settings: draft.read().settings.clone(),
                    onchange: move |settings| draft.write().settings = settings,
                }
            }
            section { class: "editor-section",
                h2 { "Questions ({questions})" }
                if let Some(folder) = &folder {
                    p { class: "muted",
                        "A question's image is chosen from the images in the quiz's folder. "
                        button {
                            onclick: {
                                let folder = folder.clone();
                                move |_| {
                                    let _ = std::process::Command::new("open").arg(&folder).spawn();
                                }
                            },
                            "Show in Finder"
                        }
                        " "
                        button { onclick: move |_| rescan += 1, "Look for Images Again" }
                    }
                } else {
                    p { class: "muted",
                        "Save the quiz first, then put its images in its folder to choose them for questions."
                    }
                }
                for index in 0..questions {
                    QuestionEditor {
                        key: "{index}",
                        draft,
                        index,
                        folder: folder.clone(),
                        images: images.clone(),
                    }
                }
                button {
                    onclick: move |_| draft.write().questions.push(new_question()),
                    "Add a Question"
                }
            }
            CreditsEditor { draft, images }
        }
    }
}

/// A labelled field.
#[component]
fn Field(label: String, children: Element) -> Element {
    rsx! {
        label { class: "field",
            span { class: "field-label", "{label}" }
            {children}
        }
    }
}

/// A number typed into a field, or None if it is not one.
fn number<T: std::str::FromStr>(event: &FormEvent) -> Option<T> {
    event.value().trim().parse().ok()
}

/// Timing and scoring: the time limit, the most points, the curve, its floor and steepness, and the graph.
#[component]
fn SettingsFields(settings: Settings, onchange: EventHandler<Settings>) -> Element {
    let scoring = settings.scoring;
    rsx! {
        div { class: "settings-fields",
            div { class: "settings-inputs",
                Field { label: "Seconds to answer",
                    input {
                        r#type: "number",
                        min: "1",
                        value: "{settings.time_limit}",
                        onchange: {
                            let settings = settings.clone();
                            move |event: FormEvent| {
                                if let Some(seconds) = number::<u32>(&event).filter(|seconds| *seconds > 0) {
                                    onchange.call(Settings { time_limit: seconds, ..settings.clone() });
                                }
                            }
                        },
                    }
                }
                Field { label: "Points for an answer at once",
                    input {
                        r#type: "number",
                        min: "1",
                        value: "{settings.max_points}",
                        onchange: {
                            let settings = settings.clone();
                            move |event: FormEvent| {
                                if let Some(points) = number::<u32>(&event).filter(|points| *points > 0) {
                                    onchange.call(Settings { max_points: points, ..settings.clone() });
                                }
                            }
                        },
                    }
                }
                Field { label: "The points fall",
                    select {
                        value: if scoring.curve == Curve::Linear { "linear" } else { "logarithmic" },
                        onchange: {
                            let settings = settings.clone();
                            move |event: FormEvent| {
                                let curve = if event.value() == "linear" { Curve::Linear } else { Curve::Logarithmic };
                                onchange.call(Settings {
                                    scoring: ScoringSettings { curve, ..settings.scoring },
                                    ..settings.clone()
                                });
                            }
                        },
                        option { value: "linear", "Evenly (linear)" }
                        option { value: "logarithmic", "Fast at first, then slowly (logarithmic)" }
                    }
                }
                Field { label: "Points at the time limit, % of the most",
                    input {
                        r#type: "number",
                        min: "0",
                        max: "100",
                        value: "{(scoring.floor * 100.0).round()}",
                        onchange: {
                            let settings = settings.clone();
                            move |event: FormEvent| {
                                if let Some(percent) = number::<f64>(&event).filter(|p| (0.0..=100.0).contains(p)) {
                                    onchange.call(Settings {
                                        scoring: ScoringSettings { floor: percent / 100.0, ..settings.scoring },
                                        ..settings.clone()
                                    });
                                }
                            }
                        },
                    }
                }
                if scoring.curve == Curve::Logarithmic {
                    Field { label: "How fast at first (steepness)",
                        input {
                            r#type: "number",
                            min: "0.1",
                            step: "0.5",
                            value: "{scoring.steepness}",
                            onchange: {
                                let settings = settings.clone();
                                move |event: FormEvent| {
                                    if let Some(steepness) = number::<f64>(&event).filter(|s| *s > 0.0) {
                                        onchange.call(Settings {
                                            scoring: ScoringSettings { steepness, ..settings.scoring },
                                            ..settings.clone()
                                        });
                                    }
                                }
                            },
                        }
                    }
                }
            }
            CurveGraph { settings: settings.clone() }
        }
    }
}

/// The points a correct answer scores against how long it took, from at once to the time limit, with a few read out.
#[component]
fn CurveGraph(settings: Settings) -> Element {
    const WIDTH: f64 = 320.0;
    const HEIGHT: f64 = 150.0;
    const LEFT: f64 = 44.0;
    const BOTTOM: f64 = 22.0;
    let plot_width = WIDTH - LEFT - 8.0;
    let plot_height = HEIGHT - BOTTOM - 8.0;
    let samples = settings.scoring.preview(61);
    let last = (samples.len() - 1) as f64;
    let points: Vec<String> = samples
        .iter()
        .enumerate()
        .map(|(i, fraction)| {
            let x = LEFT + plot_width * i as f64 / last;
            let y = 8.0 + plot_height * (1.0 - fraction);
            format!("{x:.1},{y:.1}")
        })
        .collect();
    let limit = settings.time_limit;
    let most = settings.max_points;
    let at = |seconds: u32| settings.scoring.points(most, limit, u64::from(seconds) * 1000);
    let marks: Vec<u32> = [1, 2, 3].map(|quarter| (limit * quarter).div_ceil(4)).into();
    rsx! {
        div { class: "curve",
            svg {
                class: "curve-graph",
                view_box: "0 0 {WIDTH} {HEIGHT}",
                role: "img",
                "aria-label": "Points scored against seconds taken",
                line { x1: "{LEFT}", y1: "8", x2: "{LEFT}", y2: "{8.0 + plot_height}", class: "axis" }
                line {
                    x1: "{LEFT}",
                    y1: "{8.0 + plot_height}",
                    x2: "{LEFT + plot_width}",
                    y2: "{8.0 + plot_height}",
                    class: "axis",
                }
                polyline { points: "{points.join(\" \")}", class: "curve-line" }
                text { x: "{LEFT - 6.0}", y: "14", class: "curve-label end", "{most}" }
                text { x: "{LEFT - 6.0}", y: "{8.0 + plot_height}", class: "curve-label end", "0" }
                text { x: "{LEFT}", y: "{HEIGHT - 6.0}", class: "curve-label", "0 s" }
                text { x: "{LEFT + plot_width}", y: "{HEIGHT - 6.0}", class: "curve-label end", "{limit} s" }
            }
            p { class: "muted small",
                "A correct answer scores {most} at once"
                for seconds in marks {
                    ", {at(seconds)} after {seconds} s"
                }
                ", and {at(limit)} at the time limit."
            }
        }
    }
}

/// One question: its text, image, answers with the correct one chosen, source, own settings, and moving or removing it.
#[component]
fn QuestionEditor(draft: Signal<Quiz>, index: usize, folder: Option<PathBuf>, images: Vec<String>) -> Element {
    let Some(question) = draft.read().questions.get(index).cloned() else {
        return rsx! {};
    };
    let quiz_settings = draft.read().settings.clone();
    let own_settings = question.settings.apply(&quiz_settings);
    let count = draft.read().questions.len();
    let mut own_open = use_signal(|| false);
    let has_own = !question.settings.is_empty() || own_open();
    let name = format!("question-{index}-correct");

    rsx! {
        div { class: "question-card editor-question",
            div { class: "question-heading",
                h3 { "Question {index + 1}" }
                div { class: "spacer" }
                button {
                    disabled: index == 0,
                    title: "Move up",
                    onclick: move |_| draft.write().questions.swap(index, index - 1),
                    "↑"
                }
                button {
                    disabled: index + 1 >= count,
                    title: "Move down",
                    onclick: move |_| draft.write().questions.swap(index, index + 1),
                    "↓"
                }
                button {
                    disabled: count == 1,
                    onclick: move |_| {
                        draft.write().questions.remove(index);
                    },
                    "Remove"
                }
            }
            Field { label: "Question",
                input {
                    value: "{question.text}",
                    placeholder: "What is the capital of Australia?",
                    oninput: move |event| draft.write().questions[index].text = event.value(),
                }
            }
            div { class: "answers-editor",
                span { class: "field-label", "Answers: choose the correct one" }
                for (position , answer) in question.answers.iter().enumerate() {
                    div { key: "{position}", class: "answer-editor",
                        input {
                            r#type: "radio",
                            name: "{name}",
                            title: "The correct answer",
                            checked: position == question.correct,
                            onchange: move |_| draft.write().questions[index].correct = position,
                        }
                        span { class: "answer-swatch answer-{position}", AnswerShape { position } }
                        input {
                            value: "{answer}",
                            oninput: move |event| draft.write().questions[index].answers[position] = event.value(),
                        }
                        button {
                            disabled: question.answers.len() <= MIN_ANSWERS,
                            title: "Remove this answer",
                            onclick: move |_| {
                                let mut quiz = draft.write();
                                let question = &mut quiz.questions[index];
                                question.answers.remove(position);
                                if question.correct > position || question.correct >= question.answers.len() {
                                    question.correct = question.correct.saturating_sub(1);
                                }
                            },
                            "×"
                        }
                    }
                }
                if question.answers.len() < MAX_ANSWERS {
                    button {
                        onclick: move |_| draft.write().questions[index].answers.push(String::new()),
                        "Add an Answer"
                    }
                }
            }
            div { class: "image-editor",
                Field { label: "Image",
                    select {
                        disabled: folder.is_none(),
                        value: "{question.image.as_ref().map(|image| image.file.clone()).unwrap_or_default()}",
                        onchange: move |event: FormEvent| {
                            let file = event.value();
                            let mut quiz = draft.write();
                            let question = &mut quiz.questions[index];
                            question.image = if file.is_empty() {
                                None
                            } else {
                                let alt = question.image.take().map(|image| image.alt).unwrap_or_default();
                                Some(Image { file, alt })
                            };
                        },
                        option { value: "", "None" }
                        for file in images.iter() {
                            option { key: "{file}", value: "{file}", "{file}" }
                        }
                    }
                }
                if let Some(image) = &question.image {
                    Field { label: "Describe the image, for those who cannot see it",
                        input {
                            value: "{image.alt}",
                            oninput: move |event| {
                                if let Some(image) = &mut draft.write().questions[index].image {
                                    image.alt = event.value();
                                }
                            },
                        }
                    }
                    if let Some(folder) = &folder {
                        img { class: "question-image", src: image_url(folder, &image.file), alt: "{image.alt}" }
                    }
                    if draft.read().image_credit(&image.file).is_none() {
                        p { class: "muted small",
                            "This image has no credit below. One from Wikimedia Commons needs its author and licence."
                        }
                    }
                }
            }
            Field { label: "Source, a link to read more",
                input {
                    value: "{question.source.clone().unwrap_or_default()}",
                    placeholder: "https://en.wikipedia.org/wiki/…",
                    oninput: move |event| draft.write().questions[index].source = Some(event.value()),
                }
            }
            label { class: "own-settings",
                input {
                    r#type: "checkbox",
                    checked: has_own,
                    onchange: move |event: FormEvent| {
                        own_open.set(event.checked());
                        if !event.checked() {
                            draft.write().questions[index].settings = SettingsOverride::default();
                        }
                    },
                }
                "Its own timing or scoring"
            }
            if has_own {
                SettingsFields {
                    settings: own_settings,
                    onchange: move |settings: Settings| {
                        let mut quiz = draft.write();
                        let changes = changes(&quiz.settings, &settings);
                        quiz.questions[index].settings = changes;
                    },
                }
            }
        }
    }
}

/// The quiz's credits: the articles its questions came from, and each image's author and licence, as the licences of
/// Wikipedia's and Wikimedia Commons' content require.
#[component]
fn CreditsEditor(draft: Signal<Quiz>, images: Vec<String>) -> Element {
    let sources = draft.read().sources.clone();
    let credits = draft.read().images.clone();
    // Images which questions use, or which are in the folder, without a credit yet.
    let mut uncredited: Vec<String> = draft
        .read()
        .questions
        .iter()
        .filter_map(|question| question.image.as_ref().map(|image| image.file.clone()))
        .chain(images)
        .filter(|file| draft.read().image_credit(file).is_none())
        .collect();
    uncredited.sort();
    uncredited.dedup();

    rsx! {
        section { class: "editor-section",
            h2 { "Credits" }
            h3 { "Sources" }
            for (index , source) in sources.iter().enumerate() {
                div { key: "{index}", class: "credit-row",
                    input {
                        placeholder: "Article title",
                        value: "{source.title}",
                        oninput: move |event| draft.write().sources[index].title = event.value(),
                    }
                    input {
                        placeholder: "https://en.wikipedia.org/wiki/…",
                        value: "{source.url}",
                        oninput: move |event| draft.write().sources[index].url = event.value(),
                    }
                    button {
                        onclick: move |_| {
                            draft.write().sources.remove(index);
                        },
                        "×"
                    }
                }
            }
            button {
                onclick: move |_| {
                    draft.write().sources.push(Source {
                        title: String::new(),
                        url: String::new(),
                        revision: None,
                    })
                },
                "Add a Source"
            }
            h3 { "Images" }
            for (index , credit) in credits.iter().enumerate() {
                div { key: "{index}", class: "credit-row",
                    input {
                        placeholder: "File",
                        value: "{credit.file}",
                        oninput: move |event| draft.write().images[index].file = event.value(),
                    }
                    input {
                        placeholder: "Author",
                        value: "{credit.author}",
                        oninput: move |event| draft.write().images[index].author = event.value(),
                    }
                    input {
                        placeholder: "Licence, e.g. CC BY-SA 4.0",
                        value: "{credit.licence}",
                        oninput: move |event| draft.write().images[index].licence = event.value(),
                    }
                    input {
                        placeholder: "Where it came from",
                        value: "{credit.source.clone().unwrap_or_default()}",
                        oninput: move |event| draft.write().images[index].source = Some(event.value()),
                    }
                    button {
                        onclick: move |_| {
                            draft.write().images.remove(index);
                        },
                        "×"
                    }
                }
            }
            div { class: "credit-adds",
                for file in uncredited {
                    button {
                        key: "{file}",
                        onclick: {
                            let file = file.clone();
                            move |_| {
                                draft.write().images.push(ImageCredit {
                                    file: file.clone(),
                                    author: String::new(),
                                    licence: String::new(),
                                    source: None,
                                })
                            }
                        },
                        "Credit {file}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own in the temporary folder, removed afterwards.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("quiz-party-{name}-{}", std::process::id()));
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
    fn saves_a_new_quiz_in_a_folder_of_its_own() {
        let quizzes = TempFolder::new("save");
        let mut quiz = new_quiz();
        quiz.title = "  My Quiz ".into();
        quiz.questions[0].text = "Q?".into();
        quiz.questions[0].answers = vec!["A".into(), " B ".into()];
        let (folder, saved) = save(&quizzes.0, None, &quiz).unwrap();
        assert_eq!(folder, quizzes.0.join("my-quiz"));
        assert_eq!(saved.title, "My Quiz");
        assert_eq!(Quiz::load(&folder).unwrap(), saved);
        // Another with the same title has a folder of its own.
        let (again, _) = save(&quizzes.0, None, &quiz).unwrap();
        assert_eq!(again, quizzes.0.join("my-quiz-2"));
        // Saving to its folder replaces it.
        quiz.title = "Renamed".into();
        let (same, _) = save(&quizzes.0, Some(&folder), &quiz).unwrap();
        assert_eq!(same, folder);
        assert_eq!(Quiz::load(&folder).unwrap().title, "Renamed");
    }

    #[test]
    fn refuses_a_quiz_with_problems() {
        let quizzes = TempFolder::new("refuse");
        let problems = save(&quizzes.0, None, &new_quiz()).unwrap_err();
        assert!(
            problems.contains(&"Question 1 has no text.".to_string()),
            "{problems:?}"
        );
        assert!(std::fs::read_dir(&quizzes.0).unwrap().next().is_none(), "nothing saved");
    }

    #[test]
    fn a_question_keeps_only_the_settings_it_changes() {
        let quiz = Settings::default();
        let question = Settings {
            time_limit: 30,
            ..quiz.clone()
        };
        assert_eq!(
            changes(&quiz, &question),
            SettingsOverride {
                time_limit: Some(30),
                ..Default::default()
            }
        );
        assert!(changes(&quiz, &quiz).is_empty());
    }

    #[test]
    fn finds_a_quizs_images() {
        let quiz = TempFolder::new("images");
        std::fs::create_dir_all(quiz.0.join("images")).unwrap();
        for file in ["images/flag.svg", "images/notes.txt", "photo.JPG", "quiz.md"] {
            std::fs::write(quiz.0.join(file), "").unwrap();
        }
        assert_eq!(images_in(&quiz.0), ["images/flag.svg", "photo.JPG"]);
    }
}
