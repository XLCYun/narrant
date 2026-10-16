import AppKit
import Foundation

private let expectedBundleIdentifier = "com.cyunlab.narrant.upgrade-test"

/// 输出固定诊断并失败，避免把任何用户控制的应用信息写进 evidence log。
private func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("\(message)\n".utf8))
    exit(1)
}

/// 只向 exact PID 且 bundle identity 匹配的 Narrant 应用请求正常终止。
@main
private enum TerminateNarrantApplication {
    /// 校验唯一 PID 参数与应用身份，再通过 AppKit 发出一次正常终止请求。
    static func main() {
        guard CommandLine.arguments.count == 2,
              let rawPID = Int32(CommandLine.arguments[1]), rawPID > 0 else {
            fail("Narrant terminate helper 参数无效")
        }
        guard let application = NSRunningApplication(processIdentifier: pid_t(rawPID)),
              !application.isTerminated,
              application.bundleIdentifier == expectedBundleIdentifier else {
            fail("PID 不是 exact Narrant application")
        }
        guard application.terminate() else {
            fail("Narrant application 未接受 terminate 请求")
        }
    }
}
