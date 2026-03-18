import SwiftUI

// MARK: - Sidebar Navigation

/// サイドバーのセクション
enum SidebarSection: String, CaseIterable, Identifiable {
    case session = "Session"
    case mixer = "Mixer"
    case audio = "Audio"
    case scene = "Scene"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .session: "network"
        case .mixer: "slider.horizontal.3"
        case .audio: "waveform"
        case .scene: "cube"
        }
    }
}

// MARK: - ContentView

struct ContentView: View {
    @Environment(CplpClient.self) private var client
    @State private var selectedSection: SidebarSection? = .session

    var body: some View {
        NavigationSplitView {
            List(SidebarSection.allCases, selection: $selectedSection) { section in
                Label(section.rawValue, systemImage: section.icon)
            }
            .navigationTitle("CPLP")
            .listStyle(.sidebar)
        } detail: {
            Group {
                switch selectedSection {
                case .session:
                    SessionView()
                case .mixer:
                    MixerView()
                case .audio:
                    AudioControlView()
                case .scene:
                    SceneMetalView()
                case nil:
                    Text("Select a section")
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .toolbar {
            ToolbarItem(placement: .automatic) {
                HStack(spacing: 8) {
                    Circle()
                        .fill(client.isInitialized ? .green : .red)
                        .frame(width: 8, height: 8)
                    Text("v\(client.version)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}
