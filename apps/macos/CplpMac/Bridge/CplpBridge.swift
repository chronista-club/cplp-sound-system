import Foundation
import Combine

/// Rust FFI ラッパー — SwiftUI の ObservableObject として公開
@MainActor
class CplpBridge: ObservableObject {
    @Published private(set) var isInitialized: Bool = false
    @Published private(set) var isAudioRunning: Bool = false
    @Published private(set) var version: String = ""
    @Published private(set) var meterLeft: Float = 0
    @Published private(set) var meterRight: Float = 0

    private var meterTimer: Timer?
    /// Timer 内の Task をキャンセルするためのハンドル
    private var meterTask: Task<Void, Never>?

    init() {
        let v = cplp_version()
        version = "\(v.major).\(v.minor).\(v.patch)"
        isInitialized = true
    }

    deinit {
        // deinit は nonisolated。@MainActor プロパティには直アクセスできない。
        // Timer は ARC 解放時に自動 invalidate されるが、明示的に停止。
        // meterTask はキャンセルして、destroy 後の FFI 呼び出しを防ぐ。
        meterTask?.cancel()
        cplp_audio_stop()
        cplp_destroy()
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

    // MARK: - メーターポーリング

    private func startMeterPolling() {
        // Timer はメインランループで発火 → @MainActor 内なので安全
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
}
