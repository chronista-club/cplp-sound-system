import SwiftUI

struct ContentView: View {
    @EnvironmentObject var bridge: CplpBridge

    var body: some View {
        VStack(spacing: 0) {
            // ヘッダー
            HStack {
                VStack(alignment: .leading) {
                    Text("CPLP Sound System")
                        .font(.title2)
                        .fontWeight(.bold)
                    Text("v\(bridge.version)")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
                Spacer()
                StatusIndicator(label: "Runtime", isActive: bridge.isInitialized)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)

            Divider()

            // 3D シーン
            SceneMetalView()
                .frame(minHeight: 300)

            Divider()

            // オーディオコントロール
            AudioControlView()
                .padding(12)
        }
        .frame(minWidth: 700, minHeight: 500)
    }
}

struct StatusIndicator: View {
    let label: String
    let isActive: Bool

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(isActive ? Color.green : Color.gray)
                .frame(width: 8, height: 8)
            Text(label)
                .font(.caption)
        }
    }
}
