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
