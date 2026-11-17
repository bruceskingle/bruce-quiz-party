# The Quiz File Format

A quiz is a folder holding a Markdown file, `quiz.md`, and the images its questions show, e.g.:

```
around-the-world/
  quiz.md
  images/
    flag.svg
```

The file is meant to be read and edited by hand, as well as by the controller's editor and the generator, which write
it in the same layout. [quizzes/around-the-world](../quizzes/around-the-world/quiz.md) is an example. The
`bruce-quiz-format` crate reads and writes it, and its errors give the line of the problem.

## Settings

The file starts with the quiz's settings in [TOML](https://toml.io), between two `+++` lines:

```toml
+++
title = "Around the World"
description = "A short sample quiz: capitals, planets, flags and history."
licence = "CC BY-SA 4.0"
time_limit = 20
max_points = 1000
scoring = "linear"
floor = 0.5

[[sources]]
title = "Canberra"
url = "https://en.wikipedia.org/wiki/Canberra"
revision = "2026-10-01"

[[images]]
file = "images/flag.svg"
author = "Bruce's Quiz Party"
licence = "CC0 1.0"
source = "https://commons.wikimedia.org/wiki/File:…"
+++
```

| Key | Required | Default | Meaning |
|---|---|---|---|
| `title` | yes | | The quiz's title. |
| `description` | no | | A sentence or two for the lists of quizzes. |
| `licence` | no | | The licence of the quiz's content, `CC BY-SA 4.0` for one drawn from Wikipedia. |
| `time_limit` | no | 20 | Seconds to answer each question. |
| `max_points` | no | 1000 | Points for a correct answer given at once. |
| `scoring` | no | `linear` | How the points fall as time passes: `linear` or `logarithmic`, see Scoring. |
| `floor` | no | 0.5 | The fraction of `max_points` a correct answer still scores at the time limit, 0 to 1. |
| `steepness` | no | 9 | How steeply a `logarithmic` curve falls at first; larger is steeper. |
| `[[sources]]` | no | | Each article the questions were drawn from: `title`, `url`, and the `revision` date used. |
| `[[images]]` | no | | Each image's credit: its `file`, `author`, `licence`, and `source` page. |

An unknown key is an error, so that a misspelt one is not silently ignored.

## Questions

Each question starts with a `##` heading, its text, followed by its answers as a task list, with the correct one ticked
`[x]`:

```markdown
## What is the capital of Australia?

- [ ] Sydney
- [x] Canberra
- [ ] Melbourne
- [ ] Perth

Source: https://en.wikipedia.org/wiki/Canberra#History
```

- **Answers**: 2 to 6, each on one line, exactly one ticked `[x]`. They are shown in the order written, the first in
  red with a triangle, then blue diamond, yellow circle, green square, purple star and orange hexagon.
- **Image questions**: an image line, `![description](path)`, after the heading and before the answers, makes it an
  image question, the heading being its short prompt, e.g. "Which country's flag is this?". The path is relative to
  the quiz's folder, and the description is for players who cannot see it. Only freely licensed images are used, each
  credited in `[[images]]`.
- **Source**: an optional `Source:` line, a link to where the answer comes from, e.g. a Wikipedia link to the section,
  shown as a QR code when the answer is revealed.
- **Settings**: a question may change the quiz's `time_limit`, `max_points`, `scoring`, `floor` or `steepness` in a
  `toml` block after its answers:

  ````markdown
  ```toml
  time_limit = 15
  scoring = "logarithmic"
  ```
  ````

Blank lines are ignored; any other line is an error.

## Scoring

A wrong answer, or none, scores 0. A correct one scores `max_points` if given at once, falling to `floor` ×
`max_points` at the time limit:

- `linear`: the points fall evenly.
- `logarithmic`: the points fall steeply at first and then flatten, by `1 - ln(1 + steepness × t) / ln(1 + steepness)`,
  where `t` is the fraction of the time limit gone.

The time is measured by the controller, from when it sent the question to that player, less the connection's round
trip, see [Protocol.md](Protocol.md). Points are rounded to the nearest whole point.

## Licence of quizzes

Quizzes drawn from Wikipedia are published under CC BY-SA 4.0, with `licence = "CC BY-SA 4.0"`, every article used in
`[[sources]]`, and every image's author, licence and source in `[[images]]`; the controller shows these credits at the
end of the quiz.
