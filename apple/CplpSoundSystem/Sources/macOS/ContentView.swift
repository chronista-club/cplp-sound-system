import SwiftUI

// MARK: - ContentView

struct ContentView: View {
    @EnvironmentObject var client: CplpClient
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        ZStack {
            // Scene Canvas — メインビュー (AudioGraph の可視化)
            #if os(macOS)
            SceneMetalView()
            #endif

            // ステータスバー (左上)
            VStack {
                HStack(spacing: 8) {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(client.isInitialized ? Color.green : Color.gray)
                            .frame(width: 8, height: 8)
                        Text("CPLP v\(client.version)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 6))

                    Spacer()

                    // ツールバー (右上)
                    HStack(spacing: 8) {
                        ToolbarButton(
                            icon: client.isAudioRunning ? "stop.fill" : "play.fill",
                            label: client.isAudioRunning ? "Stop" : "Play"
                        ) {
                            if client.isAudioRunning {
                                client.stopAudio()
                            } else {
                                client.startAudio()
                            }
                        }

                        ToolbarButton(icon: "pianokeys", label: "MIDI") {
                            openWindow(id: "midi-console")
                        }
                    }
                    .padding(.horizontal, 6)
                    .padding(.vertical, 4)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 6))
                }
                .padding(12)

                Spacer()
            }
        }
        .frame(minWidth: 700, minHeight: 500)
    }
}

// MARK: - ToolbarButton

struct ToolbarButton: View {
    let icon: String
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label(label, systemImage: icon)
                .font(.caption)
        }
        .buttonStyle(.borderless)
    }
}

// MARK: - StatusIndicator（他ビューからも使用）

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
