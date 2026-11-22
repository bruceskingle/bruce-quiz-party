# Bruce's Quiz Party on the App Store

What App Store Connect is given for Bruce's Quiz Party: its privacy policy, which App Store Connect links to, the text of
its listing, its screenshots and the notes for App Review. The iOS app and the Mac app have the same bundle identifier,
`org.skingle.bruce-quiz-party`, so they are one app in App Store Connect, with an iOS version and a macOS version: the
app's information is shared, and each version has its own text and screenshots.

## Privacy Policy

*Bruce's Quiz Party, for iPhone, iPad and Mac. Last changed 21 November 2026.*

Bruce's Quiz Party collects no data about you. It has no accounts, no analytics, no advertising and no tracking, and
its author receives nothing from it.

**The party stays on your Wi-Fi.** The Mac app runs the party itself. Phones join it over your local network, in the
iOS app or a browser, and send it only what the game needs: the name each player types, and their answers. These
stay on the host's Mac, in memory, and are gone when the party ends. Nothing about a party is sent over the internet.

**On the iPhone or iPad,** the app asks for:

- **The local network,** to find parties on your Wi-Fi and join them.
- **The camera,** only to scan the QR code on the big screen. Nothing it sees is kept or sent anywhere.

It remembers, on the device, the name you last played as and the party you last joined, so that you can rejoin.

**On the Mac,** the app keeps your quizzes, and its settings, in its own folder, and reads or writes other folders
only when you choose them. It goes online only when you ask it to:

