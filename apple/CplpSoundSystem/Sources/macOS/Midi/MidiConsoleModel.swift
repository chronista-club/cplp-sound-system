import Foundation
import Observation

@Observable
final class MidiConsoleModel {
    private(set) var events: [UmpEvent] = []
    var filterMessageType: UmpMessageType? = nil
    var filterChannel: Int? = nil
    var isCapturing: Bool = true
    var hideTransport: Bool = true  // Timing Clock / Active Sensing を非表示

    static let maxEvents = 1000

    // Timing Clock (0xF8), Active Sensing (0xFE)
    private static let transportStatuses: Set<UInt8> = [0xF8, 0xFE]

    var filteredEvents: [UmpEvent] {
        events.filter { event in
            if hideTransport, event.isTransport { return false }
            if let mt = filterMessageType, event.messageType != mt { return false }
            if let ch = filterChannel, event.channel != ch { return false }
            return true
        }
    }

    var connectedDevices: [String] = []

    func append(_ event: UmpEvent) {
        guard isCapturing else { return }
        if events.count >= Self.maxEvents {
            events.removeFirst()
        }
        events.append(event)
    }

    func clear() {
        events.removeAll()
    }
}
