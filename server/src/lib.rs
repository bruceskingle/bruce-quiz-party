//! Bruce's Quiz Party's controller: the Mac app which runs a quiz party, shows its big screen, and serves the web
//! client the players' phones join.

/// Whether this is the Mac App Store's build, see the `store` feature in Cargo.toml.
pub const STORE: bool = cfg!(feature = "store");

mod answer_shape;
mod big_screen;
mod bonjour;
mod dock_icon;
mod download;
mod editor;
mod folder_access;
pub mod game;
mod generator;
mod library;
pub mod net;
pub mod party;
mod phone_image;
mod qr;

use std::path::PathBuf;

use clap::Parser;
use dioxus::prelude::*;

/// The command line.
#[derive(Debug, Clone, Parser)]
#[command(version, about)]
pub struct Args {
    /// The folder of quizzes, each a folder with a quiz.md. By default, the quizzes folder in the app's data folder.
    #[arg(long)]
    pub quizzes: Option<PathBuf>,
    /// The Claude Code command, for writing quizzes with Claude. By default, `claude` on the PATH, or the copy that
    /// comes with the VS Code extension; without it, the Quizzes page does not offer to write one.
    #[arg(long)]
    pub claude: Option<PathBuf>,
}

impl Args {
    /// The folder of quizzes, the one given or the default.
    pub fn quizzes_folder(&self) -> PathBuf {
        self.quizzes.clone().unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("bruce-quiz-party")
                .join("quizzes")
        })
    }
}

/// The stylesheet, shared by every window.
const MAIN_CSS: Asset = asset!("/assets/main.css");

/// The background of the app, as in main.css, for the window before the stylesheet has loaded, so that it does not
/// flash white as it opens.
const BACKGROUND_COLOR: (u8, u8, u8, u8) = (0x0f, 0x11, 0x16, 0xff);
const INITIAL_HEAD: &str = "<style>html, body { background-color: #0f1116; color: #e6eef3; }</style>";

/// The party's web server, or why it could not be started, for the window to show.
#[derive(Clone)]
pub struct PartyServer(pub Result<net::Server, String>);

/// Starts the party's web server and opens the window.
pub fn launch() {
    let args = Args::parse();
    let folder = args.quizzes_folder();
    let _ = library::QUIZZES_FOLDER.set(folder.clone());
    // The party plays the quizzes which could be read; choosing them comes with setup mode.
    let quizzes = quiz_format::load_library(&folder)
        .map(|entries| {
            entries
                .into_iter()
                .filter_map(|(folder, quiz)| {
                    Some(party::PartyQuiz {
                        folder,
                        quiz: quiz.ok()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let server = PartyServer(net::start(party::Party::new(quizzes)).map_err(|error| {
        format!(
            "{:#}",
            anyhow::Error::from(error).context("cannot start the party's web server")
        )
    }));
    let window = dioxus::desktop::tao::window::WindowBuilder::new()
        .with_title("Bruce's Quiz Party")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1280, 800))
        .with_resizable(true);
    let config = dioxus::desktop::Config::new()
        .with_window(window)
        .with_background_color(BACKGROUND_COLOR)
        .with_custom_head(INITIAL_HEAD.to_string())
        // The webview's context menu (Reload, Inspect Element...) is only offered in debug builds.
        .with_disable_context_menu(!cfg!(debug_assertions))
        // The quizzes' images are served from their folders, as quizimg://localhost/<path>, see library.rs.
        .with_asynchronous_custom_protocol("quizimg", library::serve_image);
    dioxus::LaunchBuilder::new()
        .with_cfg(config)
        .with_context(args)
        .with_context(server)
        .launch(App);
}

/// The main window's pages.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Page {
    /// The lobby, as on the big screen.
    Party,
    /// The quiz library.
    Quizzes,
}

/// The main window: the navigation bar, and the page below it.
#[component]
fn App() -> Element {
    let mut page = use_signal(|| Page::Party);
    // Once the app is running, so that the window library has made the application first.
    use_hook(dock_icon::set);
    let tab = |this: Page| if page() == this { "nav-item active" } else { "nav-item" };
    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        div { class: "app-layout",
            nav { class: "nav",
                span { class: "brand", "Bruce's Quiz Party" }
                span { class: tab(Page::Party), onclick: move |_| page.set(Page::Party), "Party" }
                span { class: tab(Page::Quizzes), onclick: move |_| page.set(Page::Quizzes), "Quizzes" }
                div { class: "spacer" }
            }
            match page() {
                Page::Party => rsx! { big_screen::BigScreen {} },
                Page::Quizzes => rsx! { library::Library {} },
            }
        }
    }
}
