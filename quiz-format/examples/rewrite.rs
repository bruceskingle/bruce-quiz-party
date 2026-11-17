//! Prints a quiz as the writer writes it, to compare with the file: `cargo run --example rewrite -- <quiz folder>`.

fn main() {
    let folder = std::env::args().nth(1).expect("give the quiz's folder");
    match bruce_quiz_format::Quiz::load(std::path::Path::new(&folder)) {
        Ok(quiz) => print!("{}", quiz.to_markdown()),
        Err(error) => {
            eprintln!("{:#}", anyhow::Error::from(error));
            std::process::exit(1);
        }
    }
}
