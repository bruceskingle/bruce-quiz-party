//! Bruce's Quiz Generator: writes a draft quiz on a topic from Wikipedia's articles, with Claude, into the quizzes
//! folder, for reviewing in the controller's editor before it is played. For Bruce's own use: it runs Claude Code on
//! his subscription. The controller only reads the files it writes.

mod claude;
mod draft;
mod generate;
mod publish;
mod settings;
mod wikipedia;

use std::path::PathBuf;

use clap::Parser;
use dioxus::prelude::*;
use quiz_format::Quiz;

use crate::{
    draft::{Difficulty, Request},
    settings::Destination,
};

/// The command line.
#[derive(Debug, Clone, Parser)]
#[command(version, about)]
struct Args {
    /// The folder of quizzes to save into, e.g. the controller's, instead of where the window last chose.
    #[arg(long, conflicts_with = "library")]
    quizzes: Option<PathBuf>,
    /// A local checkout of a library's git repository to save into, instead of where the window last chose.
    #[arg(long)]
    library: Option<PathBuf>,
    /// With --library, commit and push the quiz with git.
    #[arg(long, requires = "library")]
    publish: bool,
    /// The Claude Code command. By default, `claude` on the PATH, or the copy that comes with the VS Code extension.
    #[arg(long)]
    claude: Option<PathBuf>,
    /// Writes a quiz on this topic without opening the window, saying each step as it goes.
    #[arg(long)]
    topic: Option<String>,
    /// With --topic, how many questions.
    #[arg(long, default_value_t = 10)]
    questions: usize,
    /// With --topic, how hard: easy, medium or hard.
    #[arg(long, default_value = "medium")]
    difficulty: String,
}

impl Difficulty {
    fn parse(text: &str) -> Difficulty {
        match text {
            "easy" => Difficulty::Easy,
            "hard" => Difficulty::Hard,
            _ => Difficulty::Medium,
        }
    }
}

/// Writes a quiz from the command line, without the window.
fn run_without_window(args: &Args, topic: &str) -> anyhow::Result<()> {
    let claude = claude::find(args.claude.as_deref())
        .ok_or_else(|| anyhow::anyhow!("Claude Code cannot be found: install it, or give --claude <path>"))?;
    let request = Request {
        topic: topic.to_string(),
        questions: args.questions,
        difficulty: Difficulty::parse(&args.difficulty),
    };
    let (folder, quiz) = generate::generate(&request, &claude, &args.destination(), &|step| println!("{step}"))?;
    println!(
        "Saved “{}”, {} questions, in {}",
        quiz.title,
        quiz.questions.len(),
        folder.display()
    );
    Ok(())
}

impl Args {
    /// Where to save: as given on the command line, or else where the window last chose.
    fn destination(&self) -> Destination {
        if let Some(path) = &self.quizzes {
            Destination::Folder { path: path.clone() }
        } else if let Some(checkout) = &self.library {
            Destination::Library {
                checkout: checkout.clone(),
                publish: self.publish,
            }
        } else {
            settings::load()
        }
    }
}

/// The controller's stylesheet, so that the two apps look alike.
const MAIN_CSS: &str = include_str!("../../server/assets/main.css");
const GENERATOR_CSS: &str = include_str!("../assets/generator.css");

/// The background of the app, as in main.css, so that the window does not flash white as it opens.
const BACKGROUND_COLOR: (u8, u8, u8, u8) = (0x0f, 0x11, 0x16, 0xff);
const INITIAL_HEAD: &str = "<style>html, body { background-color: #0f1116; color: #e6eef3; }</style>";

fn main() {
    let args = Args::parse();
    if let Some(topic) = &args.topic {
        if let Err(error) = run_without_window(&args, topic) {
            eprintln!("The quiz could not be written: {error:#}");
            std::process::exit(1);
        }
        return;
    }
    let window = dioxus::desktop::tao::window::WindowBuilder::new()
        .with_title("Bruce's Quiz Generator")
        .with_inner_size(dioxus::desktop::LogicalSize::new(960, 760))
        .with_resizable(true);
    let config = dioxus::desktop::Config::new()
        .with_window(window)
        .with_background_color(BACKGROUND_COLOR)
        .with_custom_head(INITIAL_HEAD.to_string())
        .with_disable_context_menu(!cfg!(debug_assertions));
    dioxus::LaunchBuilder::new()
        .with_cfg(config)
        .with_context(args)
        .launch(App);
}

/// How the last generation went.
#[derive(Clone, PartialEq)]
enum Outcome {
    Saved(PathBuf, Quiz),
    Failed(String),
}

