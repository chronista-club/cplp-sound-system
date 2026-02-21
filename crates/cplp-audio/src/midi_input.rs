use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use midir::{MidiInput, MidiInputConnection};
use tracing::info;

use crate::plugin_host::NoteController;

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
    pub fn connect(port_index: usize, note_ctrl: NoteController) -> Result<Self> {
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

        let connection = midi_in
            .connect(
                port,
                "cplp-input",
                move |_timestamp, message, _| {
                    if let Ok(mut ctrl) = note_ctrl.lock() {
                        handle_midi_message(message, &mut ctrl);
                    }
                },
                (),
            )
            .context("MIDI ポートへの接続に失敗")?;

        Ok(Self {
            _connection: connection,
        })
    }

    /// ポート名で検索して接続
    pub fn connect_by_name(name: &str, note_ctrl: NoteController) -> Result<Self> {
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
                Self::connect(idx, note_ctrl)
            }
            None => bail!("'{}' を含む MIDI ポートが見つかりません", name),
        }
    }

    /// 最初に見つかった MIDI 入力ポートに接続
    pub fn connect_first(note_ctrl: NoteController) -> Result<Self> {
        Self::connect(0, note_ctrl)
    }
}

/// MIDI メッセージを解析して NoteController に転送
fn handle_midi_message(message: &[u8], note_ctrl: &mut NoteController) {
    if message.len() < 3 {
        return;
    }

    let status = message[0] & 0xF0;
    let key = message[1];
    let velocity = message[2];

    match status {
        0x90 => {
            if velocity > 0 {
                note_ctrl.note_on(key, velocity);
            } else {
                note_ctrl.note_off(key);
            }
        }
        0x80 => {
            note_ctrl.note_off(key);
        }
        _ => {}
    }
}
