//! Asking Claude, one of two ways, as the user chooses:
//!
//! - Claude Code, run headless (`claude -p`) on the user's own subscription, with no tools, so that it only writes, from
//!   what it is given, in a folder of its own, so that it reads nothing else. It must be installed, and cannot be run
//!   by the Mac App Store's copy of the controller, which the sandbox keeps from starting other programs.
//! - Claude's API, with the user's own API key, kept in the Keychain, see api_key.rs, and the model they choose, billed
//!   to their key. Either copy of the controller can use it.
//!
//! Either way the answer is JSON, as a JSON Schema says.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// How to ask Claude.
#[derive(Debug, Clone, PartialEq)]
pub enum Claude {
    /// Claude Code, the command at this path.
    Code(PathBuf),
    /// Claude's API, with this API key, and this model, by its ID, e.g. `claude-opus-5-5`.
    Api { key: String, model: String },
}

/// A model the user may choose, for the API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    /// Roughly what a quiz costs with it, from its prices and a typical quiz's size, for choosing.
    pub cost: &'static str,
}

/// The models offered, the default first.
pub const MODELS: [Model; 4] = [
    Model {
        id: "claude-opus-5-5",
        name: "Claude Opus 5.5",
        cost: "about $0.20–0.35 a quiz",
    },
    Model {
        id: "claude-sonnet-5-5",
        name: "Claude Sonnet 5.5",
        cost: "about $0.10–0.20 a quiz",
    },
    Model {
        id: "claude-haiku-5-5",
        name: "Claude Haiku 5.5",
        cost: "under a cent a quiz",
    },
    Model {
        id: "claude-fable-5-2",
        name: "Claude Fable 5.2",
        cost: "about $0.40–0.70 a quiz",
    },
];

/// The model chosen unless the user chooses another.
pub const DEFAULT_MODEL: &str = MODELS[0].id;

/// Which way the user chose to ask Claude, remembered between runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Choice {
    /// Claude Code, if it is installed.
    Code,
    /// The API, with this model.
    Api { model: String },
}

impl Default for Choice {
    fn default() -> Self {
        Choice::Api {
            model: DEFAULT_MODEL.to_string(),
        }
    }
}

/// The file the choice is kept in, in the generator's data folder.
fn choice_file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bruce-quiz-generator")
        .join("claude.json")
}

/// The way of asking last chosen: as saved, or else Claude Code if it is installed, as it was the only way before, or
/// else the API with the default model.
pub fn saved_choice(code_installed: bool) -> Choice {
    std::fs::read_to_string(choice_file())
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or(if code_installed {
            Choice::Code
        } else {
            Choice::default()
        })
}

/// Remembers the way of asking chosen.
pub fn save_choice(choice: &Choice) -> anyhow::Result<()> {
    let path = choice_file();
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(choice)?)
        .with_context(|| format!("cannot write {}", path.display()))
}

/// Asks Claude, with a JSON Schema for its answer, and returns the answer's JSON.
pub fn ask(claude: &Claude, prompt: &str, schema: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    match claude {
        Claude::Code(path) => ask_code(path, prompt, schema),
        Claude::Api { key, model } => ask_api(key, model, prompt, schema),
    }
}

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

/// Asks Claude Code, with a JSON Schema for its answer, and returns the answer's JSON.
fn ask_code(claude: &Path, prompt: &str, schema: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
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

/// Claude's API.
const API: &str = "https://api.anthropic.com/v1/messages";

/// The API's version, which the requests are written for.
const API_VERSION: &str = "2026-10-21";

/// The most Claude may write, thinking included: a quiz's JSON is a few thousand tokens.
const MAX_TOKENS: u32 = 16_000;

/// How long an answer may take: a large quiz can take a few minutes.
const API_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The models which can try a request again on another model if theirs declines it, see the API's server-side
/// fallback.
const WITH_FALLBACK: [&str; 3] = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-2"];

/// Asks Claude's API, with a JSON Schema for its answer, and returns the answer's JSON.
fn ask_api(key: &str, model: &str, prompt: &str, schema: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(API_TIMEOUT))
        // The API's own explanation of an error is in the reply, so it is read rather than turned into an error.
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut request = agent
        .post(API)
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION);
    if WITH_FALLBACK.contains(&model) {
        request = request.header("anthropic-beta", "server-side-fallback-2026-07-01");
    }
    let mut response = request
        .send_json(api_request(model, prompt, schema))
        .context("cannot reach Claude's API")?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .context("cannot read Claude's answer")?;
    api_answer(status, &body)
}

/// The request to the API: the prompt, the schema as its structured output, and an effort which suits writing a quiz.
fn api_request(model: &str, prompt: &str, schema: &serde_json::Value) -> serde_json::Value {
    // Claude Fable is the most capable model, chosen for the best quiz, at its recommended effort.
    let effort = if model.starts_with("claude-fable") {
        "high"
    } else {
        "medium"
    };
    let mut request = serde_json::json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "output_config": {
            "effort": effort,
            "format": { "type": "json_schema", "schema": api_schema(schema) },
        },
        "messages": [{ "role": "user", "content": prompt }],
    });
    if WITH_FALLBACK.contains(&model) {
        // If the model declines, e.g. a classifier mistakes a quiz about a war for something harmful, the API tries
        // another model which can answer.
        request["fallbacks"] = serde_json::json!("default");
    }
    request
}

