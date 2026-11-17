# The Protocol

The party controller, the Mac app, runs the party. Players' phones, in the browser client or the iOS app, and host
phones, in the iOS app's host mode, connect to it with a WebSocket on the local network and exchange JSON messages.

The controller is the authority: it holds the game, the clock and the scores. Clients only show what they are sent and
send what is pressed.

The messages are defined in Rust in [protocol/src/lib.rs](../protocol/src/lib.rs), which is the master copy. The JSON
Schema generated from them, [protocol/schema/client.json](../protocol/schema/client.json) and
[protocol/schema/server.json](../protocol/schema/server.json), is what the iOS app's Swift types are checked against. A
test fails if the schema is out of date; regenerate it with:

```sh
UPDATE_SCHEMA=1 cargo test -p bruce-quiz-protocol
```

The tests also write an example of every message to `protocol/schema/examples-client.json` and
`examples-server.json`, and the iOS app's tests check that its Swift types read and write them exactly.

Change `PROTOCOL_VERSION` whenever a change means an old client would misread the messages.

## Connecting

- **WebSocket**: `ws://<mac>:<port>/ws`. The same web server serves the browser client.
- **Bonjour**: the controller advertises `_quizparty._tcp` under the Mac's name, with the TXT record
  `protocol=1 path=/ws`, for the iOS app to find it. The game code is not advertised: a player still needs it from the
  big screen, so that only those in the room can join, unless the host has chosen a party which needs no code.
- **Party info**: `GET /party.json` gives `{"code_required": true}`, for the join screens to ask for the game code
  only if the party needs it. It never gives the code itself.
- **Join QR code**: the browser client's URL with the game code, `http://<mac>:<port>/?code=ABCD`, or without it,
  `http://<mac>:<port>/`, for a party which needs no code.
- **Host pairing QR code**: `quizparty://host?host=<mac>&port=<port>&pair=<code>`, shown on the Mac only while the
  party is set up. It opens the iOS app, which pairs as a host with the one-time code: the code changes as soon as a
  phone pairs, and pairing is refused once the party has started.
- **URL scheme**: `quizparty://join?host=<mac>&port=<port>&code=ABCD` opens the iOS app. The app's own scanner reads
  the join QR code too, so the QR code stays the browser's URL, which works for everyone.

## Messages

Every message is a JSON object whose `type` names it, in `snake_case`.

From a client:

| `type` | Meaning |
|---|---|
| `join` | Joins as a player with `protocol`, `game_code` and `name`, or rejoins with the `token` from a `welcome`, which needs no code. `game_code` may be empty or left out if the party needs none. |
| `answer` | Answers `question` (`{quiz, index}`) with `choice`, from 0. Only the first answer counts. |
| `pong` | Answers a `ping` at once, with its `nonce`. |
| `pair_host` | Pairs the iOS app as a host with the one-time `pairing_code` from the setup QR code. |
| `host_connect` | Connects a paired host phone with its `token`. |
| `host` | A host's `command`: `start_quiz`, `next`, `skip`, `pause`, `resume`, `remove_player`, `end_party`, `set_auto_advance`. Only a paired host's count. |

From the controller:

| `type` | Meaning |
|---|---|
| `welcome` | The player has joined: their `player` number, `name`, and the `token` to keep for rejoining. |
| `rejected` | A join or pairing was refused: `reason` and `message`. The connection is closed. |
| `ping` | Asks for a `pong`, to measure the round trip. |
| `lobby` | The players who have joined. |
| `question` | A question, without its answer, sent to every player at the same moment. |
| `answer_locked` | The player's answer was taken. |
| `answer_count` | How many players have answered so far. |
| `reveal` | The correct answer, how many chose each, the source link, and how this player did. |
| `standings` | The places after a question, other than a quiz's last. |
| `leaderboard` | After a quiz's last question, its leaderboard, sorted by its places, then the party's, sorted by the totals. |
| `countdown` | The party moves on soon, by itself or as the host pressed Next: what comes `next`, e.g. "Next question", and `in_ms`, for a countdown under the reveal or the standings. Whatever the party shows next ends it. |
| `paused`, `resumed` | The clock stopped or started again. While it is stopped, answers are refused; when it starts, the question is sent again with the time it had left. |
| `removed`, `party_ended` | The player was removed, or the party is over. |
| `paired` | The phone is paired as a host: the `token` to keep, securely. |
| `host_state` | Everything a host phone shows, whenever it changes: the game code and whether it is needed, the quizzes, the players, the phase with the correct answer and who has answered, whether it is paused, auto-advance, and what Next does. |

For example:

```json
{"type":"join","protocol":1,"game_code":"ABCD","name":"Sam"}
{"type":"answer","question":{"quiz":0,"index":2},"choice":1}
{"type":"host","command":{"command":"next"}}
```

## Fair timing

Every player has the same time, however slow their connection:

1. The controller pings each client every few seconds and keeps a running estimate of its round trip.
2. A question is sent to every player at the same moment, and the controller notes when it sent it to each.
3. An answer's time is from when the question was sent to that player until the answer arrived, less the player's round
   trip: the question's way out and the answer's way back.
4. An answer whose time, so corrected, is over the limit is refused, as are any but a player's first.
5. The question ends when every connected player has answered, or when the time limit has passed plus the slowest
   connected player's round trip, at most 2 seconds, so that an answer given in time but still on its way is counted.
6. A phone which rejoins during a question is sent it again with `remaining_ms`, the time left, and its answer if it
   had given one.

## Hosts

A host runs the party from the Mac, or from a phone paired as a host, which can do everything the Mac's controls do.

1. In setup, before players can join, the Mac shows the host pairing QR code. The iOS app sends `pair_host` with its
   one-time code and is sent `paired` with a token, which it keeps in the Keychain. The Mac keeps the tokens for as
   long as it runs, so a phone pairs again after the Mac app is restarted.
2. Later, the phone connects with `host_connect` and its token. A token the Mac does not know is refused with
   `not_authorised`, and the phone forgets it.
3. A host connection is sent `host_state` whenever the party changes, and sends `host` commands. Commands from any
   other connection are ignored.

## Moving on by itself

With auto-advance on, the party moves on as if the host pressed Next: the reveal of each answer for `reveal_secs`, then
the standings for `standings_secs`, then the next question. After a quiz's last question it shows the quiz's
leaderboard, then the party's, and waits there for the host to choose the next quiz. A question still ends when
everyone has answered or the time is up. While the clock is stopped, nothing moves on.

Whether or not it moves on by itself, Next pressed on the reveal, the standings or a quiz's leaderboard counts down for
`countdown_secs` (3 by default, 0 for at once) before moving on, so that the players see what is coming; pressed again
during the countdown, it moves on at once. Ending a question, Skip, and leaving the party's leaderboard are at once.
The players are sent `countdown` whenever the party is counting down, for the whole wait if it moves on by itself.

## Ranks

Players with the same score share a place, and the next place is skipped: scores 900, 800, 800 and 700 are 1st, 2nd,
2nd and 4th.

## Answer colours and shapes

Each answer position has the same colour and shape on the big screen and the phones, so that players who cannot tell
the colours apart can tell the shapes. `ANSWER_STYLES` in the protocol crate is the master copy:

| Position | Colour | Hex | Shape |
|---|---|---|---|
| 1 | Red | `#d64545` | Triangle |
| 2 | Blue | `#2f6fde` | Diamond |
| 3 | Yellow | `#e0b400` | Circle |
| 4 | Green | `#2e9d4f` | Square |
| 5 | Purple | `#8a55d6` | Star |
| 6 | Orange | `#e8772e` | Hexagon |
