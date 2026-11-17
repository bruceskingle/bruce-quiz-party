//! Adding a quiz to a library: its index rewritten, and, if asked, the quiz and the index committed and pushed with
//! the user's own git and its credentials, so that the library's readers can download it.

use std::{path::Path, process::Command};

use anyhow::Context;

/// Rewrites the library's index after a quiz was saved in its folder, and commits and pushes them if `publish`.
pub fn add_to_library(
    checkout: &Path,
    folder: &Path,
    title: &str,
    publish: bool,
    progress: &dyn Fn(String),
) -> anyhow::Result<()> {
    progress("Updating the library's index…".into());
    quiz_format::library::write_index(checkout)
        .with_context(|| format!("cannot write the index of the library in {}", checkout.display()))?;
    if !publish {
        return Ok(());
    }
    let name = folder
        .strip_prefix(checkout)
        .context("the quiz is not in the library's folder")?
        .to_string_lossy()
        .into_owned();
    git(checkout, &["rev-parse", "--is-inside-work-tree"])
        .with_context(|| format!("{} is not a git checkout", checkout.display()))?;
    progress("Committing the quiz to the library…".into());
    git(checkout, &["add", "--", &name, quiz_format::library::LIBRARY_INDEX])?;
    let message = format!("Add the quiz “{title}”");
    git(
        checkout,
        &[
            "commit",
            "-m",
            &message,
            "--",
            &name,
            quiz_format::library::LIBRARY_INDEX,
        ],
    )?;
    progress("Pushing the library…".into());
    git(checkout, &["push"]).context("the quiz is committed, but could not be pushed: push it yourself")?;
    Ok(())
}

/// Runs git in a folder, failing with what it said if it fails.
fn git(folder: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(args)
        .output()
        .context("cannot run git")?;
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Publishes into a checkout of a throwaway repository, pushing to a throwaway bare one, both in the temporary
    /// folder.
    #[test]
    fn commits_and_pushes_a_quiz() {
        let root = std::env::temp_dir().join(format!("quiz-publish-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let remote = root.join("remote.git");
        let checkout = root.join("library");
        std::fs::create_dir_all(&root).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git").args(args).current_dir(&root).output().unwrap();
            assert!(
                status.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        };
        run(&["init", "--bare", "-q", "remote.git"]);
        run(&["clone", "-q", remote.to_str().unwrap(), "library"]);
        for (key, value) in [
            ("user.name", "Test"),
            ("user.email", "test@example.org"),
            ("commit.gpgsign", "false"),
        ] {
            git(&checkout, &["config", key, value]).unwrap();
        }
        let quiz =
            quiz_format::Quiz::parse("+++\ntitle = \"Flags\"\n+++\n## Whose?\n- [x] Japan\n- [ ] Laos\n").unwrap();
        let folder = checkout.join("flags");
        quiz.save(&folder).unwrap();

        add_to_library(&checkout, &folder, "Flags", true, &|_| {}).unwrap();
        let pushed = Command::new("git")
            .args([
                "--git-dir",
                remote.to_str().unwrap(),
                "log",
                "--name-only",
                "--format=%s",
            ])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&pushed.stdout);
        assert!(log.contains("Add the quiz “Flags”"), "{log}");
        assert!(log.contains("flags/quiz.md") && log.contains("library.json"), "{log}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
