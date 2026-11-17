// The game on the player's phone: the lobby, each question's answer buttons, how they did, and the rankings, as the
// controller sends them.

import QuizPartyKit
import SwiftUI

/// The screens of a party joined.
struct PlayView: View {
    let session: PlayerSession
    /// Leaves the party, back to the join screen.
    let leave: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            Text("Bruce's Quiz Party")
                .font(.title3.weight(.heavy))
                .foregroundStyle(Theme.accent)
                .padding(.vertical, 8)
            Group {
                switch session.screen {
                case .connecting(let text):
                    MessageScreen(text: text, busy: true)
                case .lobby:
                    LobbyScreen(session: session)
                case .question:
                    if let question = session.question {
                        QuestionScreen(session: session, question: question)
                    }
                case .result(let reveal):
                    ResultScreen(reveal: reveal, answers: session.question?.answers ?? [])
                case .ranking(let ranking):
                    RankingScreen(ranking: ranking, player: session.player)
                case .refused(let text), .ended(let text):
                    MessageScreen(text: text, busy: false)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.horizontal)
            .animation(.default, value: session.screen)
            if showsLeave {
                Button(isOver ? "Done" : "Leave the Party", role: isOver ? nil : .destructive, action: leave)
                    .padding(.bottom, 8)
            }
        }
    }

    private var isOver: Bool {
        if case .ended = session.screen { true } else { false }
    }

    /// Leaving is offered between questions, not where a stray tap would cost an answer.
    private var showsLeave: Bool {
        switch session.screen {
        case .question: false
        default: true
        }
    }
}

/// A message on its own, e.g. while connecting.
private struct MessageScreen: View {
    let text: String
    let busy: Bool

    var body: some View {
        VStack(spacing: 16) {
            if busy {
                ProgressView().controlSize(.large)
            }
            Text(text).font(.title2.bold()).multilineTextAlignment(.center)
        }
    }
}

/// Joined, waiting for the host to start a quiz.
private struct LobbyScreen: View {
    let session: PlayerSession

    var body: some View {
        VStack(spacing: 12) {
            Text("You're in, \(session.name)!").font(.largeTitle.bold()).multilineTextAlignment(.center)
            Text("Waiting for the host to start the quiz…").foregroundStyle(Theme.muted)
            ScrollView {
                FlowList(players: session.players, me: session.player)
            }
        }
        .padding(.top, 24)
    }
}

/// The players in the lobby, this player highlighted and those whose phones are away dimmed.
private struct FlowList: View {
    let players: [PlayerSummary]
    let me: PlayerID?

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 110))], spacing: 8) {
            ForEach(players, id: \.player) { player in
                Text(player.name)
                    .font(.headline)
                    .lineLimit(1)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .frame(maxWidth: .infinity)
                    .background(player.player == me ? Theme.accent.opacity(0.35) : Theme.panel, in: .capsule)
                    .opacity(player.connected ? 1 : 0.45)
            }
        }
    }
}

/// A question: the countdown, the text and any image, and a button per answer, locked after the first tap.
private struct QuestionScreen: View {
    let session: PlayerSession
    let question: QuestionView

    var body: some View {
        TimelineView(.animation) { context in
            let now = context.date
            VStack(spacing: 10) {
                HStack {
                    Text("Question \(question.number) of \(question.total)")
                    Spacer()
                    Text(session.answered)
                }
                .font(.subheadline)
                .foregroundStyle(Theme.muted)
                countdown(now)
                Text(question.text)
                    .font(.title2.bold())
                    .multilineTextAlignment(.center)
                    .minimumScaleFactor(0.6)
                if let path = question.image, let url = session.address.fileURL(path) {
                    QuestionImage(url: url, alt: question.imageAlt ?? "")
                        .frame(maxHeight: 180)
                }
                answers(enabled: session.canAnswer(at: now))
                Text(session.choice == nil ? (session.canAnswer(at: now) ? " " : "Time's up!") : "Answer locked in!")
                    .font(.title3.weight(.heavy))
            }
        }
        .sensoryFeedback(.impact(weight: .heavy), trigger: session.choice)
    }

