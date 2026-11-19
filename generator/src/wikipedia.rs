//! Reading Wikipedia for a quiz: searching for a topic's articles, reading their text with the revision read, and
//! finding their images on Wikimedia Commons, kept only if they are freely licensed, with their authors and licences.
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
    /// The name of its main image, e.g. `Flag of Japan.svg`, if it has one.
    pub image: Option<String>,
    /// The names of the other images in it, e.g. for picture questions, less icons and logos.
    pub images: Vec<String>,
}

impl Article {
    /// Cuts its text to at most `count` characters, so that more articles fit in one request to Claude.
    pub fn cut_to(&mut self, count: usize) {
        if let Some((cut, _)) = self.text.char_indices().nth(count) {
            self.text.truncate(cut);
        }
    }

    /// Its images, the main one first, at most `count`.
    pub fn pictures(&self, count: usize) -> impl Iterator<Item = &String> {
        self.image
            .iter()
            .chain(self.images.iter().filter(|name| Some(*name) != self.image.as_ref()))
            .take(count)
    }
}

/// How many images' licences are asked for at once, the most Wikipedia's API allows.
const PICTURES_AT_ONCE: usize = 50;

/// Images smaller than this on either side, in pixels, are icons, not pictures to ask about.
const SMALLEST_PICTURE: u32 = 150;

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

impl Default for Wikipedia {
    fn default() -> Self {
        Self::new()
    }
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

    /// The articles which exist, of those named, under their own titles, e.g. after a redirect, in the order named,
    /// without repeats or disambiguation pages; so that titles suggested by Claude, which may be wrong, are checked.
    pub fn existing(&self, titles: &[String]) -> anyhow::Result<Vec<String>> {
        let mut existing: Vec<String> = Vec::new();
        for titles in titles.chunks(PICTURES_AT_ONCE) {
            let json = self.get(&[
                ("action", "query"),
                ("titles", &titles.join("|")),
                ("prop", "pageprops"),
                ("ppprop", "disambiguation"),
                ("redirects", "1"),
            ])?;
            for title in parse_existing(&json, titles)? {
                if !existing.contains(&title) {
                    existing.push(title);
                }
            }
        }
        Ok(existing)
    }

    /// The articles in a category, e.g. `Bridges in London`, not its subcategories, at most 500, in the order of
    /// their names.
    pub fn category(&self, name: &str) -> anyhow::Result<Vec<String>> {
        let title = format!("Category:{name}");
        let json = self.get(&[
            ("action", "query"),
            ("list", "categorymembers"),
            ("cmtitle", &title),
            ("cmtype", "page"),
            ("cmnamespace", "0"),
            ("cmlimit", "max"),
        ])?;
        parse_category(&json).with_context(|| format!("cannot read the category {name}"))
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
        let mut article = parse_article(&json).with_context(|| format!("cannot read the article {title}"))?;
        // Its images in the order they appear, its own first, then those of the boxes of links at its end, which
        // the query above would list in the order of their names.
        let json = self.get(&[
            ("action", "parse"),
            ("page", &article.title),
            ("prop", "images"),
            ("redirects", "1"),
        ])?;
        article.images = parse_images(&json).with_context(|| format!("cannot read the images of {title}"))?;
        Ok(article)
    }

    /// The images which are freely licensed, of those named, in the order named, each with its licence, author and a
    /// rendering to download; others, and icons, are left out.
    pub fn pictures(&self, names: &[String]) -> anyhow::Result<Vec<Picture>> {
        let mut pictures = Vec::new();
        for names in names.chunks(PICTURES_AT_ONCE) {
            let titles = names
                .iter()
                .map(|name| format!("File:{name}"))
                .collect::<Vec<_>>()
                .join("|");
            let json = self.get(&[
                ("action", "query"),
                ("titles", &titles),
                ("prop", "imageinfo"),
                ("iiprop", "url|size|extmetadata"),
                ("iiurlwidth", "800"),
            ])?;
            let mut found = parse_pictures(&json)?;
            // Wikipedia answers in an order of its own.
            found.sort_by_key(|picture| names.iter().position(|name| *name == picture.name));
            pictures.extend(found);
        }
        Ok(pictures)
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
        // The main image is named with underscores, the others with spaces, as their titles are.
        image: page.pageimage.map(|name| name.replace('_', " ")),
        images: Vec::new(),
    })
}

