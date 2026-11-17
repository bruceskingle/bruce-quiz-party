// Where a party is and its game code, read from the join QR code or a quizparty:// link.

import Foundation

/// A party controller's web server on the local network.
public struct PartyAddress: Hashable, Sendable, Codable {
    /// Its host name or IP address.
    public var host: String
    public var port: UInt16

    public init(host: String, port: UInt16) {
        self.host = host
        self.port = port
    }

    /// The URL of its WebSocket.
    public var webSocketURL: URL? {
        url(scheme: "ws", path: webSocketPath)
    }

    /// The URL of a file on its web server, e.g. a question's image.
    public func fileURL(_ path: String) -> URL? {
        url(scheme: "http", path: path)
    }

    private func url(scheme: String, path: String) -> URL? {
        var components = URLComponents()
        components.scheme = scheme
        // An IPv6 address goes in brackets.
        components.host = host.contains(":") ? "[\(host)]" : host
        components.port = Int(port)
        components.path = path
        return components.url
    }

    /// Reads an address typed by a player, e.g. `192.168.1.20:8765`, or the whole URL under the QR code. Without a
    /// port, the controller's usual one is assumed.
    public init?(typed text: String) {
        var text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !text.contains("://") {
            text = "http://" + text
        }
        guard let components = URLComponents(string: text), let host = components.host, !host.isEmpty else {
            return nil
        }
        self.host = host
        self.port = UInt16(components.port ?? Int(defaultPort))
    }
}

/// The port the controller uses when it is free.
public let defaultPort: UInt16 = 8765

/// What the join QR code or a link says: where the party is, and its game code if it says.
public struct JoinLink: Equatable, Sendable {
    public var address: PartyAddress
    public var code: String?

    public init(address: PartyAddress, code: String?) {
        self.address = address
        self.code = code
    }

    /// Reads the join QR code, the browser client's URL `http://<mac>:<port>/?code=ABCD`, or the app's own link,
    /// `quizparty://join?host=<mac>&port=<port>&code=ABCD`. Anything else is not a party's.
    public init?(url: URL) {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false) else { return nil }
        let query = Dictionary(
            (components.queryItems ?? []).map { ($0.name, $0.value ?? "") }, uniquingKeysWith: { first, _ in first })
        let code = query["code"].map { $0.trimmingCharacters(in: .whitespaces).uppercased() }.flatMap {
            $0.isEmpty ? nil : $0
        }
        switch components.scheme?.lowercased() {
        case "http":
            guard let host = components.host, !host.isEmpty, code != nil else { return nil }
            address = PartyAddress(host: host, port: UInt16(components.port ?? 80))
        case urlScheme:
            guard components.host == "join", let host = query["host"], !host.isEmpty,
                let port = query["port"].flatMap({ UInt16($0) })
            else { return nil }
            address = PartyAddress(host: host, port: port)
        default:
            return nil
        }
        self.code = code
    }

    /// Reads the text of a scanned QR code.
    public init?(scanned text: String) {
        guard let url = URL(string: text.trimmingCharacters(in: .whitespacesAndNewlines)) else { return nil }
        self.init(url: url)
    }
}

/// What the host pairing QR code says, shown on the Mac only while the party is set up:
/// `quizparty://host?host=<mac>&port=<port>&pair=<code>`.
public struct HostLink: Equatable, Sendable {
    public var address: PartyAddress
    /// The one-time pairing code.
    public var pairingCode: String

    public init(address: PartyAddress, pairingCode: String) {
        self.address = address
        self.pairingCode = pairingCode
    }

    public init?(url: URL) {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
            components.scheme?.lowercased() == urlScheme, components.host == "host"
        else { return nil }
        let query = Dictionary(
            (components.queryItems ?? []).map { ($0.name, $0.value ?? "") }, uniquingKeysWith: { first, _ in first })
        guard let host = query["host"], !host.isEmpty, let port = query["port"].flatMap({ UInt16($0) }),
            let code = query["pair"], !code.isEmpty
        else { return nil }
        address = PartyAddress(host: host, port: port)
        pairingCode = code
    }

    /// Reads the text of a scanned QR code.
    public init?(scanned text: String) {
        guard let url = URL(string: text.trimmingCharacters(in: .whitespacesAndNewlines)) else { return nil }
        self.init(url: url)
    }
}