    /// The time left, as a shrinking bar which turns red in the last five seconds.
    private func countdown(_ now: Date) -> some View {
        let left = max(0, session.endsAt?.timeIntervalSince(now) ?? 0)
        let fraction = min(1, left / max(0.001, Double(question.timeLimitMs) / 1000))
        return GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.panel)
                Capsule()
                    .fill(left < 5 ? AnswerStyle.at(0).color : Theme.accent)
                    .frame(width: geometry.size.width * fraction)
            }
        }
        .frame(height: 10)
    }

    /// The answer buttons, two to a row, or one each when there are only two.
    private func answers(enabled: Bool) -> some View {
        let columns = question.answers.count <= 2 ? 1 : 2
        return Grid(horizontalSpacing: 10, verticalSpacing: 10) {
            ForEach(Array(stride(from: 0, to: question.answers.count, by: columns)), id: \.self) { start in
                GridRow {
                    ForEach(start..<min(start + columns, question.answers.count), id: \.self) { position in
                        AnswerButton(
                            position: position, text: question.answers[position], enabled: enabled,
                            chosen: session.choice.map { Int($0) == position }
                        ) {
                            session.answer(position)
                        }
                    }
                }
            }
        }
        .frame(maxHeight: .infinity)
    }
}

/// An answer: its colour, its shape and its text, filling its share of the screen to be easy to hit.
private struct AnswerButton: View {
    let position: Int
    let text: String
    let enabled: Bool
    /// Whether this is the answer chosen, once one is; nil before.
    let chosen: Bool?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                AnswerShape(position: position)
                    .fill(.white)
                    .frame(width: 30, height: 30)
                Text(text)
                    .font(.title3.bold())
                    .multilineTextAlignment(.center)
                    .minimumScaleFactor(0.5)
            }
            .foregroundStyle(.white)
            .padding(10)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(AnswerStyle.at(position).color, in: .rect(cornerRadius: 18))
            .overlay {
                if chosen == true {
                    RoundedRectangle(cornerRadius: 18).stroke(.white, lineWidth: 5)
                }
            }
            .opacity(chosen == false || (chosen == nil && !enabled) ? 0.35 : 1)
            .scaleEffect(chosen == true ? 1.03 : 1)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityLabel("\(AnswerStyle.at(position).name) \(AnswerStyle.at(position).shape): \(text)")
    }
}

/// How the player did, e.g. "Correct! +850 points, you're 3rd with 2400 points".
private struct ResultScreen: View {
    let reveal: Reveal
    let answers: [String]

    var body: some View {
        let correctText = answers.indices.contains(Int(reveal.correct)) ? answers[Int(reveal.correct)] : ""
        VStack(spacing: 16) {
            VStack(spacing: 8) {
                if let result = reveal.result {
                    Text(result.correct ? "Correct!" : result.choice == nil ? "Too slow!" : "Wrong!")
                        .font(.system(size: 40, weight: .heavy))
                    Text("+\(result.points) points").font(.title.weight(.heavy))
                    Text("You're \(ordinal(result.quizRank)) with \(result.quizScore) points").font(.title3)
                } else {
                    Text("The answer was").font(.title2.bold())
                    Text(correctText).font(.title.weight(.heavy))
                }
            }
            .multilineTextAlignment(.center)
            .padding(.vertical, 28)
            .frame(maxWidth: .infinity)
            .background(panelColour, in: .rect(cornerRadius: 18))
            if let result = reveal.result, !result.correct {
                Text("The answer was \(correctText)").foregroundStyle(Theme.muted)
            }
            Spacer()
        }
        .padding(.top, 24)
        .sensoryFeedback(reveal.result?.correct == true ? .success : .error, trigger: reveal.question)
    }

    private var panelColour: Color {
        guard let result = reveal.result else { return Theme.panel }
        return result.correct ? Theme.right : Theme.wrong
    }
}

/// The standings or a leaderboard: the top five, and this player's place.
private struct RankingScreen: View {
    let ranking: Ranking
    let player: PlayerID?

    var body: some View {
        VStack(spacing: 12) {
            Text(ranking.title).font(.largeTitle.bold()).multilineTextAlignment(.center)
            if let me = ranking.row(of: player) {
                Text("You're \(ordinal(me.rank)) with \(me.score) points").font(.title3).foregroundStyle(Theme.muted)
            }
            VStack(spacing: 8) {
                ForEach(ranking.top) { row in
                    HStack {
                        Text(ordinal(row.rank)).frame(width: 48, alignment: .leading).foregroundStyle(Theme.muted)
                        Text(row.name).lineLimit(1)
                        Spacer()
                        Text("\(row.score)").monospacedDigit()
                    }
                    .font(.title3.bold())
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .background(
                        row.player == player ? Theme.accent.opacity(0.35) : Theme.panel, in: .rect(cornerRadius: 12))
                }
            }
            Spacer()
        }
        .padding(.top, 24)
    }
}
