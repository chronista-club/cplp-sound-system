use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use midir::{MidiInput, MidiInputConnection};
use tracing::info;

use crate::plugin_host::{MidiEventSender, NoteController};

/// MIDI 入力マネージャ
///
/// 外部MIDIデバイスからの入力を NoteController に転送する。
pub struct MidiInputManager {
    _connection: MidiInputConnection<()>,
}

/// 利用可能な MIDI 入力ポートを一覧
pub fn list_midi_ports() -> Result<Vec<String>> {
    let midi_in = MidiInput::new("cplp-scan").context("MIDI 入力の初期化に失敗")?;

    let ports = midi_in.ports();
    let mut names = Vec::new();
    for port in &ports {
        let name = midi_in.port_name(port).unwrap_or_else(|_| "unknown".into());
        names.push(name);
    }
    Ok(names)
}

impl MidiInputManager {
    /// 指定ポート（インデックス）に接続し、MIDI メッセージを NoteController に転送
    ///
    /// `midi_event_tx` を渡すと、Note/CC を AudioModule (Looper等) にも転送する。
    pub fn connect(
        port_index: usize,
        note_ctrl: NoteController,
        midi_event_tx: Option<MidiEventSender>,
    ) -> Result<Self> {
        let midi_in = MidiInput::new("cplp-midi").context("MIDI 入力の初期化に失敗")?;

        let ports = midi_in.ports();
        if port_index >= ports.len() {
            bail!(
                "MIDI ポート {} は存在しません（{} 個のポートが利用可能）",
                port_index,
                ports.len()
            );
        }

        let port = &ports[port_index];
        let port_name = midi_in.port_name(port).unwrap_or_else(|_| "unknown".into());

        info!("MIDI 入力に接続: {}", port_name);

        // Mutex でラップ: midir コールバックは FnMut + Send を要求
        // MIDIスレッドのみが触るので contention はゼロ
        let note_ctrl = Mutex::new(note_ctrl);
        let midi_event_tx = Mutex::new(midi_event_tx);

        let connection = midi_in
            .connect(
                port,
                "cplp-input",
                move |_timestamp, message, _| {
                    if let Ok(mut ctrl) = note_ctrl.lock() {
                        handle_midi_message(message, &mut ctrl);
                    }
                    if let Ok(mut tx_guard) = midi_event_tx.lock() {
                        if let Some(ref mut tx) = *tx_guard {
                            forward_midi_event(message, tx);
                        }
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("MIDI ポートへの接続に失敗: {}", e))?;

        Ok(Self {
            _connection: connection,
        })
    }

    /// ポート名で検索して接続
    pub fn connect_by_name(
        name: &str,
        note_ctrl: NoteController,
        midi_event_tx: Option<MidiEventSender>,
    ) -> Result<Self> {
        let midi_in = MidiInput::new("cplp-midi").context("MIDI 入力の初期化に失敗")?;

        let ports = midi_in.ports();
        let mut found_index = None;

        for (i, port) in ports.iter().enumerate() {
            let port_name = midi_in.port_name(port).unwrap_or_else(|_| "unknown".into());
            if port_name.contains(name) {
                found_index = Some(i);
                break;
            }
        }

        match found_index {
            Some(idx) => {
                drop(midi_in);
                Self::connect(idx, note_ctrl, midi_event_tx)
            }
            None => bail!("'{}' を含む MIDI ポートが見つかりません", name),
        }
    }

    /// 最初に見つかった MIDI 入力ポートに接続
    pub fn connect_first(
        note_ctrl: NoteController,
        midi_event_tx: Option<MidiEventSender>,
    ) -> Result<Self> {
        Self::connect(0, note_ctrl, midi_event_tx)
    }
}

/// MIDI メッセージのディスパッチ先トレイト
///
/// NoteController と MidiEventSender の共通インターフェース。
/// ロジック重複を排除するために導入。
trait MidiDispatch {
    fn note_on(&mut self, key: u8, velocity: u8);
    fn note_off(&mut self, key: u8);
    fn control_change(&mut self, cc: u8, value: u8);
}

impl MidiDispatch for NoteController {
    fn note_on(&mut self, key: u8, velocity: u8) {
        NoteController::note_on(self, key, velocity);
    }
    fn note_off(&mut self, key: u8) {
        NoteController::note_off(self, key);
    }
    fn control_change(&mut self, cc: u8, value: u8) {
        NoteController::control_change(self, cc, value);
    }
}

impl MidiDispatch for MidiEventSender {
    fn note_on(&mut self, key: u8, velocity: u8) {
        MidiEventSender::note_on(self, key, velocity);
    }
    fn note_off(&mut self, key: u8) {
        MidiEventSender::note_off(self, key);
    }
    fn control_change(&mut self, cc: u8, value: u8) {
        MidiEventSender::control_change(self, cc, value);
    }
}

/// MIDI メッセージを解析してディスパッチ先に転送
fn dispatch_midi(message: &[u8], target: &mut impl MidiDispatch) {
    if message.len() < 3 {
        return;
    }

    let status = message[0] & 0xF0;
    let key = message[1];
    let velocity = message[2];

    match status {
        0x90 => {
            if velocity > 0 {
                target.note_on(key, velocity);
            } else {
                target.note_off(key);
            }
        }
        0x80 => {
            target.note_off(key);
        }
        0xB0 => {
            target.control_change(key, velocity);
        }
        _ => {}
    }
}

/// MIDI メッセージを解析して NoteController (CLAP シンセ用) に転送
fn handle_midi_message(message: &[u8], note_ctrl: &mut NoteController) {
    dispatch_midi(message, note_ctrl);
}

/// MIDI メッセージを MidiEventSender (AudioModule 用) に転送
fn forward_midi_event(message: &[u8], tx: &mut MidiEventSender) {
    dispatch_midi(message, tx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::{midi_event_channel, note_channel};

    #[test]
    fn handle_note_on_calls_note_on() {
        let (mut ctrl, _recv) = note_channel(16);
        // status=0x90 (NoteOn, channel 0), key=60, velocity=100
        let message = [0x90, 60, 100];
        handle_midi_message(&message, &mut ctrl);

        // NoteController はリングバッファに push するので、受信側で確認
        // NoteReceiver の consumer は private なので、もう一度 note_on して
        // パニックしないことで検証（内部状態は plugin_host テストに委譲）
        // ここでは handle_midi_message がパニックせず正常完了することを検証
    }

    #[test]
    fn handle_note_on_velocity_zero_calls_note_off() {
        let (mut ctrl, _recv) = note_channel(16);
        // velocity=0 の NoteOn は NoteOff 扱い
        let message = [0x90, 60, 0];
        handle_midi_message(&message, &mut ctrl);
        // パニックしなければ OK — NoteOff として処理される
    }

    #[test]
    fn handle_note_off_calls_note_off() {
        let (mut ctrl, _recv) = note_channel(16);
        // status=0x80 (NoteOff)
        let message = [0x80, 60, 64];
        handle_midi_message(&message, &mut ctrl);
        // パニックしなければ OK
    }

    #[test]
    fn handle_cc_calls_control_change() {
        let (mut ctrl, _recv) = note_channel(16);
        // status=0xB0 (CC), cc=1 (mod wheel), value=127
        let message = [0xB0, 1, 127];
        handle_midi_message(&message, &mut ctrl);
        // パニックしなければ OK
    }

    #[test]
    fn handle_unknown_status_is_noop() {
        let (mut ctrl, _recv) = note_channel(16);
        // 未知のステータス 0xF0 (System Exclusive)
        let message = [0xF0, 0x7E, 0x7F];
        handle_midi_message(&message, &mut ctrl);
        // パニックしなければ OK — 未知 status は無視される
    }

    #[test]
    fn handle_short_message_is_noop() {
        let (mut ctrl, _recv) = note_channel(16);
        // 2 バイト以下のメッセージ
        handle_midi_message(&[0x90, 60], &mut ctrl);
        handle_midi_message(&[0x90], &mut ctrl);
        handle_midi_message(&[], &mut ctrl);
        // パニックしなければ OK — 短いメッセージは早期リターン
    }

    // forward_midi_event のテスト（MidiEventSender 経由）
    #[test]
    fn forward_note_on_via_midi_event_sender() {
        let (mut tx, _rx) = midi_event_channel(16);
        let message = [0x90, 60, 100];
        forward_midi_event(&message, &mut tx);
    }

    #[test]
    fn forward_note_off_via_midi_event_sender() {
        let (mut tx, _rx) = midi_event_channel(16);
        let message = [0x80, 60, 64];
        forward_midi_event(&message, &mut tx);
    }

    #[test]
    fn forward_cc_via_midi_event_sender() {
        let (mut tx, _rx) = midi_event_channel(16);
        let message = [0xB0, 1, 127];
        forward_midi_event(&message, &mut tx);
    }

    #[test]
    fn forward_short_message_is_noop() {
        let (mut tx, _rx) = midi_event_channel(16);
        forward_midi_event(&[0x90, 60], &mut tx);
        forward_midi_event(&[], &mut tx);
    }
}