- **Quiz libraries:** downloading quizzes from a library on the web, e.g. GitHub, which sees an ordinary web request.
- **Write a Quiz with Claude:** reading Wikipedia's articles on the topic you give, and sending them to Anthropic's
  Claude API, with your own API key, to write the questions. The key is kept in your Mac's Keychain and is sent only to
  Anthropic. What Anthropic does with the request is set out in its own privacy policy, at
  [anthropic.com/legal/privacy](https://www.anthropic.com/legal/privacy).

**Children.** The app collects nothing from anyone, children included.

**Changes** to this policy are made on this page, with the date above.

**Questions:** open an issue at [github.com/bruceskingle/bruce-quiz-party](https://github.com/bruceskingle/bruce-quiz-party/issues).

## App Information

Shared by both platforms.

| Field | Value |
|---|---|
| Name | Bruce's Quiz Party |
| Subtitle (at most 30 characters) | Quiz night on your big screen |
| Primary category | Games, with the subcategories Trivia and Family |
| Secondary category | Entertainment |
| Content rights | Yes, it contains third-party content, and I have the rights to use it: the sample quizzes' text and pictures come from Wikipedia and Wikimedia Commons, under CC BY-SA and other free licences, credited in each quiz |
| Age rating | Answer "None" throughout; nothing is unrestricted web access, as the app shows only quizzes |
| Privacy policy URL | The [Privacy Policy](#privacy-policy) above, on GitHub, see below |
| App Privacy | Data Not Collected |
| Price | Free, in every country and region |

The privacy policy's URL must work when App Review reads it. Until this page is on `main`, give the release branch's:
`https://github.com/bruceskingle/bruce-quiz-party/blob/v0.1.0/docs/AppStore.md#privacy-policy`, and after that,
`https://github.com/bruceskingle/bruce-quiz-party/blob/main/docs/AppStore.md#privacy-policy`, which lasts.

## The iOS Version

**Promotional text** (at most 170 characters, can be changed without a new version):

> Quiz night with friends and family: the questions on the big screen, the answers on your phone, and points for being
> quick. Join any party run by Bruce's Quiz Party on a Mac.

**Description:**

> Bruce's Quiz Party is a quiz night for a room full of people. The questions are on the big screen, a TV or a
> projector, run by Bruce's Quiz Party on a Mac; everyone answers on their own phone, and the quicker you answer, the
> more you score.
>
> This app is the player's side of the party:
>
> • Join in seconds: the app finds parties on your Wi-Fi, or scan the QR code on the big screen.
> • Answer each question on big, coloured buttons that match the answers on the screen.
> • See how you did after each question: right or wrong, your points, and how everyone else answered.
> • Follow the standings as the quiz goes on, and the party's final scores at the end.
> • Rejoin as the same player, with the same score, if your phone drops out.
>
> The host can use the app too: pair it with the Mac to start quizzes, move the party on, pause it, and remove players,
> from anywhere in the room.
>
> You need a Mac running Bruce's Quiz Party, free on the Mac App Store, on the same Wi-Fi. Players without the app can
> join in their phone's browser instead.
>
> Bruce's Quiz Party is free and open source, with no accounts, no ads and no tracking.

**Keywords** (at most 100 characters; the name's words are already searched):

```
trivia,pub quiz,quiz night,family,multiplayer,game show,questions,wikipedia,big screen,tv,friends
```

**Support URL:** `https://github.com/bruceskingle/bruce-quiz-party`

**Copyright:** `2026 Bruce Skingle`

**Screenshots:** in [app-store/iphone](app-store/iphone) for the 6.9" iPhone (1320 × 2868), and
[app-store/ipad](app-store/ipad) for the 13" iPad (2064 × 2752), in this order:

1. `2-question.png`, a question with its picture
2. `3-reveal.png`, the result, with how everyone answered
3. `4-standings.png`, the standings
4. `1-lobby.png`, joined, waiting for the quiz
5. `5-final.png`, the final scores

They were taken in the simulator with `server/examples/store_screenshots.rs`, which runs a party with a few made-up
players, see its comment.

## The macOS Version

**Promotional text:**

> Host a quiz night: the questions on your TV, everyone answering on their phones. Write your own quizzes, share
> them in a library, or have Claude draft one from Wikipedia.

**Description:**

> Bruce's Quiz Party turns your Mac into a quiz night. Put the big screen on a TV or projector, and everyone in the
> room joins on their phone, in the free iOS app or any browser, by scanning a QR code. Each multiple-choice question
> is answered against the clock: the quicker the right answer, the more it scores.
>
> • Run a party of several quizzes, with the standings after each question and leaderboards for each quiz and the
>   whole party.
> • Move it on yourself, or let it move on by itself, and pause it whenever you need to.
> • Control it from your iPhone, paired with the Mac, and walk around the room.
> • Pictures with questions, and a link to read more after each answer.
> • Write your own quizzes in the editor, with a graph for choosing how the scoring falls away with time.
> • Download quizzes from a library on the web, or share your own in one.
> • Write a Quiz with Claude: give it a topic, and Claude drafts the questions from Wikipedia's articles, crediting
>   them, for you to review and edit. This needs your own Anthropic API key, kept in your Keychain, and is billed to
>   it.
>
> Everything runs on your own Wi-Fi: no accounts, no ads, no tracking, and no internet connection is needed to play.
> Bruce's Quiz Party is free and open source.

**Keywords:**

```
trivia,pub quiz,quiz night,host,family,multiplayer,game show,quiz maker,wikipedia,tv,projector
```

**Support URL** and **Copyright:** as for iOS.

**Screenshots:** in [app-store/mac](app-store/mac), at 2560 × 1600; App Store Connect takes 2880 × 1800,
2560 × 1600, 1440 × 900 or 1280 × 800. See below for how they were taken.

## App Review Information

Sign-in required: no. Contact: your name, phone and email.

**Notes**, the same for both versions:

> Bruce's Quiz Party is played with two apps together, submitted together: the Mac app runs the party and shows the
> big screen; the iOS app is how players (and, optionally, the host) take part.
>
> To try it:
> 1. On the Mac, open Bruce's Quiz Party. On the Quizzes page, press Get Quizzes from a Library, and Download one or
>    two of the quizzes listed. Then on the Party page, choose them and press Start the Party. Allow incoming network
>    connections if macOS asks.
> 2. On an iPhone or iPad on the same Wi-Fi, open Bruce's Quiz Party and allow local network access. Tap the party
>    under Parties Nearby, or scan the QR code on the Mac, then enter your name and the game code shown on the Mac.
>    (A phone's browser can join too, at the address under the QR code.)
> 3. On the Mac, press Start beside a quiz. Answer on the phone; the Mac's buttons move the party on.
>
> The iOS app has nothing to show without a party on the Mac, as it is the players' controller for it.
>
> Write a Quiz with Claude, on the Mac's Quizzes page, is optional and needs the user's own Anthropic API key; the
> library's quizzes need nothing.

A new install of the Mac app has no quizzes: App Review gets them from the default library,
`https://raw.githubusercontent.com/bruceskingle/bruce-quiz-library/main`, which must be on GitHub, public, with its
`library.json`, before submitting. Without it, the reviewer has nothing to play, and the app is likely to be rejected
for too little function.

**Version release:** Manually release this version, so that both platforms go live together, when you choose.

## Taking the Mac Screenshots

The Mac app is set up in the Party page and played on the big screen, which need the host's clicks, so its
screenshots are taken by hand:

1. Run the app with the sample quizzes, `cd server && dx serve --args="--quizzes ../quizzes"`, and join from the
   iPhone simulator or a phone, so that players are listed.
2. For each screen, e.g. a question on the big screen, the reveal, the Quizzes page and the editor, press
   Shift-Command-4, then Space, then Option-click the window, which saves it without its shadow, on the Desktop. Move
   them into `docs/app-store/mac/`, named for their order, e.g. `1-question.png`.
3. The window opens at 1280 × 832 points, 2560 × 1664 pixels, which App Store Connect does not take: crop each to
   2560 × 1600, e.g. `sips --cropToHeightWidth 1600 2560 <file>`, which trims the top and bottom equally, or resize
   the window first to 1440 × 900 points for 2880 × 1800.
