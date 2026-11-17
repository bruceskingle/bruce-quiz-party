// The browser client of Bruce's Quiz Party. It only shows what the controller sends and sends what is pressed: the
// game, the clock and the scores are all the controller's. The messages are in docs/Protocol.md.
"use strict";

/** The protocol version this client speaks, protocol's PROTOCOL_VERSION. */
const PROTOCOL_VERSION = 1;

/** How long to wait before reconnecting after the connection drops, growing to the last. */
const RECONNECT_DELAYS_MS = [500, 1000, 2000, 4000];

/**
 * The shape of each answer position, as SVG paths in a 24 by 24 box: triangle, diamond, circle, square, star and
 * hexagon, the same as the big screen's, server/src/answer_shape.rs.
 */
const SHAPE_PATHS = [
  "M12 3 L22 20 L2 20 Z",
  "M12 2 L22 12 L12 22 L2 12 Z",
  "M12 3 A9 9 0 1 1 11.99 3 Z",
  "M4 4 H20 V20 H4 Z",
  "M12 2 L14.9 8.6 L22 9.2 L16.6 13.9 L18.2 21 L12 17.3 L5.8 21 L7.4 13.9 L2 9.2 L9.1 8.6 Z",
  "M7 3 H17 L22 12 L17 21 H7 L2 12 Z",
];

const params = new URLSearchParams(location.search);
const state = {
  /** The game code, from the link or typed in. */
  code: (params.get("code") || "").toUpperCase(),
  /** The player's name, as typed, kept to rejoin. */
  name: "",
  /** The player's number once joined. */
  player: null,
  socket: null,
  reconnects: 0,
  /** Whether to reconnect if the connection drops: not after being refused or removed. */
  stayConnected: false,
  /** The question being asked, as the controller sent it. */
  question: null,
  /** The answers of the question being asked, for the result. */
  answers: [],
  /** When the question's time runs out, by this phone's clock, for the countdown. */
  endsAt: 0,
  /** Whether this player has answered the question being asked. */
  locked: false,
};

const $ = (id) => document.getElementById(id);

/** Shows one screen, hiding the others. */
function show(id) {
  for (const screen of document.querySelectorAll(".screen")) {
    screen.hidden = screen.id !== id;
  }
}

function showMessage(text) {
  $("message-text").textContent = text;
  show("message");
}

/** The key the rejoin token and name are kept under: one per game code, so that a new party starts afresh. */
function storageKey(code) {
  return "quizparty:" + code;
}

/** The token and name from joining this party before, if any. */
function remembered(code) {
  try {
    return JSON.parse(localStorage.getItem(storageKey(code))) || null;
  } catch {
    return null;
  }
}

function remember(code, token, name) {
  try {
    localStorage.setItem(storageKey(code), JSON.stringify({ token, name }));
  } catch {
    // Private browsing may refuse; the player can still play, but not rejoin as themselves after a reload.
  }
}

function forget(code) {
  try {
    localStorage.removeItem(storageKey(code));
  } catch {}
}

function send(message) {
  if (state.socket && state.socket.readyState === WebSocket.OPEN) {
    state.socket.send(JSON.stringify(message));
  }
}

/** Connects to the controller and joins, or rejoins with the remembered token. */
function connect() {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  const socket = new WebSocket(scheme + "//" + location.host + "/ws");
  state.socket = socket;
  state.stayConnected = true;
  socket.onopen = () => {
    const saved = remembered(state.code);
    send({
      type: "join",
      protocol: PROTOCOL_VERSION,
      game_code: state.code,
      name: state.name || (saved && saved.name) || "",
      token: saved ? saved.token : undefined,
    });
  };
  socket.onmessage = (event) => {
    let message;
    try {
      message = JSON.parse(event.data);
    } catch {
      return;
    }
    receive(message);
  };
  socket.onclose = () => {
    if (state.socket !== socket || !state.stayConnected) return;
    const delay = RECONNECT_DELAYS_MS[Math.min(state.reconnects, RECONNECT_DELAYS_MS.length - 1)];
    state.reconnects += 1;
    showMessage("Reconnecting…");
    setTimeout(connect, delay);
  };
}

