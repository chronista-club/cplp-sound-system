import SwiftUI

/// オーディオエンジンのコントロール + レベルメーター + プラグインスキャン
struct AudioControlView: View {
    @EnvironmentObject var client: CplpClient
    @State private var isScanning: Bool = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // MARK: - オーディオエンジン ON/OFF
                GroupBox("Audio Engine") {
                    VStack(spacing: 16) {
                        HStack {
                            StatusIndicator(
                                label: client.isAudioRunning ? "Running" : "Stopped",
                                isActive: client.isAudioRunning
                            )
                            Spacer()
                            Button(client.isAudioRunning ? "Stop" : "Start") {
                                if client.isAudioRunning {
                                    client.stopAudio()
                                } else {
                                    client.startAudio()
                                }
                            }
                            .controlSize(.large)
                            .buttonStyle(.borderedProminent)
                            .tint(client.isAudioRunning ? .red : .green)
                        }

                        // レベルメーター
                        if client.isAudioRunning {
                            VStack(spacing: 8) {
                                MeterBar(label: "L", level: client.meterLeft)
                                MeterBar(label: "R", level: client.meterRight)
                            }
                            .transition(.opacity)
                        }
                    }
                    .padding(.vertical, 4)
                }

                // MARK: - プラグインスキャン
                GroupBox("Plugins") {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("\(client.plugins.count) plugin(s) found")
                                .foregroundStyle(.secondary)
                            Spacer()
                            Button {
                                isScanning = true
                                client.scanPlugins()
                                isScanning = false
                            } label: {
                                HStack(spacing: 4) {
                                    if isScanning {
                                        ProgressView()
                                            .controlSize(.small)
                                    }
                                    Text("Scan")
                                }
                            }
                            .disabled(isScanning)
                        }

                        if !client.plugins.isEmpty {
                            Divider()
                            ForEach(client.plugins) { plugin in
                                HStack {
                                    Image(systemName: "puzzlepiece.extension")
                                        .foregroundStyle(.secondary)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(plugin.name)
                                            .font(.body)
                                        Text(plugin.id)
                                            .font(.caption2)
                                            .foregroundStyle(.tertiary)
                                    }
                                    Spacer()
                                }
                                .padding(.vertical, 2)
                            }
                        }
                    }
                    .padding(.vertical, 4)
                }

                Spacer()
            }
            .padding(20)
        }
        .navigationTitle("Audio")
    }
}

/// 水平レベルメーター
struct MeterBar: View {
    let label: String
    let level: Float

    var body: some View {
        HStack(spacing: 8) {
            Text(label)
                .font(.caption.monospaced())
                .frame(width: 16)

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 4)
                        .fill(Color.gray.opacity(0.2))

                    RoundedRectangle(cornerRadius: 4)
                        .fill(meterColor)
                        .frame(width: max(0, geo.size.width * CGFloat(level)))
                }
            }
            .frame(height: 12)

            Text(String(format: "%.1f", 20 * log10(max(level, 0.0001))))
                .font(.caption.monospaced())
                .frame(width: 50, alignment: .trailing)
        }
    }

    private var meterColor: Color {
        if level > 0.9 {
            return .red
        } else if level > 0.6 {
            return .yellow
        } else {
            return .green
        }
    }
}