fn parse_existing(json: &str, asked: &[String]) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Query {
        #[serde(default)]
        normalized: Vec<Renamed>,
        #[serde(default)]
        redirects: Vec<Renamed>,
        #[serde(default)]
        pages: Vec<Page>,
    }
    #[derive(Deserialize)]
    struct Renamed {
        from: String,
        to: String,
    }
    #[derive(Deserialize)]
    struct Page {
        title: String,
        #[serde(default)]
        missing: bool,
        #[serde(default)]
        invalid: bool,
        pageprops: Option<serde_json::Value>,
    }
    let answer: Answer<Query> = serde_json::from_str(json).context("Wikipedia's answer is not as expected")?;
    let Some(query) = answer.query else {
        return Ok(Vec::new());
    };
    let renamed = |title: &str, names: &[Renamed]| {
        names
            .iter()
            .find(|name| name.from == title)
            .map(|name| name.to.clone())
            .unwrap_or_else(|| title.to_string())
    };
    Ok(asked
        .iter()
        .map(|title| renamed(&renamed(title, &query.normalized), &query.redirects))
        .filter(|title| {
            query.pages.iter().any(|page| {
                page.title == *title
                    && !page.missing
                    && !page.invalid
                    && page
                        .pageprops
                        .as_ref()
                        .is_none_or(|props| props.get("disambiguation").is_none())
            })
        })
        .collect())
}

fn parse_category(json: &str) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Query {
        categorymembers: Vec<Member>,
    }
    #[derive(Deserialize)]
    struct Member {
        title: String,
    }
    let answer: Answer<Query> = serde_json::from_str(json).context("Wikipedia's answer is not as expected")?;
    Ok(answer
        .query
        .map(|query| query.categorymembers.into_iter().map(|member| member.title).collect())
        .unwrap_or_default())
}

/// The name of the category a line names, e.g. `Bridges in London` from `Category:Bridges in London` or its address,
/// or None if it names an article.
pub fn category_name(given: &str) -> Option<String> {
    let title = article_title(given);
    let (prefix, name) = title.split_once(':')?;
    (prefix.trim().eq_ignore_ascii_case("category") && !name.trim().is_empty()).then(|| name.trim().to_string())
}

/// The names of the pictures in an article, in the order they appear, from Wikipedia's parse of it.
fn parse_images(json: &str) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Answer {
        parse: Parsed,
    }
    #[derive(Deserialize)]
    struct Parsed {
        #[serde(default)]
        images: Vec<String>,
    }
    let answer: Answer = serde_json::from_str(json).context("Wikipedia's answer is not as expected")?;
    Ok(answer
        .parse
        .images
        .into_iter()
        .map(|name| name.replace('_', " "))
        .filter(|name| is_picture(name))
        .collect())
}

/// Whether an image in an article may be a picture to ask about, by its name: not an icon or a logo of Wikipedia's
/// own, e.g. `Commons-logo.svg` or `Question book-new.svg`, which many articles have.
fn is_picture(name: &str) -> bool {
    const NOT_PICTURES: &[&str] = &[
        "commons-logo",
        "wiki",
        "question book",
        "edit-clear",
        "ambox",
        "padlock",
        "lock-",
        " icon",
        "-icon",
        "icon.",
        "symbol support",
        "symbol oppose",
        "symbol question",
        "disambig",
        "portal-puzzle",
        "pog.svg",
        "increase2",
        "decrease2",
        "decrease positive",
        "increase negative",
        "steady2",
        "loudspeaker",
        "sound-",
        "folder hexagonal",
        "crystal clear",
        "nuvola",
        "office-book",
        "text document",
        "gnome-",
        "emblem-",
        "oojs",
        "star full",
        "star empty",
        "star half",
    ];
    let lower = name.to_lowercase();
    let picture_type = [".jpg", ".jpeg", ".png", ".svg", ".gif", ".webp", ".tif", ".tiff"]
        .iter()
        .any(|extension| lower.ends_with(extension));
    picture_type && !NOT_PICTURES.iter().any(|not| lower.contains(not))
}

/// An article's title from what a person gives: its title, or its address, e.g.
/// `https://en.wikipedia.org/wiki/Flag_of_Japan#History`, which is copied from a browser more easily.
pub fn article_title(given: &str) -> String {
    let given = given.trim();
    let Some((_, path)) = given.split_once("/wiki/") else {
        return given.to_string();
    };
    let path = path.split(['#', '?']).next().unwrap_or_default();
    percent_decode(path).replace('_', " ")
}

/// Text with its `%xx` escapes decoded, as in a Wikipedia address, e.g. `Caf%C3%A9`.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
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

