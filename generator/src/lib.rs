//! Writing a draft quiz on a topic from Wikipedia's articles, with Claude: used by the controller's Quizzes page, and
//! by the `bruce-quiz-generator` command. It asks Claude which articles to read, unless they are given, reads them from
//! Wikipedia itself, checking the images' licences, asks Claude for the questions as JSON, through Claude Code run
//! headless with no tools or through Claude's API with the user's own key, see claude.rs, and saves the draft in a
//! quizzes folder, or in a library checkout, optionally committed and pushed with the user's own git.

pub mod api_key;
pub mod claude;
pub mod draft;
pub mod generate;
pub mod publish;
pub mod settings;
pub mod suggest;
pub mod wikipedia;
