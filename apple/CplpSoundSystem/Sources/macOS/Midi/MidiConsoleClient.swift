import CoreMIDI
import CplpBridge
import Foundation

final class MidiConsoleClient: @unchecked Sendable {
    private var midiClient: MIDIClientRef = 0
    private var inputPort: MIDIPortRef = 0
    private var outputPort: MIDIPortRef = 0
    private let model: MidiConsoleModel

    init(model: MidiConsoleModel) {
        self.model = model
        setupCoreMidi()
    }

    deinit {
        if inputPort != 0 { MIDIPortDispose(inputPort) }
        if outputPort != 0 { MIDIPortDispose(outputPort) }
        if midiClient != 0 { MIDIClientDispose(midiClient) }
    }

    // MARK: - Setup

    private func setupCoreMidi() {
        let clientName = "CPLP MIDI Console" as CFString

        let status = MIDIClientCreateWithBlock(clientName, &midiClient) { [weak self] notification in
            self?.handleNotification(notification)
        }
        guard status == noErr else {
            print("[MidiConsole] MIDIClientCreate failed: \(status)")
            return
        }

        let portStatus = MIDIInputPortCreateWithProtocol(
            midiClient,
            "CPLP UMP Input" as CFString,
            ._2_0,
            &inputPort
        ) { [weak self] eventList, srcConnRefCon in
            self?.handleEventList(eventList, srcRef: srcConnRefCon)
        }
        guard portStatus == noErr else {
            print("[MidiConsole] MIDIInputPortCreate failed: \(portStatus)")
            return
        }

        let outStatus = MIDIOutputPortCreate(midiClient, "CPLP MIDI Out" as CFString, &outputPort)
        if outStatus != noErr {
            print("[MidiConsole] MIDIOutputPortCreate failed: \(outStatus)")
        }

        connectAllSources()
    }

    // MARK: - Send SysEx

    func sendSysEx(_ data: [UInt8], to destination: MIDIEndpointRef) {
        let count = data.count
        let buffer = UnsafeMutableBufferPointer<UInt8>.allocate(capacity: count)
        _ = buffer.initialize(from: data)
        let bufferBase = buffer.baseAddress!

        let request = UnsafeMutablePointer<MIDISysexSendRequest>.allocate(capacity: 1)
        request.pointee = MIDISysexSendRequest(
            destination: destination,
            data: bufferBase,
            bytesToSend: UInt32(count),
            complete: false,
            reserved: (0, 0, 0),
            completionProc: { req in
                // completionRefCon が nil なら既に失敗パスで解放済み
                guard let refCon = req.pointee.completionRefCon else {
                    return
                }
                let rawBuf = refCon.assumingMemoryBound(to: UInt8.self)
                rawBuf.deallocate()
                req.deallocate()
            },
            completionRefCon: UnsafeMutableRawPointer(bufferBase)
        )
        let status = MIDISendSysex(request)
        if status != noErr {
            print("[MidiConsole] MIDISendSysex failed: \(status)")
            // completionRefCon を nil にして completionProc が解放しないようにする
            request.pointee.completionRefCon = nil
            bufferBase.deallocate()
            request.deallocate()
        }
    }

    // MARK: - Find Destination

    func findDestination(nameContaining keyword: String) -> MIDIEndpointRef? {
        let count = MIDIGetNumberOfDestinations()
        for i in 0..<count {
            let dest = MIDIGetDestination(i)
            let name = Self.endpointName(dest)
            if name.contains(keyword) {
                return dest
            }
        }
        return nil
    }

    // MARK: - Connect Sources

    private func connectAllSources() {
        let sourceCount = MIDIGetNumberOfSources()
        var deviceNames: [String] = []

        for i in 0..<sourceCount {
            let source = MIDIGetSource(i)
            let name = Self.endpointName(source)
            let status = MIDIPortConnectSource(inputPort, source, nil)
            if status == noErr {
                deviceNames.append(name)
                print("[MidiConsole] Connected: \(name)")
            } else {
                print("[MidiConsole] Failed to connect \(name): \(status)")
            }
        }

        DispatchQueue.main.async { [weak self] in
            self?.model.connectedDevices = deviceNames
        }
    }

