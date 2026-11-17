// The messages between the controller and the phones, the Swift copy of protocol/src/lib.rs, which is the master copy.
// The tests decode and encode the examples which the Rust tests write to protocol/schema/examples-*.json, so that the
// two cannot drift apart unnoticed. The JSON's keys are snake_case and Swift's camelCase: always use
// `ServerMessage.decode` and `ClientMessage.encoded`, which convert them.

import Foundation

/// The version of the messages, sent when joining; the controller refuses a client whose version is not its own.
public let protocolVersion: UInt32 = 1

/// The Bonjour service the controller advertises.
public let bonjourService = "_quizparty._tcp"

/// The path of the WebSocket on the controller's web server.
public let webSocketPath = "/ws"

/// The URL scheme which opens the app, e.g. `quizparty://join?host=…&port=…&code=…`.
public let urlScheme = "quizparty"

/// A player, as numbered by the controller for this party.
public typealias PlayerID = UInt32

/// A question of the party: its quiz and its place in it.
public struct QuestionRef: Codable, Hashable, Sendable {
    /// The quiz's index among the party's quizzes.
    public var quiz: UInt32
    /// The question's index in its quiz, from 0.
    public var index: UInt32

    public init(quiz: UInt32, index: UInt32) {
        self.quiz = quiz
        self.index = index
    }
}

/// A message from the phone to the controller.
public enum ClientMessage: Encodable, Equatable, Sendable {
    /// Joins as a player, or rejoins as the same player with the token from an earlier `welcome`.
    case join(protocolVersion: UInt32 = QuizPartyKit.protocolVersion, gameCode: String, name: String, token: String?)
    /// Answers the question being asked; only the first answer counts.
    case answer(question: QuestionRef, choice: UInt8)
    /// Answers a ping at once, for the controller to measure the round trip.
    case pong(nonce: UInt64)
    /// Pairs the app as a host, with the one-time code from the setup QR code.
    case pairHost(protocolVersion: UInt32 = QuizPartyKit.protocolVersion, pairingCode: String, deviceName: String)
    /// Connects a paired host phone with its token.
    case hostConnect(protocolVersion: UInt32 = QuizPartyKit.protocolVersion, token: String)
    /// A host's command.
    case host(command: HostCommand)

    private enum Keys: String, CodingKey {
        case type, `protocol`, gameCode, name, token, question, choice, nonce, pairingCode, deviceName, command
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: Keys.self)
        switch self {
        case .join(let version, let gameCode, let name, let token):
            try container.encode("join", forKey: .type)
            try container.encode(version, forKey: .protocol)
            try container.encode(gameCode, forKey: .gameCode)
            try container.encode(name, forKey: .name)
            try container.encodeIfPresent(token, forKey: .token)
        case .answer(let question, let choice):
            try container.encode("answer", forKey: .type)
            try container.encode(question, forKey: .question)
            try container.encode(choice, forKey: .choice)
        case .pong(let nonce):
            try container.encode("pong", forKey: .type)
            try container.encode(nonce, forKey: .nonce)
        case .pairHost(let version, let pairingCode, let deviceName):
            try container.encode("pair_host", forKey: .type)
            try container.encode(version, forKey: .protocol)
            try container.encode(pairingCode, forKey: .pairingCode)
            try container.encode(deviceName, forKey: .deviceName)
        case .hostConnect(let version, let token):
            try container.encode("host_connect", forKey: .type)
            try container.encode(version, forKey: .protocol)
            try container.encode(token, forKey: .token)
        case .host(let command):
            try container.encode("host", forKey: .type)
            try container.encode(command, forKey: .command)
        }
    }

    /// The message as the JSON the controller reads.
    public func encoded() throws -> Data {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return try encoder.encode(self)
    }
}

/// What a host can do, from the Mac or a paired host phone.
public enum HostCommand: Encodable, Equatable, Sendable {
    case startQuiz(quiz: UInt32)
    case next
    case skip
    case pause
    case resume
    case removePlayer(player: PlayerID)
    case endParty
    case setAutoAdvance(AutoAdvance)

