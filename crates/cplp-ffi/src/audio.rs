use std::sync::atomic::{AtomicU8, Ordering};

use atomic_float::AtomicF32;

use crate::runtime;
use crate::types::CplpResult;

/// オーディオエンジンの状態
#[repr(u8)]
enum AudioState {
    Stopped = 0,
    Running = 1,
}

/// オーディオメーター値（lock-free、Swift 側から 30-60fps でポーリング）
static METER_LEFT: AtomicF32 = AtomicF32::new(0.0);
static METER_RIGHT: AtomicF32 = AtomicF32::new(0.0);
static AUDIO_STATE: AtomicU8 = AtomicU8::new(0);

/// ステレオメーター値（FFI 転送用）
#[repr(C)]
pub struct CplpAudioMeters {
    pub left: f32,
    pub right: f32,
}

/// プラグインスキャン結果（1 件分）
#[repr(C)]
pub struct CplpPluginInfo {
    /// プラグイン ID（C 文字列、呼び出し側で free 不要）
    pub id: *const std::ffi::c_char,
    /// プラグイン名（C 文字列）
    pub name: *const std::ffi::c_char,
}

/// プラグインスキャン結果リスト
#[repr(C)]
pub struct CplpPluginList {
    pub items: *mut CplpPluginInfo,
    pub count: u32,
}

