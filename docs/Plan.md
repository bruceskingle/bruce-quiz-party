# The Plan

Bruce's Quiz Party, from the project spec of 27 Oct 2026, with the decisions made since. This is the plan of record:
change it here when a decision changes it.

A multiplayer quiz party game: a Mac app runs the party and the big screen, and players answer multiple-choice
questions on their phones, scoring more the faster they answer.

## Decisions since the spec

- **Open source, not sold** (13 Nov 2026): licensed MIT OR Apache-2.0 like Bruce's other projects. The spec's "the
  controller may be sold" no longer applies.
- **Shared crates in this repo** (13 Nov 2026): `bruce-quiz-format` and `bruce-quiz-protocol` are specific to this
  app, so they stay in this repo's Cargo workspace and are not published.
- **The look** (13 Nov 2026): Bruce's Smart Home's stylesheet is copied in, rather than first making a shared UI crate.
- **No commits by Claude**: each milestone is left uncommitted for Bruce to review, not committed as the spec said.
- **Signing with the paid account** (16 Nov 2026): the iOS app is signed with Bruce's paid Apple Developer team, not
  the free Personal Team, so installs last a year rather than 7 days. The team is set only in the git-ignored
  `ios/Signing.xcconfig`; the iOS bundle ID is the Mac app's, `org.skingle.bruce-quiz-party`.
- **Auto-advance** (17 Nov 2026): an option, off by default, for the party to move on by itself: the reveal for a few
  seconds, then the standings for a time set by the host (5 seconds by default), then the next question. It stops at
  the party's leaderboard for the host to choose the next quiz. The big screen and the phones count down to moving on,
  and Next pressed on the reveal or the standings counts down too, 3 seconds by default, set in setup.
- **Host tokens** (17 Nov 2026): the Mac keeps host phones' tokens in memory, for as long as it runs; a new party keeps
  the phones paired. The phone keeps its token in the Keychain.
- **Game code** (17 Nov 2026): made once as the controller starts and kept from party to party, until the host presses
  New Code, in setup or the lobby; setup has a checkbox for whether players need it at all. A player who rejoins with
  their token needs no code, so a new code shuts out only newcomers. The browser client and the iOS app remember the
  party last joined and its code, so a player can come back after leaving.
- **Answer colours and shapes** (17 Nov 2026): kept as they are for now, though the pairing resembles other quiz
  games'.
- **The generator reads Wikipedia itself** (17 Nov 2026): it searches, reads the articles and checks the images'
  licences, and gives Claude only their text, with no tools, asking for the questions as JSON; so the credits and
  licences are exact and checked in code. Lists are read after other articles, and only the articles questions come
  from are credited. It names itself to Wikimedia with the project's GitHub address (Bruce's choice, 17 Nov 2026). The
  Anthropic API key option is left for later; it uses Claude Code on Bruce's subscription.
- **Libraries** (17 Nov 2026): a library is a folder of quiz folders with an index, `library.json`, served from
  anywhere, e.g. a GitHub repository's raw files; Bruce's is `bruceskingle/bruce-quiz-library`. The generator saves
  into a quizzes folder or a library's local checkout, committing and pushing with the user's git if asked, so that
  anyone can keep their own. The controller downloads from any library, Bruce's by default, trusting only safe paths
  and quizzes which read.
- **Names**: the app is "Bruce's Quiz Party", the crates `bruce-quiz-*`, the Bonjour service `_quizparty._tcp`.

## Projects

- `server/`: the party controller, a Dioxus desktop app for macOS. Runs the party, shows the big-screen display,
  hosts the quiz editor, and serves the browser client. It has no dependency on Claude or Claude Code.
- `generator/`: the quiz generator, a separate Dioxus Mac app which uses Claude Code and Wikipedia to write quiz files.
  For Bruce's own use.
- `ios/`: the iOS app, SwiftUI, a thin client with a player mode and a host mode. All game logic is in the controller.
- `web/`: the browser player client, served by the controller, for guests without the iOS app.
- `protocol/`: the message definitions and their JSON Schema, so that Rust and Swift stay in step.
- `quiz-format/`: reads and writes the quiz file format, used by both Mac apps.
- `quizzes/`: sample quiz files.

## Architecture and joining

- The controller is the authority: it holds the game state, the clock and the scores. Clients only show what they are
  sent and send answers.
- WebSockets over the local network, served by the controller (axum and tokio inside the Dioxus app), which also
  serves the browser client.
- Fair timing: each answer is timed by the controller, from when the question was sent to that player, corrected for
  the client's round trip, measured with pings. Questions are sent to every player at the same moment. See
  [Protocol.md](Protocol.md).
- Joining as a player: a QR code on the big screen opens the browser client with the game code, or the iOS app by its
  URL scheme; a short game code is shown for typing in; the iOS app also finds the controller with Bonjour
  (`_quizparty._tcp`, with `NSLocalNetworkUsageDescription` and `NSBonjourServices` in its Info.plist).
- Players enter a display name. A phone which drops off and reconnects rejoins as the same player with its score, by a
  token stored on the device.
- The controller may need to allow incoming connections through the macOS firewall: handle and explain the prompt.

## Party flow

A party has one or more quizzes, fewer than 10. The host runs the party and does not play.

