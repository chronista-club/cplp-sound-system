import SwiftUI

@main
struct CplpMacApp: App {
    // cplp_init() は @StateObject 評価より先に実行する必要がある。
    // static let は App 構造体のプロパティ初期化より先に評価される。
    private static let runtimeReady: Bool = {
        let result = cplp_init()
        if result != CPLP_RESULT_OK {
            let msg = cplp_last_error().map { String(cString: $0) } ?? "unknown"
            fatalError("cplp_init failed: \(msg)")
        }
        return true
    }()

    @StateObject private var bridge = CplpBridge()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(bridge)
        }
    }

    init() {
        // static let で既に初期化済みだが、参照して評価を保証
        _ = Self.runtimeReady
    }
}
