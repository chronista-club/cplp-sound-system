import CoreMIDI
import Foundation
import Observation

@MainActor
@Observable
final class KeystageManager {
    private let midiClient: MidiConsoleClient
    private(set) var isConnected: Bool = false
    private(set) var statusMessage: String = "Keystage not detected"

    // Keystage SysEx constants
    private static let korgId: UInt8 = 0x42
    private static let familyId: [UInt8] = [0x00, 0x01, 0x69]
    private static let memberId49: UInt8 = 0x01
    private static let memberId61: UInt8 = 0x09

    // Function IDs
    private static let funcSceneDumpRequest: UInt8 = 0x10
    private static let funcSceneDump: UInt8 = 0x40
    private static let funcAck: UInt8 = 0x23
    private static let funcNak: UInt8 = 0x24

    private static let sceneDataMagic = "2087ScnD"
    private static let sceneDataSize = 532

    private var keystageDestination: MIDIEndpointRef?
    private var pendingSceneDump: [UInt8] = []
    private var awaitingDump = false

    init(midiClient: MidiConsoleClient) {
        self.midiClient = midiClient
    }

    // MARK: - Scene File Path

    private static var sceneFilePath: URL {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dir = appSupport.appendingPathComponent("CPLP Sound System", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("keystage_scene.bin")
    }

    // MARK: - Detect & Connect

    func detectKeystage() {
        if let dest = midiClient.findDestination(nameContaining: "Keystage") {
            keystageDestination = dest
            isConnected = true
            statusMessage = "Keystage detected"
            print("[Keystage] Found destination: \(MidiConsoleClient.endpointDisplayName(dest))")
            loadOrDumpScene()
        } else {
            isConnected = false
            statusMessage = "Keystage not detected"
        }
    }

    // MARK: - Scene Management

    private func loadOrDumpScene() {
        let path = Self.sceneFilePath
        if FileManager.default.fileExists(atPath: path.path) {
            // Saved scene exists → send to Keystage
            do {
                let data = try Data(contentsOf: path)
                sendSceneData(Array(data))
                statusMessage = "Scene loaded → Keystage"
            } catch {
                print("[Keystage] Failed to read scene file: \(error)")
                requestSceneDump()
            }
        } else {
            // No saved scene → request dump from Keystage
            requestSceneDump()
        }
    }

    // MARK: - Request Scene Dump (Func=10)

    private func requestSceneDump() {
        guard let dest = keystageDestination else { return }
        statusMessage = "Requesting scene dump..."
        awaitingDump = true

        // F0 42 4g 00 01 69 mm 01 00 00 10 F7
        let globalChannel: UInt8 = 0x00
        let sysex: [UInt8] = [
            0xF0,
            Self.korgId,
            0x40 | globalChannel,
            Self.familyId[0], Self.familyId[1], Self.familyId[2],
            Self.memberId49,
            0x01, 0x00, 0x00,  // length = 1
            Self.funcSceneDumpRequest,
            0xF7
        ]
        midiClient.sendSysEx(sysex, to: dest)
        print("[Keystage] Scene dump requested")
    }

    // MARK: - Send Scene Data (Func=40)

    private func sendSceneData(_ sceneData: [UInt8]) {
        guard let dest = keystageDestination else { return }

        // Wrap scene data in SysEx: F0 42 4g 00 01 69 mm [len x3] 40 [data...] F7
        let globalChannel: UInt8 = 0x00
        let dataLen = sceneData.count + 1  // +1 for function ID
        let lenBytes = Self.encode7bit3(dataLen)

        var sysex: [UInt8] = [
            0xF0,
            Self.korgId,
            0x40 | globalChannel,
            Self.familyId[0], Self.familyId[1], Self.familyId[2],
            Self.memberId49,
            lenBytes.0, lenBytes.1, lenBytes.2,
            Self.funcSceneDump,
        ]
        sysex.append(contentsOf: Self.encode7bitData(sceneData))
        sysex.append(0xF7)

        midiClient.sendSysEx(sysex, to: dest)
        print("[Keystage] Scene data sent (\(sceneData.count) bytes)")
    }

    // MARK: - Handle Incoming SysEx

    func handleSysEx(_ data: [UInt8]) {
        guard data.count > 10,
              data[0] == 0xF0,
              data[1] == Self.korgId,
              data[3] == Self.familyId[0],
              data[4] == Self.familyId[1],
              data[5] == Self.familyId[2]
        else { return }

        let funcId = data[10]

        switch funcId {
        case Self.funcSceneDump:
            handleSceneDumpReceived(data)
        case Self.funcAck:
            print("[Keystage] ACK received")
            statusMessage = "Scene applied ✓"
        case Self.funcNak:
            print("[Keystage] NAK received")
            statusMessage = "Scene apply failed"
        default:
            print("[Keystage] SysEx func=\(String(format: "0x%02X", funcId))")
        }
    }

    private func handleSceneDumpReceived(_ data: [UInt8]) {
        guard awaitingDump else { return }
        awaitingDump = false

        // Extract scene data (after header, decode 7-bit)
        // data.count > 10 は呼び出し元で保証済み。ただし payload が空でないことを確認する
        guard data.count > 12 else {
            print("[Keystage] Scene dump too short: \(data.count) bytes")
            return
        }
        let payload = Array(data[11..<(data.count - 1)])  // skip F7
        let decoded = Self.decode7bitData(payload)

        // Save to file
        do {
            let fileData = Data(decoded)
            try fileData.write(to: Self.sceneFilePath)
            print("[Keystage] Scene saved to \(Self.sceneFilePath.path) (\(decoded.count) bytes)")
            statusMessage = "Scene saved (\(decoded.count) bytes)"
        } catch {
            print("[Keystage] Failed to save scene: \(error)")
            statusMessage = "Scene save failed"
        }
    }

    // MARK: - Force re-dump

    func resaveScene() {
        requestSceneDump()
    }

    // MARK: - 7-bit encoding helpers

    // KORG SysEx uses 7-bit encoding for data (MSB stripped, sent in groups)
    // Length field: 3 bytes, 7-bit each, little-endian
    private static func encode7bit3(_ value: Int) -> (UInt8, UInt8, UInt8) {
        return (
            UInt8(value & 0x7F),
            UInt8((value >> 7) & 0x7F),
            UInt8((value >> 14) & 0x7F)
        )
    }

    // KORG data encoding: every 7 bytes → 8 bytes (high bits packed in first byte)
    private static func encode7bitData(_ data: [UInt8]) -> [UInt8] {
        var result: [UInt8] = []
        var i = 0
        while i < data.count {
            let chunkSize = min(7, data.count - i)
            var highBits: UInt8 = 0
            for j in 0..<chunkSize {
                if data[i + j] & 0x80 != 0 {
                    highBits |= (1 << UInt8(j))
                }
            }
            result.append(highBits)
            for j in 0..<chunkSize {
                result.append(data[i + j] & 0x7F)
            }
            i += chunkSize
        }
        return result
    }

    private static func decode7bitData(_ data: [UInt8]) -> [UInt8] {
        var result: [UInt8] = []
        var i = 0
        while i < data.count {
            let highBits = data[i]
            i += 1
            for j in 0..<7 {
                guard i < data.count else { break }
                var byte = data[i]
                if highBits & (1 << UInt8(j)) != 0 {
                    byte |= 0x80
                }
                result.append(byte)
                i += 1
            }
        }
        return result
    }
}

// MARK: - MidiConsoleClient extension

extension MidiConsoleClient {
    static func endpointDisplayName(_ endpoint: MIDIEndpointRef) -> String {
        var name: Unmanaged<CFString>?
        let status = MIDIObjectGetStringProperty(endpoint, kMIDIPropertyDisplayName, &name)
        if status == noErr, let cfName = name?.takeRetainedValue() {
            return cfName as String
        }
        return "Unknown"
    }
}
