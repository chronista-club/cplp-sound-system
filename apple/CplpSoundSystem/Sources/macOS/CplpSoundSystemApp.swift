import SwiftUI

@main
struct CplpSoundSystemApp: App {
    /// cplp_init() は @StateObject 評価より先に実行する必要がある。
    /// static let は App 構造体のプロパティ初期化より先に評価される。
    private static let runtimeReady: Bool = {
        let client = CplpClient()
        do {
            try client.initialize()
        } catch {
            fatalError("cplp_init failed: \(error.localizedDescription)")
        }
        _sharedClient = client
        return true
    }()

    private static var _sharedClient: CplpClient!

    @StateObject private var client: CplpClient = {
        _ = CplpSoundSystemApp.runtimeReady
        return CplpSoundSystemApp._sharedClient
    }()

    @State private var midiModel = MidiConsoleModel()
    @State private var midiClient: MidiConsoleClient?
    @State private var keystageManager: KeystageManager?

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(client)
                .environment(midiModel)
                .environment(keystageManager)
        }
        .defaultSize(width: 900, height: 650)

        Window("MIDI 2.0 Console", id: "midi-console") {
            MidiConsoleView()
                .environment(midiModel)
        }
        .defaultSize(width: 800, height: 500)
        .defaultLaunchBehavior(.suppressed)
    }

    init() {
        let model = MidiConsoleModel()
        let midi = MidiConsoleClient(model: model)
        let keystage = KeystageManager(midiClient: midi)
        _midiModel = State(initialValue: model)
        _midiClient = State(initialValue: midi)
        _keystageManager = State(initialValue: keystage)

        // Keystage detection is triggered manually from MIDI overview
        // Auto-detect will be enabled once SysEx receive handling is connected
    }
}
