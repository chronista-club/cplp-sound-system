import SwiftUI

// MARK: - AudioControlView

/// オーディオエンジンの Start/Stop + メーター表示 + プラグインスキャン
struct AudioControlView: View {
    @Environment(CplpClient.self) private var client

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // エンジンコントロール
                GroupBox("Audio Engine") {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Circle()
                                .fill(client.audioRunning ? .green : .red)
                                .frame(width: 10, height: 10)
                            Text(client.audioRunning ? "Running" : "Stopped")
                                .font(.headline)
                            Spacer()

                            if client.audioRunning {
                                Button("Stop") {
                                    client.stopAudio()
                                }
                                .buttonStyle(.bordered)
                            } else {
                                Button("Start") {
                                    client.startAudio()
                                }
                                .buttonStyle(.borderedProminent)
                            }
                        }
                    }
                    .padding(.vertical, 4)
                }

                // メーター表示
                GroupBox("Meters") {
                    VStack(alignment: .leading, spacing: 8) {
                        MeterBar(label: "L", value: client.meterLeft)
                        MeterBar(label: "R", value: client.meterRight)
                    }
                    .padding(.vertical, 4)
                }

                // プラグインスキャン
                GroupBox("Plugins") {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Text("CLAP Plugins")
                                .font(.headline)
                            Spacer()
                            Button("Scan") {
                                client.scanPlugins()
                            }
                            .buttonStyle(.bordered)
                        }

                        if client.plugins.isEmpty {
                            Text("No plugins found. Click Scan to search.")
                                .foregroundStyle(.secondary)
                                .padding(.vertical, 8)
                        } else {
                            ForEach(client.plugins) { plugin in
                                HStack {
                                    Image(systemName: "puzzlepiece.extension")
                                        .foregroundStyle(.purple)
                                    VStack(alignment: .leading) {
                                        Text(plugin.name)
                                            .font(.body)
                                        Text(plugin.pluginId)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                }
                            }
                        }
                    }
                    .padding(.vertical, 4)
                }
            }
            .padding()
        }
        .navigationTitle("Audio")
    }
}

// MARK: - MeterBar

/// 水平メーターバー
struct MeterBar: View {
    let label: String
    let value: Float

    var body: some View {
        HStack(spacing: 8) {
            Text(label)
                .font(.caption.bold())
                .frame(width: 16, alignment: .trailing)

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    // 背景
                    RoundedRectangle(cornerRadius: 3)
                        .fill(.quaternary)

                    // メーター値
                    RoundedRectangle(cornerRadius: 3)
                        .fill(meterColor)
                        .frame(width: max(0, geo.size.width * CGFloat(value)))
                }
            }
            .frame(height: 12)

            Text(String(format: "%.1f", value))
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 32, alignment: .trailing)
        }
    }

    private var meterColor: Color {
        if value > 0.9 {
            return .red
        } else if value > 0.7 {
            return .yellow
        } else {
            return .green
        }
    }
}
