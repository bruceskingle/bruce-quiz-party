// Checks the Swift messages against the examples the Rust tests write, protocol/schema/examples-*.json, so that the
// iOS app reads every message the controller sends and sends what it expects.

import Foundation
import Testing

@testable import QuizPartyKit

/// The repository's protocol/schema folder, found from this file's place in it.
private let schemaFolder = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()  // QuizPartyKitTests
    .deletingLastPathComponent()  // Tests
    .deletingLastPathComponent()  // QuizPartyKit
    .deletingLastPathComponent()  // ios
    .deletingLastPathComponent()  // the repository
    .appendingPathComponent("protocol/schema")

/// The examples in a file, each as its own JSON.
private func examples(_ name: String) throws -> [Data] {
    let data = try Data(contentsOf: schemaFolder.appendingPathComponent(name))
    let array = try #require(try JSONSerialization.jsonObject(with: data) as? [Any])
    return try array.map { try JSONSerialization.data(withJSONObject: $0) }
}

/// JSON as objects, to compare regardless of the order of keys.
private func object(_ data: Data) throws -> NSObject {
    try #require(try JSONSerialization.jsonObject(with: data) as? NSObject)
}

@Test func readsEveryServerMessage() throws {
    let messages = try examples("examples-server.json").map { try ServerMessage.decode($0) }
    for message in messages {
        if case .unknown(let type) = message {
            Issue.record("The example \(type) is not known")
        }
    }
    #expect(messages.count == 31)
    #expect(messages[30] == .countdown(next: "Next question", inMs: 3000))
    #expect(messages.first == .welcome(player: 1, token: "0123456789abcdef", name: "Sam"))
    guard case .question(let view) = messages[8] else {
        Issue.record("No question: \(messages[8])")
        return
    }
    #expect(view.quizTitle == "Around the World")
    #expect(view.image == "/image/1/3?p=12345")
    #expect(view.imageAlt == "A red disc on white")
    #expect(view.answers == ["Japan", "Bangladesh", "Palau"])
    #expect((view.timeLimitMs, view.remainingMs, view.maxPoints) == (20_000, 12_345, 1000))
    guard case .question(let plain) = messages[9] else { return }
    #expect(plain.image == nil && plain.imageAlt == nil)
    guard case .reveal(let reveal) = messages[12] else { return }
    #expect(reveal.result?.quizRank == 2)
    #expect(reveal.nextImage == "/image/1/4?p=12345")
    #expect(reveal.progress?.words == "After 6 of 20 Questions")
    guard case .reveal(let tooSlow) = messages[13] else { return }
    #expect(tooSlow.result?.choice == nil && tooSlow.source == nil)
    guard case .leaderboard(let board) = messages[16] else { return }
    #expect(board.sortedBy == .quiz(1))
    #expect(board.rows[0].quizzes == [nil, QuizResult(score: 2400, rank: 1)])
    guard case .hostState(let state) = messages[23] else { return }
    #expect(state.phase == .lobby)
    #expect(state.gameCode == "ABCD" && state.advanceInMs == nil && state.next == nil)
    #expect(state.codeRequired == true)
    guard case .hostState(let waiting) = messages[24] else { return }
    #expect(waiting.paused && waiting.autoAdvance.enabled && waiting.advanceInMs == 12_345)
    #expect(waiting.next == "Show Standings")
    #expect(waiting.codeRequired == false)
    guard case .question(_, let correct, let answered) = waiting.phase else {
        Issue.record("No question: \(waiting.phase)")
        return
    }
    #expect(correct == 0 && answered == [1])
    guard case .hostState(let standings) = messages[26] else { return }
    #expect(
        standings.phase
            == .standings(
                [Standing(player: 1, name: "Sam", score: 2400, rank: 1, lastPoints: 850)],
                progress: Progress(quizTitle: "Flags of Asia", asked: 6, total: 20)))
    guard case .hostState(let setup) = messages[29] else { return }
    #expect(setup.phase == .setup)
}

