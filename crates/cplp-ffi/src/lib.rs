//! cplp-ffi — Rust → C/Swift FFI ブリッジ
//!
//! macOS SwiftUI アプリから cplp-sound-system の機能を利用するための
//! C 互換インターフェース。cbindgen でヘッダーを自動生成する。

pub mod audio;
mod error;
pub mod midi;
mod mixer;
mod scene;
mod session;
pub mod types;

use std::sync::{Arc, Mutex, RwLock};

use cplp_audio::engine::AudioEngine;
use cplp_audio::plugin_host::{NoteController, NoteReceiver, note_channel};
use cplp_core::config::AppConfig;
use cplp_core::MixerState;
use types::{CplpResult, CplpSessionStatus, CplpVersion};

/// NoteReceiver を audio_start まで保持するグローバルストレージ
/// cplp_init で作成し、cplp_audio_start で take して audio callback に渡す
pub(crate) static MIDI_NOTE_RECV: std::sync::OnceLock<Mutex<Option<NoteReceiver>>> =
    std::sync::OnceLock::new();

fn midi_note_recv() -> &'static Mutex<Option<NoteReceiver>> {
    MIDI_NOTE_RECV.get_or_init(|| Mutex::new(None))
}

/// グローバルランタイム — Arc + RwLock で安全に管理
///
/// Arc により、runtime() で取得した参照が生存中は destroy で解放されない。
/// RwLock により、init/destroy（write）と runtime()（read）が安全に共存。
static RUNTIME: RwLock<Option<Arc<CplpRuntime>>> = RwLock::new(None);

/// cplp ランタイム（tokio + AudioEngine + 設定）
pub(crate) struct CplpRuntime {
    pub _tokio: tokio::runtime::Runtime,
    pub engine: Mutex<AudioEngine>,
    pub config: AppConfig,
    /// セッション状態
    pub session: Mutex<SessionState>,
    /// ミキサー状態（cplp-core の MixerState を保持）
    pub mixer: Mutex<MixerState>,
    /// MIDI ノートコントローラ（Swift → Rust の MIDI 入力用）
    pub note_ctrl: Mutex<NoteController>,
}

/// FFI 側で管理するセッション状態
pub(crate) struct SessionState {
    pub status: CplpSessionStatus,
    pub lobby_url: Option<String>,
    pub peer_count: u32,
}

// SAFETY: AudioEngine 内の cpal::Stream は !Send だが、
// FFI 境界では init/destroy/audio_start/stop はすべて
// Swift メインスレッドから呼ばれることが前提。
// engine は Mutex で排他制御されている。
// tokio Runtime は Send + Sync。
unsafe impl Send for CplpRuntime {}
unsafe impl Sync for CplpRuntime {}

/// ランタイムへの Arc 参照を取得（内部ヘルパー）
///
/// Arc::clone を返すため、呼び出し側が保持している間は destroy されない。
pub(crate) fn runtime() -> Option<Arc<CplpRuntime>> {
    RUNTIME.read().ok()?.clone()
}

/// ランタイムを初期化する
///
/// アプリ起動時に一度だけ呼ぶこと。二重初期化はエラーを返す。
#[unsafe(no_mangle)]
pub extern "C" fn cplp_init() -> CplpResult {
    // tracing 初期化
    let _ = tracing_subscriber::fmt()
        .with_env_filter("cplp=debug")
        .try_init();

    tracing::info!("cplp_init: ランタイムを初期化");

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return error::set_error(
                CplpResult::InitError,
                format!("tokio Runtime 作成失敗: {e}"),
            );
        }
    };

    let config = AppConfig::default();
    let engine = AudioEngine::new(config.audio.clone());

    let (note_ctrl, note_recv) = note_channel(256);
    // NoteReceiver をグローバルに保持（audio_start で使用）
    {
        let mut guard = midi_note_recv().lock().unwrap();
        *guard = Some(note_recv);
    }

    let runtime = Arc::new(CplpRuntime {
        _tokio: rt,
        engine: Mutex::new(engine),
        config,
        session: Mutex::new(SessionState {
            status: CplpSessionStatus::Disconnected,
            lobby_url: None,
            peer_count: 0,
        }),
        mixer: Mutex::new(MixerState::new()),
        note_ctrl: Mutex::new(note_ctrl),
    });

    // RwLock write で排他的にアクセス — TOCTOU 不可能
    match RUNTIME.write() {
        Ok(mut guard) => {
            if guard.is_some() {
                return error::set_error(CplpResult::InitError, "cplp_init: 既に初期化済み");
            }
            *guard = Some(runtime);
            tracing::info!("cplp_init: 完了");
            CplpResult::Ok
        }
        Err(e) => error::set_error(
            CplpResult::InternalError,
            format!("RwLock poisoned: {e}"),
        ),
    }
}

