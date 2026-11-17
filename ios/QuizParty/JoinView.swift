// Joining a party: choosing it from those Bonjour finds, scanning its QR code or typing its address, then the game
// code and a name.

import QuizPartyKit
import SwiftUI

/// The join screen.
struct JoinView: View {
    /// Why the last join was refused.
    @Binding var refusal: String?
    /// A party chosen by a link, to fill in.
    @Binding var link: JoinLink?
    /// Joins with a new session.
    let join: (PlayerSession) -> Void

    @State private var browser = PartyBrowser()
    @State private var address: PartyAddress?
    /// The name of the party chosen from those found, or its address.
    @State private var partyName = ""
    @State private var code = ""
    @State private var name = JoinMemory().lastName
    @State private var typedAddress = ""
    @State private var scanning = false
    @State private var resolving: PartyBrowser.Found?
    @State private var problem: String?
    @FocusState private var focus: Field?
    @Environment(\.scenePhase) private var scenePhase

    private enum Field {
        case address, code, name
    }

    var body: some View {
        NavigationStack {
            Form {
                if let address {
                    chosen(address)
                } else {
                    choose
                }
                if let message = refusal ?? problem {
                    Section {
                        Text(message).foregroundStyle(Theme.error)
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.background)
            .navigationTitle("Bruce's Quiz Party")
            .navigationBarTitleDisplayMode(.inline)
        }
        .onAppear { browser.start() }
        .onDisappear { browser.stop() }
        .onChange(of: scenePhase) { _, phase in
            // iOS ends the search while the app is in the background, so it is stopped then and started afresh.
            if phase == .active {
                browser.start()
            } else if phase == .background {
                browser.stop()
            }
        }
        .onChange(of: link, initial: true) { _, link in
            if let link {
                use(link)
                self.link = nil
            }
        }
        .sheet(isPresented: $scanning) {
            ScannerSheet { text in
                scanning = false
                if let link = JoinLink(scanned: text) {
                    use(link)
                } else {
                    problem = "That QR code isn't a party's. Scan the one on the big screen."
                }
            }
        }
    }

    /// Choosing a party: those found nearby, scanning, or typing the address.
    @ViewBuilder private var choose: some View {
        Section {
            Button {
                problem = nil
                scanning = true
            } label: {
                Label("Scan the QR Code", systemImage: "qrcode.viewfinder")
                    .font(.title3.bold())
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.borderedProminent)
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets())
        } footer: {
            Text("Scan the code on the big screen.")
        }
        Section("Parties Nearby") {
            if browser.parties.isEmpty {
                HStack {
                    ProgressView()
                    Text(browser.problem ?? "Looking…").foregroundStyle(Theme.muted)
                }
            }
            ForEach(browser.parties) { party in
                Button {
                    resolve(party)
                } label: {
                    HStack {
                        Label(party.name, systemImage: "party.popper")
                        Spacer()
                        if resolving == party {
                            ProgressView()
                        }
                    }
                }
                .disabled(resolving != nil)
            }
        }
        Section {
            TextField("e.g. 192.168.1.20:8765", text: $typedAddress)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .focused($focus, equals: .address)
                .submitLabel(.next)
                .onSubmit(useTypedAddress)
            Button("Use This Address", action: useTypedAddress)
                .disabled(typedAddress.trimmingCharacters(in: .whitespaces).isEmpty)
        } header: {
            Text("Or Type the Address")
        } footer: {
            Text("The address is under the QR code on the big screen.")
        }
    }

    /// The party chosen: the game code and the player's name.
    @ViewBuilder private func chosen(_ address: PartyAddress) -> some View {
        Section {
            HStack {
                Label(partyName, systemImage: "party.popper")
                Spacer()
                Button("Change") {
                    self.address = nil
                    refusal = nil
                }
            }
        }
        Section("Game Code") {
            TextField("ABCD", text: $code)
                .font(.title2.monospaced().bold())
                .textInputAutocapitalization(.characters)
                .autocorrectionDisabled()
                .focused($focus, equals: .code)
                .submitLabel(.next)
                .onSubmit { focus = .name }
                .onChange(of: code) { _, new in
                    if new != new.uppercased() { code = new.uppercased() }
                }
        }
        Section("Your Name") {
            TextField("Name", text: $name)
                .font(.title2)
                .textInputAutocapitalization(.words)
                .autocorrectionDisabled()
                .focused($focus, equals: .name)
                .submitLabel(.join)
                .onSubmit { joinParty(address) }
                .onChange(of: name) { _, new in
                    if new.count > 20 { name = String(new.prefix(20)) }
                }
        }
        Section {
            Button {
                joinParty(address)
            } label: {
                Text("Join").font(.title2.bold()).frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.borderedProminent)
            .disabled(
                code.trimmingCharacters(in: .whitespaces).isEmpty || name.trimmingCharacters(in: .whitespaces).isEmpty
            )
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets())
        }
    }

    /// Uses a party from its QR code or link: with a remembered token for its code, rejoins at once, as the same
    /// player; otherwise asks for the name.
    private func use(_ link: JoinLink) {
        problem = nil
        address = link.address
        partyName = "\(link.address.host):\(link.address.port)"
        if let linkCode = link.code {
            code = linkCode
            if JoinMemory().entry(code: linkCode) != nil {
                join(PlayerSession(address: link.address, code: linkCode, name: ""))
                return
            }
            #if DEBUG
                // For trying the app in the simulator without typing: launched with `-autoJoin YES -quizparty:name
                // Sam`, a link joins at once.
                if UserDefaults.standard.bool(forKey: "autoJoin"), !name.isEmpty {
                    joinParty(link.address)
                    return
                }
            #endif
        }
        focus = code.isEmpty ? .code : .name
    }

    private func useTypedAddress() {
        guard let typed = PartyAddress(typed: typedAddress) else {
            problem = "That isn't an address. It looks like 192.168.1.20:8765."
            return
        }
        if let link = JoinLink(scanned: typedAddress) {
            use(link)
        } else {
            use(JoinLink(address: typed, code: nil))
        }
    }

    private func resolve(_ party: PartyBrowser.Found) {
        problem = nil
        resolving = party
        Task {
            do {
                let found = try await PartyBrowser.resolve(party)
                use(JoinLink(address: found, code: nil))
                partyName = party.name
            } catch {
                problem = "Can't reach \(party.name): \(error.localizedDescription)"
            }
            resolving = nil
        }
    }

    private func joinParty(_ address: PartyAddress) {
        let name = name.trimmingCharacters(in: .whitespaces)
        let code = code.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty, !code.isEmpty else { return }
        JoinMemory().lastName = name
        // With a token remembered for this code, the controller rejoins as the same player, with their score.
        join(PlayerSession(address: address, code: code, name: name))
    }
}
