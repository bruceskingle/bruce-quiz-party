//! Reading Wikipedia for a quiz: searching for a topic's articles, reading their text with the revision read, and
//! finding each one's main image on Wikimedia Commons, kept only if it is freely licensed, with its author and licence.
//! The parsing of each answer is separate from fetching it, so that the tests can give it saved answers.

use std::{path::Path, time::Duration};

use anyhow::Context;
use serde::Deserialize;

/// Wikipedia's API, in English.
const API: &str = "https://en.wikipedia.org/w/api.php";

/// The generator's name for Wikimedia's servers, which ask automated clients to say what they are and where to find
/// out more, with the project's address, as Bruce chose on 17 Nov 2026.
const USER_AGENT: &str = "BrucesQuizGenerator/0.1 (https://github.com/bruceskingle/bruce-quiz-party)";

/// The pause before each request to Wikipedia, so that the generator's requests come one at a time, not in a burst.
const REQUEST_GAP: Duration = Duration::from_millis(300);

/// How many times a request is made when Wikipedia says there have been too many, waiting 4, then 8 seconds.
const RETRIES: u32 = 3;

/// The most text of an article given to Claude, in characters, so that a few articles fit in one request.
const MAX_ARTICLE_CHARS: usize = 12_000;

/// An article read for a quiz.
#[derive(Debug, Clone, PartialEq)]
pub struct Article {
    pub title: String,
    pub url: String,
    /// The date of the revision read, e.g. 2026-11-17, for the credits.
    pub revision: String,
    /// Its text, plain, cut to [`MAX_ARTICLE_CHARS`].
    pub text: String,
    /// The name of its main image, e.g. `Flag_of_Japan.svg`, if it has one.
    pub image: Option<String>,
}

/// A freely licensed image for a quiz, from Wikimedia Commons.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// Its name, e.g. `Flag of Japan.svg`.
    pub name: String,
    /// What it shows, from its description, for Claude to decide whether a question can use it.
    pub description: String,
    /// Where to download it from: a rendering a few hundred pixels wide, as PNG or JPEG, so that every client can
    /// show it, SVG ones too.
    pub download: String,
    /// Its page on Commons, for the credits.
    pub page: String,
    pub author: String,
    /// E.g. CC BY-SA 4.0.
    pub licence: String,
}

/// A client for Wikipedia's API.
pub struct Wikipedia {
    agent: ureq::Agent,
}

impl Wikipedia {
    pub fn new() -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .new_agent();
        Self { agent }
    }

    /// Asks Wikipedia's API, a moment after the last request, as Wikimedia asks of automated clients, and again after
    /// a wait if it says there have been too many requests.
    fn get(&self, query: &[(&str, &str)]) -> anyhow::Result<String> {
        let mut wait = REQUEST_GAP;
        for attempt in 1.. {
            std::thread::sleep(wait);
            let mut request = self.agent.get(API);
            for (key, value) in query.iter().chain(&[("format", "json"), ("formatversion", "2")]) {
                request = request.query(*key, *value);
            }
            match request.call() {
                Ok(mut response) => {
                    return response
                        .body_mut()
                        .read_to_string()
                        .context("cannot read Wikipedia's answer");
                }
                Err(ureq::Error::StatusCode(429)) if attempt < RETRIES => wait = Duration::from_secs(2 << attempt),
                Err(error) => return Err(error).context("cannot reach Wikipedia"),
            }
        }
        unreachable!()
    }

    /// The titles of the articles best matching a topic, best first.
    pub fn search(&self, topic: &str, count: usize) -> anyhow::Result<Vec<String>> {
        let count = count.to_string();
        let json = self.get(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", topic),
            ("srlimit", &count),
        ])?;
        parse_search(&json)
    }

    /// An article's text, address, revision and main image.
    pub fn article(&self, title: &str) -> anyhow::Result<Article> {
        let json = self.get(&[
            ("action", "query"),
            ("prop", "extracts|revisions|info|pageimages"),
            ("titles", title),
            ("explaintext", "1"),
            ("exsectionformat", "plain"),
            ("rvprop", "ids|timestamp"),
            ("inprop", "url"),
            ("piprop", "name"),
            ("redirects", "1"),
        ])?;
        parse_article(&json).with_context(|| format!("cannot read the article {title}"))
    }

    /// An image's licence, author and a rendering to download, if it is freely licensed; None if it is not, or is
    /// not on Commons.
    pub fn picture(&self, name: &str) -> anyhow::Result<Option<Picture>> {
        let title = format!("File:{name}");
        let json = self.get(&[
            ("action", "query"),
            ("titles", &title),
            ("prop", "imageinfo"),
            ("iiprop", "url|extmetadata"),
            ("iiurlwidth", "800"),
        ])?;
        parse_picture(&json)
    }

    /// Downloads a picture's rendering into a file.
    pub fn download(&self, picture: &Picture, path: &Path) -> anyhow::Result<()> {
        let mut response = self
            .agent
            .get(&picture.download)
            .call()
            .with_context(|| format!("cannot download {}", picture.name))?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(20 * 1024 * 1024)
            .read_to_vec()
            .with_context(|| format!("cannot download {}", picture.name))?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
    }
}

