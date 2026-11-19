// Question images fetched from the controller, kept for when they are shown: the next question's is fetched during the
// reveal, so that it is there as the question is asked, rather than every phone fetching it then, with the clock
// running.

import Foundation

/// The question images fetched, the last few, by their URL.
public actor ImageCache {
    /// The one cache the app uses.
    public static let shared = ImageCache()

    /// An image's bytes and type, e.g. `image/svg+xml`.
    public struct Image: Sendable, Equatable {
        public var data: Data
        public var mimeType: String
    }

    /// How many images are kept: the one shown, the next, and a few more.
    private let limit = 4
    private var images: [URL: Task<Image, Error>] = [:]
    private var order: [URL] = []

    public init() {}

    /// The image at a URL: the one fetched before, or being fetched, or fetched now.
    public func image(at url: URL) async throws -> Image {
        if let fetching = images[url] {
            return try await fetching.value
        }
        let fetching = Task {
            let (data, response) = try await URLSession.shared.data(from: url)
            guard let response = response as? HTTPURLResponse, response.statusCode == 200 else {
                throw URLError(.badServerResponse)
            }
            return Image(data: data, mimeType: response.mimeType ?? "application/octet-stream")
        }
        images[url] = fetching
        order.append(url)
        if order.count > limit {
            images[order.removeFirst()] = nil
        }
        do {
            return try await fetching.value
        } catch {
            // A failed fetch is not kept, so that the next asking tries again.
            images[url] = nil
            order.removeAll { $0 == url }
            throw error
        }
    }

    /// Starts fetching an image, to have it when it is shown.
    public nonisolated func prefetch(_ url: URL) {
        Task { _ = try? await image(at: url) }
    }
}
