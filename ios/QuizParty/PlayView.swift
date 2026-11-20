// The game on the player's phone: the lobby, each question's answer buttons, how they did, and the rankings, as the
// controller sends them.

import QuizPartyKit
import SwiftUI

/// The screens of a party joined.
struct PlayView: View {
    let session: PlayerSession
    /// Leaves the party, back to the join screen.
    let leave: () -> Void
    /// Whether the player is being asked to confirm leaving.
    @State private var confirmingLeave = false

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
            .overlay(alignment: .top) {
                if session.paused {
                    Label("Paused", systemImage: "pause.circle.fill")
                        .font(.title2.weight(.heavy))
                        .foregroundStyle(Theme.background)
                        .padding(.horizontal, 20)
                        .padding(.vertical, 10)
                        .background(Theme.accent, in: .capsule)
                        .padding(.top, 8)
                }
            }
            .animation(.default, value: session.screen)
            if let nextUp = session.nextUp, showsCountdown {
                // The countdown to what comes next, under the result or the standings.
                TimelineView(.periodic(from: .now, by: 0.25)) { context in
                    let seconds = Int(max(0, nextUp.at.timeIntervalSince(context.date)).rounded(.up))
                    Text("\(nextUp.title) in \(seconds)")
                        .font(.title2.weight(.heavy))
                        .foregroundStyle(Theme.accent)
                        .monospacedDigit()
                }
                .padding(.bottom, 8)
            }
            if showsLeave {
                if isOver {
                    Button("Done", action: leave).padding(.bottom, 8)
                } else {
                    // Asked first, as a stray tap would take the player out of the party.
                    Button("Leave the Party", role: .destructive) { confirmingLeave = true }
                        .padding(.bottom, 8)
                        .confirmationDialog(
                            "Leave the party?", isPresented: $confirmingLeave, titleVisibility: .visible
                        ) {
                            Button("Leave the Party", role: .destructive, action: leave)
                            Button("Stay", role: .cancel) {}
                        } message: {
                            Text("You can join again with the same name and keep your score.")
                        }
                }
            }
        }
    }

    private var isOver: Bool {
        if case .ended = session.screen { true } else { false }
    }

    /// The countdown to moving on is shown under the result and the standings.
    private var showsCountdown: Bool {
        switch session.screen {
        case .result, .ranking: true
        default: false
        }
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
            // While the clock is stopped, so is the countdown.
            let now = session.pausedAt ?? context.date
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
                Text(status(at: now))
                    .font(.title3.weight(.heavy))
            }
        }
        .sensoryFeedback(.impact(weight: .heavy), trigger: session.choice)
    }

    /// What to say under the answers: that the answer is locked in, or the time is up, or nothing.
    private func status(at now: Date) -> String {
        if session.choice != nil { return "Answer locked in!" }
        if session.paused || session.canAnswer(at: now) { return " " }
        return "Time's up!"
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
            if let progress = reveal.progress {
                Text(progress.words).font(.title3).foregroundStyle(Theme.muted)
            }
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
            if !reveal.counts.isEmpty {
                AnswerCounts(
                    counts: reveal.counts, answers: answers, correct: Int(reveal.correct),
                    choice: reveal.result?.choice.map(Int.init)
                )
                .padding(.top, 8)
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

/// Every answer with how many chose it, as the big screen shows them: a row per answer, in its colour and shape, with
/// its text, its count, and a bar across it as long as its share of the most chosen; the correct answer's bright and
/// ticked and the others faded, and this player's choice outlined.
private struct AnswerCounts: View {
    let counts: [UInt32]
    let answers: [String]
    let correct: Int
    let choice: Int?

    var body: some View {
        let most = CGFloat(max(1, counts.max() ?? 0))
        VStack(spacing: 8) {
            ForEach(Array(counts.enumerated()), id: \.offset) { position, count in
                HStack(spacing: 10) {
                    AnswerShape(position: position).fill(.white).frame(width: 22, height: 22)
                    Text(text(position)).fontWeight(.bold).frame(maxWidth: .infinity, alignment: .leading)
                    if position == correct {
                        Image(systemName: "checkmark").fontWeight(.heavy)
                    }
                    Text("\(count)").font(.title3.weight(.heavy)).monospacedDigit()
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(alignment: .leading) {
                    GeometryReader { space in
                        AnswerStyle.at(position).color
                            .opacity(position == correct ? 1 : 0.45)
                            .frame(width: space.size.width * CGFloat(count) / most)
                    }
                }
                .background(Theme.panel)
                .clipShape(.rect(cornerRadius: 12))
                .overlay {
                    if position == choice {
                        RoundedRectangle(cornerRadius: 12).strokeBorder(.white, lineWidth: 3)
                    }
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(label(position: position, count: count))
            }
        }
    }

    /// An answer's text, if the phone saw the question, which it did not if it joined during the reveal.
    private func text(_ position: Int) -> String {
        answers.indices.contains(position) ? answers[position] : ""
    }

    /// What VoiceOver reads for an answer, e.g. "red triangle, Paris, 3 players, correct, your answer".
    private func label(position: Int, count: UInt32) -> String {
        let style = AnswerStyle.at(position)
        var words = "\(style.name) \(style.shape), \(text(position)), \(count) \(count == 1 ? "player" : "players")"
        if position == correct { words += ", correct" }
        if position == choice { words += ", your answer" }
        return words
    }
}

/// The standings or a leaderboard: the top five, and this player's place.
private struct RankingScreen: View {
    let ranking: Ranking
    let player: PlayerID?

    var body: some View {
        VStack(spacing: 12) {
            Text(ranking.title).font(.largeTitle.bold()).multilineTextAlignment(.center)
            if let progress = ranking.progress {
                Text(progress).font(.title3).foregroundStyle(Theme.muted)
            }
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