fn parse_pictures(json: &str) -> anyhow::Result<Vec<Picture>> {
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
        width: u32,
        #[serde(default)]
        height: u32,
        #[serde(default)]
        extmetadata: std::collections::HashMap<String, Field>,
    }
    #[derive(Deserialize)]
    struct Field {
        value: serde_json::Value,
    }
    let answer: Answer<Query> = serde_json::from_str(json).context("Wikipedia's image answer is not as expected")?;
    let pages = answer.query.map(|query| query.pages).unwrap_or_default();
    let mut pictures = Vec::new();
    for page in pages {
        let Some(info) = page.imageinfo.into_iter().next() else {
            continue;
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
            continue;
        }
        // An icon is too small to ask about; a size of 0 is one not given.
        if info.width.min(info.height) > 0 && info.width.min(info.height) < SMALLEST_PICTURE {
            continue;
        }
        let author = match field("Artist") {
            artist if artist.is_empty() => "Unknown".to_string(),
            artist => artist,
        };
        pictures.push(Picture {
            name: page.title.trim_start_matches("File:").to_string(),
            description: field("ImageDescription"),
            download: info.thumburl.unwrap_or(info.url),
            page: info.descriptionurl,
            author,
            licence,
        });
    }
    Ok(pictures)
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
        let mut article = parse_article(json).unwrap();
        let json = r#"{"parse":{"title":"Flag of Japan","pageid":1,"images":["Flag_of_Japan.svg",
            "Hinomaru_at_Tokyo.jpg","Question_book-new.svg","Commons-logo.svg"]}}"#;
        article.images = parse_images(json).unwrap();
        assert_eq!(article.title, "Flag of Japan");
        assert_eq!(article.url, "https://en.wikipedia.org/wiki/Flag_of_Japan");
        assert_eq!(article.revision, "2026-11-02");
        assert!(article.text.starts_with("The national flag"));
        assert_eq!(article.image.as_deref(), Some("Flag of Japan.svg"));
        assert_eq!(article.images, ["Flag of Japan.svg", "Hinomaru at Tokyo.jpg"]);
        assert_eq!(
            article.pictures(5).collect::<Vec<_>>(),
            ["Flag of Japan.svg", "Hinomaru at Tokyo.jpg"]
        );
        assert_eq!(article.pictures(1).collect::<Vec<_>>(), ["Flag of Japan.svg"]);
        let missing = r#"{"query":{"pages":[{"ns":0,"title":"Nothing here","missing":true}]}}"#;
        assert!(parse_article(missing).is_err());
    }

    #[test]
    fn keeps_only_free_images() {
        let picture = |licence: &str, non_free: &str| {
            format!(
                r#"{{"query":{{"pages":[{{"title":"File:Flag of Japan.svg","imageinfo":[{{
                "width":900,"height":600,
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
        let parse_picture = |json: &str| parse_pictures(json).unwrap().into_iter().next();
        let free = parse_picture(&picture("Public domain", "false")).unwrap();
        assert_eq!(free.name, "Flag of Japan.svg");
        assert_eq!(free.author, "Some & One");
        assert_eq!(free.description, "The flag of Japan");
        assert_eq!(free.licence, "Public domain");
        assert!(free.download.ends_with(".png"), "an SVG is downloaded as PNG");
        assert!(parse_picture(&picture("CC BY-SA 4.0", "")).is_some());
        assert!(parse_picture(&picture("Fair use", "")).is_none());
        assert!(parse_picture(&picture("CC BY-SA 4.0", "true")).is_none());
        assert!(parse_picture(&picture("CC BY-NC 2.0", "")).is_none());
        let icon = picture("Public domain", "").replace(r#""width":900,"height":600"#, r#""width":40,"height":30"#);
        assert!(parse_picture(&icon).is_none(), "an icon is not a picture");
    }

    #[test]
    fn checks_which_articles_exist() {
        let json = r#"{"batchcomplete":true,"query":{
            "normalized":[{"fromencoded":false,"from":"eiffel tower","to":"Eiffel tower"}],
            "redirects":[{"from":"Eiffel tower","to":"Eiffel Tower"}],
            "pages":[{"pageid":1,"ns":0,"title":"Eiffel Tower"},
                {"ns":0,"title":"Tower of Nowhere","missing":true},
                {"pageid":3,"ns":0,"title":"Big Ben (disambiguation)","pageprops":{"disambiguation":""}},
                {"pageid":4,"ns":0,"title":"Taj Mahal"}]}}"#;
        let asked = [
            "eiffel tower",
            "Tower of Nowhere",
            "Big Ben (disambiguation)",
            "Taj Mahal",
        ]
        .map(String::from);
        assert_eq!(parse_existing(json, &asked).unwrap(), ["Eiffel Tower", "Taj Mahal"]);
    }

    #[test]
    fn reads_a_category() {
        let json = r#"{"query":{"categorymembers":[{"pageid":1,"ns":0,"title":"Tower Bridge"},
            {"pageid":2,"ns":0,"title":"London Bridge"}]}}"#;
        assert_eq!(parse_category(json).unwrap(), ["Tower Bridge", "London Bridge"]);
        assert_eq!(
            category_name("Category:Bridges in London").as_deref(),
            Some("Bridges in London")
        );
        assert_eq!(
            category_name("https://en.wikipedia.org/wiki/Category:Bridges_in_London").as_deref(),
            Some("Bridges in London")
        );
        assert_eq!(category_name("Tower Bridge"), None);
        assert_eq!(category_name("Star Trek: Voyager"), None);
    }

    #[test]
    fn reads_titles_and_addresses_of_articles() {
        assert_eq!(article_title(" Flag of Japan "), "Flag of Japan");
        assert_eq!(
            article_title("https://en.wikipedia.org/wiki/Flag_of_Japan#History"),
            "Flag of Japan"
        );
        assert_eq!(article_title("https://en.m.wikipedia.org/wiki/Caf%C3%A9?x=1"), "Café");
    }
}
