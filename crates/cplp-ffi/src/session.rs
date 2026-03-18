use std::ffi::{CStr, CString};

use cplp_core::{MixerState, PeerId, TrackState};

use crate::error;
use crate::runtime;
use crate::types::{CplpResult, CplpSessionState, CplpSessionStatus};

// セッション状態を保持する thread_local CString（lobby_url のライフタイム管理）
//
// cplp_session_get_state が返す lobby_url ポインタは、次の FFI 呼び出しまで有効。
// thread_local に保持することでダングリングポインタを防ぐ。
thread_local! {
    static LOBBY_URL_CSTR: std::cell::RefCell<Option<CString>> = const { std::cell::RefCell::new(None) };
}

/// セッションに接続
///
/// # Safety
/// `lobby_url` は有効な null 終端 C 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_session_connect(lobby_url: *const std::ffi::c_char) -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };

    // lobby_url のバリデーション
    if lobby_url.is_null() {
        return error::set_error(
            CplpResult::InvalidArgument,
            "cplp_session_connect: lobby_url が null",
        );
    }

    let url_str = match unsafe { CStr::from_ptr(lobby_url) }.to_str() {
        Ok(s) => s.to_string(),
        Err(e) => {
            return error::set_error(
                CplpResult::InvalidArgument,
                format!("cplp_session_connect: lobby_url が不正な UTF-8: {e}"),
            );
        }
    };

    if url_str.is_empty() {
        return error::set_error(
            CplpResult::InvalidArgument,
            "cplp_session_connect: lobby_url が空",
        );
    }

    tracing::info!("cplp_session_connect: url={url_str}");

    // セッション状態を更新
    let Ok(mut session) = rt.session.lock() else {
        return error::set_error(CplpResult::InternalError, "session Mutex poisoned");
    };

    if session.status == CplpSessionStatus::Connected
        || session.status == CplpSessionStatus::Connecting
    {
        return error::set_error(
            CplpResult::SessionError,
            "cplp_session_connect: 既に接続中または接続済み",
        );
    }

    // Connecting → Connected（現時点ではネットワーク接続を同期的に完了とする）
    // TODO: 実際の WebSocket/WebRTC 接続はここで tokio spawn する
    session.status = CplpSessionStatus::Connecting;
    session.lobby_url = Some(url_str);
    session.peer_count = 1; // 自分自身

    // ミキサーにローカルトラックを追加
    if let Ok(mut mixer) = rt.mixer.lock() {
        let local_id = PeerId::new("local");
        *mixer = MixerState::new();
        mixer.add_track(local_id, TrackState::new("Me"));
    }

    // 接続完了（stub: 即座に Connected にする）
    session.status = CplpSessionStatus::Connected;

    tracing::info!(
        "cplp_session_connect: 完了 (peers={})",
        session.peer_count
    );
    CplpResult::Ok
}

/// セッションから切断
#[unsafe(no_mangle)]
pub extern "C" fn cplp_session_disconnect() -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };

    let Ok(mut session) = rt.session.lock() else {
        return error::set_error(CplpResult::InternalError, "session Mutex poisoned");
    };

    if session.status == CplpSessionStatus::Disconnected {
        return error::set_error(
            CplpResult::SessionError,
            "cplp_session_disconnect: 既に切断済み",
        );
    }

    tracing::info!("cplp_session_disconnect");

    session.status = CplpSessionStatus::Disconnecting;

    // TODO: 実際のネットワーク切断処理

    // ミキサー状態をクリア
    if let Ok(mut mixer) = rt.mixer.lock() {
        *mixer = MixerState::new();
    }

    session.status = CplpSessionStatus::Disconnected;
    session.lobby_url = None;
    session.peer_count = 0;

    tracing::info!("cplp_session_disconnect: 完了");
    CplpResult::Ok
}

