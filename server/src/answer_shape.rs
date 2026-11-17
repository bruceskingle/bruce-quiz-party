//! The shape of each answer position, drawn as SVG, so that players who cannot tell the colours apart can tell the
//! answers apart. The shapes are those named in protocol's `ANSWER_STYLES`, in the same order.

use dioxus::prelude::*;

/// The SVG path of each shape, in a 24 by 24 box: triangle, diamond, circle, square, star and hexagon.
const SHAPE_PATHS: [&str; 6] = [
    "M12 3 L22 20 L2 20 Z",
    "M12 2 L22 12 L12 22 L2 12 Z",
    "M12 3 A9 9 0 1 1 11.99 3 Z",
    "M4 4 H20 V20 H4 Z",
    "M12 2 L14.9 8.6 L22 9.2 L16.6 13.9 L18.2 21 L12 17.3 L5.8 21 L7.4 13.9 L2 9.2 L9.1 8.6 Z",
    "M7 3 H17 L22 12 L17 21 H7 L2 12 Z",
];

/// The shape of the answer at a position, from 0.
#[component]
pub fn AnswerShape(position: usize) -> Element {
    let path = SHAPE_PATHS[position % SHAPE_PATHS.len()];
    let name = protocol::ANSWER_STYLES[position % protocol::ANSWER_STYLES.len()].shape;
    rsx! {
        svg { view_box: "0 0 24 24", role: "img", "aria-label": name,
            path { d: path }
        }
    }
}

#[cfg(test)]
mod tests {
    /// The answers' colours in main.css are those of protocol's `ANSWER_STYLES`, which the phones use too.
    #[test]
    fn css_colours_match_the_protocol() {
        let css = include_str!("../assets/main.css");
        for (position, style) in protocol::ANSWER_STYLES.iter().enumerate() {
            let rule = format!(".answer-{position} {{ background: {}; }}", style.colour);
            assert!(css.contains(&rule), "main.css lacks {rule}");
        }
    }

    /// The phones' colours and shapes, in the browser client, are the big screen's.
    #[test]
    fn the_browser_client_matches() {
        let css = include_str!("../../web/style.css");
        for (position, style) in protocol::ANSWER_STYLES.iter().enumerate() {
            let rule = format!(".answer-{position} {{ background: {}; }}", style.colour);
            assert!(css.contains(&rule), "web/style.css lacks {rule}");
        }
        let js = include_str!("../../web/app.js");
        for path in super::SHAPE_PATHS {
            assert!(js.contains(&format!("\"{path}\"")), "web/app.js lacks the shape {path}");
        }
    }

    /// The iOS app's colours and shapes are the big screen's too.
    #[test]
    fn the_ios_app_matches() {
        let swift = include_str!("../../ios/QuizPartyKit/Sources/QuizPartyKit/AnswerStyle.swift");
        for style in protocol::ANSWER_STYLES {
            let colour = format!("colour: \"{}\", shape: \"{}\"", style.colour, style.shape);
            assert!(swift.contains(&colour), "AnswerStyle.swift lacks {colour}");
        }
        for path in super::SHAPE_PATHS {
            assert!(
                swift.contains(&format!("path: \"{path}\"")),
                "AnswerStyle.swift lacks the shape {path}"
            );
        }
    }
}
