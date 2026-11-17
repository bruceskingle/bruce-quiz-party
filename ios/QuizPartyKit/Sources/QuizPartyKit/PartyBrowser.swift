// Finds parties on the local network with Bonjour, as the controller advertises them, so that a player can tap one
// rather than type its address.

import Foundation
@preconcurrency import Network
import Observation

/// The parties advertised on the local network, kept up to date while browsing.
@MainActor
@Observable
public final class PartyBrowser {
    /// A party found on the network.
    public struct Found: Identifiable, Hashable, Sendable {
        /// The name it is advertised under, the Mac's name.
        public let name: String
        let endpoint: NWEndpoint

        public var id: String { name }
    }

    /// The parties found, by name.
    public private(set) var parties: [Found] = []
    /// Why none can be found, e.g. that the app may not use the local network, to show the player.
    public private(set) var problem: String?

    private var browser: NWBrowser?

    public init() {}

    /// Starts looking, if it is not already.
    public func start() {
        guard browser == nil else { return }
        let browser = NWBrowser(for: .bonjour(type: bonjourService, domain: nil), using: .tcp)
        browser.browseResultsChangedHandler = { results, _ in
            let found = results.compactMap { result -> Found? in
                guard case .service(let name, _, _, _) = result.endpoint else { return nil }
                return Found(name: name, endpoint: result.endpoint)
            }
            MainActor.assumeIsolated {
                self.parties = found.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
            }
        }
        browser.stateUpdateHandler = { [weak browser] state in
            MainActor.assumeIsolated {
                switch state {
                case .ready:
                    self.problem = nil
                case .failed(let error) where PartyBrowser.isDefunct(error):
                    // iOS closes the search while the app is suspended; a new one picks up where it left off.
                    self.restart(after: browser)
                case .waiting(let error), .failed(let error):
                    self.problem = PartyBrowser.describe(error)
                default:
                    break
                }
            }
        }
        browser.start(queue: .main)
        self.browser = browser
    }

    /// Starts a new search in place of one which failed, if it is still the current one, after a moment so that a
    /// search which keeps failing does not spin.
    private func restart(after failed: NWBrowser?) {
        guard let failed, failed === browser else { return }
        failed.cancel()
        browser = nil
        Task {
            try? await Task.sleep(for: .seconds(1))
            if browser == nil {
                start()
            }
        }
    }

    /// Whether an error means only that the system closed the search, as it does while the app is in the background.
    private static func isDefunct(_ error: NWError) -> Bool {
        if case .dns(let code) = error, code == -65569 { true } else { false }
    }

    /// Stops looking.
    public func stop() {
        browser?.cancel()
        browser = nil
        parties = []
    }

    /// The words for a browsing error: most often that the player has not allowed the app to use the local network.
    private static func describe(_ error: NWError) -> String {
        if case .dns(let code) = error, code == -65555 {
            return
                "Quiz Party may not use the local network. Allow it in Settings ▸ Privacy & Security ▸ Local Network."
        }
        return "Can't look for parties: \(error.localizedDescription)"
    }

    /// The address of a party found, by connecting to it once: an IPv4 address, which the WebSocket's URL can hold
    /// without the interface an IPv6 link-local address would need.
    public static func resolve(_ found: Found) async throws -> PartyAddress {
        let parameters = NWParameters.tcp
        if let ip = parameters.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options {
            ip.version = .v4
        }
        let connection = NWConnection(to: found.endpoint, using: parameters)
        let resolver = Resolver(connection: connection)
        return try await resolver.run()
    }
}

/// Resolves a Bonjour service to an address by connecting to it, giving up after a few seconds.
private final class Resolver: @unchecked Sendable {
    /// Touched only on `queue`.
    private let connection: NWConnection
    private let queue = DispatchQueue(label: "quizparty.resolve")
    private var continuation: CheckedContinuation<PartyAddress, Error>?

    init(connection: NWConnection) {
        self.connection = connection
    }

    func run() async throws -> PartyAddress {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                self.continuation = continuation
                self.connection.stateUpdateHandler = { state in
                    switch state {
                    case .ready:
                        if case .hostPort(let host, let port) = self.connection.currentPath?.remoteEndpoint {
                            self.finish(.success(PartyAddress(host: Resolver.text(host), port: port.rawValue)))
                        } else {
                            self.finish(.failure(ResolveError.noAddress))
                        }
                    case .failed(let error), .waiting(let error):
                        self.finish(.failure(error))
                    default:
                        break
                    }
                }
                self.connection.start(queue: self.queue)
                self.queue.asyncAfter(deadline: .now() + 5) { self.finish(.failure(ResolveError.timedOut)) }
            }
        }
    }

    /// Ends the attempt once, with its outcome.
    private func finish(_ result: Result<PartyAddress, Error>) {
        guard let continuation else { return }
        self.continuation = nil
        connection.cancel()
        continuation.resume(with: result)
    }

    /// A host as text for a URL, without any interface, e.g. `192.168.1.20`.
    private static func text(_ host: NWEndpoint.Host) -> String {
        let text: String
        switch host {
        case .ipv4(let address): text = "\(address)"
        case .ipv6(let address): text = "\(address)"
        case .name(let name, _): text = name
        @unknown default: text = "\(host)"
        }
        return String(text.split(separator: "%").first ?? Substring(text))
    }
}

/// Why a party found could not be reached.
public enum ResolveError: LocalizedError {
    case noAddress
    case timedOut

    public var errorDescription: String? {
        switch self {
        case .noAddress: "The party didn't give its address."
        case .timedOut: "The party didn't answer. Is this phone on the same Wi-Fi as the Mac?"
        }
    }
}
