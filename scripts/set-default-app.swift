// 把指定应用设为若干 UTI 的默认打开程序（macOS 12+）
// 用法：swift scripts/set-default-app.swift /path/to/App.app <UTI>...
import AppKit
import UniformTypeIdentifiers

let args = CommandLine.arguments.dropFirst()
guard let appPath = args.first, args.count > 1 else {
    FileHandle.standardError.write("用法：set-default-app.swift <App.app> <UTI>...\n".data(using: .utf8)!)
    exit(2)
}
let appURL = URL(fileURLWithPath: appPath)
var failed = false

for uti in args.dropFirst() {
    guard let type = UTType(uti) else {
        print("跳过未知类型 \(uti)")
        continue
    }
    let done = DispatchSemaphore(value: 0)
    NSWorkspace.shared.setDefaultApplication(at: appURL, toOpen: type) { error in
        if let error {
            print("✗ \(uti)：\(error.localizedDescription)")
            failed = true
        } else {
            print("✓ \(uti) → \(appURL.lastPathComponent)")
        }
        done.signal()
    }
    done.wait()
}
exit(failed ? 1 : 0)