    // MARK: - Handle Events

    private func handleEventList(
        _ eventListPtr: UnsafePointer<MIDIEventList>,
        srcRef: UnsafeMutableRawPointer?
    ) {
        let sourceName = "MIDI"

        eventListPtr.unsafeSequence().forEach { packet in
            let wordCount = Int(packet.pointee.wordCount)
            guard wordCount > 0 else { return }

            let words = withUnsafePointer(to: packet.pointee.words) { ptr in
                // withMemoryRebound のクロージャ内で Array にコピーし、ポインタを外に持ち出さない
                ptr.withMemoryRebound(to: UInt32.self, capacity: wordCount) { boundPtr in
                    Array(UnsafeBufferPointer(start: boundPtr, count: wordCount))
                }
            }

            // Rust Engine に MIDI イベントを転送
            // CoreMIDI RT スレッドから Mutex を避けるため非 RT キューにディスパッチ
            let wordsCopy = words
            DispatchQueue.global(qos: .userInteractive).async {
                Self.forwardToEngine(words: wordsCopy)
            }

            if let event = UmpDecoder.decode(words: words, sourceName: sourceName) {
                DispatchQueue.main.async { [weak self] in
                    self?.model.append(event)
                }
            }
        }
    }

    // MARK: - Notifications

    private func handleNotification(_ notification: UnsafePointer<MIDINotification>) {
        switch notification.pointee.messageID {
        case .msgSetupChanged:
            print("[MidiConsole] MIDI setup changed, reconnecting...")
            connectAllSources()
        default:
            break
        }
    }

    // MARK: - Forward to Rust Engine

    private static func forwardToEngine(words: [UInt32]) {
        guard let first = words.first else { return }
        let mt = UInt8(first >> 28)

        switch mt {
        case 0x4: // MIDI 2.0 Channel Voice
            let status = UInt8((first >> 20) & 0xF)
            let note = UInt8((first >> 8) & 0x7F)
            switch status {
            case 0x9: // Note On
                // MIDI 2.0: velocity は word1 の上位 16bit (u16)
                let vel16 = words.count > 1 ? UInt16(words[1] >> 16) : 0
                let vel7 = UInt8(vel16 >> 9) // 16bit → 7bit
                let velocity = max(vel7, vel16 > 0 ? 1 : 0)
                cplp_midi_note_on(note, velocity)
            case 0x8: // Note Off
                cplp_midi_note_off(note)
            case 0xB: // CC
                let cc = UInt8((first >> 8) & 0x7F)
                let val32 = words.count > 1 ? words[1] : 0
                let val7 = UInt8(val32 >> 25) // 32bit → 7bit
                cplp_midi_cc(cc, val7)
            default:
                break
            }

        case 0x2: // MIDI 1.0 Channel Voice (互換)
            let statusByte = UInt8((first >> 16) & 0xF0)
            let data1 = UInt8((first >> 8) & 0x7F)
            let data2 = UInt8(first & 0x7F)
            switch statusByte {
            case 0x90:
                cplp_midi_note_on(data1, data2)
            case 0x80:
                cplp_midi_note_off(data1)
            case 0xB0:
                cplp_midi_cc(data1, data2)
            default:
                break
            }

        default:
            break
        }
    }

    // MARK: - Helpers

    private static func endpointName(_ endpoint: MIDIEndpointRef) -> String {
        var name: Unmanaged<CFString>?
        let status = MIDIObjectGetStringProperty(endpoint, kMIDIPropertyDisplayName, &name)
        if status == noErr, let cfName = name?.takeRetainedValue() {
            return cfName as String
        }
        return "Unknown"
    }

}
