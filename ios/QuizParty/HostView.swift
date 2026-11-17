// The host's remote control: starting quizzes, moving on, skipping, pausing, removing players and ending the party,
// with the question being asked, its answer, and who has answered. The controller decides everything; this only shows
// its state and sends the host's commands.

import QuizPartyKit
import SwiftUI

/// The host's controls for a party.
struct HostView: View {
    let session: HostSession
    /// Stops controlling the party, back to the join screen.
    let leave: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                switch session.status {
                case .connecting(let text):
                    ProgressView(text).controlSize(.large)
                case .refused(let text):
                    ContentUnavailableView(
                        "Can't Control the Party", systemImage: "xmark.octagon", description: Text(text))
                case .connected:
                    if let state = session.state {
                        HostControls(session: session, state: state)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.background)
            .navigationTitle(session.state.map { "Hosting · \($0.gameCode)" } ?? "Hosting")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Leave", action: leave)
                }
            }
        }
    }
}

/// The controls, for the party's state.
private struct HostControls: View {
    let session: HostSession
    let state: HostState
    @State private var confirmingEnd = false
    @State private var removing: PlayerSummary?

    var body: some View {
        VStack(spacing: 0) {
            List {
                if state.paused {
                    Section {
                        Label("Paused", systemImage: "pause.circle.fill")
                            .font(.title3.bold())
                            .foregroundStyle(Theme.accent)
                    }
                }
                phase
                players
                moving
                Section {
                    Button("End the Party", role: .destructive) { confirmingEnd = true }
                        .disabled(state.phase == .ended || state.phase == .setup)
                }
            }
            .scrollContentBackground(.hidden)
            buttons
        }
        .confirmationDialog("End the party for everyone?", isPresented: $confirmingEnd, titleVisibility: .visible) {
            Button("End the Party", role: .destructive) { session.send(.endParty) }
        }
        .confirmationDialog(
            "Remove \(removing?.name ?? "the player") from the party?",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                if let removing { session.send(.removePlayer(player: removing.player)) }
            }
        }
        .sensoryFeedback(.impact, trigger: state.phase)
    }

    /// What the party is doing.
    @ViewBuilder private var phase: some View {
        switch state.phase {
        case .setup:
            Section {
                Text("The party is being set up on the Mac. Players can join once it starts.")
            }
        case .lobby:
            Section("Start a Quiz") {
                ForEach(Array(state.quizzes.enumerated()), id: \.offset) { index, quiz in
                    Button {
                        session.send(.startQuiz(quiz: UInt32(index)))
                    } label: {
                        HStack {
                            VStack(alignment: .leading) {
                                Text(quiz.title).font(.headline)
                                Text("\(quiz.questions) questions").font(.subheadline).foregroundStyle(Theme.muted)
                            }
                            Spacer()
                            if quiz.played {
                                Text("Played").font(.caption).foregroundStyle(Theme.muted)
                            }
                            Image(systemName: "play.fill")
                        }
                    }
                }
            }
        case .question(let view, let correct, let answered):
            Section("\(view.quizTitle) · Question \(view.number) of \(view.total)") {
                HStack(alignment: .firstTextBaseline) {
                    Text(view.text).font(.title3.bold())
                    Spacer()
                    countdown
                }
                ForEach(Array(view.answers.enumerated()), id: \.offset) { position, text in
                    AnswerRow(position: position, text: text, correct: position == Int(correct), count: nil)
                }
                Text("\(answered.count) of \(state.players.filter(\.connected).count) answered")
                    .foregroundStyle(Theme.muted)
            }
        case .reveal(let reveal):
            Section("The Answer") {
                ForEach(Array(reveal.counts.enumerated()), id: \.offset) { position, count in
                    AnswerRow(
                        position: position, text: answerText(position), correct: position == Int(reveal.correct),
                        count: count)
                }
            }
        case .standings(let standings):
            RankingSection(ranking: Ranking(standings))
        case .leaderboard(let board):
            RankingSection(ranking: Ranking(board))
        case .ended:
            Section {
                Text("The party is over. Start a new one on the Mac.")
            }
        }
    }

    /// The time left for the question, counting down.
    private var countdown: some View {
        TimelineView(.periodic(from: .now, by: 0.25)) { context in
            let left = max(0, session.questionEndsAt?.timeIntervalSince(context.date) ?? 0)
            Text(state.paused ? "Paused" : "\(Int(left.rounded(.up))) s")
                .font(.title3.monospacedDigit().bold())
                .foregroundStyle(left < 5 ? AnswerStyle.at(0).color : Theme.accent)
        }
    }

    private func answerText(_ position: Int) -> String {
        guard let answers = session.lastQuestion?.answers, answers.indices.contains(position) else { return "" }
        return answers[position]
    }

    /// The players, with who has answered the question being asked; swipe to remove one.
    private var players: some View {
        let answered: Set<PlayerID> =
            if case .question(_, _, let answered) = state.phase { Set(answered) } else { [] }
        let asking = if case .question = state.phase { true } else { false }
        return Section("Players (\(state.players.count))") {
            if state.players.isEmpty {
                Text("No one has joined yet.").foregroundStyle(Theme.muted)
            }
            ForEach(state.players, id: \.player) { player in
                HStack {
                    Text(player.name)
                    if !player.connected {
                        Text("away").font(.caption).foregroundStyle(Theme.muted)
                    }
                    Spacer()
                    if asking {
                        Image(systemName: answered.contains(player.player) ? "checkmark.circle.fill" : "circle")
                            .foregroundStyle(answered.contains(player.player) ? Theme.right : Theme.muted)
                    }
                }
                .swipeActions {
                    Button("Remove", role: .destructive) { removing = player }
                }
            }
        }
    }

    /// Whether the party moves on by itself, and how long it waits.
    private var moving: some View {
        let auto = state.autoAdvance
        return Section {
            Toggle(
                "Move On by Itself",
                isOn: Binding(
                    get: { auto.enabled },
                    set: { enabled in
                        var changed = auto
                        changed.enabled = enabled
                        session.send(.setAutoAdvance(changed))
                    }))
            if auto.enabled {
                Stepper(
                    "The answer for \(auto.revealSecs) s",
                    value: Binding(
                        get: { Int(auto.revealSecs) },
                        set: { seconds in
                            var changed = auto
                            changed.revealSecs = UInt32(seconds)
                            session.send(.setAutoAdvance(changed))
                        }), in: 1...600)
                Stepper(
                    "The standings for \(auto.standingsSecs) s",
                    value: Binding(
                        get: { Int(auto.standingsSecs) },
                        set: { seconds in
                            var changed = auto
                            changed.standingsSecs = UInt32(seconds)
                            session.send(.setAutoAdvance(changed))
                        }), in: 1...600, step: 5)
            }
            Stepper(
                auto.countdownSecs == 0 ? "Next moves on at once" : "Next counts down \(auto.countdownSecs) s",
                value: Binding(
                    get: { Int(auto.countdownSecs) },
                    set: { seconds in
                        var changed = auto
                        changed.countdownSecs = UInt32(seconds)
                        session.send(.setAutoAdvance(changed))
                    }), in: 0...60)
        } header: {
            Text("Moving On")
        } footer: {
            if let advanceAt = session.advanceAt {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text("Moving on in \(Int(max(0, advanceAt.timeIntervalSince(context.date)).rounded(.up))) s")
                }
            }
        }
    }

    /// Pause or resume, skip, and Next, along the bottom where a thumb reaches them.
    private var buttons: some View {
        let running: Bool =
            switch state.phase {
            case .question, .reveal, .standings, .leaderboard: true
            default: false
            }
        let asking = if case .question = state.phase { true } else { false }
        return HStack(spacing: 10) {
            if running {
                Button {
                    session.send(state.paused ? .resume : .pause)
                } label: {
                    Image(systemName: state.paused ? "play.fill" : "pause.fill")
                        .frame(minWidth: 30, minHeight: 36)
                }
                .buttonStyle(.bordered)
                .accessibilityLabel(state.paused ? "Resume" : "Pause")
            }
            if asking {
                Button {
                    session.send(.skip)
                } label: {
                    Text("Skip").frame(minHeight: 36)
                }
                .buttonStyle(.bordered)
            }
            if let next = state.next {
                Button {
                    session.send(.next)
                } label: {
                    Text(next).font(.headline).frame(maxWidth: .infinity, minHeight: 36)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding()
    }
}

/// An answer in the host's view: its shape and colour, its text, whether it is right, and how many chose it.
private struct AnswerRow: View {
    let position: Int
    let text: String
    let correct: Bool
    let count: UInt32?

    var body: some View {
        HStack {
            AnswerShape(position: position)
                .fill(AnswerStyle.at(position).color)
                .frame(width: 22, height: 22)
            Text(text).fontWeight(correct ? .bold : .regular)
            Spacer()
            if let count {
                Text("\(count)").monospacedDigit().foregroundStyle(Theme.muted)
            }
            if correct {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(Theme.right)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// The standings or a leaderboard, in full, as the host may want to read out more than the top few.
private struct RankingSection: View {
    let ranking: Ranking

    var body: some View {
        Section(ranking.title) {
            ForEach(ranking.rows) { row in
                HStack {
                    Text(ordinal(row.rank)).foregroundStyle(Theme.muted).frame(width: 44, alignment: .leading)
                    Text(row.name)
                    Spacer()
                    Text("\(row.score)").monospacedDigit()
                }
            }
        }
    }
}
