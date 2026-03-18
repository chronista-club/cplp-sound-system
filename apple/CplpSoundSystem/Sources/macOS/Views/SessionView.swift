import SwiftUI

// MARK: - SessionView

/// セッション接続・切断と状態表示
struct SessionView: View {
    @Environment(CplpClient.self) private var client
    @State private var lobbyURL: String = "ws://localhost:3000/lobby"

    var body: some View {
        Form {
            // 接続設定
            Section("Connection") {
                TextField("Lobby URL", text: $lobbyURL)
                    .textFieldStyle(.roundedBorder)

                HStack {
                    Button("Connect") {
                        client.connectSession(lobbyURL: lobbyURL)
                    }
                    .disabled(client.sessionStatus == .connected || client.sessionStatus == .connecting)

                    Button("Disconnect") {
                        client.disconnectSession()
                    }
                    .disabled(client.sessionStatus == .disconnected || client.sessionStatus == .disconnecting)
                }
            }

            // ステータス表示
            Section("Status") {
                LabeledContent("Connection") {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(statusColor)
                            .frame(width: 8, height: 8)
                        Text(client.sessionStatus.rawValue)
                    }
                }

                LabeledContent("Peers", value: "\(client.peerCount)")
            }

            // ピア一覧（接続時のみ）
            if client.sessionStatus == .connected {
                Section("Peers") {
                    if client.tracks.isEmpty {
                        Text("No peers connected")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(client.tracks) { track in
                            HStack {
                                Image(systemName: "person.circle")
                                    .foregroundStyle(.blue)
                                VStack(alignment: .leading) {
                                    Text(track.label)
                                        .font(.body)
                                    Text(track.peerId)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle("Session")
    }

    private var statusColor: Color {
        switch client.sessionStatus {
        case .connected: .green
        case .connecting: .orange
        case .disconnecting: .orange
        case .disconnected: .red
        }
    }
}
