use std::ffi::{CStr, CString};

use cplp_core::PeerId;

use crate::error;
use crate::runtime;
use crate::types::{CplpMixerState, CplpResult, CplpSessionStatus, CplpTrackInfo};

/// ミキサー操作の現在タイムスタンプを取得（LWW 用）
fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// peer_id の C 文字列を Rust の PeerId に変換するヘルパー
///
/// # Safety
/// `peer_id` は有効な null 終端 C 文字列であること。
unsafe fn parse_peer_id(peer_id: *const std::ffi::c_char) -> Result<PeerId, CplpResult> {
    if peer_id.is_null() {
        return Err(error::set_error(
            CplpResult::InvalidArgument,
            "peer_id が null",
        ));
    }
    let s = unsafe { CStr::from_ptr(peer_id) }
        .to_str()
        .map_err(|e| {
            error::set_error(
                CplpResult::InvalidArgument,
                format!("peer_id が不正な UTF-8: {e}"),
            )
        })?;
    Ok(PeerId::new(s))
}

/// セッション接続チェック（ミキサー操作の前提条件）
fn require_connected() -> Result<(), CplpResult> {
    let rt = runtime().ok_or(CplpResult::NotInitialized)?;
    let session = rt
        .session
        .lock()
        .map_err(|_| error::set_error(CplpResult::InternalError, "session Mutex poisoned"))?;
    if session.status != CplpSessionStatus::Connected {
        return Err(error::set_error(
            CplpResult::SessionError,
            "セッション未接続",
        ));
    }
    Ok(())
}

