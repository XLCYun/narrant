import AppKit
import Foundation
import CoreGraphics

private let expectedBundleIdentifier = "com.cyunlab.narrant"

/// 输出固定诊断并失败，避免把任何用户控制的应用信息写进 evidence log。
private func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("\(message)\n".utf8))
    exit(1)
}

/// 只观察或正常终止 exact PID 且 bundle identity 匹配的正式应用。
@main
private enum TerminateNarrantApplication {
    /// 校验 PID 与应用身份，再观察窗口就绪状态或发出正常终止请求。
    static func main() {
        guard (2...3).contains(CommandLine.arguments.count),
              let rawPID = Int32(CommandLine.arguments[1]), rawPID > 0 else {
            fail("Narrant terminate helper 参数无效")
        }
        guard let application = NSRunningApplication(processIdentifier: pid_t(rawPID)),
              !application.isTerminated,
              application.bundleIdentifier == expectedBundleIdentifier else {
            fail("PID 不是 exact Narrant application")
        }
        if CommandLine.arguments.count == 3 {
            guard CommandLine.arguments[2] == "ready" else { fail("未知窗口操作") }
            let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
            if application.isFinishedLaunching && windows.contains(where: {
                ($0[kCGWindowOwnerPID as String] as? Int) == Int(rawPID)
                    && ($0[kCGWindowLayer as String] as? Int) == 0
            }) {
                print("ready")
            }
            return
        }
        guard application.terminate() else {
            fail("Narrant application 未接受 terminate 请求")
        }
    }
}
