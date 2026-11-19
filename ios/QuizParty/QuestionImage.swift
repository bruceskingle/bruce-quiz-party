// A question's image, drawn by WebKit rather than UIImage, as quiz images from Wikimedia Commons are often SVG, which
// UIImage cannot draw. Its bytes come from the app's image cache, which may have fetched it already, during the
// previous question's reveal.

import QuizPartyKit
import SwiftUI
import WebKit

/// The image at a URL on the controller, scaled to fit, on a transparent background.
struct QuestionImage: UIViewRepresentable {
    let url: URL
    /// A description, for VoiceOver.
    let alt: String

    func makeUIView(context: Context) -> WKWebView {
        let view = WKWebView(frame: .zero, configuration: WKWebViewConfiguration())
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.backgroundColor = .clear
        view.scrollView.isScrollEnabled = false
        view.isUserInteractionEnabled = false
        view.isAccessibilityElement = true
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {
        view.accessibilityLabel = alt
        guard context.coordinator.shown != url else { return }
        context.coordinator.shown = url
        context.coordinator.loading?.cancel()
        let url = url
        context.coordinator.loading = Task { @MainActor in
            // From the cache, as data, if it can be had; otherwise WebKit fetches it itself.
            let source: String
            if let image = try? await ImageCache.shared.image(at: url) {
                source = "data:\(image.mimeType);base64,\(image.data.base64EncodedString())"
            } else {
                source = url.absoluteString.replacingOccurrences(of: "\"", with: "%22")
            }
            guard !Task.isCancelled else { return }
            let page = """
                <!doctype html><meta name="viewport" content="width=device-width, initial-scale=1">
                <style>html,body{margin:0;height:100%;background:transparent;display:flex;align-items:center;\
                justify-content:center}img{max-width:100%;max-height:100%;object-fit:contain}</style>
                <img src="\(source)" alt="">
                """
            view.loadHTMLString(page, baseURL: url)
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    /// Remembers the image shown, so that redrawing the question does not reload it, and its loading, to cancel it if
    /// another is shown first.
    final class Coordinator {
        var shown: URL?
        var loading: Task<Void, Never>?
    }
}