1. **Setup mode**, on the Mac only, before guests arrive: the host chooses the party's quizzes. The Mac shows a
   one-time host pairing QR code; scanning it in the iOS app's host mode pairs that phone as a host, with a secure
   token on both sides. More than one phone may be paired.
2. **Party mode**: the pairing QR code is never shown again, so the screen is safe to show on a TV or projector. It
   shows the join QR code, the game code, and the lobby of players.
3. **Choosing the next quiz**: from the Mac or a host phone.
4. **Each question**: sent to every player at once, with a countdown. It ends when time runs out or everyone has
   answered. The big screen reveals the correct answer and how many chose each, then the standings. The host presses
   Next, on the Mac or a host phone; nothing moves on by itself.
5. **End of a quiz**: the quiz's leaderboard, then the party's. Then the host picks the next quiz or ends the party.

The host can do everything from the Mac or a paired host phone: start a quiz, Next, skip a question, pause, remove a
player, end the party.

## Quizzes

- A hand-editable Markdown file per quiz, with its images beside it: see [QuizFormat.md](QuizFormat.md).
- Settings: title, time limit, maximum points, scoring curve (linear or logarithmic) and its floor, which questions may
  override.
- Questions: text, or an image with a short prompt. 2 to 6 text answers, one correct, and a source link, e.g. a
  Wikipedia deep link with its `#section`.
- Scoring: a wrong answer or none scores 0; a correct one up to the maximum, falling with time along the curve to the
  floor. The editor shows a small preview graph of the curve.
- Editor, on the Mac: create, edit, reorder and delete questions; add images; set answers and settings; preview a
  question as on the big screen and a phone.

## Quiz generator

A separate Mac app; the controller only reads the files it writes. The user enters a topic, and optionally the number
of questions and the difficulty; the generator looks up Wikipedia articles and has Claude write a draft quiz, running
Claude Code headless (`claude -p`) with Bruce's own subscription, or optionally an Anthropic API key kept in the
Keychain. The draft is opened in the controller's editor for review before play.

## Licences of quiz content

- Wikipedia's text is CC BY-SA 4.0: quizzes drawn from it are published under the same licence with attribution, so
  they may quote it. Each quiz records the articles used: title, URL and revision date.
- Images: only freely licensed ones from Wikimedia Commons, never non-free or fair-use; each image's author, licence
  and source URL are in the quiz file.
- The controller shows the current quiz's credits on a credits screen and at the end of the quiz.
- Quiz library: the controller can browse and download quizzes from a public online library, probably a GitHub repo;
  ask Bruce for it when building this. Downloaded quizzes keep their licence and credits.
- The app's code is separate from the quiz content and is not covered by CC BY-SA.

## Screens

Each answer position has a fixed colour and shape, the same on the big screen and the phones: red triangle, blue
diamond, yellow circle, green square, purple star, orange hexagon. See [Protocol.md](Protocol.md).

- **Big screen**: the lobby (join QR code, game code, players); the question (text or image and prompt, coloured
  answers, countdown, how many have answered); the reveal (the correct answer, how many chose each, and a QR code to
  the source, a normal `https://en.wikipedia.org/wiki/...` link), then the standings.
- **Player phone**: the question and any image too, with large coloured, shaped answer buttons, 2 to 6, laid out to
  fit, locked after the first tap; after each question their own result, e.g. "Correct! +850 points, you're 3rd".
- **Host phone**: the remote control (choose the next quiz, Next, skip, pause, remove a player, end the party), and
  the current question with its correct answer and who has answered.
- **Leaderboards**: the quiz's, sorted by the quiz's ranks, with each player's quiz score and rank and their party rank
  beside; then the party's, sorted by the party's ranks, with a column per quiz played (score and rank), the total and
  the party rank. Ties share a rank.

## Signing

- The iOS app is signed with Bruce's paid Apple Developer team (decided 16 Nov 2026; the spec planned the free
  Personal Team), its settings only in the git-ignored `ios/Signing.xcconfig`; most guests will use the browser
  client. The Mac app is ad-hoc signed
  (`codesign --sign -`).
- Signing's settings are only in a git-ignored file per app, so that changing team, or moving to TestFlight for iOS
  or Developer ID and notarisation for the Mac, changes no checked-in file; the README says how. Where the tools sign
  as part of the build, as Xcode does for iOS, it need not be a separate step (17 Nov 2026).
- One command each to build and run the Mac app, and to build and install the iOS app on a connected iPhone.

## Milestones

Each ends with a short summary for Bruce, its changes left for him to review.

1. Repo skeleton, house style, protocol definition, sample quiz file.
2. Mac server: load a quiz, lobby with QR code and game code, browser client joins.
3. Full game loop in the browser client: questions, timing, scoring, reveal, Next.
4. Leaderboards: quiz board and party board, multiple quizzes per party.
5. iOS app player mode, with Bonjour discovery and QR joining.
6. Setup mode, host pairing QR code and iOS host mode.
7. Quiz editor on the Mac, with scoring curve preview.
8. Separate quiz generator app (Claude Code and Wikipedia), then the online quiz library download in the controller.
9. Polish, reconnection handling, README. The icon is done for the iOS app and the controller, from `art/icon.png`.