    private enum Keys: String, CodingKey {
        case command, quiz, player, autoAdvance
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: Keys.self)
        switch self {
        case .startQuiz(let quiz):
            try container.encode("start_quiz", forKey: .command)
            try container.encode(quiz, forKey: .quiz)
        case .next: try container.encode("next", forKey: .command)
        case .skip: try container.encode("skip", forKey: .command)
        case .pause: try container.encode("pause", forKey: .command)
        case .resume: try container.encode("resume", forKey: .command)
        case .removePlayer(let player):
            try container.encode("remove_player", forKey: .command)
            try container.encode(player, forKey: .player)
        case .endParty: try container.encode("end_party", forKey: .command)
        case .setAutoAdvance(let autoAdvance):
            try container.encode("set_auto_advance", forKey: .command)
            try container.encode(autoAdvance, forKey: .autoAdvance)
        }
    }
}

/// Whether the party moves on by itself after each question: the reveal for a while, then the standings, then the
/// next question.
public struct AutoAdvance: Codable, Equatable, Sendable {
    public var enabled: Bool
    /// How long the reveal is shown, in seconds.
    public var revealSecs: UInt32
    /// How long the standings, or a quiz's leaderboard, are shown, in seconds.
    public var standingsSecs: UInt32
    /// How long to count down after the host presses Next on the reveal or the standings, in seconds; 0 is at once.
    public var countdownSecs: UInt32

    public init(enabled: Bool, revealSecs: UInt32, standingsSecs: UInt32, countdownSecs: UInt32) {
        self.enabled = enabled
        self.revealSecs = revealSecs
        self.standingsSecs = standingsSecs
        self.countdownSecs = countdownSecs
    }
}

/// A message from the controller to the phone.
public enum ServerMessage: Decodable, Equatable, Sendable {
    /// The player has joined or rejoined: the token to keep for rejoining.
    case welcome(player: PlayerID, token: String, name: String)
    /// The join or pairing was refused; the connection will be closed.
    case rejected(reason: RejectReason, message: String)
    /// Asks for a pong at once.
    case ping(nonce: UInt64)
    /// The party is waiting for the next quiz: who has joined.
    case lobby(players: [PlayerSummary])
    /// A question, without its answer.
    case question(QuestionView)
    /// The player's answer was taken.
    case answerLocked(question: QuestionRef, choice: UInt8)
    /// How many have answered so far.
    case answerCount(question: QuestionRef, answered: UInt32, players: UInt32)
    /// The correct answer, how many chose each, and how this player did.
    case reveal(Reveal)
    /// The places after a question, and how far through the quiz.
    case standings([Standing], progress: Progress?)
    /// A leaderboard at the end of a quiz, the quiz's and then the party's.
    case leaderboard(Leaderboard)
    /// The party will move on soon: what comes next, e.g. "Next question", and how long until then.
    case countdown(next: String, inMs: UInt32)
    case paused
    case resumed
    /// The host removed this player.
    case removed
    case partyEnded
    /// The phone is paired as a host: the token to keep, securely.
    case paired(token: String)
    /// Everything a host phone shows.
    case hostState(HostState)
    /// A message this app does not know, from a newer controller, which it ignores.
    case unknown(type: String)