/// セッション状態を取得
///
/// 返される `CplpSessionState` 内の `lobby_url` ポインタは、
/// 同一スレッド上の次の FFI 呼び出しまで有効。
#[unsafe(no_mangle)]
pub extern "C" fn cplp_session_get_state() -> CplpSessionState {
    let rt = match runtime() {
        Some(rt) => rt,
        None => {
            return CplpSessionState {
                status: CplpSessionStatus::Disconnected,
                peer_count: 0,
                lobby_url: std::ptr::null(),
            };
        }
    };

    let Ok(session) = rt.session.lock() else {
        return CplpSessionState {
            status: CplpSessionStatus::Disconnected,
            peer_count: 0,
            lobby_url: std::ptr::null(),
        };
    };

    // lobby_url を thread_local の CString に保存して、安定したポインタを返す
    let url_ptr = match &session.lobby_url {
        Some(url) => match CString::new(url.as_str()) {
            Ok(cstr) => {
                LOBBY_URL_CSTR.with(|cell| {
                    *cell.borrow_mut() = Some(cstr);
                });
                // thread_local に保存した CString のポインタを返す
                LOBBY_URL_CSTR.with(|cell| {
                    cell.borrow()
                        .as_ref()
                        .map(|c| c.as_ptr())
                        .unwrap_or(std::ptr::null())
                })
            }
            Err(_) => std::ptr::null(),
        },
        None => std::ptr::null(),
    };

    CplpSessionState {
        status: session.status,
        peer_count: session.peer_count,
        lobby_url: url_ptr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cplp_init, RUNTIME};
    use serial_test::serial;
    use std::ffi::CString;

    /// テストごとにグローバル状態をクリーンアップするヘルパー
    fn cleanup_runtime() {
        if let Ok(mut guard) = RUNTIME.write() {
            *guard = None;
        }
    }

    /// init + connect のヘルパー（接続済み状態を作る）
    fn init_and_connect() {
        assert!(matches!(cplp_init(), CplpResult::Ok));
        let url = CString::new("ws://localhost:8080").unwrap();
        let result = unsafe { cplp_session_connect(url.as_ptr()) };
        assert!(matches!(result, CplpResult::Ok));
    }

    #[test]
    #[serial]
    fn session_connect_without_init_returns_not_initialized() {
        cleanup_runtime();

        let url = CString::new("ws://localhost:8080").unwrap();
        let result = unsafe { cplp_session_connect(url.as_ptr()) };
        assert!(matches!(result, CplpResult::NotInitialized));
    }

    #[test]
    #[serial]
    fn session_connect_null_url_returns_invalid_argument() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        let result = unsafe { cplp_session_connect(std::ptr::null()) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_connect_empty_url_returns_invalid_argument() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        let url = CString::new("").unwrap();
        let result = unsafe { cplp_session_connect(url.as_ptr()) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_connect_valid_url_returns_ok() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        let url = CString::new("ws://localhost:8080").unwrap();
        let result = unsafe { cplp_session_connect(url.as_ptr()) };
        assert!(matches!(result, CplpResult::Ok));

        // 状態が Connected に変わっていること
        let state = cplp_session_get_state();
        assert_eq!(state.status, CplpSessionStatus::Connected);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_connect_already_connected_returns_session_error() {
        cleanup_runtime();
        init_and_connect();

        let url = CString::new("ws://localhost:9999").unwrap();
        let result = unsafe { cplp_session_connect(url.as_ptr()) };
        assert!(matches!(result, CplpResult::SessionError));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_connect_sets_lobby_url_in_state() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        let url_str = "ws://example.com/lobby";
        let url = CString::new(url_str).unwrap();
        let result = unsafe { cplp_session_connect(url.as_ptr()) };
        assert!(matches!(result, CplpResult::Ok));

        let state = cplp_session_get_state();
        assert!(!state.lobby_url.is_null());
        let returned_url = unsafe { CStr::from_ptr(state.lobby_url) }.to_str().unwrap();
        assert_eq!(returned_url, url_str);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_connect_initializes_local_track_in_mixer() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        let url = CString::new("ws://localhost:8080").unwrap();
        let result = unsafe { cplp_session_connect(url.as_ptr()) };
        assert!(matches!(result, CplpResult::Ok));

        // ミキサーに "local" トラックが追加されていることを確認
        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = cplp_core::PeerId::new("local");
        assert!(mixer.tracks.contains_key(&local_id));
        assert_eq!(mixer.tracks[&local_id].label, "Me");

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_disconnect_without_init_returns_not_initialized() {
        cleanup_runtime();

        let result = cplp_session_disconnect();
        assert!(matches!(result, CplpResult::NotInitialized));
    }

    #[test]
    #[serial]
    fn session_disconnect_when_already_disconnected_returns_session_error() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        // 初期状態は Disconnected なので、disconnect すると SessionError
        let result = cplp_session_disconnect();
        assert!(matches!(result, CplpResult::SessionError));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_disconnect_clears_lobby_url() {
        cleanup_runtime();
        init_and_connect();

        let result = cplp_session_disconnect();
        assert!(matches!(result, CplpResult::Ok));

        let state = cplp_session_get_state();
        assert!(state.lobby_url.is_null());

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_disconnect_clears_mixer() {
        cleanup_runtime();
        init_and_connect();

        let result = cplp_session_disconnect();
        assert!(matches!(result, CplpResult::Ok));

        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        assert!(mixer.tracks.is_empty());

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_connect_disconnect_connect_cycle() {
        cleanup_runtime();

        assert!(matches!(cplp_init(), CplpResult::Ok));

        // 1st connect
        let url = CString::new("ws://localhost:8080").unwrap();
        assert!(matches!(
            unsafe { cplp_session_connect(url.as_ptr()) },
            CplpResult::Ok
        ));

        // disconnect
        assert!(matches!(cplp_session_disconnect(), CplpResult::Ok));

        // 2nd connect
        let url2 = CString::new("ws://localhost:9090").unwrap();
        assert!(matches!(
            unsafe { cplp_session_connect(url2.as_ptr()) },
            CplpResult::Ok
        ));

        let state = cplp_session_get_state();
        assert_eq!(state.status, CplpSessionStatus::Connected);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn session_get_state_without_init_returns_disconnected() {
        cleanup_runtime();

        let state = cplp_session_get_state();
        assert_eq!(state.status, CplpSessionStatus::Disconnected);
        assert_eq!(state.peer_count, 0);
        assert!(state.lobby_url.is_null());
    }

    #[test]
    #[serial]
    fn session_get_state_lobby_url_pointer_is_valid_cstring() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        let url_str = "ws://test.example.com:1234/room";
        let url = CString::new(url_str).unwrap();
        assert!(matches!(
            unsafe { cplp_session_connect(url.as_ptr()) },
            CplpResult::Ok
        ));

        let state = cplp_session_get_state();
        assert!(!state.lobby_url.is_null());

        // ポインタが有効な C 文字列であること
        let c_str = unsafe { CStr::from_ptr(state.lobby_url) };
        let rust_str = c_str.to_str().unwrap();
        assert_eq!(rust_str, url_str);

        cleanup_runtime();
    }
}