#[derive(Deserialize)]
struct Answer<T> {
    query: Option<T>,
}

fn parse_search(json: &str) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Query {
        search: Vec<Hit>,
    }
    #[derive(Deserialize)]
    struct Hit {
        title: String,
    }
    let answer: Answer<Query> = serde_json::from_str(json).context("Wikipedia's search answer is not as expected")?;
    Ok(answer
        .query
        .map(|query| query.search.into_iter().map(|hit| hit.title).collect())
        .unwrap_or_default())
}

fn parse_article(json: &str) -> anyhow::Result<Article> {
    #[derive(Deserialize)]
    struct Query {
        pages: Vec<Page>,
    }
    #[derive(Deserialize)]
    struct Page {
        title: String,
        #[serde(default)]
        missing: bool,
        fullurl: Option<String>,
        extract: Option<String>,
        #[serde(default)]
        revisions: Vec<Revision>,
        pageimage: Option<String>,
    }
    #[derive(Deserialize)]
    struct Revision {
        timestamp: String,
    }
    let answer: Answer<Query> = serde_json::from_str(json).context("Wikipedia's answer is not as expected")?;
    let page = answer
        .query
        .and_then(|query| query.pages.into_iter().next())
        .context("Wikipedia has no such article")?;
    anyhow::ensure!(!page.missing, "Wikipedia has no article {}", page.title);
    let mut text = page.extract.unwrap_or_default();
    if let Some((cut, _)) = text.char_indices().nth(MAX_ARTICLE_CHARS) {
        text.truncate(cut);
    }
    Ok(Article {
        url: page
            .fullurl
            .unwrap_or_else(|| format!("https://en.wikipedia.org/wiki/{}", page.title.replace(' ', "_"))),
        title: page.title,
        revision: page
            .revisions
            .first()
            .map(|revision| revision.timestamp.chars().take(10).collect())
            .unwrap_or_default(),
        text,
        image: page.pageimage,
    })
}

/// The licences an image may have: free ones only, which allow anyone to copy it, never non-free or fair use.
fn is_free(licence: &str) -> bool {
    let licence = licence.trim().to_ascii_lowercase();
    licence == "cc0"
        || licence.starts_with("cc0 ")
        || licence.starts_with("public domain")
        || licence == "pd"
        || licence.starts_with("pd-")
        || licence.starts_with("cc by ")
        || licence.starts_with("cc by-sa ")
        || licence == "cc by"
        || licence == "cc by-sa"
}

