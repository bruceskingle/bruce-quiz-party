// swift-tools-version: 6.0
// The iOS app's code other than its screens: the protocol's messages, the connection to the controller, finding it
// with Bonjour, and the answer styles. It is a package of its own so that `swift test` can test it on the Mac, without
// a simulator.

import PackageDescription

let package = Package(
    name: "QuizPartyKit",
    platforms: [.iOS(.v18), .macOS(.v15)],
    products: [
        .library(name: "QuizPartyKit", targets: ["QuizPartyKit"])
    ],
    targets: [
        .target(name: "QuizPartyKit"),
        .testTarget(name: "QuizPartyKitTests", dependencies: ["QuizPartyKit"]),
    ]
)
