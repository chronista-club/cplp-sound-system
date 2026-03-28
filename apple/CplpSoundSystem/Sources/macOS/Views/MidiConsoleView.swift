import SwiftUI

struct MidiConsoleView: View {
    @Environment(MidiConsoleModel.self) private var model

    private static let timeFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss.SSS"
        return f
    }()

    var body: some View {
        @Bindable var model = model

        VStack(spacing: 0) {
            // Filter bar
            HStack(spacing: 12) {
                Toggle(isOn: $model.isCapturing) {
                    Image(systemName: model.isCapturing ? "pause.fill" : "play.fill")
                }
                .toggleStyle(.button)
                .help(model.isCapturing ? "Pause" : "Resume")

                Button {
                    model.clear()
                } label: {
                    Image(systemName: "trash")
                }
                .help("Clear")

                Divider().frame(height: 20)

                // Message type filter
                Picker("Type", selection: $model.filterMessageType) {
                    Text("All Types").tag(UmpMessageType?.none)
                    Divider()
                    ForEach(UmpMessageType.allCases) { mt in
                        Text(mt.label).tag(UmpMessageType?.some(mt))
                    }
                }
                .frame(width: 140)

                // Channel filter
                Picker("Ch", selection: $model.filterChannel) {
                    Text("All Ch").tag(Int?.none)
                    Divider()
                    ForEach(0..<16, id: \.self) { ch in
                        Text("Ch \(ch + 1)").tag(Int?.some(ch))
                    }
                }
                .frame(width: 100)

                Divider().frame(height: 20)

                Toggle("Hide Clock", isOn: $model.hideTransport)
                    .toggleStyle(.checkbox)
                    .font(.caption)

                Spacer()

                // Device info
                if !model.connectedDevices.isEmpty {
                    Text(model.connectedDevices.joined(separator: ", "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text("\(model.filteredEvents.count) events")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.bar)

            Divider()

            // Header
            HStack(spacing: 0) {
                Text("Time")
                    .frame(width: 100, alignment: .leading)
                Text("Type")
                    .frame(width: 60, alignment: .leading)
                Text("Ch")
                    .frame(width: 30, alignment: .center)
                Text("Raw")
                    .frame(width: 200, alignment: .leading)
                Text("Description")
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .font(.caption.monospaced())
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 4)
            .background(.bar)

            Divider()

            // Event list
            ScrollViewReader { proxy in
                List(model.filteredEvents) { event in
                    UmpEventRow(event: event, timeFormatter: Self.timeFormatter)
                        .listRowInsets(EdgeInsets(top: 1, leading: 12, bottom: 1, trailing: 12))
                        .listRowSeparator(.hidden)
                        .id(event.id)
                }
                .listStyle(.plain)
                .font(.system(.caption, design: .monospaced))
                .onChange(of: model.filteredEvents.last?.id) { _, newId in
                    if let id = newId {
                        proxy.scrollTo(id, anchor: .bottom)
                    }
                }
            }
        }
        .frame(minWidth: 600, minHeight: 300)
    }
}

// MARK: - Event Row

struct UmpEventRow: View {
    let event: UmpEvent
    let timeFormatter: DateFormatter

    var body: some View {
        HStack(spacing: 0) {
            Text(timeFormatter.string(from: event.timestamp))
                .frame(width: 100, alignment: .leading)
                .foregroundStyle(.secondary)

            Text(event.messageType.label)
                .frame(width: 60, alignment: .leading)
                .foregroundStyle(messageColor)

            Text(event.channel >= 0 ? "\(event.channel + 1)" : "-")
                .frame(width: 30, alignment: .center)

            Text(event.rawWords.map { String(format: "%08X", $0) }.joined(separator: " "))
                .frame(width: 200, alignment: .leading)
                .foregroundStyle(.tertiary)

            Text(event.description)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var messageColor: Color {
        switch event.messageType {
        case .midi2Channel: return .green
        case .midi1Channel: return .blue
        case .systemCommon: return .orange
        default: return .secondary
        }
    }
}
