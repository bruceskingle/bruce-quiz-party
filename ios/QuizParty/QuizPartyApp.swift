// Bruce's Quiz Party for iPhone: a player's phone in a party run by the Mac. It finds the party with Bonjour or its
// QR code, joins, and shows what the controller sends; the game itself is all the controller's.

import QuizPartyKit
import SwiftUI

@main
struct QuizPartyApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
        }
    }
}

/// The look of the browser client and the big screen: dark, with the same accent, brighter and bolder as it is a game.
enum Theme {
    static let background = Color(red: 0x0f / 255, green: 0x11 / 255, blue: 0x16 / 255)
    static let panel = Color(red: 0x17 / 255, green: 0x1b / 255, blue: 0x24 / 255)
    static let accent = Color(red: 0x06 / 255, green: 0xb6 / 255, blue: 0xd4 / 255)
    static let muted = Color(red: 0x9a / 255, green: 0xa6 / 255, blue: 0xb2 / 255)
    static let error = Color(red: 0xff / 255, green: 0x6b / 255, blue: 0x6b / 255)
    static let right = AnswerStyle.at(3).color
    static let wrong = AnswerStyle.at(0).color
}

/// The join screen until the player joins, then the game; a quizparty:// link joins from anywhere.
struct RootView: View {
    @State private var session: PlayerSession?
    /// Why the last join was refused, shown on the join screen.
    @State private var refusal: String?
    /// A party chosen by a link, for the join screen to fill in.
    @State private var link: JoinLink?

    var body: some View {
        ZStack {
            Theme.background.ignoresSafeArea()
            if let session {
                PlayView(session: session) { leave() }
            } else {
                JoinView(refusal: $refusal, link: $link) { join($0) }
            }
        }
        .onOpenURL { url in
            if let link = JoinLink(url: url) {
                leave()
                self.link = link
            }
        }
        #if DEBUG
            .onAppear {
                // For trying the app in the simulator, which asks before opening a link: launched with `-joinLink
                // <link>`, it uses the link at once.
                if let text = UserDefaults.standard.string(forKey: "joinLink"), let link = JoinLink(scanned: text) {
                    self.link = link
                }
            }
        #endif
        .onChange(of: session?.screen) { _, screen in
            if case .refused(let message) = screen {
                refusal = message
                leave()
            }
        }
        .onChange(of: session == nil, initial: true) { _, idle in
            // Keep the screen on while playing, so that it is awake when the next question comes.
            UIApplication.shared.isIdleTimerDisabled = !idle
        }
    }

    private func join(_ session: PlayerSession) {
        refusal = nil
        session.start()
        self.session = session
    }

    private func leave() {
        session?.leave()
        session = nil
    }
}
