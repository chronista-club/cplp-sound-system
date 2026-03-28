import SwiftUI

// MARK: - サイドバー項目

enum SidebarItem: String, CaseIterable, Identifiable {
    case session = "Session"
    case mixer = "Mixer"
    case audio = "Audio"
    case scene = "Scene"
    case midi = "MIDI"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .session: return "network"
        case .mixer: return "slider.horizontal.3"
        case .audio: return "speaker.wave.2"
        case .scene: return "cube"
        case .midi: return "pianokeys"
        }
    }
}

// MARK: - ContentView

struct ContentView: View {
    @EnvironmentObject var client: CplpClient
    @Environment(\.openWindow) private var openWindow
    @Environment(KeystageManager.self) private var keystageManager: KeystageManager?
    @State private var selectedItem: SidebarItem? = .session

    var body: some View {
        NavigationSplitView {
            // サイドバー
            List(SidebarItem.allCases, selection: $selectedItem) { item in
                Label(item.rawValue, systemImage: item.icon)
                    .tag(item)
            }
            .listStyle(.sidebar)
            .navigationSplitViewColumnWidth(min: 160, ideal: 200, max: 260)
            .safeAreaInset(edge: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("CPLP Sound System")
                        .font(.headline)
                    HStack(spacing: 6) {
                        Circle()
                            .fill(client.isInitialized ? Color.green : Color.gray)
                            .frame(width: 8, height: 8)
                        Text("v\(client.version)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
            }
        } detail: {
            // メインコンテンツ
            switch selectedItem {
            case .session:
                SessionView()
            case .mixer:
                MixerView()
            case .audio:
                AudioControlView()
            case .scene:
                #if os(macOS)
                SceneMetalView()
                #else
                Text("Scene view is only available on macOS.")
                #endif
            case .midi:
                MidiOverviewView()
                    .environment(keystageManager)
            case nil:
                Text("Select an item from the sidebar.")
                    .foregroundStyle(.secondary)
            }
        }
        .frame(minWidth: 700, minHeight: 500)
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