/// トラックのボリュームを設定
///
/// # Safety
/// `peer_id` は有効な null 終端 C 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_mixer_set_volume(
    peer_id: *const std::ffi::c_char,
    volume: f32,
) -> CplpResult {
    if let Err(r) = require_connected() {
        return r;
    }

    let pid = match unsafe { parse_peer_id(peer_id) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    if !(0.0..=1.0).contains(&volume) {
        return error::set_error(
            CplpResult::InvalidArgument,
            format!("volume が範囲外: {volume} (0.0–1.0)"),
        );
    }

    let rt = runtime().unwrap();
    let Ok(mut mixer) = rt.mixer.lock() else {
        return error::set_error(CplpResult::InternalError, "mixer Mutex poisoned");
    };

    if !mixer.tracks.contains_key(&pid) {
        return error::set_error(
            CplpResult::InvalidArgument,
            format!("トラックが存在しません: {pid}"),
        );
    }

    mixer.apply_fader(&pid, volume, now_ts());
    tracing::debug!("cplp_mixer_set_volume: {pid} → {volume}");
    CplpResult::Ok
}

/// トラックのパンを設定
///
/// # Safety
/// `peer_id` は有効な null 終端 C 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_mixer_set_pan(
    peer_id: *const std::ffi::c_char,
    pan: f32,
) -> CplpResult {
    if let Err(r) = require_connected() {
        return r;
    }

    let pid = match unsafe { parse_peer_id(peer_id) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    if !(-1.0..=1.0).contains(&pan) {
        return error::set_error(
            CplpResult::InvalidArgument,
            format!("pan が範囲外: {pan} (-1.0–1.0)"),
        );
    }

    let rt = runtime().unwrap();
    let Ok(mut mixer) = rt.mixer.lock() else {
        return error::set_error(CplpResult::InternalError, "mixer Mutex poisoned");
    };

    if !mixer.tracks.contains_key(&pid) {
        return error::set_error(
            CplpResult::InvalidArgument,
            format!("トラックが存在しません: {pid}"),
        );
    }

    mixer.apply_pan(&pid, pan, now_ts());
    tracing::debug!("cplp_mixer_set_pan: {pid} → {pan}");
    CplpResult::Ok
}

/// トラックのミュートを設定
///
/// # Safety
/// `peer_id` は有効な null 終端 C 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_mixer_set_mute(
    peer_id: *const std::ffi::c_char,
    mute: bool,
) -> CplpResult {
    if let Err(r) = require_connected() {
        return r;
    }

    let pid = match unsafe { parse_peer_id(peer_id) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let rt = runtime().unwrap();
    let Ok(mut mixer) = rt.mixer.lock() else {
        return error::set_error(CplpResult::InternalError, "mixer Mutex poisoned");
    };

    if !mixer.tracks.contains_key(&pid) {
        return error::set_error(
            CplpResult::InvalidArgument,
            format!("トラックが存在しません: {pid}"),
        );
    }

    mixer.apply_mute(&pid, mute, now_ts());
    tracing::debug!("cplp_mixer_set_mute: {pid} → {mute}");
    CplpResult::Ok
}

/// ミキサー状態を取得
///
/// 戻り値は `cplp_mixer_state_free` で解放すること。
#[unsafe(no_mangle)]
pub extern "C" fn cplp_mixer_get_state() -> CplpMixerState {
    let empty = CplpMixerState {
        tracks: std::ptr::null_mut(),
        track_count: 0,
        master_volume: 1.0,
    };

    let Some(rt) = runtime() else {
        return empty;
    };

    let Ok(mixer) = rt.mixer.lock() else {
        return empty;
    };

    let mut items: Vec<CplpTrackInfo> = mixer
        .tracks
        .iter()
        .filter_map(|(peer_id, t)| {
            let peer_id_c = CString::new(peer_id.0.as_str()).ok()?;
            let label_c = CString::new(t.label.as_str()).ok()?;
            Some(CplpTrackInfo {
                peer_id: peer_id_c.into_raw(),
                label: label_c.into_raw(),
                volume: t.volume,
                pan: t.pan,
                mute: t.mute,
                solo: t.solo,
            })
        })
        .collect();

    items.shrink_to_fit();
    let count = items.len() as u32;
    let ptr = items.as_mut_ptr();
    std::mem::forget(items);

    CplpMixerState {
        tracks: ptr,
        track_count: count,
        master_volume: mixer.master_volume,
    }
}

/// ミキサー状態を解放
///
/// # Safety
/// `cplp_mixer_get_state` で返された値のみ渡すこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_mixer_state_free(state: CplpMixerState) {
    if state.tracks.is_null() {
        return;
    }
    let items = unsafe {
        Vec::from_raw_parts(
            state.tracks,
            state.track_count as usize,
            state.track_count as usize,
        )
    };
    for item in items {
        if !item.peer_id.is_null() {
            drop(unsafe { CString::from_raw(item.peer_id as *mut _) });
        }
        if !item.label.is_null() {
            drop(unsafe { CString::from_raw(item.label as *mut _) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::cplp_session_connect;
    use crate::types::CplpResult;
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
    fn mixer_set_volume_without_session_returns_session_error() {
        cleanup_runtime();
        assert!(matches!(cplp_init(), CplpResult::Ok));

        // 未接続状態でミキサー操作
        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_volume(peer.as_ptr(), 0.5) };
        assert!(matches!(result, CplpResult::SessionError));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_volume_null_peer_id_returns_invalid_argument() {
        cleanup_runtime();
        init_and_connect();

        let result = unsafe { cplp_mixer_set_volume(std::ptr::null(), 0.5) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_volume_nonexistent_peer_returns_invalid_argument() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("nonexistent-peer").unwrap();
        let result = unsafe { cplp_mixer_set_volume(peer.as_ptr(), 0.5) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_volume_out_of_range_high_returns_invalid_argument() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_volume(peer.as_ptr(), 1.5) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_volume_out_of_range_low_returns_invalid_argument() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_volume(peer.as_ptr(), -0.1) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_volume_valid_returns_ok() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_volume(peer.as_ptr(), 0.7) };
        assert!(matches!(result, CplpResult::Ok));

        // 実際に volume が変わっていることを確認
        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = PeerId::new("local");
        assert!((mixer.tracks[&local_id].volume - 0.7).abs() < f32::EPSILON);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_pan_out_of_range_returns_invalid_argument() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();

        // > 1.0
        let result = unsafe { cplp_mixer_set_pan(peer.as_ptr(), 1.5) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        // < -1.0
        let result = unsafe { cplp_mixer_set_pan(peer.as_ptr(), -1.5) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_pan_valid_returns_ok() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_pan(peer.as_ptr(), -0.5) };
        assert!(matches!(result, CplpResult::Ok));

        // 実際に pan が変わっていることを確認
        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = PeerId::new("local");
        assert!((mixer.tracks[&local_id].pan - (-0.5)).abs() < f32::EPSILON);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_mute_null_peer_returns_invalid_argument() {
        cleanup_runtime();
        init_and_connect();

        let result = unsafe { cplp_mixer_set_mute(std::ptr::null(), true) };
        assert!(matches!(result, CplpResult::InvalidArgument));

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_mute_valid_returns_ok() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_mute(peer.as_ptr(), true) };
        assert!(matches!(result, CplpResult::Ok));

        // 実際に mute が変わっていることを確認
        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = PeerId::new("local");
        assert!(mixer.tracks[&local_id].mute);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_get_state_without_init_returns_empty() {
        cleanup_runtime();

        let state = cplp_mixer_get_state();
        assert!(state.tracks.is_null());
        assert_eq!(state.track_count, 0);
    }

    #[test]
    #[serial]
    fn mixer_get_state_returns_tracks_after_connect() {
        cleanup_runtime();
        init_and_connect();

        let state = cplp_mixer_get_state();
        assert!(!state.tracks.is_null());
        assert_eq!(state.track_count, 1); // "local" トラック

        // トラック情報を検証
        let track = unsafe { &*state.tracks };
        assert!(!track.peer_id.is_null());
        let peer_str = unsafe { CStr::from_ptr(track.peer_id) }.to_str().unwrap();
        assert_eq!(peer_str, "local");

        assert!(!track.label.is_null());
        let label_str = unsafe { CStr::from_ptr(track.label) }.to_str().unwrap();
        assert_eq!(label_str, "Me");

        // 解放
        unsafe { cplp_mixer_state_free(state) };

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_state_free_null_tracks_is_safe() {
        // null tracks で呼んでもクラッシュしないこと
        let state = CplpMixerState {
            tracks: std::ptr::null_mut(),
            track_count: 0,
            master_volume: 1.0,
        };
        unsafe { cplp_mixer_state_free(state) };
        // パニックしなければ OK
    }

    #[test]
    #[serial]
    fn mixer_state_free_allocated_state_no_leak() {
        cleanup_runtime();
        init_and_connect();

        // get_state で確保されたメモリを free で解放
        let state = cplp_mixer_get_state();
        assert!(!state.tracks.is_null());
        assert!(state.track_count > 0);

        // free がパニックせず正常に完了すること（メモリリーク検証は miri に委譲）
        unsafe { cplp_mixer_state_free(state) };

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_volume_zero_is_valid() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_volume(peer.as_ptr(), 0.0) };
        assert!(matches!(result, CplpResult::Ok));

        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = PeerId::new("local");
        assert!((mixer.tracks[&local_id].volume - 0.0).abs() < f32::EPSILON);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_volume_one_is_valid() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_volume(peer.as_ptr(), 1.0) };
        assert!(matches!(result, CplpResult::Ok));

        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = PeerId::new("local");
        assert!((mixer.tracks[&local_id].volume - 1.0).abs() < f32::EPSILON);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_pan_minus_one_is_valid() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_pan(peer.as_ptr(), -1.0) };
        assert!(matches!(result, CplpResult::Ok));

        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = PeerId::new("local");
        assert!((mixer.tracks[&local_id].pan - (-1.0)).abs() < f32::EPSILON);

        cleanup_runtime();
    }

    #[test]
    #[serial]
    fn mixer_set_pan_plus_one_is_valid() {
        cleanup_runtime();
        init_and_connect();

        let peer = CString::new("local").unwrap();
        let result = unsafe { cplp_mixer_set_pan(peer.as_ptr(), 1.0) };
        assert!(matches!(result, CplpResult::Ok));

        let rt = crate::runtime().unwrap();
        let mixer = rt.mixer.lock().unwrap();
        let local_id = PeerId::new("local");
        assert!((mixer.tracks[&local_id].pan - 1.0).abs() < f32::EPSILON);

        cleanup_runtime();
    }
}