@Test func writesEveryClientMessageAsTheControllerReadsIt() throws {
    let question = QuestionRef(quiz: 1, index: 2)
    let messages: [ClientMessage] = [
        .join(gameCode: "ABCD", name: "Sam", token: nil),
        .join(gameCode: "ABCD", name: "", token: "0123456789abcdef"),
        .answer(question: question, choice: 1),
        .pong(nonce: 42),
        .pairHost(pairingCode: "XYZW1234", deviceName: "Sam's iPhone"),
        .hostConnect(token: "fedcba9876543210"),
        .host(command: .startQuiz(quiz: 1)),
        .host(command: .next),
        .host(command: .skip),
        .host(command: .pause),
        .host(command: .resume),
        .host(command: .removePlayer(player: 2)),
        .host(command: .endParty),
        .host(command: .setAutoAdvance(AutoAdvance(enabled: true, revealSecs: 5, standingsSecs: 30, countdownSecs: 3))),
    ]
    let expected = try examples("examples-client.json")
    #expect(messages.count == expected.count)
    for (message, json) in zip(messages, expected) {
        #expect(try object(message.encoded()) == object(json), "\(message)")
    }
}

@Test func ignoresMessagesItDoesNotKnow() throws {
    let message = try ServerMessage.decode(Data(#"{"type":"fireworks","colour":"gold"}"#.utf8))
    #expect(message == .unknown(type: "fireworks"))
}

@Test func readsJoinLinks() throws {
    let browser = try #require(JoinLink(scanned: "http://192.168.1.20:8765/?code=abcd"))
    #expect(browser == JoinLink(address: PartyAddress(host: "192.168.1.20", port: 8765), code: "ABCD"))
    let app = try #require(JoinLink(scanned: "quizparty://join?host=mac.local&port=9000&code=WXYZ"))
    #expect(app == JoinLink(address: PartyAddress(host: "mac.local", port: 9000), code: "WXYZ"))
    #expect(JoinLink(scanned: "https://en.wikipedia.org/wiki/Flag_of_Japan") == nil)
    #expect(JoinLink(scanned: "http://example.com/") == nil, "a web page without a code")
    let open = try #require(JoinLink(scanned: "http://192.168.1.20:8765/"), "a party which needs no code")
    #expect(open == JoinLink(address: PartyAddress(host: "192.168.1.20", port: 8765), code: nil))
    #expect(JoinLink(scanned: "http://192.168.1.20:8765/page") == nil, "not the front page")
    #expect(JoinLink(scanned: "quizparty://join?host=mac.local") == nil, "no port")
}

@Test func readsWhetherThePartyNeedsItsCode() throws {
    #expect(try PartyInfo.decode(Data(#"{"code_required":false}"#.utf8)) == PartyInfo(codeRequired: false))
}

@Test func makesImageAddressesWithTheirQuery() throws {
    let url = try #require(PartyAddress(host: "192.168.1.20", port: 8765).fileURL("/image/1/3?p=12345"))
    #expect(url.absoluteString == "http://192.168.1.20:8765/image/1/3?p=12345")
}

@Test func readsTypedAddresses() {
    #expect(PartyAddress(typed: " 192.168.1.20:8765 ") == PartyAddress(host: "192.168.1.20", port: 8765))
    #expect(PartyAddress(typed: "192.168.1.20") == PartyAddress(host: "192.168.1.20", port: 8765))
    #expect(
        PartyAddress(typed: "http://192.168.1.20:9000/?code=ABCD") == PartyAddress(host: "192.168.1.20", port: 9000))
    #expect(PartyAddress(typed: "") == nil)
}

@Test func buildsURLs() {
    let address = PartyAddress(host: "192.168.1.20", port: 8765)
    #expect(address.webSocketURL?.absoluteString == "ws://192.168.1.20:8765/ws")
    #expect(address.fileURL("/quiz/0/my flag.svg")?.absoluteString == "http://192.168.1.20:8765/quiz/0/my%20flag.svg")
}

@Test func ordinals() {
    #expect(
        [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111].map { ordinal($0) } == [
            "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st", "111th",
        ])
}

