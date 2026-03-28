//! MIDI FFI — Swift からの MIDI イベント入力

use crate::{runtime, types::CplpResult};

/// MIDI NoteOn を送信
///
/// Swift の CoreMIDI コールバックから呼ばれる。
/// MIDI 2.0 の高分解能ベロシティは呼び出し側で 7bit にスケールすること（暫定）。
#[unsafe(no_mangle)]
pub extern "C" fn cplp_midi_note_on(key: u8, velocity: u8) -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };
    match rt.note_ctrl.lock() {
        Ok(mut ctrl) => {
            ctrl.note_on(key, velocity);
            CplpResult::Ok
        }
        Err(_) => CplpResult::InternalError,
    }
}

/// MIDI NoteOff を送信
#[unsafe(no_mangle)]
pub extern "C" fn cplp_midi_note_off(key: u8) -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };
    match rt.note_ctrl.lock() {
        Ok(mut ctrl) => {
            ctrl.note_off(key);
            CplpResult::Ok
        }
        Err(_) => CplpResult::InternalError,
    }
}

/// MIDI ControlChange を送信
#[unsafe(no_mangle)]
pub extern "C" fn cplp_midi_cc(cc: u8, value: u8) -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };
    match rt.note_ctrl.lock() {
        Ok(mut ctrl) => {
            ctrl.control_change(cc, value);
            CplpResult::Ok
        }
        Err(_) => CplpResult::InternalError,
    }
}
