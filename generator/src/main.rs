//! The quiz generator from the command line: writes a draft quiz on a topic, saying each step as it goes. The
//! controller's Quizzes page does the same with a window; this is for scripts, or for stepping through a generation.

use std::path::PathBuf;

use bruce_quiz_generator::{
    claude,
    draft::{Difficulty, Request},
    generate, settings,
    settings::Destination,
    wikipedia::article_title,
};
use clap::Parser;

/// The command line.
#[derive(Debug, Clone, Parser)]
#[command(version, about)]
struct Args {
    /// The subject, e.g. "Flags of Europe": searched for on Wikipedia unless articles are given.
    #[arg(long, required_unless_present = "article", default_value = "")]
    topic: String,
    /// A Wikipedia article to write from, by its title or address, instead of searching; given again for each.
    #[arg(long)]
    article: Vec<String>,
    /// Anything else for Claude, e.g. "Every question is a picture question".
    #[arg(long, default_value = "")]
    instructions: String,
    /// How many questions.
    #[arg(long, default_value_t = 10)]
    questions: usize,
    /// How hard: easy, medium or hard.
    #[arg(long, default_value = "medium")]
    difficulty: String,
    /// The folder of quizzes to save into, e.g. the controller's, instead of where the controller last chose.
    #[arg(long, conflicts_with = "library")]
    quizzes: Option<PathBuf>,
    /// A local checkout of a library's git repository to save into, instead of where the controller last chose.
    #[arg(long)]
    library: Option<PathBuf>,
    /// With --library, commit and push the quiz with git.
    #[arg(long, requires = "library")]
    publish: bool,
    /// The Claude Code command. By default, `claude` on the PATH, or the copy that comes with the VS Code extension.
    #[arg(long)]
    claude: Option<PathBuf>,
}

impl Args {
    /// Where to save: as given, or else where the controller's Quizzes page last chose.
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

fn main() {
    let args = Args::parse();
    if let Err(error) = run(&args) {
        eprintln!("The quiz could not be written: {error:#}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> anyhow::Result<()> {
    let claude = claude::find(args.claude.as_deref())
        .ok_or_else(|| anyhow::anyhow!("Claude Code cannot be found: install it, or give --claude <path>"))?;
    let request = Request {
        topic: args.topic.clone(),
        articles: args.article.iter().map(|given| article_title(given)).collect(),
        instructions: args.instructions.clone(),
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