/// The window: the topic and options, what is happening, and the draft written.
#[component]
fn App() -> Element {
    let args = use_context::<Args>();
    let mut destination = use_signal(|| args.destination());
    let claude = claude::find(args.claude.as_deref());
    let mut topic = use_signal(String::new);
    let mut count = use_signal(|| 10usize);
    let mut difficulty = use_signal(|| Difficulty::Medium);
    let mut steps = use_signal(Vec::<String>::new);
    let mut running = use_signal(|| false);
    let mut outcome = use_signal(|| None::<Outcome>);

    let generate = {
        let claude = claude.clone();
        move |_| {
            let Some(claude) = claude.clone() else { return };
            let request = Request {
                topic: topic().trim().to_string(),
                questions: count(),
                difficulty: difficulty(),
            };
            if request.topic.is_empty() {
                return;
            }
            let destination = destination();
            running.set(true);
            steps.set(Vec::new());
            outcome.set(None);
            spawn(async move {
                let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
                let work = tokio::task::spawn_blocking(move || {
                    generate::generate(&request, &claude, &destination, &|step| {
                        let _ = sender.send(step);
                    })
                });
                while let Some(step) = receiver.recv().await {
                    steps.write().push(step);
                }
                outcome.set(Some(match work.await {
                    Ok(Ok((folder, quiz))) => Outcome::Saved(folder, quiz),
                    Ok(Err(error)) => Outcome::Failed(format!("{error:#}")),
                    Err(error) => Outcome::Failed(format!("{:#}", anyhow::Error::from(error))),
                }));
                running.set(false);
            });
        }
    };

    rsx! {
        document::Style { {MAIN_CSS} }
        document::Style { {GENERATOR_CSS} }
        div { class: "app-layout",
            nav { class: "nav",
                span { class: "brand", "Bruce's Quiz Generator" }
                div { class: "spacer" }
            }
            div { class: "content generator",
                p { class: "muted",
                    "Writes a draft quiz from Wikipedia's articles on a topic, with Claude. "
                    "Review it on the controller's Quizzes page before playing or sharing it."
                }
                DestinationChooser {
                    destination: destination(),
                    disabled: running(),
                    onchange: move |chosen: Destination| {
                        let _ = settings::save(&chosen);
                        destination.set(chosen);
                    },
                }
                div { class: "generator-form",
                    input {
                        class: "topic",
                        placeholder: "A topic, e.g. Flags of Europe, The Moon, 1980s pop music",
                        value: "{topic}",
                        disabled: running(),
                        oninput: move |event| topic.set(event.value()),
                    }
                    label {
                        select {
                            disabled: running(),
                            value: "{count}",
                            onchange: move |event: FormEvent| count.set(event.value().parse().unwrap_or(10)),
                            for n in [5, 8, 10, 12, 15, 20] {
                                option { key: "{n}", value: "{n}", "{n} questions" }
                            }
                        }
                    }
                    label {
                        select {
                            disabled: running(),
                            value: match difficulty() {
                                Difficulty::Easy => "easy",
                                Difficulty::Medium => "medium",
                                Difficulty::Hard => "hard",
                            },
                            onchange: move |event: FormEvent| difficulty.set(Difficulty::parse(&event.value())),
                            option { value: "easy", "Easy" }
                            option { value: "medium", "Medium" }
                            option { value: "hard", "Hard" }
                        }
                    }
                    button {
                        class: "primary",
                        disabled: running() || claude.is_none() || topic().trim().is_empty(),
                        onclick: generate,
                        if running() {
                            "Writing…"
                        } else {
                            "Write the Quiz"
                        }
                    }
                }
                match &claude {
                    Some(path) => rsx! {
                        p { class: "muted small", "Using Claude Code at {path.display()}, on your subscription." }
                    },
                    None => rsx! {
                        p { class: "invalid",
                            "Claude Code cannot be found. Install it, or start the generator with --claude <path>."
                        }
                    },
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
                    Some(Outcome::Saved(folder, quiz)) => rsx! {
                        DraftPreview { folder, quiz }
                    },
                    None => rsx! {},
                }
            }
        }
    }
}

/// Where to save: a quizzes folder, to play the quiz there, or a library checkout, to share it. Each choice shows its
/// folder, and keeps it while the other is chosen, so that switching back and forth loses neither.
#[component]
fn DestinationChooser(destination: Destination, disabled: bool, onchange: EventHandler<Destination>) -> Element {
    let mut folder = use_signal(|| match &destination {
        Destination::Folder { path } => path.clone(),
        Destination::Library { .. } => Destination::default_folder(),
    });
    let mut checkout = use_signal(|| match &destination {
        Destination::Library { checkout, .. } => checkout.clone(),
        Destination::Folder { .. } => dirs::home_dir().unwrap_or_default().join("git/bruce-quiz-library"),
    });
    let is_library = matches!(destination, Destination::Library { .. });
    let publish = matches!(destination, Destination::Library { publish: true, .. });
    let to_folder = move || Destination::Folder { path: folder() };
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
                    onchange: move |_| onchange.call(to_folder()),
                }
                "A quizzes folder, to play it there"
            }
            input {
                class: "destination-path",
                value: "{folder.read().display()}",
                disabled: is_library,
                oninput: move |event: FormEvent| {
                    folder.set(PathBuf::from(event.value()));
                    onchange.call(to_folder());
                },
            }
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

/// The draft written: where it is, and its questions with their correct answers, to check at a glance.
#[component]
fn DraftPreview(folder: PathBuf, quiz: Quiz) -> Element {
    rsx! {
        div { class: "draft",
            h1 { "{quiz.title}" }
            if let Some(description) = &quiz.description {
                p { class: "muted", "{description}" }
            }
            p {
                "Saved in {folder.display()}. Open the controller's Quizzes page to review and edit it. "
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
            h3 { "Credits" }
            p { class: "muted small", "Licence: {quiz.licence.clone().unwrap_or_default()}" }
            ul { class: "muted small",
                for source in quiz.sources.iter() {
                    li { key: "{source.url}", "{source.title}, revision of {source.revision.clone().unwrap_or_default()}" }
                }
                for credit in quiz.images.iter() {
                    li { key: "{credit.file}", "{credit.file}: {credit.author}, {credit.licence}" }
                }
            }
        }
    }
}