    private enum Keys: String, CodingKey {
        case type, player, token, name, reason, message, nonce, players, question, choice, answered, standings, next
        case inMs, progress
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: Keys.self)
        let type = try container.decode(String.self, forKey: .type)
        switch type {
        case "welcome":
            self = .welcome(
                player: try container.decode(PlayerID.self, forKey: .player),
                token: try container.decode(String.self, forKey: .token),
                name: try container.decode(String.self, forKey: .name))
        case "rejected":
            self = .rejected(
                reason: try container.decode(RejectReason.self, forKey: .reason),
                message: try container.decode(String.self, forKey: .message))
        case "ping": self = .ping(nonce: try container.decode(UInt64.self, forKey: .nonce))
        case "lobby": self = .lobby(players: try container.decode([PlayerSummary].self, forKey: .players))
        case "question": self = .question(try QuestionView(from: decoder))
        case "answer_locked":
            self = .answerLocked(
                question: try container.decode(QuestionRef.self, forKey: .question),
                choice: try container.decode(UInt8.self, forKey: .choice))
        case "answer_count":
            self = .answerCount(
                question: try container.decode(QuestionRef.self, forKey: .question),
                answered: try container.decode(UInt32.self, forKey: .answered),
                players: try container.decode(UInt32.self, forKey: .players))
        case "reveal": self = .reveal(try Reveal(from: decoder))
        case "standings":
            self = .standings(
                try container.decode([Standing].self, forKey: .standings),
                progress: try container.decodeIfPresent(Progress.self, forKey: .progress))
        case "leaderboard": self = .leaderboard(try Leaderboard(from: decoder))
        case "countdown":
            self = .countdown(
                next: try container.decode(String.self, forKey: .next),
                inMs: try container.decode(UInt32.self, forKey: .inMs))
        case "paused": self = .paused
        case "resumed": self = .resumed
        case "removed": self = .removed
        case "party_ended": self = .partyEnded
        case "paired": self = .paired(token: try container.decode(String.self, forKey: .token))
        case "host_state": self = .hostState(try HostState(from: decoder))
        default: self = .unknown(type: type)
        }
    }

    /// Reads a message from the controller's JSON.
    public static func decode(_ data: Data) throws -> ServerMessage {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(ServerMessage.self, from: data)
    }
}

/// Why a join or pairing was refused.
public enum RejectReason: String, Decodable, Equatable, Sendable {
    case wrongGameCode = "wrong_game_code"
    case wrongProtocol = "wrong_protocol"
    case badName = "bad_name"
    case closed
    case notAuthorised = "not_authorised"
}

/// A player, as listed in the lobby.
public struct PlayerSummary: Decodable, Equatable, Hashable, Sendable {
    public var player: PlayerID
    public var name: String
    /// Whether the player's phone is connected now.
    public var connected: Bool
}

/// A question as the players see it, without its answer.
public struct QuestionView: Decodable, Equatable, Sendable {
    public var question: QuestionRef
    public var quizTitle: String
    /// The question's number in its quiz, from 1.
    public var number: UInt32
    /// How many questions the quiz has.
    public var total: UInt32
    public var text: String
    /// The image's path on the controller's web server, if it is an image question.
    public var image: String?
    public var imageAlt: String?
    /// The answers, from 2 to 6.
    public var answers: [String]
    public var timeLimitMs: UInt32
    /// How long is left, less than the limit for a phone which rejoined during the question.
    public var remainingMs: UInt32
    public var maxPoints: UInt32
}

/// The end of a question.
public struct Reveal: Decodable, Equatable, Sendable {
    public var question: QuestionRef
    /// How far through the quiz, nil from a controller too old to say.
    public var progress: Progress?
    /// The index of the correct answer.
    public var correct: UInt8
    /// How many chose each answer.
    public var counts: [UInt32]
    /// A link to read more.
    public var source: String?
    /// How this player did, sent only to players.
    public var result: PlayerResult?
}

/// How far through its quiz the party is, after a question.
public struct Progress: Decodable, Equatable, Sendable {
    public var quizTitle: String
    /// How many of its questions have been asked, counting the one just closed.
    public var asked: UInt32
    /// How many questions the quiz has.
    public var total: UInt32

    public init(quizTitle: String, asked: UInt32, total: UInt32) {
        self.quizTitle = quizTitle
        self.asked = asked
        self.total = total
    }

    /// The progress in words, e.g. "After 6 of 20 Questions", as the controller's `Progress::words` says it.
    public var words: String {
        "After \(asked) of \(total) \(total == 1 ? "Question" : "Questions")"
    }
}

/// How a player did on a question.
public struct PlayerResult: Decodable, Equatable, Sendable {
    /// The player's answer, if they answered.
    public var choice: UInt8?
    public var correct: Bool
    public var points: UInt32
    public var quizScore: UInt32
    /// The player's place in the quiz so far, from 1.
    public var quizRank: UInt32
}

