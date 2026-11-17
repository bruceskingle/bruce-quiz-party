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
    #expect(messages.count == 29)
    #expect(messages.first == .welcome(player: 1, token: "0123456789abcdef", name: "Sam"))
    guard case .question(let view) = messages[8] else {
        Issue.record("No question: \(messages[8])")
        return
    }
    #expect(view.quizTitle == "Around the World")
    #expect(view.image == "/quiz/around-the-world/images/flag.svg")
    #expect(view.imageAlt == "A red disc on white")
    #expect(view.answers == ["Japan", "Bangladesh", "Palau"])
    #expect((view.timeLimitMs, view.remainingMs, view.maxPoints) == (20_000, 12_345, 1000))
    guard case .question(let plain) = messages[9] else { return }
    #expect(plain.image == nil && plain.imageAlt == nil)
    guard case .reveal(let reveal) = messages[12] else { return }
    #expect(reveal.result?.quizRank == 2)
    guard case .reveal(let tooSlow) = messages[13] else { return }
    #expect(tooSlow.result?.choice == nil && tooSlow.source == nil)
    guard case .leaderboard(let board) = messages[16] else { return }
    #expect(board.sortedBy == .quiz(1))
    #expect(board.rows[0].quizzes == [nil, QuizResult(score: 2400, rank: 1)])
    guard case .hostState(let state) = messages[23] else { return }
    #expect(state.phase == .lobby)
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
    #expect(JoinLink(scanned: "quizparty://join?host=mac.local") == nil, "no port")
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