@Test func shapesFillTheirBox() {
    for position in 0..<6 {
        let bounds = AnswerShape(position: position).path(in: CGRect(x: 0, y: 0, width: 24, height: 24))
            .boundingRect
        #expect(bounds.minX >= 1.9 && bounds.maxX <= 22.1 && bounds.minY >= 1.9 && bounds.maxY <= 22.1, "\(position)")
        #expect(bounds.width > 15 && bounds.height > 15, "\(position): \(bounds)")
    }
    // The circle is the whole circle, not the sliver between its ends.
    let circle = AnswerShape(position: 2).path(in: CGRect(x: 0, y: 0, width: 24, height: 24)).boundingRect
    #expect(abs(circle.minX - 3) < 0.1 && abs(circle.maxY - 21) < 0.1, "\(circle)")
}

@Test func coloursAreTheProtocols() {
    #expect(answerStyles.map(\.colour) == ["#d64545", "#2f6fde", "#e0b400", "#2e9d4f", "#8a55d6", "#e8772e"])
}

@MainActor
@Test func aSessionFollowsTheGame() throws {
    let suite = "QuizPartyKitTests-\(UUID().uuidString)"
    let memory = JoinMemory(suite: suite)
    defer { UserDefaults().removePersistentDomain(forName: suite) }
    let session = PlayerSession(
        address: PartyAddress(host: "127.0.0.1", port: 1), code: "abcd", name: "Sam", memory: memory)
    let messages = try examples("examples-server.json").map { try ServerMessage.decode($0) }

    session.handle(messages[0])  // welcome
    #expect(session.screen == .lobby)
    #expect(memory.entry(code: "ABCD") == JoinMemory.Entry(token: "0123456789abcdef", name: "Sam"))
    #expect(memory.lastParty == .init(address: PartyAddress(host: "127.0.0.1", port: 1), code: "ABCD"))
    session.handle(messages[8])  // a question
    #expect(session.screen == .question)
    #expect(session.canAnswer(at: Date()))
    session.answer(2)
    #expect(session.choice == 2)
    #expect(!session.canAnswer(at: Date()), "only the first answer counts")
    session.handle(messages[11])  // the answer count
    #expect(session.answered == "1 of 2 answered")
    session.handle(messages[12])  // the reveal
    guard case .result(let reveal) = session.screen else {
        Issue.record("No result: \(session.screen)")
        return
    }
    #expect(reveal.correct == 0)
    session.handle(messages[16])  // the quiz's leaderboard
    guard case .ranking(let ranking) = session.screen else { return }
    #expect(ranking.title == "Around the World: final scores")
    #expect(ranking.rows.map(\.name) == ["Jo"])
    session.handle(messages[1])  // a wrong game code
    #expect(session.screen == .refused("That isn't this party's code."))
    #expect(memory.entry(code: "ABCD") == nil)
}

/// Keeps a host pairing in memory, for the tests.
private final class MemoryTokens: HostTokenStore, @unchecked Sendable {
    var pairing: HostPairing?
    func load() -> HostPairing? { pairing }
    func save(_ pairing: HostPairing) { self.pairing = pairing }
    func forget() { pairing = nil }
}

@Test func readsHostLinks() throws {
    let link = try #require(HostLink(scanned: "quizparty://host?host=192.168.1.20&port=8765&pair=ABCDEFGHJK"))
    #expect(link == HostLink(address: PartyAddress(host: "192.168.1.20", port: 8765), pairingCode: "ABCDEFGHJK"))
    #expect(HostLink(scanned: "quizparty://join?host=192.168.1.20&port=8765&code=ABCD") == nil)
    #expect(JoinLink(scanned: "quizparty://host?host=192.168.1.20&port=8765&pair=ABCDEFGHJK") == nil)
}

