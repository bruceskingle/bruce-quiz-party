//! Asking Claude, through Claude Code run headless (`claude -p`) on Bruce's own subscription: with no tools, so that it
//! only writes, from what it is given, in a folder of its own, so that it reads nothing else.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::Context;

/// The Claude Code command: the one given, else `claude` on the PATH or in its usual places, else the newest copy that
/// comes with the VS Code extension.
pub fn find(given: Option<&Path>) -> Option<PathBuf> {
    if let Some(given) = given {
        return Some(given.to_path_buf());
    }
    let home = dirs::home_dir().unwrap_or_default();
    let on_path = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|folder| folder.join("claude"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let usual = [
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(found) = on_path.into_iter().chain(usual).find(|path| path.is_file()) {
        return Some(found);
    }
    // The extension's folders are named with their version, e.g. anthropic.claude-code-2.1.325-darwin-arm64.
    let mut bundled: Vec<PathBuf> = std::fs::read_dir(home.join(".vscode/extensions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("anthropic.claude-code-"))
        })
        .map(|path| path.join("resources/native-binary/claude"))
        .filter(|path| path.is_file())
        .collect();
    bundled.sort_by_key(|path| version_of(path));
    bundled.pop()
}

/// The version in an extension's folder name, as numbers to sort by, so that 2.1.325 comes after 2.1.99.
fn version_of(path: &Path) -> Vec<u32> {
    let folder = path
        .ancestors()
        .nth(3)
        .and_then(|folder| folder.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    folder
        .trim_start_matches("anthropic.claude-code-")
        .split(['.', '-'])
        .map_while(|part| part.parse().ok())
        .collect()
}

/// Asks Claude, with a JSON Schema for its answer, and returns the answer's JSON.
pub fn ask(claude: &Path, prompt: &str, schema: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    // A folder of its own, empty, so that Claude Code has nothing of Bruce's to look at.
    let folder = std::env::temp_dir().join(format!("quiz-generator-{}", std::process::id()));
    std::fs::create_dir_all(&folder)?;
    let mut child = Command::new(claude)
        .args([
            "-p",
            "--output-format",
            "json",
            "--no-session-persistence",
            "--tools",
            "",
        ])
        .arg("--json-schema")
        .arg(schema.to_string())
        .current_dir(&folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot run {}", claude.display()))?;
    child
        .stdin
        .take()
        .context("cannot give Claude the prompt")?
        .write_all(prompt.as_bytes())?;
    let output = child.wait_with_output().context("Claude Code stopped")?;
    let _ = std::fs::remove_dir_all(&folder);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() && stdout.trim().is_empty() {
        anyhow::bail!(
            "Claude Code failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    answer(&stdout)
}

/// The answer's JSON from Claude Code's output: its structured output, or else the JSON in its reply's text.
fn answer(stdout: &str) -> anyhow::Result<serde_json::Value> {
    let output: serde_json::Value =
        serde_json::from_str(stdout.trim()).context("Claude Code's output is not the JSON expected")?;
    if output.get("is_error").and_then(|value| value.as_bool()) == Some(true) {
        let message = output
            .get("result")
            .and_then(|value| value.as_str())
            .unwrap_or("no reason given");
        anyhow::bail!("Claude could not answer: {message}");
    }
    if let Some(structured) = output.get("structured_output").filter(|value| value.is_object()) {
        return Ok(structured.clone());
    }
    let text = output
        .get("result")
        .and_then(|value| value.as_str())
        .context("Claude Code's output has no answer")?;
    json_in(text).context("Claude's answer has no JSON")
}

/// The JSON object in some text, e.g. in a Markdown code block.
fn json_in(text: &str) -> Option<serde_json::Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(&text[start..=end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_structured_answer() {
        let stdout = r#"{"type":"result","subtype":"success","is_error":false,"result":"Done.",
            "structured_output":{"title":"T"}}"#;
        assert_eq!(answer(stdout).unwrap()["title"], "T");
    }

    #[test]
    fn reads_json_in_the_text() {
        let stdout = r#"{"type":"result","is_error":false,"result":"Here it is:\n```json\n{\"title\": \"T\"}\n```"}"#;
        assert_eq!(answer(stdout).unwrap()["title"], "T");
    }

    #[test]
    fn says_why_it_failed() {
        let stdout = r#"{"type":"result","is_error":true,"result":"Credit balance is too low"}"#;
        let error = answer(stdout).unwrap_err().to_string();
        assert!(error.contains("Credit balance"), "{error}");
    }

    #[test]
    fn sorts_versions_as_numbers() {
        let path = |version: &str| {
            PathBuf::from(format!(
                "/x/.vscode/extensions/anthropic.claude-code-{version}-darwin-arm64/resources/native-binary/claude"
            ))
        };
        assert!(version_of(&path("2.1.325")) > version_of(&path("2.1.99")));
        assert_eq!(version_of(&path("2.1.325")), [2, 1, 325]);
    }
}
