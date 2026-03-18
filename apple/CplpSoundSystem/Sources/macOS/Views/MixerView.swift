import SwiftUI
import CplpBridge

/// ミキサー UI — トラック別のフェーダー / パン / ミュート
struct MixerView: View {
    @EnvironmentObject var client: CplpClient

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // MARK: - マスター
                GroupBox("Master") {
                    HStack {
                        Text("Volume")
                            .foregroundStyle(.secondary)
                        Spacer()
                        Text(String(format: "%.0f%%", client.masterVolume * 100))
                            .monospacedDigit()
                            .frame(width: 50, alignment: .trailing)
                    }
                    .padding(.vertical, 4)
                }

                // MARK: - トラック一覧
                if client.tracks.isEmpty {
                    GroupBox("Tracks") {
                        VStack(spacing: 12) {
                            Image(systemName: "slider.horizontal.3")
                                .font(.largeTitle)
                                .foregroundStyle(.tertiary)
                            Text("No tracks available.")
                                .foregroundStyle(.secondary)
                            Text("Connect to a session to see mixer tracks.")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 20)
                    }
                } else {
                    ForEach(client.tracks) { track in
                        TrackChannelStrip(track: track)
                    }
                }

                Spacer()
            }
            .padding(20)
        }
        .navigationTitle("Mixer")
        .onAppear {
            client.startMixerPolling()
        }
        .onDisappear {
            client.stopMixerPolling()
        }
    }
}

// MARK: - チャンネルストリップ

struct TrackChannelStrip: View {
    @EnvironmentObject var client: CplpClient
    let track: TrackState

    @State private var volume: Float
    @State private var pan: Float
    @State private var mute: Bool

    init(track: TrackState) {
        self.track = track
        self._volume = State(initialValue: track.volume)
        self._pan = State(initialValue: track.pan)
        self._mute = State(initialValue: track.mute)
    }

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                // ヘッダー
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(track.label.isEmpty ? track.peerId : track.label)
                            .font(.headline)
                        if !track.label.isEmpty {
                            Text(track.peerId)
                                .font(.caption2)
                                .foregroundStyle(.tertiary)
                        }
                    }
                    Spacer()
                    if track.solo {
                        Text("SOLO")
                            .font(.caption2)
                            .fontWeight(.bold)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.yellow.opacity(0.3))
                            .clipShape(RoundedRectangle(cornerRadius: 4))
                    }
                }

                // Volume フェーダー
                HStack {
                    Text("Vol")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 30, alignment: .leading)
                    Slider(value: $volume, in: 0...1) { editing in
                        if !editing {
                            client.mixerSetVolume(peerId: track.peerId, volume: volume)
                        }
                    }
                    Text(String(format: "%.0f%%", volume * 100))
                        .font(.caption.monospaced())
                        .frame(width: 44, alignment: .trailing)
                }

                // Pan スライダー
                HStack {
                    Text("Pan")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 30, alignment: .leading)
                    Slider(value: $pan, in: -1...1) { editing in
                        if !editing {
                            client.mixerSetPan(peerId: track.peerId, pan: pan)
                        }
                    }
                    Text(panLabel)
                        .font(.caption.monospaced())
                        .frame(width: 44, alignment: .trailing)
                }

                // Mute トグル
                HStack {
                    Toggle("Mute", isOn: $mute)
                        .toggleStyle(.switch)
                        .onChange(of: mute) { _, newValue in
                            client.mixerSetMute(peerId: track.peerId, mute: newValue)
                        }
                }
            }
            .padding(.vertical, 4)
        }
        .onChange(of: track.volume) { _, newValue in volume = newValue }
        .onChange(of: track.pan) { _, newValue in pan = newValue }
        .onChange(of: track.mute) { _, newValue in mute = newValue }
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
