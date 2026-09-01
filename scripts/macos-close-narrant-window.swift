import AppKit
import ApplicationServices
import Foundation

private let expectedBundleIdentifier = "com.cyunlab.narrant"

/// 输出固定诊断并失败，避免把窗口标题或用户路径写进 evidence log。
private func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("\(message)\n".utf8))
    exit(1)
}

/// 读取一个必需 Accessibility 属性并保持强类型。
private func attribute<T>(_ element: AXUIElement, _ name: CFString, as type: T.Type) -> T {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name, &value) == .success,
          let typed = value as? T else {
        fail("无法读取 Narrant 窗口 Accessibility 属性")
    }
    return typed
}

/// 只对 exact PID、bundle identity 和唯一主窗口执行一次原生 close-button AXPress。
@main
private enum CloseNarrantWindow {
    static func main() {
        if CommandLine.arguments.count == 2, CommandLine.arguments[1] == "--probe-trust" {
            exit(AXIsProcessTrusted() ? 0 : 77)
        }
        guard CommandLine.arguments.count == 2,
              let rawPID = Int32(CommandLine.arguments[1]), rawPID > 0,
              AXIsProcessTrusted() else {
            fail("macOS runner 未预授权 Narrant close helper")
        }
        guard let application = NSRunningApplication(processIdentifier: pid_t(rawPID)),
              !application.isTerminated,
              application.bundleIdentifier == expectedBundleIdentifier else {
            fail("PID 不是 exact Narrant application")
        }
        let app = AXUIElementCreateApplication(pid_t(rawPID))
        let windows: [AXUIElement] = attribute(app, kAXWindowsAttribute as CFString, as: [AXUIElement].self)
        let main: AXUIElement = attribute(app, kAXMainWindowAttribute as CFString, as: AXUIElement.self)
        guard windows.count == 1, CFEqual(windows[0], main) else {
            fail("Narrant 必须只有一个主窗口")
        }
        let close: AXUIElement = attribute(main, kAXCloseButtonAttribute as CFString, as: AXUIElement.self)
        let enabled: Bool = attribute(close, kAXEnabledAttribute as CFString, as: Bool.self)
        guard enabled, AXUIElementPerformAction(close, kAXPressAction as CFString) == .success else {
            fail("Narrant 原生关闭按钮未接受 AXPress")
        }
    }
}
