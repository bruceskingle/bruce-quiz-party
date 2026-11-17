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
  big screen, so that only those in the room can join.
- **Join QR code**: the browser client's URL with the game code, `http://<mac>:<port>/?code=ABCD`.
- **URL scheme**: `quizparty://join?host=<mac>&port=<port>&code=ABCD` opens the iOS app. The app's own scanner reads
  the join QR code too, so the QR code stays the browser's URL, which works for everyone.

## Messages

Every message is a JSON object whose `type` names it, in `snake_case`.

From a client:

| `type` | Meaning |
|---|---|
| `join` | Joins as a player with `protocol`, `game_code` and `name`, or rejoins with the `token` from a `welcome`. |
| `answer` | Answers `question` (`{quiz, index}`) with `choice`, from 0. Only the first answer counts. |
| `pong` | Answers a `ping` at once, with its `nonce`. |
| `pair_host` | Pairs the iOS app as a host with the one-time `pairing_code` from the setup QR code. |
| `host_connect` | Connects a paired host phone with its `token`. |
| `host` | A host's `command`: `start_quiz`, `next`, `skip`, `pause`, `resume`, `remove_player`, `end_party`. |

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
| `paused`, `resumed` | The clock stopped or started again. |
| `removed`, `party_ended` | The player was removed, or the party is over. |
| `paired` | The phone is paired as a host: the `token` to keep, securely. |
| `host_state` | Everything a host phone shows, including the correct answer, whenever it changes. |

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
