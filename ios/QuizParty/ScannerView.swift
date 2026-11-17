// Scans the join QR code on the big screen with the camera, using AVFoundation's QR code reader.

@preconcurrency import AVFoundation
import SwiftUI

/// A sheet showing the camera until it reads a QR code.
struct ScannerSheet: View {
    /// Called with the first code's text.
    let scanned: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var denied = false

    var body: some View {
        NavigationStack {
            ZStack {
                Color.black.ignoresSafeArea()
                if denied {
                    ContentUnavailableView(
                        "No Camera", systemImage: "camera.fill",
                        description: Text(
                            "Allow Quiz Party to use the camera in Settings to scan the code, or type the address instead."
                        ))
                } else {
                    QRScanner(scanned: scanned).ignoresSafeArea()
                    RoundedRectangle(cornerRadius: 24)
                        .stroke(Theme.accent, lineWidth: 4)
                        .frame(width: 260, height: 260)
                }
            }
            .navigationTitle("Scan the QR Code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .task {
            switch AVCaptureDevice.authorizationStatus(for: .video) {
            case .notDetermined: denied = !(await AVCaptureDevice.requestAccess(for: .video))
            case .authorized: denied = false
            default: denied = true
            }
        }
    }
}

/// The camera's picture, reading QR codes.
private struct QRScanner: UIViewControllerRepresentable {
    let scanned: (String) -> Void

    func makeUIViewController(context: Context) -> ScannerController {
        let controller = ScannerController()
        controller.scanned = scanned
        return controller
    }

    func updateUIViewController(_ controller: ScannerController, context: Context) {}
}

/// Runs the capture session and reports the first QR code read.
private final class ScannerController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    var scanned: ((String) -> Void)?
    private let session = AVCaptureSession()
    private var preview: AVCaptureVideoPreviewLayer?
    private var done = false

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        guard let camera = AVCaptureDevice.default(for: .video),
            let input = try? AVCaptureDeviceInput(device: camera),
            session.canAddInput(input)
        else { return }
        session.addInput(input)
        let output = AVCaptureMetadataOutput()
        guard session.canAddOutput(output) else { return }
        session.addOutput(output)
        output.setMetadataObjectsDelegate(self, queue: .main)
        output.metadataObjectTypes = [.qr]
        let preview = AVCaptureVideoPreviewLayer(session: session)
        preview.videoGravity = .resizeAspectFill
        view.layer.addSublayer(preview)
        self.preview = preview
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        preview?.frame = view.bounds
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        let session = session
        // Starting the session blocks, so it is done off the main thread.
        DispatchQueue.global(qos: .userInitiated).async { session.startRunning() }
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        let session = session
        DispatchQueue.global(qos: .userInitiated).async { session.stopRunning() }
    }

    nonisolated func metadataOutput(
        _ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection
    ) {
        let text = objects.compactMap { ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue }.first
        MainActor.assumeIsolated {
            guard let text, !done else { return }
            done = true
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            scanned?(text)
        }
    }
}
