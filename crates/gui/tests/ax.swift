import AppKit
import ApplicationServices

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

let args = CommandLine.arguments
if args.count < 3 { fail("Usage: ax PID dump|wait|press|key [value]") }
guard AXIsProcessTrusted() else { fail("Accessibility permission is required for the test runner.") }
guard let pid = Int32(args[1]) else { fail("Invalid PID") }
let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 2)
// Opt in to the same accessibility tree that a screen reader requests.
_ = AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, true as CFBoolean)
_ = AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, true as CFBoolean)

func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
    return value
}

func text(_ element: AXUIElement) -> String {
    ["AXRole", "AXTitle", "AXDescription", "AXValue", "AXIdentifier"]
        .compactMap { attribute(element, $0) as? String }
        .joined(separator: " | ")
}

func descendants() -> [(AXUIElement, String)] {
    var result: [(AXUIElement, String)] = []
    var queue = ((attribute(app, "AXWindows") as? [AXUIElement]) ?? []).filter {
        attribute($0, "AXRole") as? String == "AXWindow"
    }
    if let value = attribute(app, "AXFocusedWindow"), CFGetTypeID(value) == AXUIElementGetTypeID() {
        let window = unsafeBitCast(value, to: AXUIElement.self)
        if attribute(window, "AXRole") as? String == "AXWindow" { queue.append(window) }
    }
    var visited = Set<AXUIElement>()
    while !queue.isEmpty && result.count < 5000 {
        let element = queue.removeLast()
        guard visited.insert(element).inserted else { continue }
        result.append((element, text(element)))
        if let children = attribute(element, "AXChildren") as? [AXUIElement] {
            queue.append(contentsOf: children.reversed())
        }
    }
    return result
}

let command = args[2]
let value = args.count > 3 ? args[3] : ""
switch command {
case "dump":
    print(descendants().map { $0.1 }.joined(separator: "\n"))
case "wait", "press":
    let deadline = Date().addingTimeInterval(15)
    var done = false
    while Date() < deadline {
        let nodes = descendants()
        if let found = nodes.first(where: { node in
            if command == "press" {
                let title = attribute(node.0, "AXTitle") as? String
                let description = attribute(node.0, "AXDescription") as? String
                let identifier = attribute(node.0, "AXIdentifier") as? String
                return title == value || description == value || identifier == value
            }
            return node.1.contains(value)
        }) {
            if command == "press" {
                let error = AXUIElementPerformAction(found.0, kAXPressAction as CFString)
                if error != .success { fail("AXPress failed for \(value): \(error.rawValue)") }
            }
            print(found.1)
            done = true
            break
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    if !done {
        print(descendants().map { $0.1 }.joined(separator: "\n"))
        fail("Timed out waiting for \(value)")
    }
case "key":
    let keys: [String: (CGKeyCode, CGEventFlags)] = [
        "refresh": (15, .maskCommand), "fresh": (15, [.maskCommand, .maskShift]),
        "stop": (47, .maskCommand), "settings": (43, .maskCommand),
        "escape": (53, []), "tab": (48, []), "down": (125, []),
        "open": (125, .maskCommand), "up": (126, .maskCommand), "quit": (12, .maskCommand),
        "find": (3, .maskCommand), "back": (33, .maskCommand), "forward": (30, .maskCommand),
        "stage": (17, .maskCommand), "review": (14, [.maskCommand, .maskShift])
    ]
    guard let (code, flags) = keys[value] else { fail("Unknown key") }
    guard CGPreflightPostEventAccess() else { fail("Permission to post native events is required.") }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [])
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
    let source = CGEventSource(stateID: .hidSystemState)
    guard let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
          let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false) else { fail("Could not create key event") }
    down.flags = flags
    up.flags = flags
    down.postToPid(pid)
    RunLoop.current.run(until: Date().addingTimeInterval(0.05))
    up.postToPid(pid)
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
    print("Sent \(value)")
case "type":
    guard CGPreflightPostEventAccess() else { fail("Permission to post native events is required.") }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [])
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
    let source = CGEventSource(stateID: .hidSystemState)
    for unit in value.utf16 {
        var character = unit
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false) else { fail("Could not create key event") }
        down.keyboardSetUnicodeString(stringLength: 1, unicodeString: &character)
        up.keyboardSetUnicodeString(stringLength: 1, unicodeString: &character)
        down.postToPid(pid)
        up.postToPid(pid)
        RunLoop.current.run(until: Date().addingTimeInterval(0.03))
    }
    print("Typed \(value)")
default:
    fail("Unknown command")
}
