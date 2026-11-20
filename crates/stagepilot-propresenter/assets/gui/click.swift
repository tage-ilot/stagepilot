// ProPresenter Link: one left click at the screen position given as two numbers (x y), the way a mouse does.
// AppleScript's own `click at` did not land on ProPresenter's library list; a real mouse event does.
// Run as `xcrun swift click.swift X Y`. It clicks once and prints "clicked X Y". Nothing else.
import Foundation
import CoreGraphics

let args = CommandLine.arguments
guard args.count == 3, let x = Double(args[1]), let y = Double(args[2]), x >= 0, y >= 0, x < 20000, y < 20000 else {
    print("usage: click.swift X Y")
    exit(2)
}
let point = CGPoint(x: x, y: y)
CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
usleep(200_000)
for kind in [CGEventType.leftMouseDown, CGEventType.leftMouseUp] {
    CGEvent(mouseEventSource: nil, mouseType: kind, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(80_000)
}
print("clicked", x, y)
