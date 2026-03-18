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