/// ランタイムを破棄する
///
/// アプリ終了時に呼ぶこと。Arc の参照カウントが 0 になった時点で実際に解放される。
#[unsafe(no_mangle)]
pub extern "C" fn cplp_destroy() -> CplpResult {
    tracing::info!("cplp_destroy: ランタイムを破棄");

    match RUNTIME.write() {
        Ok(mut guard) => {
            if guard.take().is_none() {
                return error::set_error(
                    CplpResult::NotInitialized,
                    "cplp_destroy: 未初期化",
                );
            }
            tracing::info!("cplp_destroy: 完了");
            CplpResult::Ok
        }
        Err(e) => error::set_error(
            CplpResult::InternalError,
            format!("RwLock poisoned: {e}"),
        ),
    }
}

/// バージョン情報を取得
#[unsafe(no_mangle)]
pub extern "C" fn cplp_version() -> CplpVersion {
    CplpVersion {
        major: 0,
        minor: 1,
        patch: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// テストごとにグローバル状態をクリーンアップするヘルパー
    fn cleanup_runtime() {
        if let Ok(mut guard) = RUNTIME.write() {
            *guard = None;
        }
    }

    #[test]
    #[serial]
    fn test_init_and_destroy_lifecycle() {
        cleanup_runtime();

        // 初期化 → 成功
        let result = cplp_init();
        assert!(matches!(result, CplpResult::Ok));

        // 破棄 → 成功
        let result = cplp_destroy();
        assert!(matches!(result, CplpResult::Ok));
    }

    #[test]
    #[serial]
    fn test_double_init_returns_error() {
        cleanup_runtime();

        let result = cplp_init();
        assert!(matches!(result, CplpResult::Ok));

        // 二重初期化 → InitError
        let result = cplp_init();
        assert!(matches!(result, CplpResult::InitError));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn test_destroy_without_init_returns_not_initialized() {
        cleanup_runtime();

        // 未初期化で destroy → NotInitialized
        let result = cplp_destroy();
        assert!(matches!(result, CplpResult::NotInitialized));
    }

    #[test]
    #[serial]
    fn test_double_destroy_returns_not_initialized() {
        cleanup_runtime();

        let result = cplp_init();
        assert!(matches!(result, CplpResult::Ok));

        let result = cplp_destroy();
        assert!(matches!(result, CplpResult::Ok));

        // 二重 destroy → NotInitialized
        let result = cplp_destroy();
        assert!(matches!(result, CplpResult::NotInitialized));
    }

    #[test]
    #[serial]
    fn test_version_returns_correct_values() {
        let v = cplp_version();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 0);
    }

    #[test]
    #[serial]
    fn test_runtime_helper_returns_none_when_not_initialized() {
        cleanup_runtime();

        assert!(runtime().is_none());
    }

    #[test]
    #[serial]
    fn test_runtime_helper_returns_some_after_init() {
        cleanup_runtime();

        cplp_init();
        assert!(runtime().is_some());

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn test_arc_reference_survives_destroy() {
        cleanup_runtime();

        cplp_init();

        // Arc 参照を取得
        let arc = runtime().expect("runtime should be Some");
        let strong_before = Arc::strong_count(&arc);
        assert!(strong_before >= 2); // RUNTIME + arc

        // destroy でグローバルを None に
        cplp_destroy();

        // runtime() は None だが、保持した Arc は有効
        assert!(runtime().is_none());
        assert_eq!(Arc::strong_count(&arc), 1); // arc だけが保持

        // Arc 経由で config にアクセスできることを確認（UAF でないことの証明）
        let _ = &arc.config;

        drop(arc);
    }

    #[test]
    #[serial]
    fn test_init_destroy_init_cycle() {
        cleanup_runtime();

        // init → destroy → init の再初期化サイクルが正常動作すること
        assert!(matches!(cplp_init(), CplpResult::Ok));
        assert!(matches!(cplp_destroy(), CplpResult::Ok));
        assert!(matches!(cplp_init(), CplpResult::Ok));

        cleanup_runtime();
    }
}