@MainActor
@Test func aHostSessionKeepsItsTokenAndTheState() throws {
    let tokens = MemoryTokens()
    let address = PartyAddress(host: "127.0.0.1", port: 1)
    let session = HostSession(address: address, pairingCode: "ABCDEFGHJK", deviceName: "Test", tokens: tokens)
    let messages = try examples("examples-server.json").map { try ServerMessage.decode($0) }
    session.handle(.paired(token: "fedcba9876543210"))
    #expect(tokens.pairing == HostPairing(address: address, token: "fedcba9876543210"))
    session.handle(messages[24])  // waiting to move on
    #expect(session.status == .connected)
    #expect(session.state?.next == "Show Standings")
    let wait = try #require(session.advanceAt).timeIntervalSinceNow
    #expect(wait > 12 && wait <= 12.345)
    // Refused once paired: the token is no good, so it is forgotten.
    session.handle(.rejected(reason: .notAuthorised, message: "Not paired."))
    #expect(session.status == .refused("Not paired."))
    #expect(tokens.pairing == nil)
}

@MainActor
@Test func aPausedPlayerCannotAnswer() throws {
    let suite = "QuizPartyKitTests-\(UUID().uuidString)"
    defer { UserDefaults().removePersistentDomain(forName: suite) }
    let session = PlayerSession(
        address: PartyAddress(host: "127.0.0.1", port: 1), code: "ABCD", name: "Sam", memory: JoinMemory(suite: suite))
    let messages = try examples("examples-server.json").map { try ServerMessage.decode($0) }
    session.handle(messages[0])
    session.handle(messages[8])
    session.handle(.paused)
    #expect(session.paused && !session.canAnswer(at: Date()))
    session.handle(.resumed)
    session.handle(messages[8])
    #expect(!session.paused && session.canAnswer(at: Date()))
}

@MainActor
@Test func aPlayerSeesTheCountdownUntilThePartyMovesOn() throws {
    let suite = "QuizPartyKitTests-\(UUID().uuidString)"
    defer { UserDefaults().removePersistentDomain(forName: suite) }
    let session = PlayerSession(
        address: PartyAddress(host: "127.0.0.1", port: 1), code: "ABCD", name: "Sam", memory: JoinMemory(suite: suite))
    let messages = try examples("examples-server.json").map { try ServerMessage.decode($0) }
    session.handle(messages[12])  // the reveal
    session.handle(messages[30])  // the countdown
    let wait = try #require(session.nextUp).at.timeIntervalSinceNow
    #expect(session.nextUp?.title == "Next question" && wait > 2.9 && wait <= 3)
    session.handle(messages[15])  // the standings
    #expect(session.nextUp == nil)
    guard case .ranking(let ranking) = session.screen else {
        Issue.record("No standings: \(session.screen)")
        return
    }
    #expect(ranking.progress == "After 6 of 20 Questions")
}

@MainActor
@Test func thePartyLastJoinedIsRememberedUntilItsCodeIsRefused() throws {
    let suite = "QuizPartyKitTests-\(UUID().uuidString)"
    defer { UserDefaults().removePersistentDomain(forName: suite) }
    let memory = JoinMemory(suite: suite)
    let address = PartyAddress(host: "192.168.1.20", port: 8765)
    let session = PlayerSession(address: address, code: "", name: "Sam", memory: memory)
    session.handle(.welcome(player: 1, token: "0123456789abcdef", name: "Sam"))
    #expect(memory.lastParty == .init(address: address, code: ""), "a party which needs no code")
    let again = PlayerSession(address: address, code: "", name: "", memory: memory)
    again.handle(.rejected(reason: .wrongGameCode, message: "This party needs its game code."))
    #expect(memory.lastParty == nil, "the code is forgotten, and with it the party")
}
