import SwiftUI

// MARK: - CplpClient (SharedObject)

/// Rust FFI ランタイムを管理するクライアント
///
/// visionOS では @Observable + SharedObject としてアプリ全体で共有する。
/// macOS の CplpBridge に相当するが、visionOS 空間コンピューティング向けに拡張。
@MainActor
@Observable
final class CplpClient {
    private(set) var isInitialized: Bool = false
    private(set) var isAudioRunning: Bool = false
    private(set) var version: String = ""

    /// SceneGraph のスナップショット（FFI から取得した最新のノード情報）
    private(set) var sceneNodes: [SceneNodeData] = []

    /// ミキサートラック情報（空間配置用）
    private(set) var mixerTracks: [MixerTrackData] = []

    init() {
        let result = cplp_init()
        if result == CPLP_RESULT_OK {
            isInitialized = true
            let v = cplp_version()
            version = "\(v.major).\(v.minor).\(v.patch)"
        }
    }

    deinit {
        cplp_audio_stop()
        cplp_destroy()
    }

    // MARK: - Audio Control

    func startAudio() {
        let result = cplp_audio_start()
        if result == CPLP_RESULT_OK {
            isAudioRunning = true
        }
    }

    func stopAudio() {
        let result = cplp_audio_stop()
        if result == CPLP_RESULT_OK {
            isAudioRunning = false
        }
    }

    // MARK: - Scene Data Refresh

    /// FFI から SceneGraph データを取得してローカルキャッシュを更新
    func refreshSceneGraph() {
        // TODO: cplp_scene_get_nodes() FFI が実装されたらここで取得
        // 現時点ではデモ用のモジュールデータを生成
        sceneNodes = SceneNodeData.demoRackModules()
    }

    /// ミキサー状態を取得して空間配置用データに変換
    func refreshMixerTracks() {
        let state = cplp_mixer_get_state()
        defer { cplp_mixer_state_free(state) }

        var tracks: [MixerTrackData] = []
        for i in 0..<Int(state.track_count) {
            let trackInfo = state.tracks[i]
            let peerId = trackInfo.peer_id.map { String(cString: $0) } ?? "unknown"
            let label = trackInfo.label.map { String(cString: $0) } ?? peerId

            tracks.append(MixerTrackData(
                peerId: peerId,
                label: label,
                volume: trackInfo.volume,
                pan: trackInfo.pan,
                isMuted: trackInfo.mute,
                isSolo: trackInfo.solo
            ))
        }
        mixerTracks = tracks
    }
}

// MARK: - Data Models

/// SceneGraph ノードの Swift 表現（FFI ブリッジ用）
struct SceneNodeData: Identifiable {
    let id: String
    let name: String
    let position: SIMD3<Float>
    let scale: SIMD3<Float>
    let color: SIMD3<Float>
    let children: [SceneNodeData]

    /// デモ用: ユーロラック 2 行 x 数モジュールのレイアウト
    static func demoRackModules() -> [SceneNodeData] {
        let hpWidth: Float = 0.00508  // 1HP = 5.08mm
        let moduleHeight: Float = 0.1286  // 3U = 128.6mm
        let rowSpacing: Float = 0.015

        var modules: [SceneNodeData] = []
        // Row 0
        let row0Modules: [(String, Int, SIMD3<Float>)] = [
            ("VCO-1", 10, SIMD3(0.8, 0.3, 0.4)),
            ("VCF-1", 12, SIMD3(0.3, 0.5, 0.8)),
            ("VCA-1", 8, SIMD3(0.4, 0.8, 0.3)),
            ("ENV-1", 8, SIMD3(0.9, 0.6, 0.2)),
            ("LFO-1", 4, SIMD3(0.6, 0.2, 0.7)),
        ]

        var xOffset: Float = 0
        for (name, hp, color) in row0Modules {
            let width = Float(hp) * hpWidth
            modules.append(SceneNodeData(
                id: "row0-\(name)",
                name: name,
                position: SIMD3(xOffset + width / 2, 0, 0),
                scale: SIMD3(width, moduleHeight, 0.02),
                color: color,
                children: []
            ))
            xOffset += width
        }

        // Row 1
        let row1Modules: [(String, Int, SIMD3<Float>)] = [
            ("SEQ-1", 16, SIMD3(0.5, 0.5, 0.9)),
            ("MIX-1", 6, SIMD3(0.7, 0.7, 0.3)),
            ("FX-1", 14, SIMD3(0.3, 0.6, 0.6)),
            ("OUT-1", 6, SIMD3(0.9, 0.3, 0.3)),
        ]

        xOffset = 0
        for (name, hp, color) in row1Modules {
            let width = Float(hp) * hpWidth
            modules.append(SceneNodeData(
                id: "row1-\(name)",
                name: name,
                position: SIMD3(xOffset + width / 2, -(moduleHeight + rowSpacing), 0),
                scale: SIMD3(width, moduleHeight, 0.02),
                color: color,
                children: []
            ))
            xOffset += width
        }

        return modules
    }
}

