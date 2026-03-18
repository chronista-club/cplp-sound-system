import SwiftUI

@main
struct CplpSoundSystemApp: App {
    /// cplp_init() は @StateObject 評価より先に実行する必要がある。
    /// static let は App 構造体のプロパティ初期化より先に評価される。
    private static let runtimeReady: Bool = {
        let client = CplpClient()
        do {
            try client.initialize()
        } catch {
            fatalError("cplp_init failed: \(error.localizedDescription)")
        }
        _sharedClient = client
        return true
    }()

    private static var _sharedClient: CplpClient!

    @StateObject private var client: CplpClient = {
        _ = CplpSoundSystemApp.runtimeReady
        return CplpSoundSystemApp._sharedClient
    }()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(client)
        }
        .defaultSize(width: 900, height: 650)
    }
}