/// A player's place after a question.
public struct Standing: Decodable, Equatable, Sendable {
    public var player: PlayerID
    public var name: String
    public var score: UInt32
    public var rank: UInt32
    public var lastPoints: UInt32
}

/// A leaderboard, sorted by a quiz's ranks or by the party's.
public struct Leaderboard: Decodable, Equatable, Sendable {
    public var sortedBy: LeaderboardKind
    /// The titles of the quizzes played so far.
    public var quizzes: [String]
    public var rows: [LeaderboardRow]
}

/// Which ranking a leaderboard is sorted by.
public enum LeaderboardKind: Decodable, Equatable, Sendable {
    /// A quiz's, by its index in the leaderboard's quizzes.
    case quiz(UInt32)
    /// The party's, by the totals.
    case party

    private enum Keys: String, CodingKey {
        case kind, quiz
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: Keys.self)
        switch try container.decode(String.self, forKey: .kind) {
        case "quiz": self = .quiz(try container.decode(UInt32.self, forKey: .quiz))
        case "party": self = .party
        case let kind:
            throw DecodingError.dataCorruptedError(
                forKey: .kind, in: container, debugDescription: "Unknown leaderboard \(kind)")
        }
    }
}

/// A player's row in a leaderboard.
public struct LeaderboardRow: Decodable, Equatable, Sendable {
    public var player: PlayerID
    public var name: String
    /// The player's score and place in each quiz played, nil for one they did not play.
    public var quizzes: [QuizResult?]
    public var total: UInt32
    public var rank: UInt32
}

/// A player's score and place in a quiz.
public struct QuizResult: Decodable, Equatable, Sendable {
    public var score: UInt32
    public var rank: UInt32
}

/// Everything a host phone shows.
public struct HostState: Decodable, Equatable, Sendable {
    public var gameCode: String
    /// Whether new players need the game code to join; nil from a controller too old to say, which always needs it.
    public var codeRequired: Bool?
    public var quizzes: [QuizSummary]
    public var players: [PlayerSummary]
    public var phase: HostPhase
    public var paused: Bool
    public var autoAdvance: AutoAdvance
    /// How long until the party moves on by itself, if it will, when this was sent.
    public var advanceInMs: UInt32?
    /// What Next does now, e.g. "Show Standings", or nil when there is nothing to move on to.
    public var next: String?
}

/// One of the party's quizzes, to choose the next from.
public struct QuizSummary: Decodable, Equatable, Sendable {
    public var title: String
    public var questions: UInt32
    public var played: Bool
}

/// What the party is doing, as a host phone sees it.
public enum HostPhase: Decodable, Equatable, Sendable {
    /// Being set up on the Mac: no one can join yet.
    case setup
    case lobby
    case question(view: QuestionView, correct: UInt8, answered: [PlayerID])
    case reveal(Reveal)
    case standings([Standing], progress: Progress?)
    case leaderboard(Leaderboard)
    case ended

    private enum Keys: String, CodingKey {
        case phase, view, correct, answered, standings, progress
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: Keys.self)
        switch try container.decode(String.self, forKey: .phase) {
        case "setup": self = .setup
        case "lobby": self = .lobby
        case "question":
            self = .question(
                view: try container.decode(QuestionView.self, forKey: .view),
                correct: try container.decode(UInt8.self, forKey: .correct),
                answered: try container.decode([PlayerID].self, forKey: .answered))
        case "reveal": self = .reveal(try Reveal(from: decoder))
        case "standings":
            self = .standings(
                try container.decode([Standing].self, forKey: .standings),
                progress: try container.decodeIfPresent(Progress.self, forKey: .progress))
        case "leaderboard": self = .leaderboard(try Leaderboard(from: decoder))
        case "ended": self = .ended
        case let phase:
            throw DecodingError.dataCorruptedError(
                forKey: .phase, in: container, debugDescription: "Unknown phase \(phase)")
        }
    }
}
