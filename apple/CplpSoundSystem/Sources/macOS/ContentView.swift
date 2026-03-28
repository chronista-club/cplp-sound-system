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

                // モジュール一覧 (左下)
                if !client.plugins.isEmpty {
                    HStack {
                        ModuleListHUD()
                            .environmentObject(client)
                        Spacer()
                    }
                    .padding(12)
                }
            }
        }
        .frame(minWidth: 700, minHeight: 500)
    }
}

// MARK: - ModuleListHUD

struct ModuleListHUD: View {
    @EnvironmentObject var client: CplpClient
    @State private var addedModules: [(nodeId: UInt32, name: String, pluginId: String)] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Modules")
                .font(.caption.bold())
                .foregroundStyle(.secondary)

            ForEach(Array(addedModules.enumerated()), id: \.offset) { _, module in
                HStack(spacing: 6) {
                    Image(systemName: "waveform")
                        .font(.caption2)
                        .foregroundStyle(.green)
                    Text(module.name)
                        .font(.caption)
                    Spacer()
                    Button {
                        // TODO: CLAP GUI を別ウィンドウで開く
                        print("[GUI] Open GUI for \(module.name) (\(module.pluginId))")
                    } label: {
                        Image(systemName: "rectangle.on.rectangle")
                            .font(.caption2)
                    }
                    .buttonStyle(.borderless)
                    .help("Open Plugin GUI")
                }
            }
        }
        .padding(8)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 8))
        .frame(maxWidth: 200)
        .onReceive(NotificationCenter.default.publisher(for: .moduleAdded)) { notification in
            if let info = notification.userInfo,
               let nodeId = info["nodeId"] as? UInt32,
               let name = info["name"] as? String,
               let pluginId = info["pluginId"] as? String {
                addedModules.append((nodeId: nodeId, name: name, pluginId: pluginId))
            }
        }
    }
}

extension Notification.Name {
    static let moduleAdded = Notification.Name("moduleAdded")
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
                                        cplp_scene_rebuild()
                                        NotificationCenter.default.post(
                                            name: .moduleAdded,
                                            object: nil,
                                            userInfo: [
                                                "nodeId": nodeId,
                                                "name": plugin.name,
                                                "pluginId": plugin.id,
                                            ]
                                        )
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
