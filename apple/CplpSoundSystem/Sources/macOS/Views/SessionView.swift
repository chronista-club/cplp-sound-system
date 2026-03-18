import SwiftUI
import CplpBridge

/// ロビー接続 UI — connect/disconnect + 状態表示 + ピア一覧
struct SessionView: View {
    @EnvironmentObject var client: CplpClient
    @State private var lobbyUrlInput: String = "ws://localhost:3000"

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // MARK: - 接続状態
                GroupBox("Connection Status") {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Status")
                                .foregroundStyle(.secondary)
                            Spacer()
                            HStack(spacing: 6) {
                                Circle()
                                    .fill(statusColor)
                                    .frame(width: 10, height: 10)
                                Text(statusLabel)
                                    .fontWeight(.medium)
                            }
                        }

                        HStack {
                            Text("Peers")
                                .foregroundStyle(.secondary)
                            Spacer()
                            Text("\(client.peerCount)")
                                .fontWeight(.medium)
                                .monospacedDigit()
                        }

                        if !client.lobbyUrl.isEmpty {
                            HStack {
                                Text("Lobby")
                                    .foregroundStyle(.secondary)
                                Spacer()
                                Text(client.lobbyUrl)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }
                        }
                    }
                    .padding(.vertical, 4)
                }

                // MARK: - 接続コントロール
                GroupBox("Connect") {
                    VStack(alignment: .leading, spacing: 12) {
                        TextField("Lobby URL", text: $lobbyUrlInput)
                            .textFieldStyle(.roundedBorder)
                            .disabled(isConnectedOrConnecting)

                        HStack {
                            Spacer()
                            if isConnectedOrConnecting {
                                Button("Disconnect") {
                                    client.sessionDisconnect()
                                }
                                .buttonStyle(.borderedProminent)
                                .tint(.red)
                                .disabled(client.sessionStatus == CPLP_SESSION_STATUS_DISCONNECTING)
                            } else {
                                Button("Connect") {
                                    client.sessionConnect(url: lobbyUrlInput)
                                }
                                .buttonStyle(.borderedProminent)
                                .disabled(lobbyUrlInput.isEmpty)
                            }
                        }
                    }
                    .padding(.vertical, 4)
                }

                // MARK: - ピア一覧（接続時のみ）
                if client.sessionStatus == CPLP_SESSION_STATUS_CONNECTED, client.peerCount > 0 {
                    GroupBox("Peers") {
                        VStack(alignment: .leading, spacing: 8) {
                            // ミキサー状態からピア情報を取得
                            if client.tracks.isEmpty {
                                Text("No peer tracks available yet.")
                                    .foregroundStyle(.secondary)
                                    .font(.caption)
                            } else {
                                ForEach(client.tracks) { track in
                                    HStack {
                                        Image(systemName: "person.circle")
                                            .foregroundStyle(.secondary)
                                        VStack(alignment: .leading, spacing: 2) {
                                            Text(track.label.isEmpty ? track.peerId : track.label)
                                                .font(.body)
                                            Text(track.peerId)
                                                .font(.caption2)
                                                .foregroundStyle(.tertiary)
                                        }
                                        Spacer()
                                        StatusIndicator(
                                            label: track.mute ? "Muted" : "Active",
                                            isActive: !track.mute
                                        )
                                    }
                                    .padding(.vertical, 2)
                                }
                            }
                        }
                        .padding(.vertical, 4)
                    }
                }

                Spacer()
            }
            .padding(20)
        }
        .navigationTitle("Session")
        .onAppear {
            client.refreshSessionState()
        }
    }

    // MARK: - ヘルパー

    private var isConnectedOrConnecting: Bool {
        client.sessionStatus == CPLP_SESSION_STATUS_CONNECTED
            || client.sessionStatus == CPLP_SESSION_STATUS_CONNECTING
    }

    private var statusColor: Color {
        switch client.sessionStatus {
        case CPLP_SESSION_STATUS_CONNECTED:
            return .green
        case CPLP_SESSION_STATUS_CONNECTING, CPLP_SESSION_STATUS_DISCONNECTING:
            return .orange
        case CPLP_SESSION_STATUS_DISCONNECTED:
            return .gray
        default:
            return .gray
        }
    }

    private var statusLabel: String {
        switch client.sessionStatus {
        case CPLP_SESSION_STATUS_CONNECTED:
            return "Connected"
        case CPLP_SESSION_STATUS_CONNECTING:
            return "Connecting..."
        case CPLP_SESSION_STATUS_DISCONNECTING:
            return "Disconnecting..."
        case CPLP_SESSION_STATUS_DISCONNECTED:
            return "Disconnected"
        default:
            return "Unknown"
        }
    }
}