fn parse_picture(json: &str) -> anyhow::Result<Option<Picture>> {
    #[derive(Deserialize)]
    struct Query {
        pages: Vec<Page>,
    }
    #[derive(Deserialize)]
    struct Page {
        title: String,
        #[serde(default)]
        imageinfo: Vec<Info>,
    }
    #[derive(Deserialize)]
    struct Info {
        thumburl: Option<String>,
        url: String,
        descriptionurl: String,
        #[serde(default)]
        extmetadata: std::collections::HashMap<String, Field>,
    }
    #[derive(Deserialize)]
    struct Field {
        value: serde_json::Value,
    }
    let answer: Answer<Query> = serde_json::from_str(json).context("Wikipedia's image answer is not as expected")?;
    let Some(page) = answer.query.and_then(|query| query.pages.into_iter().next()) else {
        return Ok(None);
    };
    let Some(info) = page.imageinfo.into_iter().next() else {
        return Ok(None);
    };
    let field = |name: &str| {
        info.extmetadata
            .get(name)
            .map(|field| match &field.value {
                serde_json::Value::String(text) => strip_html(text),
                other => other.to_string(),
            })
            .unwrap_or_default()
    };
    let licence = field("LicenseShortName");
    // Non-free images, e.g. logos used on Wikipedia as fair use, are marked so, and never used.
    if field("NonFree").eq_ignore_ascii_case("true") || !is_free(&licence) {
        return Ok(None);
    }
    let author = match field("Artist") {
        artist if artist.is_empty() => "Unknown".to_string(),
        artist => artist,
    };
    Ok(Some(Picture {
        name: page.title.trim_start_matches("File:").to_string(),
        description: field("ImageDescription"),
        download: info.thumburl.unwrap_or(info.url),
        page: info.descriptionurl,
        author,
        licence,
    }))
}

/// Text without its HTML tags and with its spaces tidied, as Commons' descriptions and authors are HTML.
fn strip_html(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_search_results() {
        let json = r#"{"batchcomplete":true,"query":{"searchinfo":{"totalhits":2},"search":[
            {"ns":0,"title":"Flag of Japan","pageid":1},{"ns":0,"title":"Japan","pageid":2}]}}"#;
        assert_eq!(parse_search(json).unwrap(), ["Flag of Japan", "Japan"]);
    }

    #[test]
    fn reads_an_article() {
        let json = r#"{"batchcomplete":true,"query":{"pages":[{"pageid":1,"ns":0,"title":"Flag of Japan",
            "contentmodel":"wikitext","fullurl":"https://en.wikipedia.org/wiki/Flag_of_Japan",
            "extract":"The national flag of Japan is a rectangular white banner bearing a crimson-red circle.",
            "revisions":[{"revid":123,"parentid":122,"timestamp":"2026-11-02T10:11:12Z"}],
            "pageimage":"Flag_of_Japan.svg"}]}}"#;
        let article = parse_article(json).unwrap();
        assert_eq!(article.title, "Flag of Japan");
        assert_eq!(article.url, "https://en.wikipedia.org/wiki/Flag_of_Japan");
        assert_eq!(article.revision, "2026-11-02");
        assert!(article.text.starts_with("The national flag"));
        assert_eq!(article.image.as_deref(), Some("Flag_of_Japan.svg"));
        let missing = r#"{"query":{"pages":[{"ns":0,"title":"Nothing here","missing":true}]}}"#;
        assert!(parse_article(missing).is_err());
    }

    #[test]
    fn keeps_only_free_images() {
        let picture = |licence: &str, non_free: &str| {
            format!(
                r#"{{"query":{{"pages":[{{"title":"File:Flag of Japan.svg","imageinfo":[{{
                "thumburl":"https://upload.wikimedia.org/thumb/800px-Flag_of_Japan.svg.png",
                "url":"https://upload.wikimedia.org/Flag_of_Japan.svg",
                "descriptionurl":"https://commons.wikimedia.org/wiki/File:Flag_of_Japan.svg",
                "extmetadata":{{
                    "LicenseShortName":{{"value":"{licence}"}},
                    "NonFree":{{"value":"{non_free}"}},
                    "Artist":{{"value":"<a href=\"//commons.wikimedia.org/wiki/User:X\">Some &amp; One</a>"}},
                    "ImageDescription":{{"value":"The <b>flag</b> of Japan"}}}}}}]}}]}}}}"#
            )
        };
        let free = parse_picture(&picture("Public domain", "false")).unwrap().unwrap();
        assert_eq!(free.name, "Flag of Japan.svg");
        assert_eq!(free.author, "Some & One");
        assert_eq!(free.description, "The flag of Japan");
        assert_eq!(free.licence, "Public domain");
        assert!(free.download.ends_with(".png"), "an SVG is downloaded as PNG");
        assert!(parse_picture(&picture("CC BY-SA 4.0", "")).unwrap().is_some());
        assert!(parse_picture(&picture("Fair use", "")).unwrap().is_none());
        assert!(parse_picture(&picture("CC BY-SA 4.0", "true")).unwrap().is_none());
        assert!(parse_picture(&picture("CC BY-NC 2.0", "")).unwrap().is_none());
    }
}
