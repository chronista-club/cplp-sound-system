import SwiftUI

struct MidiOverviewView: View {
    @Environment(\.openWindow) private var openWindow
    @Environment(KeystageManager.self) private var keystageManager: KeystageManager?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // MARK: - Keystage Status
                GroupBox("Keystage") {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Status")
                                .foregroundStyle(.secondary)
                            Spacer()
                            HStack(spacing: 6) {
                                Circle()
                                    .fill(keystageManager?.isConnected == true ? Color.green : Color.gray)
                                    .frame(width: 10, height: 10)
                                Text(keystageManager?.statusMessage ?? "Not initialized")
                                    .fontWeight(.medium)
                            }
                        }

                        HStack {
                            Spacer()
                            if keystageManager?.isConnected == true {
                                Button("Re-save Scene") {
                                    keystageManager?.resaveScene()
                                }
                                .buttonStyle(.bordered)
                            } else {
                                Button("Detect Keystage") {
                                    keystageManager?.detectKeystage()
                                }
                                .buttonStyle(.bordered)
                            }
                        }
                    }
                    .padding(.vertical, 4)
                }

                // MARK: - Console
                GroupBox("MIDI 2.0 Console") {
                    VStack(spacing: 12) {
                        Image(systemName: "pianokeys")
                            .font(.system(size: 36))
                            .foregroundStyle(.secondary)
                        Button("Open Console Window") {
                            openWindow(id: "midi-console")
                        }
                        .buttonStyle(.borderedProminent)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
                }

                Spacer()
            }
            .padding(20)
        }
        .navigationTitle("MIDI")
    }
}
