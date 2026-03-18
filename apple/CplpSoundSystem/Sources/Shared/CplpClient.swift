import CplpBridge
import Foundation

// MARK: - Data Models

/// トラック状態の Swift 表現
struct TrackState: Identifiable, Sendable {
    var id: String { peerId }
    let peerId: String
    let label: String
    var volume: Float
    var pan: Float
    var isMuted: Bool
    var isSolo: Bool
}

/// プラグインエントリの Swift 表現
struct PluginEntry: Identifiable, Sendable {
    var id: String { pluginId }
    let pluginId: String
    let name: String
}

// MARK: - CplpClient

/// Rust FFI ランタイムを管理するクライアント
///
/// cplp-ffi の全 FFI 関数を Swift から安全に呼び出すためのラッパー。
/// @MainActor で UI スレッドに限定し、@Observable で SwiftUI バインディングを提供する。
///
/// ## ライフサイクル
/// ```
/// init() → cplp_init()
///   ↓
/// startAudio() / connectSession() / ...
///   ↓
/// deinit → cplp_audio_stop() → cplp_destroy()
/// ```
@MainActor
@Observable
final class CplpClient {

    // MARK: - Published Properties

    /// ランタイムが初期化済みか
    private(set) var isInitialized: Bool = false

    /// オーディオエンジンが稼働中か
    private(set) var audioRunning: Bool = false

    /// セッション接続状態
    private(set) var sessionStatus: SessionStatus = .disconnected

    /// 接続中のピア数
    private(set) var peerCount: UInt32 = 0

    /// ミキサートラック一覧
    private(set) var tracks: [TrackState] = []

    /// スキャン済みプラグイン一覧
    private(set) var plugins: [PluginEntry] = []

    /// バージョン文字列
    private(set) var version: String = ""

    /// マスターボリューム
    private(set) var masterVolume: Float = 1.0

    /// オーディオメーター（L/R）
    private(set) var meterLeft: Float = 0.0
    private(set) var meterRight: Float = 0.0

    // MARK: - Session Status

    enum SessionStatus: String, Sendable {
        case disconnected = "Disconnected"
        case connecting = "Connecting"
        case connected = "Connected"
        case disconnecting = "Disconnecting"
    }

    // MARK: - Private

    private var pollTimer: Timer?

    // MARK: - Init / Destroy

    init() {
        let result = cplp_init()
        if result == CPLP_RESULT_OK {
            isInitialized = true
            let v = cplp_version()
            version = "\(v.major).\(v.minor).\(v.patch)"
            startPolling()
        }
    }

    deinit {
        pollTimer?.invalidate()
        cplp_audio_stop()
        cplp_destroy()
    }

    /// バージョン文字列を返す
    func getVersion() -> String {
        version
    }

    // MARK: - Audio Control

    /// オーディオエンジンを開始
    func startAudio() {
        let result = cplp_audio_start()
        if result == CPLP_RESULT_OK {
            audioRunning = true
        }
    }

    /// オーディオエンジンを停止
    func stopAudio() {
        let result = cplp_audio_stop()
        if result == CPLP_RESULT_OK {
            audioRunning = false
            meterLeft = 0
            meterRight = 0
        }
    }

    /// メーター値を更新（ポーリングで呼ばれる）
    func refreshMeters() {
        let meters = cplp_audio_get_meters()
        meterLeft = meters.left
        meterRight = meters.right
        audioRunning = cplp_audio_is_running()
    }

    /// CLAP プラグインをスキャン
    func scanPlugins() {
        let list = cplp_audio_scan_plugins()
        defer { cplp_plugin_list_free(list) }

        var entries: [PluginEntry] = []
        for i in 0..<Int(list.count) {
            let item = list.items[i]
            let pluginId = item.id.map { String(cString: $0) } ?? "unknown"
            let name = item.name.map { String(cString: $0) } ?? pluginId
            entries.append(PluginEntry(pluginId: pluginId, name: name))
        }
        plugins = entries
    }

    // MARK: - Session

    /// セッションに接続
    func connectSession(lobbyURL: String) {
        lobbyURL.withCString { cStr in
            let result = cplp_session_connect(cStr)
            if result == CPLP_RESULT_OK {
                refreshSessionState()
            }
        }
    }

    /// セッションから切断
    func disconnectSession() {
        let result = cplp_session_disconnect()
        if result == CPLP_RESULT_OK {
            refreshSessionState()
        }
    }

    /// セッション状態を取得して更新
    func refreshSessionState() {
        let state = cplp_session_get_state()
        peerCount = state.peer_count

        switch state.status {
        case CPLP_SESSION_STATUS_DISCONNECTED:
            sessionStatus = .disconnected
        case CPLP_SESSION_STATUS_CONNECTING:
            sessionStatus = .connecting
        case CPLP_SESSION_STATUS_CONNECTED:
            sessionStatus = .connected
        case CPLP_SESSION_STATUS_DISCONNECTING:
            sessionStatus = .disconnecting
        default:
            sessionStatus = .disconnected
        }
    }

    // MARK: - Mixer

    /// トラックのボリュームを設定
    func setVolume(peerId: String, volume: Float) {
        peerId.withCString { cStr in
            cplp_mixer_set_volume(cStr, volume)
        }
        refreshMixerState()
    }

    /// トラックのパンを設定
    func setPan(peerId: String, pan: Float) {
        peerId.withCString { cStr in
            cplp_mixer_set_pan(cStr, pan)
        }
        refreshMixerState()
    }

    /// トラックのミュートを設定
    func setMute(peerId: String, mute: Bool) {
        peerId.withCString { cStr in
            cplp_mixer_set_mute(cStr, mute)
        }
        refreshMixerState()
    }

    /// ミキサー状態を取得して更新
    func refreshMixerState() {
        let state = cplp_mixer_get_state()
        defer { cplp_mixer_state_free(state) }

        masterVolume = state.master_volume

        var newTracks: [TrackState] = []
        for i in 0..<Int(state.track_count) {
            let info = state.tracks[i]
            let peerId = info.peer_id.map { String(cString: $0) } ?? "unknown"
            let label = info.label.map { String(cString: $0) } ?? peerId
            newTracks.append(TrackState(
                peerId: peerId,
                label: label,
                volume: info.volume,
                pan: info.pan,
                isMuted: info.mute,
                isSolo: info.solo
            ))
        }
        tracks = newTracks
    }

    /// ミキサー状態を解放
    func freeMixerState(_ state: CplpMixerState) {
        cplp_mixer_state_free(state)
    }

    // MARK: - Polling

    /// セッション/ミキサー/メーター状態をポーリングで更新
    private func startPolling() {
        pollTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.refreshMeters()
                self.refreshSessionState()
                if self.sessionStatus == .connected {
                    self.refreshMixerState()
                }
            }
        }
    }
}
