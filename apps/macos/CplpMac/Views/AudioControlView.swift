import SwiftUI

/// オーディオエンジンのコントロール + レベルメーター
struct AudioControlView: View {
    @EnvironmentObject var bridge: CplpBridge

    var body: some View {
        VStack(spacing: 16) {
            // オーディオ ON/OFF
            HStack {
                Text("Audio Engine")
                    .font(.headline)
                Spacer()
                Button(bridge.isAudioRunning ? "Stop" : "Start") {
                    if bridge.isAudioRunning {
                        bridge.stopAudio()
                    } else {
                        bridge.startAudio()
                    }
                }
                .controlSize(.large)
                .buttonStyle(.borderedProminent)
                .tint(bridge.isAudioRunning ? .red : .green)
            }

            // レベルメーター
            if bridge.isAudioRunning {
                VStack(spacing: 8) {
                    MeterBar(label: "L", level: bridge.meterLeft)
                    MeterBar(label: "R", level: bridge.meterRight)
                }
                .transition(.opacity)
            }
        }
        .padding()
        .background(RoundedRectangle(cornerRadius: 12)
            .fill(.ultraThinMaterial))
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
