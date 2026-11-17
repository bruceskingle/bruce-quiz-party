// A host phone's connection to a party: pairs with the one-time code from the Mac's setup screen, or connects with the
// token it was given then, and holds the party as a host sees it. The commands it sends are the host's buttons; the
// controller does the rest.

import Foundation
import Observation
import Security

/// Where a host phone keeps the party it is paired with and its token.
public protocol HostTokenStore: Sendable {
    func load() -> HostPairing?
    func save(_ pairing: HostPairing)
    func forget()
}

/// The party a phone is paired with as a host, and the secret it connects with.
public struct HostPairing: Codable, Equatable, Sendable {
    public var address: PartyAddress
    public var token: String

    public init(address: PartyAddress, token: String) {
        self.address = address
        self.token = token
    }
}

/// Keeps the host pairing in the Keychain, as its token lets the phone control the party.
public struct KeychainHostTokens: HostTokenStore {
    private static let service = "org.skingle.bruce-quiz-party.host"
    private static let account = "pairing"

    public init() {}

    private var query: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.service,
            kSecAttrAccount as String: Self.account,
        ]
    }

    public func load() -> HostPairing? {
        var query = query
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else {
            return nil
        }
        return try? JSONDecoder().decode(HostPairing.self, from: data)
    }

    public func save(_ pairing: HostPairing) {
        guard let data = try? JSONEncoder().encode(pairing) else { return }
        forget()
        var item = query
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    public func forget() {
        SecItemDelete(query as CFDictionary)
    }
}

/// A host phone's connection to a party, and the party as a host sees it.
@MainActor
@Observable
public final class HostSession {
    /// How the connection is.
    public enum Status: Equatable, Sendable {
        case connecting(String)
        case connected
        /// Refused, e.g. as the phone is no longer paired, with why.
        case refused(String)
    }

    public let address: PartyAddress
    public private(set) var status: Status = .connecting("Connecting…")
    /// The party as a host sees it, once it has been sent.
    public private(set) var state: HostState?
    /// When the party will move on by itself, by this phone's clock, if it will.
    public private(set) var advanceAt: Date?
    /// The question being asked, or last asked, for its answers' text in the reveal.
    public private(set) var lastQuestion: QuestionView?
    /// When the question being asked runs out of time, by this phone's clock.
    public private(set) var questionEndsAt: Date?

    private var pairingCode: String?
    private let deviceName: String
    private let tokens: any HostTokenStore
    private var connection: Connection?

    /// A session which pairs with a pairing code, or, without one, connects with the token kept when it paired.
    public init(
        address: PartyAddress, pairingCode: String?, deviceName: String,
        tokens: any HostTokenStore = KeychainHostTokens()
    ) {
        self.address = address
        self.pairingCode = pairingCode
        self.deviceName = deviceName
        self.tokens = tokens
    }

    /// Connects, and pairs or connects as a host.
    public func start() {
        guard let url = address.webSocketURL else {
            status = .refused("That isn't a party's address.")
            return
        }
        let connection = Connection(url: url)
        connection.opened = { [unowned self] in introduce() }
        connection.received = { [unowned self] in handle($0) }
        connection.dropped = { [unowned self] in status = .connecting("Reconnecting…") }
        self.connection = connection
        connection.start()
    }

    /// Stops controlling the party; the phone stays paired, to come back.
    public func leave() {
        connection?.stop()
    }

    /// Sends a host's command.
    public func send(_ command: HostCommand) {
        connection?.send(.host(command: command))
    }

    /// Pairs with the code, the first time, or connects with the token kept since.
    private func introduce() {
        if let pairingCode {
            connection?.send(.pairHost(pairingCode: pairingCode, deviceName: deviceName))
        } else if let pairing = tokens.load(), pairing.address == address {
            connection?.send(.hostConnect(token: pairing.token))
        } else {
            connection?.stop()
            status = .refused("This phone isn't paired with that party.")
        }
    }

    /// Handles a message from the controller.
    func handle(_ message: ServerMessage) {
        switch message {
        case .ping(let nonce):
            connection?.send(.pong(nonce: nonce))
        case .paired(let token):
            tokens.save(HostPairing(address: address, token: token))
            pairingCode = nil
        case .hostState(let state):
            self.state = state
            advanceAt = state.advanceInMs.map { Date().addingTimeInterval(Double($0) / 1000) }
            if case .question(let view, _, _) = state.phase {
                lastQuestion = view
                questionEndsAt = Date().addingTimeInterval(Double(view.remainingMs) / 1000)
            }
            status = .connected
            connection?.succeeded()
        case .rejected(let reason, let message):
            connection?.stop()
            if reason == .notAuthorised && pairingCode == nil {
                tokens.forget()
            }
            status = .refused(message)
        default:
            break
        }
    }
}
