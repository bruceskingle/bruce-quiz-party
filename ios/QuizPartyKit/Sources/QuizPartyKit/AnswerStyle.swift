// The colour and shape of each answer position, the same on the big screen and the phones, so that players who cannot
// tell the colours apart can tell the shapes. The master copy is protocol's ANSWER_STYLES; a Rust test,
// server/src/answer_shape.rs, checks that the colours and shape paths here are the same.

import SwiftUI

/// The colour and shape of an answer position.
public struct AnswerStyle: Sendable {
    /// The colour's name.
    public let name: String
    /// The colour, as CSS hex.
    public let colour: String
    /// The shape's name.
    public let shape: String
    /// The shape, as an SVG path in a 24 by 24 box, the same as the big screen's.
    public let path: String

    /// The style of the answer at a position, from 0.
    public static func at(_ position: Int) -> AnswerStyle {
        answerStyles[position % answerStyles.count]
    }

    /// The colour, for SwiftUI.
    public var color: Color {
        let value = UInt32(colour.dropFirst(), radix: 16) ?? 0
        return Color(
            red: Double((value >> 16) & 0xff) / 255,
            green: Double((value >> 8) & 0xff) / 255,
            blue: Double(value & 0xff) / 255)
    }
}

/// The style of each answer position, from the first to the sixth.
public let answerStyles: [AnswerStyle] = [
    AnswerStyle(name: "red", colour: "#d64545", shape: "triangle", path: "M12 3 L22 20 L2 20 Z"),
    AnswerStyle(name: "blue", colour: "#2f6fde", shape: "diamond", path: "M12 2 L22 12 L12 22 L2 12 Z"),
    AnswerStyle(name: "yellow", colour: "#e0b400", shape: "circle", path: "M12 3 A9 9 0 1 1 11.99 3 Z"),
    AnswerStyle(name: "green", colour: "#2e9d4f", shape: "square", path: "M4 4 H20 V20 H4 Z"),
    AnswerStyle(
        name: "purple", colour: "#8a55d6", shape: "star",
        path: "M12 2 L14.9 8.6 L22 9.2 L16.6 13.9 L18.2 21 L12 17.3 L5.8 21 L7.4 13.9 L2 9.2 L9.1 8.6 Z"),
    AnswerStyle(name: "orange", colour: "#e8772e", shape: "hexagon", path: "M7 3 H17 L22 12 L17 21 H7 L2 12 Z"),
]

/// An answer position's shape, scaled to fit the space it is given.
public struct AnswerShape: Shape {
    /// The answer's position, from 0.
    public let position: Int

    public init(position: Int) {
        self.position = position
    }

    public func path(in rect: CGRect) -> Path {
        let scale = min(rect.width, rect.height) / 24
        let transform = CGAffineTransform(translationX: rect.midX - 12 * scale, y: rect.midY - 12 * scale)
            .scaledBy(x: scale, y: scale)
        return SVGPath.parse(AnswerStyle.at(position).path).applying(transform)
    }
}

/// Reads the few SVG path commands the shapes use: absolute moves, lines, horizontal and vertical lines, circular
/// arcs, and closing.
enum SVGPath {
    static func parse(_ text: String) -> Path {
        var tokens = Tokens(text)
        var path = Path()
        var current = CGPoint.zero
        var start = CGPoint.zero
        while let command = tokens.command() {
            switch command {
            case "M":
                current = CGPoint(x: tokens.number(), y: tokens.number())
                start = current
                path.move(to: current)
            case "L":
                current = CGPoint(x: tokens.number(), y: tokens.number())
                path.addLine(to: current)
            case "H":
                current.x = tokens.number()
                path.addLine(to: current)
            case "V":
                current.y = tokens.number()
                path.addLine(to: current)
            case "A":
                let radius = tokens.number()
                _ = tokens.number()  // The y radius: only circles are drawn.
                _ = tokens.number()  // The rotation, which a circle does not need.
                let large = tokens.number() != 0
                let sweep = tokens.number() != 0
                let end = CGPoint(x: tokens.number(), y: tokens.number())
                addArc(to: &path, from: current, to: end, radius: radius, large: large, sweep: sweep)
                current = end
            case "Z":
                path.closeSubpath()
                current = start
            default:
                return path
            }
        }
        return path
    }

    /// Adds a circular arc between two points, converting SVG's endpoints and flags to a centre and angles, as in
    /// the SVG specification's implementation notes.
    private static func addArc(
        to path: inout Path, from: CGPoint, to: CGPoint, radius: CGFloat, large: Bool, sweep: Bool
    ) {
        let half = CGPoint(x: (to.x - from.x) / 2, y: (to.y - from.y) / 2)
        let halfChord = (half.x * half.x + half.y * half.y).squareRoot()
        guard halfChord > 0 else { return }
        let radius = max(radius, halfChord)
        let distance = (radius * radius - halfChord * halfChord).squareRoot()
        // The centre is on the chord's perpendicular bisector, on the side the flags choose.
        let side: CGFloat = large == sweep ? -1 : 1
        let middle = CGPoint(x: (from.x + to.x) / 2, y: (from.y + to.y) / 2)
        let centre = CGPoint(
            x: middle.x + side * distance * -half.y / halfChord,
            y: middle.y + side * distance * half.x / halfChord)
        let startAngle = atan2(from.y - centre.y, from.x - centre.x)
        let endAngle = atan2(to.y - centre.y, to.x - centre.x)
        // SVG's sweep flag means increasing angles, which in SwiftUI's flipped coordinates is not "clockwise".
        path.addArc(
            center: centre, radius: radius, startAngle: .radians(startAngle), endAngle: .radians(endAngle),
            clockwise: !sweep)
    }

    /// The commands and numbers of a path.
    private struct Tokens {
        private var scanner: Scanner

        init(_ text: String) {
            scanner = Scanner(string: text)
            scanner.charactersToBeSkipped = CharacterSet(charactersIn: " ,")
        }

        mutating func command() -> Character? {
            scanner.scanCharacter()
        }

        mutating func number() -> CGFloat {
            CGFloat(scanner.scanDouble() ?? 0)
        }
    }
}