/** Handles a message from the controller. */
function receive(message) {
  switch (message.type) {
    case "ping":
      send({ type: "pong", nonce: message.nonce });
      break;
    case "welcome":
      state.player = message.player;
      state.reconnects = 0;
      remember(state.code, message.token, state.name || message.name);
      $("me").textContent = message.name;
      show("lobby");
      break;
    case "rejected":
      state.stayConnected = false;
      if (message.reason === "wrong_game_code") forget(state.code);
      $("join-error").textContent = message.message;
      $("join-error").hidden = false;
      $("join-form").querySelector("button").disabled = false;
      show("join");
      break;
    case "lobby":
      state.question = null;
      showPlayers(message.players);
      if (state.player !== null) show("lobby");
      break;
    case "question":
      showQuestion(message);
      break;
    case "answer_locked":
      lock(message.choice);
      break;
    case "answer_count":
      $("answered").textContent = message.answered + " of " + message.players + " answered";
      break;
    case "reveal":
      showResult(message);
      break;
    case "standings":
      showStandings(message.standings);
      break;
    case "leaderboard":
      showLeaderboard(message);
      break;
    case "removed":
      state.stayConnected = false;
      forget(state.code);
      showMessage("The host removed you from the party.");
      break;
    case "party_ended":
      state.stayConnected = false;
      showMessage("The party is over. Thanks for playing!");
      break;
    default:
      // Messages for later milestones, or from a newer controller, are ignored.
      break;
  }
}

/** An SVG of an answer position's shape. */
function shape(position) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", SHAPE_PATHS[position % SHAPE_PATHS.length]);
  svg.appendChild(path);
  return svg;
}

/** Shows a question, with a button per answer, which stay until one is pressed. */
function showQuestion(view) {
  state.question = view;
  state.answers = view.answers;
  state.locked = false;
  state.endsAt = performance.now() + view.remaining_ms;
  $("question-number").textContent = "Question " + view.number + " of " + view.total;
  $("answered").textContent = "";
  $("question-text").textContent = view.text;
  const image = $("question-image");
  if (view.image) {
    image.src = view.image;
    image.alt = view.image_alt || "";
    image.hidden = false;
  } else {
    image.removeAttribute("src");
    image.hidden = true;
  }
  const answers = $("answers");
  answers.className = "answers count-" + view.answers.length;
  answers.replaceChildren(
    ...view.answers.map((text, position) => {
      const button = document.createElement("button");
      button.className = "answer answer-" + position;
      button.appendChild(shape(position));
      const label = document.createElement("span");
      label.textContent = text;
      button.appendChild(label);
      button.addEventListener("click", () => choose(position));
      return button;
    }),
  );
  $("locked").hidden = true;
  show("question");
  tickTimer();
}

/** Sends the player's answer: only the first tap counts, and the buttons lock at once rather than wait for the reply. */
function choose(position) {
  if (state.locked || !state.question || performance.now() > state.endsAt) return;
  send({ type: "answer", question: state.question.question, choice: position });
  lock(position);
}

/** Locks the answers, the chosen one highlighted. */
function lock(choice) {
  state.locked = true;
  const buttons = $("answers").children;
  for (let i = 0; i < buttons.length; i++) {
    buttons[i].disabled = true;
    buttons[i].classList.toggle("chosen", i === choice);
    buttons[i].classList.toggle("dimmed", i !== choice);
  }
  $("locked").hidden = false;
}

/** Shrinks the countdown bar until the time is up, then locks the answers. */
function tickTimer() {
  if (!state.question) return;
  const left = Math.max(0, state.endsAt - performance.now());
  const fraction = left / Math.max(1, state.question.time_limit_ms);
  const bar = $("timer-bar");
  bar.style.width = fraction * 100 + "%";
  bar.classList.toggle("urgent", left < 5000);
  if (left <= 0) {
    if (!state.locked) {
      for (const button of $("answers").children) button.disabled = true;
    }
    return;
  }
  requestAnimationFrame(tickTimer);
}