/// オーディオエンジンを開始（デフォルト設定でサイン波テスト出力）
#[unsafe(no_mangle)]
pub extern "C" fn cplp_audio_start() -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };

    // NoteReceiver を engine lock の前に take（ロック順序の一貫性を保つ）
    let note_recv = crate::midi_note_recv().lock().unwrap().take();

    // Mutex 内で二重起動チェック + start を原子的に行う（TOCTOU 防止）
    match rt.engine.lock() {
        Ok(mut engine) => {
            if AUDIO_STATE.load(Ordering::Acquire) == AudioState::Running as u8 {
                tracing::warn!("cplp_audio_start: 既に実行中");
                return CplpResult::Ok;
            }

            let sample_rate = rt.config.audio.sample_rate as f32;
            let mut note_recv = note_recv;

            // RT-safe 固定サイズポリフォニックシンセ
            const MAX_VOICES: usize = 16;
            let mut voices = [(0u8, 0.0f32, 0.0f32, false); MAX_VOICES];

            let source = move |data: &mut [f32]| {
                // MIDI イベントを drain
                if let Some(ref mut recv) = note_recv {
                    loop {
                        match recv.try_pop() {
                            Some(evt) => {
                                let status = evt.status & 0xF0;
                                let key = evt.key;
                                let vel = evt.velocity;
                                if status == 0x90 && vel > 0 {
                                    let gain = vel as f32 / 127.0;
                                    let mut found = false;
                                    for v in voices.iter_mut() {
                                        if v.3 && v.0 == key {
                                            v.2 = gain;
                                            found = true;
                                            break;
                                        }
                                    }
                                    if !found {
                                        for v in voices.iter_mut() {
                                            if !v.3 {
                                                *v = (key, 0.0, gain, true);
                                                break;
                                            }
                                        }
                                    }
                                } else if status == 0x80 || (status == 0x90 && vel == 0) {
                                    for v in voices.iter_mut() {
                                        if v.3 && v.0 == key {
                                            v.3 = false;
                                            break;
                                        }
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }

                let mut peak_l: f32 = 0.0;
                let mut peak_r: f32 = 0.0;

                for frame in data.chunks_exact_mut(2) {
                    let mut mix = 0.0_f32;
                    for voice in voices.iter_mut() {
                        if voice.3 {
                            let freq =
                                440.0 * (2.0_f32).powf((voice.0 as f32 - 69.0) / 12.0);
                            let s = (voice.1 * std::f32::consts::TAU).sin() * voice.2 * 0.2;
                            voice.1 += freq / sample_rate;
                            if voice.1 >= 1.0 {
                                voice.1 -= 1.0;
                            }
                            mix += s;
                        }
                    }
                    frame[0] = mix;
                    frame[1] = mix;
                    peak_l = peak_l.max(mix.abs());
                    peak_r = peak_r.max(mix.abs());
                }

                METER_LEFT.store(peak_l, Ordering::Release);
                METER_RIGHT.store(peak_r, Ordering::Release);
            };

            if let Err(e) = engine.start(source) {
                return crate::error::set_error(
                    CplpResult::AudioError,
                    format!("AudioEngine start 失敗: {e}"),
                );
            }
            AUDIO_STATE.store(AudioState::Running as u8, Ordering::Release);
            tracing::info!("cplp_audio_start: 完了");
            CplpResult::Ok
        }
        Err(e) => crate::error::set_error(
            CplpResult::InternalError,
            format!("Mutex poisoned: {e}"),
        ),
    }
}

/// オーディオエンジンを停止
#[unsafe(no_mangle)]
pub extern "C" fn cplp_audio_stop() -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };

    match rt.engine.lock() {
        Ok(mut engine) => {
            engine.stop();
            AUDIO_STATE.store(AudioState::Stopped as u8, Ordering::Relaxed);
            METER_LEFT.store(0.0, Ordering::Relaxed);
            METER_RIGHT.store(0.0, Ordering::Relaxed);
            tracing::info!("cplp_audio_stop: 完了");
            CplpResult::Ok
        }
        Err(e) => crate::error::set_error(
            CplpResult::InternalError,
            format!("Mutex poisoned: {e}"),
        ),
    }
}

/// メーター値を取得（lock-free ポーリング）
#[unsafe(no_mangle)]
pub extern "C" fn cplp_audio_get_meters() -> CplpAudioMeters {
    CplpAudioMeters {
        left: METER_LEFT.load(Ordering::Acquire),
        right: METER_RIGHT.load(Ordering::Acquire),
    }
}

/// オーディオが実行中かどうか
#[unsafe(no_mangle)]
pub extern "C" fn cplp_audio_is_running() -> bool {
    AUDIO_STATE.load(Ordering::Relaxed) == AudioState::Running as u8
}

/// CLAP プラグインをスキャン
///
/// 戻り値のリストは `cplp_plugin_list_free` で解放すること。
#[unsafe(no_mangle)]
pub extern "C" fn cplp_audio_scan_plugins() -> CplpPluginList {
    let plugins = cplp_audio::plugin_host::scan_plugins();

    let mut items: Vec<CplpPluginInfo> = plugins
        .into_iter()
        .filter_map(|p| {
            let id = std::ffi::CString::new(p.id).ok()?;
            let name = std::ffi::CString::new(p.name).ok()?;
            Some(CplpPluginInfo {
                id: id.into_raw(),
                name: name.into_raw(),
            })
        })
        .collect();

    items.shrink_to_fit(); // capacity == len を保証（Vec::from_raw_parts の安全条件）
    let count = items.len() as u32;
    let ptr = items.as_mut_ptr();
    std::mem::forget(items);

    CplpPluginList {
        items: ptr,
        count,
    }
}

/// プラグインリストを解放
///
/// # Safety
/// `cplp_audio_scan_plugins` で返されたリストのみ渡すこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_plugin_list_free(list: CplpPluginList) {
    if list.items.is_null() {
        return;
    }
    let items = unsafe { Vec::from_raw_parts(list.items, list.count as usize, list.count as usize) };
    for item in items {
        if !item.id.is_null() {
            drop(unsafe { std::ffi::CString::from_raw(item.id as *mut _) });
        }
        if !item.name.is_null() {
            drop(unsafe { std::ffi::CString::from_raw(item.name as *mut _) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// テストごとにグローバル状態をクリーンアップ
    fn cleanup() {
        // RUNTIME をクリア
        if let Ok(mut guard) = crate::RUNTIME.write() {
            *guard = None;
        }
        // AUDIO_STATE をリセット
        AUDIO_STATE.store(AudioState::Stopped as u8, Ordering::Relaxed);
        METER_LEFT.store(0.0, Ordering::Relaxed);
        METER_RIGHT.store(0.0, Ordering::Relaxed);
    }

    #[test]
    #[serial]
    fn test_audio_start_without_init_returns_not_initialized() {
        cleanup();

        let result = cplp_audio_start();
        assert!(matches!(result, CplpResult::NotInitialized));
    }

    #[test]
    #[serial]
    fn test_audio_stop_without_init_returns_not_initialized() {
        cleanup();

        let result = cplp_audio_stop();
        assert!(matches!(result, CplpResult::NotInitialized));
    }

    #[test]
    #[serial]
    fn test_audio_is_running_default_false() {
        cleanup();

        assert!(!cplp_audio_is_running());
    }

    #[test]
    #[serial]
    fn test_audio_meters_default_zero() {
        cleanup();

        let meters = cplp_audio_get_meters();
        assert_eq!(meters.left, 0.0);
        assert_eq!(meters.right, 0.0);
    }

    #[test]
    #[serial]
    fn test_scan_plugins_returns_list() {
        // scan_plugins はランタイム不要（ファイルシステムスキャン）
        let list = cplp_audio_scan_plugins();

        // プラグインが見つかるかは環境依存だが、パニックしないこと
        // count が 0 の場合でも list_free が安全であること
        unsafe { cplp_plugin_list_free(list) };
    }

    #[test]
    #[serial]
    fn test_plugin_list_free_null_items_is_safe() {
        // null ポインタの CplpPluginList を free してもパニックしないこと
        let list = CplpPluginList {
            items: std::ptr::null_mut(),
            count: 0,
        };
        unsafe { cplp_plugin_list_free(list) };
    }

    #[test]
    #[serial]
    fn test_plugin_list_free_empty_list_is_safe() {
        // count=0、有効ポインタの空リストを free してもパニックしないこと
        let mut items: Vec<CplpPluginInfo> = Vec::new();
        items.shrink_to_fit();
        let list = CplpPluginList {
            items: items.as_mut_ptr(),
            count: 0,
        };
        std::mem::forget(items);
        unsafe { cplp_plugin_list_free(list) };
    }

    #[test]
    #[serial]
    fn test_scan_plugins_and_verify_strings() {
        let list = cplp_audio_scan_plugins();

        // 各プラグインの id/name が有効な C 文字列であることを確認
        if list.count > 0 && !list.items.is_null() {
            for i in 0..list.count as usize {
                let item = unsafe { &*list.items.add(i) };
                if !item.id.is_null() {
                    let _ = unsafe { std::ffi::CStr::from_ptr(item.id) }
                        .to_str()
                        .expect("plugin id should be valid UTF-8");
                }
                if !item.name.is_null() {
                    let _ = unsafe { std::ffi::CStr::from_ptr(item.name) }
                        .to_str()
                        .expect("plugin name should be valid UTF-8");
                }
            }
        }

        unsafe { cplp_plugin_list_free(list) };
    }
}