/// ミキサートラックの Swift 表現（空間配置用）
struct MixerTrackData: Identifiable {
    var id: String { peerId }
    let peerId: String
    let label: String
    var volume: Float
    var pan: Float
    var isMuted: Bool
    var isSolo: Bool

    /// 空間配置での 3D 位置（pan を X 軸、volume を Y 軸にマッピング）
    var spatialPosition: SIMD3<Float> {
        SIMD3(pan * 0.5, volume * 0.3, -0.5)
    }
}

// MARK: - App Entry Point

#if os(visionOS)

@main
struct CplpVisionApp: App {
    @State private var client = CplpClient()
    @State private var showImmersiveSpace = false
    @State private var immersiveSpaceIsShown = false

    @Environment(\.openImmersiveSpace) var openImmersiveSpace
    @Environment(\.dismissImmersiveSpace) var dismissImmersiveSpace

    var body: some Scene {
        // メインウィンドウ: コントロールパネル
        WindowGroup("CPLP Sound System", id: "main") {
            VisionControlPanel(
                showImmersiveSpace: $showImmersiveSpace
            )
            .environment(client)
            .onChange(of: showImmersiveSpace) { _, newValue in
                Task {
                    if newValue {
                        switch await openImmersiveSpace(id: "ImmersiveRack") {
                        case .opened:
                            immersiveSpaceIsShown = true
                        case .error, .userCancelled:
                            showImmersiveSpace = false
                            immersiveSpaceIsShown = false
                        @unknown default:
                            showImmersiveSpace = false
                            immersiveSpaceIsShown = false
                        }
                    } else if immersiveSpaceIsShown {
                        await dismissImmersiveSpace()
                        immersiveSpaceIsShown = false
                    }
                }
            }
        }

        // イマーシブ空間: ユーロラック 3D 表示
        ImmersiveSpace(id: "ImmersiveRack") {
            ImmersiveRackView()
                .environment(client)
        }
        .immersionStyle(selection: .constant(.mixed), in: .mixed)

        // 空間ミキサーウィンドウ（ボリュームウィンドウ）
        WindowGroup("Spatial Mixer", id: "spatial-mixer") {
            SpatialMixerView()
                .environment(client)
        }
        .defaultSize(width: 600, height: 400, depth: 300, in: .millimeters)
    }
}

// MARK: - Control Panel (Window)

/// メインウィンドウのコントロールパネル
struct VisionControlPanel: View {
    @Environment(CplpClient.self) private var client
    @Binding var showImmersiveSpace: Bool

    var body: some View {
        NavigationStack {
            List {
                Section("System") {
                    LabeledContent("Version", value: client.version)
                    LabeledContent("Status", value: client.isInitialized ? "Ready" : "Not Initialized")
                }

                Section("Audio") {
                    Toggle("Audio Engine", isOn: Binding(
                        get: { client.isAudioRunning },
                        set: { newValue in
                            if newValue {
                                client.startAudio()
                            } else {
                                client.stopAudio()
                            }
                        }
                    ))
                }

                Section("Spatial") {
                    Toggle("Immersive Rack", isOn: $showImmersiveSpace)

                    Button("Open Spatial Mixer") {
                        // WindowGroup("Spatial Mixer") を開く
                        // TODO: @Environment(\.openWindow) で id: "spatial-mixer" を開く
                    }
                }
            }
            .navigationTitle("CPLP Sound System")
        }
    }
}

#endif
