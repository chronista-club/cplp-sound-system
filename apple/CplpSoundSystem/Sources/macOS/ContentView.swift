import CplpBridge
import SwiftUI

// MARK: - ContentView

struct ContentView: View {
    @EnvironmentObject var client: CplpClient
    @Environment(\.openWindow) private var openWindow
    @State private var showPluginPicker = false

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

                        ToolbarButton(icon: "plus.rectangle", label: "Plugin") {
                            if client.plugins.isEmpty {
                                client.scanPlugins()
                            }
                            showPluginPicker.toggle()
                        }
                        .popover(isPresented: $showPluginPicker) {
                            PluginPickerView()
                                .environmentObject(client)
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

// MARK: - PluginPickerView

struct PluginPickerView: View {
    @EnvironmentObject var client: CplpClient

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("CLAP Plugins")
                    .font(.headline)
                Spacer()
                Button {
                    client.scanPlugins()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.borderless)
            }

            Divider()

            if client.plugins.isEmpty {
                Text("No plugins found.\nScan to detect CLAP plugins.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, 16)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(client.plugins) { plugin in
                            Button {
                                plugin.id.withCString { idPtr in
                                    plugin.name.withCString { namePtr in
                                        let nodeId = cplp_graph_add_plugin(idPtr, namePtr, true)
                                        print("[Plugin] Added \(plugin.name) as node \(nodeId)")
                                    }
                                }
                            } label: {
                                HStack {
                                    Image(systemName: "waveform")
                                        .foregroundStyle(.secondary)
                                    Text(plugin.name)
                                        .font(.body)
                                    Spacer()
                                }
                                .padding(.vertical, 4)
                                .padding(.horizontal, 8)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                }
                .frame(maxHeight: 300)
            }
        }
        .padding(12)
        .frame(width: 280)
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
