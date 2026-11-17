// A player's connection to a party: joins, keeps the connection, rejoins after it drops, and holds what the screens
// show. Like the browser client, it only shows what the controller sends and sends what is pressed.

import Foundation
import Observation

/// Remembers the token and name a player joined a party with, to rejoin as the same player after the connection drops
/// or the app restarts. One per game code, so that a new party starts afresh.
public struct JoinMemory: Sendable {
    /// What was remembered.
    public struct Entry: Codable, Equatable, Sendable {
        public var token: String
        public var name: String
    }

    private let suite: String?

    /// Keeps the memory in the app's defaults, or in a suite of its own, e.g. for tests.
    public init(suite: String? = nil) {
        self.suite = suite
    }

    private var defaults: UserDefaults {
        suite.flatMap { UserDefaults(suiteName: $0) } ?? .standard
    }

    private func key(_ code: String) -> String {
        "quizparty:\(code.uppercased())"
    }

    public func entry(code: String) -> Entry? {
        defaults.data(forKey: key(code)).flatMap { try? JSONDecoder().decode(Entry.self, from: $0) }
    }

    public func remember(code: String, _ entry: Entry) {
        defaults.set(try? JSONEncoder().encode(entry), forKey: key(code))
    }

    public func forget(code: String) {
        defaults.removeObject(forKey: key(code))
    }

    /// The name last used for any party, to suggest it.
    public var lastName: String {
        get { defaults.string(forKey: "quizparty:name") ?? "" }
        nonmutating set { defaults.set(newValue, forKey: "quizparty:name") }
    }
}

/// A ranking to show: the top few and this player's place.
public struct Ranking: Equatable, Sendable {
    public struct Row: Equatable, Sendable, Identifiable {
        public var player: PlayerID
        public var name: String
        public var score: UInt32
        public var rank: UInt32
        public var id: PlayerID { player }
    }

    public var title: String
    /// Every row, in order of place.
    public var rows: [Row]

    /// The rows to list: the top five.
    public var top: [Row] { Array(rows.prefix(5)) }

    /// A player's row.
    public func row(of player: PlayerID?) -> Row? {
        rows.first { $0.player == player }
    }

    /// The ranking a leaderboard shows: the quiz's scores, or the party's totals.
    public init(_ board: Leaderboard) {
        switch board.sortedBy {
        case .quiz(let quiz):
            let index = Int(quiz)
            title = (board.quizzes.indices.contains(index) ? board.quizzes[index] : "The quiz") + ": final scores"
            rows = board.rows.compactMap { row in
                guard row.quizzes.indices.contains(index), let result = row.quizzes[index] else { return nil }
                return Row(player: row.player, name: row.name, score: result.score, rank: result.rank)
            }
        case .party:
            title = "The party so far"
            rows = board.rows.map { Row(player: $0.player, name: $0.name, score: $0.total, rank: $0.rank) }
        }
    }

    /// The standings after a question.
    public init(_ standings: [Standing]) {
        title = "Standings"
        rows = standings.map { Row(player: $0.player, name: $0.name, score: $0.score, rank: $0.rank) }
    }
}

/// A player's connection to a party, and what to show.
@MainActor
@Observable
public final class PlayerSession {
    /// What the player sees.
    public enum Screen: Equatable, Sendable {
        /// Connecting or reconnecting, with what to say meanwhile.
        case connecting(String)
        /// Waiting for the host to start a quiz.
        case lobby
        /// A question is being asked: see `question`.
        case question
        /// How the player did on the question.
        case result(Reveal)
        /// The standings, or a leaderboard.
        case ranking(Ranking)
        /// The join was refused, e.g. a wrong game code: the player must change something and join again.
        case refused(String)
        /// The party is over for this player, with why.
        case ended(String)
    }

    /// How long to wait before reconnecting after the connection drops, growing to the last.
    static let reconnectDelays: [Duration] = [.milliseconds(500), .seconds(1), .seconds(2), .seconds(4)]

    public let address: PartyAddress
    public let code: String
    /// The player's name, as the controller has it.
    public private(set) var name: String
    public private(set) var player: PlayerID?
    public private(set) var screen: Screen = .connecting("Joining…")
    /// The players in the lobby.
    public private(set) var players: [PlayerSummary] = []
    /// The question being asked, or the last one, for the result's answer.
    public private(set) var question: QuestionView?
    /// When the question's time runs out, by this phone's clock.
    public private(set) var endsAt: Date?
    /// The answer this player gave to the question, if any.
    public private(set) var choice: UInt8?
    /// E.g. "3 of 5 answered".
    public private(set) var answered: String = ""

    private let memory: JoinMemory
    private let session = URLSession(configuration: .default)
    private var task: URLSessionWebSocketTask?
    private var stayConnected = false
    private var reconnects = 0

