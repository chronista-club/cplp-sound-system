import Foundation

// MARK: - UMP Message Type (MT field, upper 4 bits of word 0)

enum UmpMessageType: UInt8, CaseIterable, Identifiable {
    case utility          = 0x0
    case systemCommon     = 0x1
    case midi1Channel     = 0x2
    case data64           = 0x3
    case midi2Channel     = 0x4
    case data128          = 0x5

    var id: UInt8 { rawValue }

    var label: String {
        switch self {
        case .utility:      return "Utility"
        case .systemCommon: return "System"
        case .midi1Channel: return "MIDI1"
        case .data64:       return "Data64"
        case .midi2Channel: return "MIDI2"
        case .data128:      return "Data128"
        }
    }
}

// MARK: - UMP Event

struct UmpEvent: Identifiable {
    let id: UUID
    let timestamp: Date
    let sourceName: String
    let messageType: UmpMessageType
    let channel: Int          // 0-15, -1 for N/A
    let rawWords: [UInt32]    // 1-4 words
    let description: String   // "Note On C4 vel=32767"

    /// Timing Clock (0xF8) or Active Sensing (0xFE)
    var isTransport: Bool {
        guard messageType == .systemCommon, let first = rawWords.first else { return false }
        let status = UInt8((first >> 16) & 0xFF)
        return status == 0xF8 || status == 0xFE
    }

    init(
        sourceName: String,
        messageType: UmpMessageType,
        channel: Int,
        rawWords: [UInt32],
        description: String
    ) {
        self.id = UUID()
        self.timestamp = Date()
        self.sourceName = sourceName
        self.messageType = messageType
        self.channel = channel
        self.rawWords = rawWords
        self.description = description
    }
}

// MARK: - UMP Decoder

enum UmpDecoder {
    static func decode(words: [UInt32], sourceName: String) -> UmpEvent? {
        guard let first = words.first else { return nil }

        let mtRaw = UInt8(first >> 28)
        let messageType = UmpMessageType(rawValue: mtRaw) ?? .utility
        let channel = Int((first >> 16) & 0xF)

        let description: String

        switch messageType {
        case .midi2Channel:
            let status = UInt8((first >> 20) & 0xF)
            let note = UInt8((first >> 8) & 0x7F)
            let noteName = Self.noteName(note)

            switch status {
            case 0x9: // Note On
                let velocity = words.count > 1 ? words[1] >> 16 : 0  // 16bit velocity
                description = "Note On  \(noteName) vel=\(velocity)"
            case 0x8: // Note Off
                let velocity = words.count > 1 ? words[1] >> 16 : 0
                description = "Note Off \(noteName) vel=\(velocity)"
            case 0xA: // Poly Pressure
                let pressure = words.count > 1 ? words[1] : 0
                description = "Poly AT  \(noteName) pressure=\(pressure)"
            case 0xB: // Control Change
                let cc = UInt8((first >> 8) & 0x7F)
                let value = words.count > 1 ? words[1] : 0
                description = "CC #\(cc) val=\(value)"
            case 0xD: // Channel Pressure
                let pressure = words.count > 1 ? words[1] : 0
                description = "Ch AT pressure=\(pressure)"
            case 0xE: // Pitch Bend
                let value = words.count > 1 ? words[1] : 0
                description = "Pitch Bend val=\(value)"
            case 0x0: // Registered Per-Note Controller
                description = "Reg Per-Note CC \(noteName)"
            case 0x1: // Assignable Per-Note Controller
                description = "Asgn Per-Note CC \(noteName)"
            default:
                let hex = words.map { String(format: "%08X", $0) }.joined(separator: " ")
                description = "MIDI2 status=0x\(String(format: "%X", status)) [\(hex)]"
            }

        case .midi1Channel:
            let statusByte = UInt8((first >> 16) & 0xF0)
            let note = UInt8((first >> 8) & 0x7F)
            let val = UInt8(first & 0x7F)
            let noteName = Self.noteName(note)

            switch statusByte {
            case 0x90:
                description = "Note On  \(noteName) vel=\(val)"
            case 0x80:
                description = "Note Off \(noteName) vel=\(val)"
            case 0xB0:
                description = "CC #\(note) val=\(val)"
            case 0xE0:
                let bend = (UInt16(val) << 7) | UInt16(note)
                description = "Pitch Bend val=\(bend)"
            case 0xD0:
                description = "Ch AT pressure=\(note)"
            case 0xA0:
                description = "Poly AT \(noteName) pressure=\(val)"
            case 0xC0:
                description = "Program Change \(note)"
            default:
                let hex = words.map { String(format: "%08X", $0) }.joined(separator: " ")
                description = "MIDI1 [\(hex)]"
            }

        default:
            let hex = words.map { String(format: "%08X", $0) }.joined(separator: " ")
            description = "\(messageType.label) [\(hex)]"
        }

        return UmpEvent(
            sourceName: sourceName,
            messageType: messageType,
            channel: channel,
            rawWords: words,
            description: description
        )
    }

    // MARK: - Note Name

    private static let noteNames = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]

    static func noteName(_ note: UInt8) -> String {
        let name = noteNames[Int(note) % 12]
        let octave = Int(note) / 12 - 1
        return "\(name)\(octave)"
    }
}
