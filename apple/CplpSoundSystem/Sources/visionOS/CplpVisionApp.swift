// CplpVisionApp.swift — visionOS エントリポイント（スタブ）
//
// macOS ビルドではコンパイルされない。
// visionOS SDK が利用可能になった段階で実装を進める。

#if os(visionOS)
import SwiftUI

@main
struct CplpVisionApp: App {
    @StateObject private var client = CplpClient()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(client)
        }

        // ImmersiveSpace placeholder
        // 3D 空間オーディオのビジュアライゼーション等を想定
        ImmersiveSpace(id: "audio-space") {
            // TODO: RealityKit ベースの 3D オーディオ可視化
            Text("CPLP Sound System — Immersive Audio Space")
        }
    }
}
#endif
