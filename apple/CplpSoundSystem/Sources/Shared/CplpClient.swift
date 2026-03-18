import Foundation
import CplpBridge

/// Rust FFI ラッパー — Swift から cplp-ffi を安全に呼び出す
///
/// ライフサイクル: `CplpClient.initialize()` → 使用 → `CplpClient.shutdown()`
/// App 全体で 1 インスタンスのみ想定（Rust 側がグローバル状態を持つため）。
@MainActor
final class CplpClient: ObservableObject {
    @Published private(set) var isInitialized: Bool = false
    @Published private(set) var isAudioRunning: Bool = false
    @Published private(set) var version: String = ""
    @Published private(set) var meterLeft: Float = 0
    @Published private(set) var meterRight: Float = 0

    // MARK: - セッション状態

    @Published private(set) var sessionStatus: CplpSessionStatus = CPLP_SESSION_STATUS_DISCONNECTED
    @Published private(set) var peerCount: UInt32 = 0
    @Published private(set) var lobbyUrl: String = ""

    // MARK: - ミキサー状態

    @Published private(set) var tracks: [TrackState] = []
    @Published private(set) var masterVolume: Float = 1.0

    // MARK: - プラグイン

    @Published private(set) var plugins: [PluginEntry] = []

    private var meterTimer: Timer?
    private var meterTask: Task<Void, Never>?
    private var sessionPollTimer: Timer?
    private var mixerPollTimer: Timer?

    // MARK: - ライフサイクル

    /// ランタイムを初期化し、バージョン情報を取得する
    func initialize() throws {
        let result = cplp_init()
        guard result == CPLP_RESULT_OK else {
            let msg = cplp_last_error().map { String(cString: $0) } ?? "unknown"
            throw CplpError.initFailed(msg)
        }
        let v = cplp_version()
        version = "\(v.major).\(v.minor).\(v.patch)"
        isInitialized = true
    }

    /// ランタイムを破棄する
    func shutdown() {
        stopMeterPolling()
        stopSessionPolling()
        stopMixerPolling()
        if isAudioRunning {
            cplp_audio_stop()
            isAudioRunning = false
        }
        if sessionStatus == CPLP_SESSION_STATUS_CONNECTED
            || sessionStatus == CPLP_SESSION_STATUS_CONNECTING
        {
            cplp_session_disconnect()
        }
        cplp_destroy()
        isInitialized = false
    }

    deinit {
        meterTask?.cancel()
    }

    // MARK: - オーディオ

    func startAudio() {
        let result = cplp_audio_start()
        if result == CPLP_RESULT_OK {
            isAudioRunning = true
            startMeterPolling()
        }
    }

    func stopAudio() {
        stopMeterPolling()
        let result = cplp_audio_stop()
        if result == CPLP_RESULT_OK {
            isAudioRunning = false
            meterLeft = 0
            meterRight = 0
        }
    }

    // MARK: - プラグインスキャン

    func scanPlugins() {
        let list = cplp_audio_scan_plugins()
        var entries: [PluginEntry] = []
        if let items = list.items {
            for i in 0..<Int(list.count) {
                let info = items[i]
                let id = info.id.map { String(cString: $0) } ?? ""
                let name = info.name.map { String(cString: $0) } ?? ""
                entries.append(PluginEntry(id: id, name: name))
            }
        }
        plugins = entries
        cplp_plugin_list_free(list)
    }

    // MARK: - セッション

    func sessionConnect(url: String) {
        url.withCString { ptr in
            let result = cplp_session_connect(ptr)
            if result == CPLP_RESULT_OK {
                startSessionPolling()
                refreshSessionState()
            }
        }
    }

    func sessionDisconnect() {
        let result = cplp_session_disconnect()
        if result == CPLP_RESULT_OK {
            stopSessionPolling()
            refreshSessionState()
        }
    }

    func refreshSessionState() {
        let state = cplp_session_get_state()
        sessionStatus = state.status
        peerCount = state.peer_count
        if let url = state.lobby_url {
            lobbyUrl = String(cString: url)
        } else {
            lobbyUrl = ""
        }
    }

    // MARK: - ミキサー

    func mixerSetVolume(peerId: String, volume: Float) {
        peerId.withCString { ptr in
            cplp_mixer_set_volume(ptr, volume)
        }
        refreshMixerState()
    }

    func mixerSetPan(peerId: String, pan: Float) {
        peerId.withCString { ptr in
            cplp_mixer_set_pan(ptr, pan)
        }
        refreshMixerState()
    }

    func mixerSetMute(peerId: String, mute: Bool) {
        peerId.withCString { ptr in
            cplp_mixer_set_mute(ptr, mute)
        }
        refreshMixerState()
    }

    func refreshMixerState() {
        let state = cplp_mixer_get_state()
        var newTracks: [TrackState] = []
        if let items = state.tracks {
            for i in 0..<Int(state.track_count) {
                let t = items[i]
                let peerId = t.peer_id.map { String(cString: $0) } ?? ""
                let label = t.label.map { String(cString: $0) } ?? ""
                newTracks.append(TrackState(
                    peerId: peerId,
                    label: label,
                    volume: t.volume,
                    pan: t.pan,
                    mute: t.mute,
                    solo: t.solo
                ))
            }
        }
        tracks = newTracks
        masterVolume = state.master_volume
        cplp_mixer_state_free(state)
    }

    // MARK: - メーターポーリング

    private func startMeterPolling() {
        meterTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            let meters = cplp_audio_get_meters()
            self.meterLeft = meters.left
            self.meterRight = meters.right
        }
    }

    private func stopMeterPolling() {
        meterTimer?.invalidate()
        meterTimer = nil
        meterTask?.cancel()
        meterTask = nil
    }

    // MARK: - セッションポーリング

    private func startSessionPolling() {
        sessionPollTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.refreshSessionState()
        }
    }

    private func stopSessionPolling() {
        sessionPollTimer?.invalidate()
        sessionPollTimer = nil
    }

    // MARK: - ミキサーポーリング

    func startMixerPolling() {
        refreshMixerState()
        mixerPollTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.refreshMixerState()
        }
    }

    func stopMixerPolling() {
        mixerPollTimer?.invalidate()
        mixerPollTimer = nil
    }
}

// MARK: - モデル型

struct TrackState: Identifiable {
    var id: String { peerId }
    let peerId: String
    let label: String
    var volume: Float
    var pan: Float
    var mute: Bool
    var solo: Bool
}

struct PluginEntry: Identifiable {
    let id: String
    let name: String
}

// MARK: - エラー型

enum CplpError: Error, LocalizedError {
    case initFailed(String)

    var errorDescription: String? {
        switch self {
        case .initFailed(let msg):
            return "cplp_init failed: \(msg)"
        }
    }
}
