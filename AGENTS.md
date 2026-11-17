# Notes for Coding Agents

Bruce's Quiz Party is a multiplayer quiz party game: a Mac app runs the party and the big screen, and players answer
multiple-choice questions on their phones, in a browser or the iOS app, scoring more the faster they answer. The plan
and its milestones are in [docs/Plan.md](docs/Plan.md).

This is also an experiment in how well Claude Code can build an app without Bruce writing any code: do all the
implementation, testing and build scripting, and ask Bruce only for decisions, paths and credentials. Ask before a
major design change, and give a short summary after each milestone. Leave changes uncommitted for Bruce to review.

## Layout

Each project can be opened on its own in VS Code (and the iOS app in Xcode).

- `quiz-format/`: the `bruce-quiz-format` crate, which reads and writes quiz files and scores answers, used by both Mac
  apps. The format is documented in [docs/QuizFormat.md](docs/QuizFormat.md).
- `protocol/`: the `bruce-quiz-protocol` crate, the messages between the controller and the phones, and their JSON
  Schema in `protocol/schema/`, which the iOS app's Swift types follow. See [docs/Protocol.md](docs/Protocol.md).
- `server/`: the party controller, `bruce-quiz-party`, a Dioxus desktop app: runs the party, shows the big screen,
  edits quizzes, and serves the browser client. `src/party.rs` (players) and `src/game.rs` (asking, timing, scoring) are
  the game's state and rules alone, with the time passed in, tested without a network; `src/big_screen.rs` is what
  the Mac and the TV show; `src/net.rs` is the web server and WebSocket (axum and Tokio, on a thread of its own), on
  port 8765 if free; `src/bonjour.rs` advertises it through macOS's own mDNS responder; `tests/join.rs` and
  `tests/game.rs` and `tests/host.rs` play the protocol over real sockets; `examples/headless.rs` runs a party
  without its window, and with `--setup`, waits in setup for a host phone to pair.
- `web/`: the browser client, plain HTML, CSS and JavaScript with no build step, built into the controller with
  `include_str!`, so the controller must be rebuilt to serve a change.
- `quizzes/`: sample quizzes, each a folder with a `quiz.md` and its images.
- `ios/`: the iOS app, SwiftUI, iOS 18 and later. `QuizParty/` is its screens, in a synchronised folder, so a new
  Swift file there needs no change to the project; `QuizPartyKit/` is a local Swift package of the rest (the
  messages, the connection, Bonjour browsing, the answer styles), so that `swift test` tests it on the Mac. Its tests
  read `protocol/schema/examples-*.json`, which the protocol's Rust tests write. Formatted with `swift-format`
  (`ios/.swift-format`).
- Still to come: `generator/` (the quiz generator, which uses Claude Code and Wikipedia; Bruce's own tool, not part of
  the controller).

The shared crates are specific to this app, so they live in this repo's Cargo workspace rather than being published
to crates.io, unlike Bruce's other libraries.

## Building and Running

- Run the controller with `dx serve` in `server/`, not `cargo run`, as Dioxus bundles the assets. Give it arguments
  with `--args`, e.g. `dx serve --args="--quizzes ../quizzes"`. `cargo build` and `cargo test` are fine.
- `cargo test --workspace` runs every test. After changing a message in `protocol/`, regenerate its schema with
  `UPDATE_SCHEMA=1 cargo test -p bruce-quiz-protocol`, and change the Swift types to match, in
  `ios/QuizPartyKit/Sources/QuizPartyKit/Protocol.swift`; `swift test --package-path ios/QuizPartyKit` checks them.
- `ios/install.sh` builds the iOS app and installs it on the connected iPhone. Signing's settings are only in the
  git-ignored `ios/Signing.xcconfig` (see `Signing.xcconfig.example`), never in the project file.
- To try the iOS app in the simulator without Bruce: run `cargo run -p bruce-quiz-party --example headless`, boot a
  simulator with `xcrun simctl boot` (it stays headless), install the Debug build, and launch it with
  `-autoJoin YES -quizparty:name Robo -joinLink "quizparty://join?host=127.0.0.1&port=8765&code=ABCD"`. Screenshots
  of the simulator with `xcrun simctl io <id> screenshot` are fine, as they show only the simulated phone. Shut it
  down afterwards. For host mode, run the headless party with `-- quizzes --setup` and launch a second simulator with
  `-hostLink` and the pairing link it prints, its host changed to 127.0.0.1. Bruce's own controller may hold port
  8765, so use the port the headless party prints, and leave his running.
- Run `cargo fmt` after a change (`rustfmt.toml`, `max_width = 120`).
- `cargo deny check` should pass: see the licences rule in the README and `deny.toml`.
- Don't take screenshots of Bruce's screen or drive apps with `osascript` to check a page: ask Bruce to look.

## Conventions

- **The look** is Bruce's Smart Home's, VS Code's dark mode: `server/assets/main.css` is copied from its `main.css`,
  so keep the shared parts in step until they move into a shared crate. The big screen and the phones are a game, so
  they may be brighter and bolder, with the same fonts and feel.
- **Answer colours and shapes** are `ANSWER_STYLES` in the protocol crate, used identically everywhere; a test checks
  that `main.css`, the browser client and the iOS app match it.
- **The controller is the authority**: game state, the clock and scores live only in the controller. Clients show what
  they are sent and send what is pressed.
- **Quiz files** are edited by hand too: keep `Quiz::to_markdown` writing what a person would, and the parser's errors
  giving the line of the problem.
- **Licences of quiz content**: quizzes drawn from Wikipedia are CC BY-SA 4.0, crediting every article and image; only
  freely licensed images from Wikimedia Commons, never non-free or fair-use ones.
- **Errors** are formatted with `{error:#}` on an `anyhow::Error`, their chain of causes, never `{error:?}`.
- **Comments** are doc comments in full sentences, saying what a thing is for and why.
- **Signing**'s settings, the team and identities, are only in a git-ignored file per app, e.g.
  `ios/Signing.xcconfig`, so that changing team, or moving between the free Personal Team or ad-hoc signing and a paid
  account, changes no checked-in file. Where the tools sign as part of the build, as Xcode does for iOS, that is
  enough; it need not be a separate step.
