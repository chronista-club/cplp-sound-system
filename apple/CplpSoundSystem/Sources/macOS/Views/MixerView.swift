import SwiftUI

// MARK: - MixerView

/// ミキサーコントロール: マスターボリューム + トラック別 Volume/Pan/Mute
struct MixerView: View {
    @Environment(CplpClient.self) private var client

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // マスターボリューム
                GroupBox("Master") {
                    VStack(alignment: .leading) {
                        HStack {
                            Text("Volume")
                            Spacer()
                            Text(String(format: "%.0f%%", client.masterVolume * 100))
                                .foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                        ProgressView(value: Double(client.masterVolume))
                            .tint(.blue)
                    }
                    .padding(.vertical, 4)
                }

                // トラック一覧
                if client.tracks.isEmpty {
                    GroupBox("Tracks") {
                        Text("Connect to a session to see tracks")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .center)
                            .padding()
                    }
                } else {
                    ForEach(client.tracks) { track in
                        TrackChannelView(track: track)
                    }
                }
            }
            .padding()
        }
        .navigationTitle("Mixer")
    }
}

// MARK: - TrackChannelView

/// 1 トラック分のチャンネルストリップ
struct TrackChannelView: View {
    @Environment(CplpClient.self) private var client
    let track: TrackState

    @State private var volume: Float
    @State private var pan: Float

    init(track: TrackState) {
        self.track = track
        self._volume = State(initialValue: track.volume)
        self._pan = State(initialValue: track.pan)
    }

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                // ヘッダー: ラベル + ミュートトグル
                HStack {
                    Text(track.label)
                        .font(.headline)
                    Text(track.peerId)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Toggle("Mute", isOn: Binding(
                        get: { track.isMuted },
                        set: { client.setMute(peerId: track.peerId, mute: $0) }
                    ))
                    .toggleStyle(.button)
                    .tint(track.isMuted ? .red : .gray)
                }

                // ボリュームスライダー
                HStack {
                    Image(systemName: "speaker.fill")
                        .foregroundStyle(.secondary)
                    Slider(value: $volume, in: 0...1) { editing in
                        if !editing {
                            client.setVolume(peerId: track.peerId, volume: volume)
                        }
                    }
                    Text(String(format: "%.0f%%", volume * 100))
                        .monospacedDigit()
                        .frame(width: 44, alignment: .trailing)
                }

                // パンスライダー
                HStack {
                    Text("L")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Slider(value: $pan, in: -1...1) { editing in
                        if !editing {
                            client.setPan(peerId: track.peerId, pan: pan)
                        }
                    }
                    Text("R")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(panLabel)
                        .monospacedDigit()
                        .frame(width: 44, alignment: .trailing)
                }
            }
            .padding(.vertical, 4)
        }
    }

    private var panLabel: String {
        if abs(pan) < 0.05 {
            return "C"
        } else if pan < 0 {
            return String(format: "L%.0f", abs(pan) * 100)
        } else {
            return String(format: "R%.0f", pan * 100)
        }
    }
}
