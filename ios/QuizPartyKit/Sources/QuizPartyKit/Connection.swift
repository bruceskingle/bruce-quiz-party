// A WebSocket to the controller which reconnects by itself when it drops, as phones' connections do when they sleep or
// change networks, for both the player's and the host's sessions.

import Foundation

/// A connection to the controller's WebSocket, opened again after it drops until it is stopped.
@MainActor
final class Connection {
    /// How long to wait before reconnecting after the connection drops, growing to the last.
    static let reconnectDelays: [Duration] = [.milliseconds(500), .seconds(1), .seconds(2), .seconds(4)]

    private let url: URL
    /// Called each time the socket is opened, to send the first message, e.g. joining.
    var opened: () -> Void = {}
    /// Called with each message the controller sends.
    var received: (ServerMessage) -> Void = { _ in }
    /// Called when the connection drops and is about to be opened again.
    var dropped: () -> Void = {}

    private let session = URLSession(configuration: .default)
    private var task: URLSessionWebSocketTask?
    private var running = false
    private var reconnects = 0

    init(url: URL) {
        self.url = url
    }

    /// Opens the connection, and keeps it open.
    func start() {
        running = true
        connect()
    }

    /// Closes the connection, and stops reopening it.
    func stop() {
        running = false
        task?.cancel(with: .goingAway, reason: nil)
        task = nil
    }

    /// Says the connection works, after the controller has accepted it, so that the next drop is retried quickly.
    func succeeded() {
        reconnects = 0
    }

    /// Sends a message, if the connection is open; a failure shows as the connection dropping.
    func send(_ message: ClientMessage) {
        guard let task, let data = try? message.encoded() else { return }
        task.send(.string(String(decoding: data, as: UTF8.self))) { _ in }
    }

    private func connect() {
        let task = session.webSocketTask(with: url)
        self.task = task
        task.resume()
        opened()
        Task { await receive(from: task) }
    }

    /// Handles the socket's messages until it closes, then reconnects if still running.
    private func receive(from task: URLSessionWebSocketTask) async {
        while true {
            let message: URLSessionWebSocketTask.Message
            do {
                message = try await task.receive()
            } catch {
                drop(task)
                return
            }
            guard self.task === task else { return }
            let data: Data
            switch message {
            case .string(let text): data = Data(text.utf8)
            case .data(let bytes): data = bytes
            @unknown default: continue
            }
            // A message this app cannot read, e.g. from a newer controller, is ignored.
            if let message = try? ServerMessage.decode(data) {
                received(message)
            }
        }
    }

    private func drop(_ task: URLSessionWebSocketTask) {
        guard self.task === task, running else { return }
        let delay = Self.reconnectDelays[min(reconnects, Self.reconnectDelays.count - 1)]
        reconnects += 1
        dropped()
        Task {
            try? await Task.sleep(for: delay)
            if self.task === task, running {
                connect()
            }
        }
    }
}