    /// A session for joining the party at `address` with `code` as `name`; an empty name rejoins with the token
    /// remembered for the code.
    public init(address: PartyAddress, code: String, name: String, memory: JoinMemory = JoinMemory()) {
        self.address = address
        self.code = code.trimmingCharacters(in: .whitespaces).uppercased()
        self.name = name.trimmingCharacters(in: .whitespaces)
        self.memory = memory
    }

    /// Connects and joins.
    public func start() {
        stayConnected = true
        connect()
    }

    /// Leaves the party: closes the connection and stops reconnecting. The token is kept, to rejoin later.
    public func leave() {
        stayConnected = false
        task?.cancel(with: .goingAway, reason: nil)
        task = nil
    }

    /// Answers the question being asked, once, while there is time; the answer is locked at once rather than waiting
    /// for the controller's reply.
    public func answer(_ position: Int) {
        guard screen == .question, choice == nil, let question, let endsAt, Date() < endsAt else { return }
        let choice = UInt8(position)
        send(.answer(question: question.question, choice: choice))
        self.choice = choice
    }

    /// Whether the answers can still be pressed.
    public func canAnswer(at date: Date) -> Bool {
        screen == .question && choice == nil && endsAt.map { date < $0 } == true
    }

    private func connect() {
        guard let url = address.webSocketURL else {
            screen = .refused("That isn't a party's address.")
            return
        }
        let task = session.webSocketTask(with: url)
        self.task = task
        task.resume()
        let saved = memory.entry(code: code)
        send(.join(gameCode: code, name: name.isEmpty ? saved?.name ?? "" : name, token: saved?.token))
        Task { await receive(from: task) }
    }

    /// Handles the connection's messages until it closes, then reconnects if the player is still playing.
    private func receive(from task: URLSessionWebSocketTask) async {
        while true {
            let message: URLSessionWebSocketTask.Message
            do {
                message = try await task.receive()
            } catch {
                dropped(task)
                return
            }
            guard self.task === task else { return }
            let data: Data
            switch message {
            case .string(let text): data = Data(text.utf8)
            case .data(let bytes): data = bytes
            @unknown default: continue
            }
            // A message this app cannot read, e.g. from a newer controller, is ignored.
            if let message = try? ServerMessage.decode(data) {
                handle(message)
            }
        }
    }

    private func dropped(_ task: URLSessionWebSocketTask) {
        guard self.task === task, stayConnected else { return }
        let delay = Self.reconnectDelays[min(reconnects, Self.reconnectDelays.count - 1)]
        reconnects += 1
        screen = .connecting("Reconnecting…")
        Task {
            try? await Task.sleep(for: delay)
            if self.task === task, stayConnected {
                connect()
            }
        }
    }

    private func send(_ message: ClientMessage) {
        guard let task, let data = try? message.encoded() else { return }
        // A failure shows as the connection dropping, handled by `receive`.
        task.send(.string(String(decoding: data, as: UTF8.self))) { _ in }
    }

    /// Handles a message from the controller.
    func handle(_ message: ServerMessage) {
        switch message {
        case .ping(let nonce):
            send(.pong(nonce: nonce))
        case .welcome(let player, let token, let name):
            self.player = player
            self.name = name
            reconnects = 0
            memory.remember(code: code, .init(token: token, name: name))
            screen = .lobby
        case .rejected(let reason, let message):
            stayConnected = false
            if reason == .wrongGameCode {
                memory.forget(code: code)
            }
            screen = .refused(message)
        case .lobby(let players):
            self.players = players
            question = nil
            if player != nil {
                screen = .lobby
            }
        case .question(let view):
            question = view
            choice = nil
            answered = ""
            endsAt = Date().addingTimeInterval(Double(view.remainingMs) / 1000)
            screen = .question
        case .answerLocked(let question, let choice):
            if question == self.question?.question {
                self.choice = choice
            }
        case .answerCount(let question, let answered, let players):
            if question == self.question?.question {
                self.answered = "\(answered) of \(players) answered"
            }
        case .reveal(let reveal):
            screen = .result(reveal)
        case .standings(let standings):
            screen = .ranking(Ranking(standings))
        case .leaderboard(let board):
            screen = .ranking(Ranking(board))
        case .removed:
            stayConnected = false
            memory.forget(code: code)
            screen = .ended("The host removed you from the party.")
        case .partyEnded:
            stayConnected = false
            screen = .ended("The party is over. Thanks for playing!")
        case .paused, .resumed, .paired, .hostState, .unknown:
            // For host mode, or from a newer controller.
            break
        }
    }
}

/// A place as an ordinal, e.g. 1st, 2nd, 3rd, 11th.
public func ordinal(_ rank: UInt32) -> String {
    let tens = rank % 100
    let suffix =
        if (11...13).contains(tens) {
            "th"
        } else {
            switch rank % 10 {
            case 1: "st"
            case 2: "nd"
            case 3: "rd"
            default: "th"
            }
        }
    return "\(rank)\(suffix)"
}
