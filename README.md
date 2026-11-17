# Bruce's Quiz Party

A multiplayer quiz party game. A Mac runs the party and shows the big screen, on a TV or projector; players join on
their phones, in the browser or the iOS app, and answer multiple-choice questions, scoring more the faster they answer.

**Status:** early development. A party of several quizzes can be played with phones in the browser or the iOS app:
questions, timed answers, the reveal, the standings, and each quiz's and the party's leaderboards. The host can run it
from the Mac or a paired iPhone, pause it, remove players, end it, or let it move on by itself. Quizzes can be
written and edited on the Mac, or drafted from Wikipedia by the quiz generator. Quizzes can be shared in a library, and downloaded from one on the
Quizzes page. See [docs/Plan.md](docs/Plan.md) for the plan and its milestones.

## Running

The controller is a [Dioxus](https://dioxuslabs.com) desktop app. With Rust and the Dioxus CLI (`cargo binstall
dioxus-cli`) installed:

```sh
cd server
dx serve --args="--quizzes ../quizzes"
```

The Party page starts in setup: choose the party's quizzes and whether it moves on by itself, and pair your iPhone as
a host by scanning the code shown there, if you like. Then press Start the Party for the lobby: phones on the same
Wi-Fi scan its QR code, or go to the address under it and type the game code, unless you chose in setup that no code
is needed. The code stays the same until you press New Code. Press Start beside a quiz to play it;
the buttons at the bottom, or the host phone, move on, pause, or end the party. The first time, macOS asks whether the app may accept incoming network connections: choose Allow, or the
phones cannot reach it.

Without `--quizzes` it uses the quizzes folder in its data folder, `~/Library/Application Support/bruce-quiz-party/quizzes`.

## The iOS app

Players can join in the browser, or in the iOS app, which finds parties on the Wi-Fi by itself (Bonjour) or scans the
join QR code. To build it and install it on an iPhone connected to the Mac, with Xcode installed:

```sh
cp ios/Signing.xcconfig.example ios/Signing.xcconfig   # once: set your Apple team in it
ios/install.sh
```

The phone must be paired with the Mac and have Developer Mode on (Settings ▸ Privacy & Security). Signing is set only
in `Signing.xcconfig`: a paid Apple Developer team's installs last a year, a free Personal Team's 7 days.

## Quizzes

The Quizzes page lists the quizzes, with Edit and New Quiz: the editor has a graph of the points a correct answer
scores as time passes, for choosing the scoring, and saves the quiz as a person would write it. A question's image is
chosen from the images in the quiz's folder; Show in Finder opens it to add them.

The quiz generator, a separate app for drafting quizzes, reads Wikipedia's articles on a topic and has Claude write
the questions, using Claude Code on your own subscription (`claude` on the PATH, or the copy that comes with its VS
Code extension). It credits the articles and images under CC BY-SA, using only freely licensed images, and saves the
draft in the quizzes folder, to review in the controller's editor:

```sh
cd generator
dx serve
```

The generator saves either into a quizzes folder, to play the quiz there, or into a local checkout of a library's git
repository, committing and pushing it if you ask, to share it. A library is a folder of quizzes with an index,
`library.json`, on the web or on disk: the controller's Quizzes page lists Bruce's,
[bruce-quiz-library](https://github.com/bruceskingle/bruce-quiz-library), or any other you give it, and downloads
quizzes from it.

Each quiz is a folder with a Markdown file, `quiz.md`, and its images, which can be edited by hand too: see
[docs/QuizFormat.md](docs/QuizFormat.md), and [quizzes/around-the-world](quizzes/around-the-world/quiz.md) for an
example. Quizzes drawn from Wikipedia are published under CC BY-SA 4.0, with credits for their articles and images.

## For developers

- [AGENTS.md](AGENTS.md): the layout of the repo, building, testing and the conventions.
- [docs/Protocol.md](docs/Protocol.md): the messages between the controller and the phones, and how timing is made fair.
- `cargo test --workspace` runs the tests, and `swift test --package-path ios/QuizPartyKit` the iOS app's;
  `cargo deny check` checks the dependencies' licences.

License
=======

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

This covers the app's code. The quizzes in `quizzes/` are content, under the licence each quiz file names.

Dependencies' licences
----------------------

A dependency is licensed MIT, Apache-2.0 or both, or another permissive licence approved by name; what it pulls in is
permissive too, never copyleft. `cargo deny check`, see [deny.toml](deny.toml), checks every crate in the build against
that, listing by name each one under another licence.

Contribution
------------

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