/// A JSON Schema as the API's structured output takes it: every object closed to other properties, as it requires,
/// and without the limits it does not take, e.g. an array's number of items, which the answer is checked for anyway.
fn api_schema(schema: &serde_json::Value) -> serde_json::Value {
    const UNSUPPORTED: [&str; 7] = [
        "minItems",
        "maxItems",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "multipleOf",
    ];
    match schema {
        serde_json::Value::Object(map) => {
            let mut map: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter(|(key, _)| !UNSUPPORTED.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), api_schema(value)))
                .collect();
            if map.get("type").and_then(|kind| kind.as_str()) == Some("object") {
                map.insert("additionalProperties".into(), false.into());
            }
            serde_json::Value::Object(map)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(items.iter().map(api_schema).collect()),
        other => other.clone(),
    }
}

/// The answer's JSON from the API's reply, or why there is none: an error, a refusal, or an answer cut short.
fn api_answer(status: u16, body: &str) -> anyhow::Result<serde_json::Value> {
    let reply: serde_json::Value = serde_json::from_str(body)
        .with_context(|| format!("Claude's API answered {status}, not with the JSON expected"))?;
    if !(200..300).contains(&status) {
        let message = reply["error"]["message"].as_str().unwrap_or("no reason given");
        let why = match status {
            401 => "the API key was not accepted: check it, or make a new one in the Claude Console",
            403 => "the API key may not use this model",
            404 => "the model is not available to this API key",
            413 => "the request is too large: ask for fewer articles",
            429 => "too many requests, or the key's spending limit is reached: wait a little, or check its limits",
            500..=599 => "Claude's API is busy or failing: try again in a few minutes",
            _ => "the request was not accepted",
        };
        anyhow::bail!("Claude's API answered {status}, {why} ({message})");
    }
    match reply["stop_reason"].as_str() {
        Some("refusal") => {
            let details = &reply["stop_details"];
            let why = details["explanation"]
                .as_str()
                .or_else(|| details["category"].as_str())
                .unwrap_or("no reason given");
            anyhow::bail!("Claude declined to write this quiz: {why}");
        }
        Some("max_tokens") => anyhow::bail!("Claude's answer was too long and was cut short: ask for fewer questions"),
        _ => {}
    }
    let text: String = reply["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    serde_json::from_str(&text)
        .ok()
        .or_else(|| json_in(&text))
        .context("Claude's answer has no JSON")
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
    fn reads_the_apis_answer() {
        let body = r#"{"stop_reason":"end_turn","content":[
            {"type":"thinking","thinking":""},
            {"type":"text","text":"{\"title\":\"T\"}"}]}"#;
        assert_eq!(api_answer(200, body).unwrap()["title"], "T");
    }

    #[test]
    fn says_why_the_api_did_not_answer() {
        let error = |status, body| api_answer(status, body).unwrap_err().to_string();
        let bad_key = error(
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
        );
        assert!(
            bad_key.contains("API key was not accepted") && bad_key.contains("invalid x-api-key"),
            "{bad_key}"
        );
        let refused = error(
            200,
            r#"{"stop_reason":"refusal","stop_details":{"category":"cyber","explanation":null},"content":[]}"#,
        );
        assert!(refused.contains("declined") && refused.contains("cyber"), "{refused}");
        let cut = error(
            200,
            r#"{"stop_reason":"max_tokens","content":[{"type":"text","text":"{\"ti"}]}"#,
        );
        assert!(cut.contains("cut short"), "{cut}");
        assert!(error(502, "<html>Bad gateway</html>").contains("502"));
    }

    #[test]
    fn makes_the_schema_one_the_api_takes() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "answers": { "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 6 },
                "questions": { "type": "array", "items": { "type": "object", "properties": {} } },
            },
            "required": ["answers"],
        });
        let api = api_schema(&schema);
        assert_eq!(api["additionalProperties"], false);
        assert_eq!(api["properties"]["questions"]["items"]["additionalProperties"], false);
        assert!(api["properties"]["answers"].get("minItems").is_none());
        assert!(api["properties"]["answers"].get("maxItems").is_none());
        assert_eq!(api["required"], serde_json::json!(["answers"]));
    }

    #[test]
    fn asks_for_a_fallback_only_where_there_is_one() {
        let schema = serde_json::json!({ "type": "object", "properties": {} });
        assert_eq!(api_request("claude-opus-5-5", "p", &schema)["fallbacks"], "default");
        assert!(api_request("claude-haiku-5-5", "p", &schema).get("fallbacks").is_none());
        assert_eq!(
            api_request("claude-fable-5-2", "p", &schema)["output_config"]["effort"],
            "high"
        );
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