/** A place as an ordinal, e.g. 1st, 2nd, 3rd, 11th. */
function ordinal(rank) {
  const tens = rank % 100;
  const suffix = tens >= 11 && tens <= 13 ? "th" : { 1: "st", 2: "nd", 3: "rd" }[rank % 10] || "th";
  return rank + suffix;
}

/** Shows how the player did, e.g. "Correct! +850 points, you're 3rd". */
function showResult(reveal) {
  state.question = null;
  const result = reveal.result;
  const panel = $("result-panel");
  const correctText = state.answers[reveal.correct] || "";
  if (!result) {
    panel.className = "result-panel";
    $("result-title").textContent = "The answer was";
    $("result-points").textContent = correctText;
    $("result-rank").textContent = "";
    $("result-answer").textContent = "";
  } else {
    const answered = result.choice !== undefined && result.choice !== null;
    panel.className = "result-panel " + (result.correct ? "right" : "wrong");
    $("result-title").textContent = result.correct ? "Correct!" : answered ? "Wrong!" : "Too slow!";
    $("result-points").textContent = "+" + result.points + " points";
    $("result-rank").textContent = "You're " + ordinal(result.quiz_rank) + " with " + result.quiz_score + " points";
    $("result-answer").textContent = result.correct ? "" : "The answer was " + correctText;
  }
  show("result");
}

/** Shows the standings: the top few, and where this player is. */
function showStandings(standings) {
  showRanking(
    "Standings",
    standings.map((standing) => ({ player: standing.player, name: standing.name, score: standing.score, rank: standing.rank })),
  );
}

/** Shows a leaderboard, the quiz's or the party's: the top few, and where this player is. */
function showLeaderboard(board) {
  const kind = board.sorted_by;
  if (kind.kind === "quiz") {
    const rows = board.rows
      .filter((row) => row.quizzes[kind.quiz])
      .map((row) => ({ player: row.player, name: row.name, ...row.quizzes[kind.quiz] }));
    showRanking(board.quizzes[kind.quiz] + ": final scores", rows);
  } else {
    showRanking(
      "The party so far",
      board.rows.map((row) => ({ player: row.player, name: row.name, score: row.total, rank: row.rank })),
    );
  }
}

/** Shows a ranking under a title: the top five, and this player's place and score. */
function showRanking(title, rows) {
  $("standings-title").textContent = title;
  const me = rows.find((row) => row.player === state.player);
  $("standings-me").textContent = me ? "You're " + ordinal(me.rank) + " with " + me.score + " points" : "";
  $("standings-list").replaceChildren(
    ...rows.slice(0, 5).map((row) => {
      const item = document.createElement("li");
      if (row.player === state.player) item.classList.add("me");
      const place = document.createElement("span");
      place.textContent = ordinal(row.rank);
      const name = document.createElement("span");
      name.textContent = row.name;
      const score = document.createElement("span");
      score.textContent = row.score;
      item.append(place, name, score);
      return item;
    }),
  );
  show("standings");
}

/** Lists the players in the lobby, this player highlighted. */
function showPlayers(players) {
  const list = $("players");
  list.replaceChildren(
    ...players.map((player) => {
      const item = document.createElement("li");
      item.textContent = player.name;
      if (player.player === state.player) item.classList.add("me");
      if (!player.connected) item.classList.add("away");
      return item;
    }),
  );
}

$("join-form").addEventListener("submit", (event) => {
  event.preventDefault();
  state.code = $("code").value.trim().toUpperCase();
  state.name = $("name").value.trim();
  $("join-error").hidden = true;
  event.submitter && (event.submitter.disabled = true);
  if (state.socket) {
    state.stayConnected = false;
    state.socket.close();
  }
  connect();
});

// With a remembered token for this party, rejoin at once; otherwise ask for a name.
$("code").value = state.code;
if (state.code && remembered(state.code)) {
  showMessage("Joining…");
  connect();
} else {
  show("join");
  (state.code ? $("name") : $("code")).focus();
}
