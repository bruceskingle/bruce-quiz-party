//! Writes a quiz file in the same layout a person would, so that one written by the editor or the generator is easy to
//! read and edit by hand.

use std::fmt::Write;

use crate::{
    Curve, Quiz, Settings,
    parse::{FRONT_MATTER_FENCE, FrontMatter, SETTINGS_FENCE, SOURCE_PREFIX},
};

impl Quiz {
    /// The text of the quiz's `quiz.md`, which [`Quiz::parse`] reads back as the same quiz.
    pub fn to_markdown(&self) -> String {
        let default = Settings::default();
        let scoring = &self.settings.scoring;
        let front = FrontMatter {
            title: self.title.clone(),
            description: self.description.clone(),
            licence: self.licence.clone(),
            time_limit: Some(self.settings.time_limit),
            max_points: Some(self.settings.max_points),
            scoring: Some(scoring.curve),
            floor: Some(scoring.floor),
            // Only written when it matters, or has been changed, as it is not used by a linear curve.
            steepness: (scoring.curve == Curve::Logarithmic || scoring.steepness != default.scoring.steepness)
                .then_some(scoring.steepness),
            sources: self.sources.clone(),
            images: self.images.clone(),
        };
        let mut out = String::new();
        out.push_str(FRONT_MATTER_FENCE);
        out.push('\n');
        out.push_str(&toml::to_string(&front).expect("a quiz's front matter is always valid TOML"));
        out.push_str(FRONT_MATTER_FENCE);
        out.push('\n');

        for question in &self.questions {
            let _ = write!(out, "\n## {}\n\n", one_line(&question.text));
            if let Some(image) = &question.image {
                let _ = write!(out, "![{}]({})\n\n", one_line(&image.alt), image.file);
            }
            for (i, answer) in question.answers.iter().enumerate() {
                let mark = if i == question.correct { 'x' } else { ' ' };
                let _ = writeln!(out, "- [{mark}] {}", one_line(answer));
            }
            if let Some(source) = &question.source {
                let _ = write!(out, "\n{SOURCE_PREFIX} {source}\n");
            }
            if !question.settings.is_empty() {
                let block = toml::to_string(&question.settings).expect("a question's settings are always valid TOML");
                let _ = write!(out, "\n{SETTINGS_FENCE}\n{block}```\n");
            }
        }
        out
    }
}

/// Text on one line, as the format has no way to continue a question or answer onto the next.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
