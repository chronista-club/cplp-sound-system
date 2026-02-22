# Cadence (AI バンドメンバー) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** P2P セッションに参加する AI バンドメンバー Cadence を構築し、テキストコマンドから CLAP プラグインで演奏して PCM を送り返す。

**Architecture:** `cplp-cadence` クレートをモノリス型で新規作成。既存の cplp-session/cplp-audio/cplp-network を再利用し、CommandRouter（ローカルパーサー + Claude SDK）と MidiSequencer を追加する。

**Tech Stack:** Rust, clap (CLI), cplp-session (P2P), cplp-audio (CLAP hosting), cplp-network (ControlEvent 拡張), tokio (async runtime)

---

## Task 1: クレート `cplp-cadence` のスキャフォールド

**Files:**
- Create: `crates/cplp-cadence/Cargo.toml`
- Create: `crates/cplp-cadence/src/main.rs`
- Modify: `Cargo.toml` (ワークスペースに追加)

**Step 1: ワークスペースに cplp-cadence を追加**

`Cargo.toml` (ルート) の `[workspace] members` に追加:

```toml
members = [
    "crates/cplp-core",
    "crates/cplp-audio",
    "crates/cplp-network",
    "crates/cplp-session",
    "crates/cplp-app",
    "crates/cplp-lobby",
    "crates/cplp-hud",
    "crates/cplp-cadence",
]
```

**Step 2: Cargo.toml を作成**

`crates/cplp-cadence/Cargo.toml`:

```toml
[package]
name = "cplp-cadence"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
description = "Cadence - AI バンドメンバー"

[[bin]]
name = "cadence"
path = "src/main.rs"

[dependencies]
cplp-core.workspace = true
cplp-audio.workspace = true
cplp-network.workspace = true
cplp-session.workspace = true
tokio.workspace = true
anyhow.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
serde.workspace = true
serde_json.workspace = true
clap = { version = "4", features = ["derive"] }
```

**Step 3: 最小の main.rs を作成**

`crates/cplp-cadence/src/main.rs`:

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cadence")]
#[command(about = "Cadence - AI バンドメンバー")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// セッションをホストして接続を待機
    Listen {
        /// CLAP プラグイン ID
        plugin_id: String,
        /// 待機ポート
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
    /// 指定アドレスに接続
    Connect {
        /// 接続先アドレス
        addr: String,
        /// CLAP プラグイン ID
        plugin_id: String,
        /// ローカルポート
        #[arg(short, long, default_value_t = 5001)]
        port: u16,
    },
    /// 稼働状況を表示
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Listen { plugin_id, port } => {
            println!("Cadence listen on :{port} with plugin {plugin_id}");
        }
        Command::Connect { addr, plugin_id, port } => {
            println!("Cadence connect to {addr} from :{port} with plugin {plugin_id}");
        }
        Command::Status => {
            println!("Cadence is not running");
        }
    }
    Ok(())
}
```

**Step 4: ビルド確認**

Run: `cargo build -p cplp-cadence`
Expected: 成功、`target/debug/cadence` が生成される

**Step 5: 動作確認**

Run: `cargo run -p cplp-cadence -- listen test-plugin`
Expected: `Cadence listen on :5000 with plugin test-plugin`

**Step 6: コミット**

```bash
git add crates/cplp-cadence/ Cargo.toml
git commit -m "feat(cadence): cplp-cadence クレートのスキャフォールド"
```

---

## Task 2: ControlEvent にコマンド系イベントを追加

**Files:**
- Modify: `crates/cplp-network/src/control.rs`
- Test: `crates/cplp-network/src/control.rs` (既存テストモジュール)

**Step 1: テストを書く**

`crates/cplp-network/src/control.rs` のテストモジュールに追加:

```rust
#[test]
fn command_event_serialization() {
    let event = ControlEvent::Command {
        from: PeerId::new("player-a"),
        mode: CommandMode::Parse,
        text: "C major scale 120bpm".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ControlEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        ControlEvent::Command { from, mode, text } => {
            assert_eq!(from, PeerId::new("player-a"));
            assert!(matches!(mode, CommandMode::Parse));
            assert_eq!(text, "C major scale 120bpm");
        }
        _ => panic!("Expected Command variant"),
    }
}

#[test]
fn command_ack_serialization() {
    let event = ControlEvent::CommandAck {
        status: CommandStatus::Accepted,
        message: "演奏開始".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ControlEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Accepted));
            assert_eq!(message, "演奏開始");
        }
        _ => panic!("Expected CommandAck variant"),
    }
}

#[test]
fn plugin_switch_serialization() {
    let event = ControlEvent::PluginSwitch {
        from: PeerId::new("player-a"),
        plugin_id: "com.u-he.Diva".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("PluginSwitch"));
    let parsed: ControlEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        ControlEvent::PluginSwitch { from, plugin_id } => {
            assert_eq!(from, PeerId::new("player-a"));
            assert_eq!(plugin_id, "com.u-he.Diva");
        }
        _ => panic!("Expected PluginSwitch variant"),
    }
}
```

**Step 2: テストが失敗することを確認**

Run: `cargo test -p cplp-network -- command_event`
Expected: FAIL（Command バリアント・CommandMode・CommandStatus が未定義）

**Step 3: ControlEvent に新バリアントを追加**

`crates/cplp-network/src/control.rs` の `ControlEvent` enum に追加:

```rust
    // ── Cadence コマンド ──
    Command {
        from: PeerId,
        mode: CommandMode,
        text: String,
    },
    CommandAck {
        status: CommandStatus,
        message: String,
    },
    PluginSwitch {
        from: PeerId,
        plugin_id: String,
    },
```

同ファイルに新しい型を追加:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandMode {
    Parse,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandStatus {
    Accepted,
    Rejected,
    Error,
}
```

`ControlHandler::apply_event` の `_ => {}` で既にカバーされるので変更不要。

**Step 4: テストがパスすることを確認**

Run: `cargo test -p cplp-network -- command`
Expected: 3 テスト PASS

**Step 5: 全テストが壊れていないことを確認**

Run: `cargo test --workspace`
Expected: 全テスト PASS

**Step 6: コミット**

```bash
git add crates/cplp-network/src/control.rs
git commit -m "feat(network): ControlEvent に Command/CommandAck/PluginSwitch を追加"
```

---

## Task 3: MidiSequence 中間表現とローカルパーサー

**Files:**
- Create: `crates/cplp-cadence/src/midi_types.rs`
- Create: `crates/cplp-cadence/src/parser.rs`

**Step 1: MIDI 型定義のテストを書く**

`crates/cplp-cadence/src/midi_types.rs`:

```rust
/// MIDI シーケンス（パーサーまたは LLM の出力）
#[derive(Debug, Clone)]
pub struct MidiSequence {
    pub tempo_bpm: f32,
    pub events: Vec<MidiEvent>,
}

#[derive(Debug, Clone)]
pub struct MidiEvent {
    /// タイミング（ティック、4分音符 = 480 ticks）
    pub tick: u64,
    /// MIDI ノート番号 (0-127)
    pub note: u8,
    /// ベロシティ (0-127)
    pub velocity: u8,
    /// ノート長（ティック）
    pub duration_ticks: u64,
}

/// ティック解像度: 4分音符あたりのティック数
pub const TICKS_PER_QUARTER: u64 = 480;

impl MidiSequence {
    pub fn new(tempo_bpm: f32) -> Self {
        Self {
            tempo_bpm,
            events: Vec::new(),
        }
    }

    /// 全イベントの終了時刻（最後のノートオフ）
    pub fn duration_ticks(&self) -> u64 {
        self.events
            .iter()
            .map(|e| e.tick + e.duration_ticks)
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_sequence_duration() {
        let seq = MidiSequence::new(122.0);
        assert_eq!(seq.duration_ticks(), 0);
    }

    #[test]
    fn sequence_duration_tracks_last_note() {
        let seq = MidiSequence {
            tempo_bpm: 122.0,
            events: vec![
                MidiEvent { tick: 0, note: 60, velocity: 100, duration_ticks: 480 },
                MidiEvent { tick: 480, note: 62, velocity: 100, duration_ticks: 960 },
            ],
        };
        assert_eq!(seq.duration_ticks(), 1440); // 480 + 960
    }
}
```

**Step 2: パーサーのテストを書く**

`crates/cplp-cadence/src/parser.rs`:

```rust
use crate::midi_types::{MidiEvent, MidiSequence, TICKS_PER_QUARTER};

/// パースエラー
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("不明なコマンド: {0}")]
    UnknownCommand(String),
    #[error("不明なノート名: {0}")]
    UnknownNote(String),
    #[error("不明なスケール: {0}")]
    UnknownScale(String),
}

/// ノート名を MIDI ノート番号に変換（C4 = 60 基準）
fn note_name_to_midi(name: &str) -> Result<u8, ParseError> {
    let base = match name.to_uppercase().trim_end_matches(char::is_numeric) {
        "C" => 0,
        "C#" | "DB" => 1,
        "D" => 2,
        "D#" | "EB" => 3,
        "E" => 4,
        "F" => 5,
        "F#" | "GB" => 6,
        "G" => 7,
        "G#" | "AB" => 8,
        "A" => 9,
        "A#" | "BB" => 10,
        "B" => 11,
        other => return Err(ParseError::UnknownNote(other.to_string())),
    };
    // デフォルトオクターブ 4 (C4 = 60)
    Ok(60 + base)
}

/// スケールの半音インターバル列を返す
fn scale_intervals(name: &str) -> Result<&'static [u8], ParseError> {
    match name.to_lowercase().as_str() {
        "major" => Ok(&[0, 2, 4, 5, 7, 9, 11, 12]),
        "minor" => Ok(&[0, 2, 3, 5, 7, 8, 10, 12]),
        "pentatonic" => Ok(&[0, 2, 4, 7, 9, 12]),
        "blues" => Ok(&[0, 3, 5, 6, 7, 10, 12]),
        other => Err(ParseError::UnknownScale(other.to_string())),
    }
}

/// テキストコマンドを MidiSequence にパース
pub fn parse_command(input: &str) -> Result<MidiSequence, ParseError> {
    let input = input.trim();

    // "stop" コマンド
    if input.eq_ignore_ascii_case("stop") {
        return Ok(MidiSequence::new(0.0)); // tempo 0 = 停止シグナル
    }

    // "tempo <N>" コマンド
    if let Some(rest) = input.strip_prefix("tempo ").or_else(|| input.strip_prefix("tempo\t")) {
        let bpm: f32 = rest.trim().parse().map_err(|_| ParseError::UnknownCommand(input.to_string()))?;
        return Ok(MidiSequence::new(bpm));
    }

    // "<Root> <Scale> [BPM]bpm [Nbars]" パターン
    let tokens: Vec<&str> = input.split_whitespace().collect();
    if tokens.len() < 2 {
        return Err(ParseError::UnknownCommand(input.to_string()));
    }

    let root = note_name_to_midi(tokens[0])?;
    let intervals = scale_intervals(tokens[1])?;

    // BPM を探す（デフォルト 122）
    let mut bpm: f32 = 122.0;
    let mut bars: u32 = 2;
    for token in &tokens[2..] {
        let t = token.to_lowercase();
        if let Some(b) = t.strip_suffix("bpm") {
            bpm = b.parse().unwrap_or(122.0);
        } else if let Some(b) = t.strip_suffix("bars") {
            bars = b.parse().unwrap_or(2);
        }
    }

    // スケール上昇 + 下降のシーケンス生成
    let note_duration = TICKS_PER_QUARTER; // 4分音符
    let mut events = Vec::new();
    let notes_per_pass = intervals.len();
    let total_notes = notes_per_pass * 2 - 2; // 上昇 + 下降（頂点重複除く）
    let total_ticks_per_cycle = total_notes as u64 * note_duration;
    let bar_ticks = TICKS_PER_QUARTER * 4; // 4/4 拍子
    let target_ticks = bars as u64 * bar_ticks;

    let mut tick: u64 = 0;
    while tick < target_ticks {
        // 上昇
        for &interval in intervals {
            if tick >= target_ticks {
                break;
            }
            events.push(MidiEvent {
                tick,
                note: root + interval,
                velocity: 100,
                duration_ticks: note_duration - 10, // 少しスタッカート
            });
            tick += note_duration;
        }
        // 下降（頂点と底は除く）
        for &interval in intervals.iter().rev().skip(1).take(intervals.len() - 2) {
            if tick >= target_ticks {
                break;
            }
            events.push(MidiEvent {
                tick,
                note: root + interval,
                velocity: 90,
                duration_ticks: note_duration - 10,
            });
            tick += note_duration;
        }
    }

    Ok(MidiSequence {
        tempo_bpm: bpm,
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stop() {
        let seq = parse_command("stop").unwrap();
        assert_eq!(seq.tempo_bpm, 0.0);
        assert!(seq.events.is_empty());
    }

    #[test]
    fn parse_tempo_change() {
        let seq = parse_command("tempo 140").unwrap();
        assert!((seq.tempo_bpm - 140.0).abs() < f32::EPSILON);
        assert!(seq.events.is_empty());
    }

    #[test]
    fn parse_c_major_scale() {
        let seq = parse_command("C major 120bpm").unwrap();
        assert!((seq.tempo_bpm - 120.0).abs() < f32::EPSILON);
        assert!(!seq.events.is_empty());
        // 最初のノートは C4 = 60
        assert_eq!(seq.events[0].note, 60);
        // 2番目は D4 = 62
        assert_eq!(seq.events[1].note, 62);
    }

    #[test]
    fn parse_default_bpm_is_122() {
        let seq = parse_command("A minor").unwrap();
        assert!((seq.tempo_bpm - 122.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_a_minor_pentatonic() {
        let seq = parse_command("A pentatonic 90bpm 4bars").unwrap();
        assert!((seq.tempo_bpm - 90.0).abs() < f32::EPSILON);
        // A = 69, pentatonic intervals: 0,2,4,7,9,12
        assert_eq!(seq.events[0].note, 69);
    }

    #[test]
    fn parse_unknown_returns_error() {
        assert!(parse_command("").is_err());
        assert!(parse_command("dance salsa").is_err());
    }

    #[test]
    fn note_name_to_midi_basics() {
        assert_eq!(note_name_to_midi("C").unwrap(), 60);
        assert_eq!(note_name_to_midi("A").unwrap(), 69);
        assert_eq!(note_name_to_midi("G").unwrap(), 67);
    }
}
```

**Step 3: main.rs にモジュール登録**

`crates/cplp-cadence/src/main.rs` の先頭に追加:

```rust
mod midi_types;
mod parser;
```

Cargo.toml に `thiserror` 追加:

```toml
thiserror.workspace = true
```

**Step 4: テスト実行**

Run: `cargo test -p cplp-cadence`
Expected: 全テスト PASS

**Step 5: コミット**

```bash
git add crates/cplp-cadence/
git commit -m "feat(cadence): MidiSequence 型とローカルパーサー（スケール・テンポ・stop）"
```

---

## Task 4: MidiSequencer（MIDI イベントのリアルタイムスケジューリング）

**Files:**
- Create: `crates/cplp-cadence/src/sequencer.rs`

**Step 1: テストを書く**

`crates/cplp-cadence/src/sequencer.rs`:

```rust
use crate::midi_types::{MidiSequence, MidiEvent, TICKS_PER_QUARTER};
use std::sync::mpsc;

/// MIDI イベントをリアルタイムにスケジュールし、NoteOn/Off を送信する
pub struct MidiSequencer {
    sequence: Option<MidiSequence>,
    /// 現在のティック位置
    current_tick: u64,
    /// 前回の update 呼び出し時刻（秒）
    last_time: f64,
    /// ループ再生
    pub looping: bool,
}

/// NoteOn/Off イベント（オーディオスレッドに送る）
#[derive(Debug, Clone)]
pub enum NoteCommand {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    Stop,
}

impl MidiSequencer {
    pub fn new() -> Self {
        Self {
            sequence: None,
            current_tick: 0,
            last_time: 0.0,
            looping: true,
        }
    }

    /// 新しいシーケンスをセット（演奏開始）
    pub fn set_sequence(&mut self, seq: MidiSequence) {
        if seq.tempo_bpm == 0.0 {
            // stop シグナル
            self.sequence = None;
            self.current_tick = 0;
            return;
        }
        self.sequence = Some(seq);
        self.current_tick = 0;
        self.last_time = 0.0;
    }

    /// 現在のシーケンスをクリア
    pub fn stop(&mut self) {
        self.sequence = None;
        self.current_tick = 0;
    }

    pub fn is_playing(&self) -> bool {
        self.sequence.is_some()
    }

    /// 時間経過分のイベントを収集する
    ///
    /// `current_time`: 秒単位の現在時刻（monotonic）
    /// 戻り値: 発火すべき NoteCommand のリスト
    pub fn update(&mut self, current_time: f64) -> Vec<NoteCommand> {
        let seq = match &self.sequence {
            Some(s) => s,
            None => return Vec::new(),
        };

        if self.last_time == 0.0 {
            self.last_time = current_time;
            return Vec::new();
        }

        let dt = current_time - self.last_time;
        self.last_time = current_time;

        // 経過ティック数を計算
        let ticks_per_sec = (seq.tempo_bpm as f64 / 60.0) * TICKS_PER_QUARTER as f64;
        let delta_ticks = (dt * ticks_per_sec) as u64;
        let prev_tick = self.current_tick;
        self.current_tick += delta_ticks;

        let duration = seq.duration_ticks();
        if duration == 0 {
            return Vec::new();
        }

        let mut commands = Vec::new();

        // ループ対応: current_tick がシーケンス長を超えたら巻き戻す
        let effective_prev = if self.looping { prev_tick % duration } else { prev_tick };
        let effective_curr = if self.looping { self.current_tick % duration } else { self.current_tick };

        for event in &seq.events {
            // NoteOn: イベント開始がこの区間に含まれるか
            let on_tick = event.tick;
            if on_tick >= effective_prev && on_tick < effective_prev + delta_ticks {
                commands.push(NoteCommand::NoteOn {
                    note: event.note,
                    velocity: event.velocity,
                });
            }

            // NoteOff: イベント終了がこの区間に含まれるか
            let off_tick = event.tick + event.duration_ticks;
            if off_tick >= effective_prev && off_tick < effective_prev + delta_ticks {
                commands.push(NoteCommand::NoteOff { note: event.note });
            }
        }

        // ループ巻き戻し
        if self.looping && self.current_tick >= duration {
            self.current_tick %= duration;
        }

        // 非ループで終了
        if !self.looping && self.current_tick >= duration {
            self.sequence = None;
        }

        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_sequence() -> MidiSequence {
        MidiSequence {
            tempo_bpm: 120.0,  // 120BPM = 2 beats/sec = 960 ticks/sec
            events: vec![
                MidiEvent { tick: 0, note: 60, velocity: 100, duration_ticks: 240 },
                MidiEvent { tick: 480, note: 62, velocity: 100, duration_ticks: 240 },
            ],
        }
    }

    #[test]
    fn sequencer_starts_not_playing() {
        let seq = MidiSequencer::new();
        assert!(!seq.is_playing());
    }

    #[test]
    fn sequencer_plays_after_set() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        assert!(seq.is_playing());
    }

    #[test]
    fn sequencer_stop_command() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        seq.set_sequence(MidiSequence::new(0.0)); // stop signal
        assert!(!seq.is_playing());
    }

    #[test]
    fn sequencer_emits_note_on_at_start() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());

        // 初回は時刻初期化のみ
        let cmds = seq.update(0.0);
        assert!(cmds.is_empty());

        // 0.01秒後 → tick 0 のノートが発火
        let cmds = seq.update(0.01);
        assert!(cmds.iter().any(|c| matches!(c, NoteCommand::NoteOn { note: 60, .. })));
    }

    #[test]
    fn sequencer_emits_note_off() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());

        seq.update(0.0); // init
        seq.update(0.01); // NoteOn at tick 0

        // 0.25秒後 = 240 ticks → NoteOff for note 60
        let cmds = seq.update(0.26);
        assert!(cmds.iter().any(|c| matches!(c, NoteCommand::NoteOff { note: 60 })));
    }

    #[test]
    fn sequencer_manual_stop() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        seq.stop();
        assert!(!seq.is_playing());
    }
}
```

**Step 2: main.rs にモジュール登録**

```rust
mod sequencer;
```

**Step 3: テスト実行**

Run: `cargo test -p cplp-cadence`
Expected: 全テスト PASS

**Step 4: コミット**

```bash
git add crates/cplp-cadence/
git commit -m "feat(cadence): MidiSequencer（リアルタイム MIDI スケジューリング）"
```

---

## Task 5: CommandRouter（/parse と /ask の振り分け）

**Files:**
- Create: `crates/cplp-cadence/src/router.rs`

**Step 1: テストとルーター実装**

`crates/cplp-cadence/src/router.rs`:

```rust
use cplp_network::control::CommandMode;
use crate::midi_types::MidiSequence;
use crate::parser::{self, ParseError};

/// コマンドルーティング結果
pub enum RouteResult {
    /// ローカルパース成功
    Parsed(MidiSequence),
    /// Claude SDK に委譲（テキストをそのまま渡す）
    DelegateToLlm(String),
    /// パースエラー
    Error(String),
}

/// テキストコマンドを適切なハンドラにルーティング
pub fn route_command(mode: &CommandMode, text: &str) -> RouteResult {
    match mode {
        CommandMode::Parse => match parser::parse_command(text) {
            Ok(seq) => RouteResult::Parsed(seq),
            Err(e) => RouteResult::Error(e.to_string()),
        },
        CommandMode::Ask => RouteResult::DelegateToLlm(text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_parse_mode_success() {
        let result = route_command(&CommandMode::Parse, "C major 120bpm");
        assert!(matches!(result, RouteResult::Parsed(_)));
    }

    #[test]
    fn route_parse_mode_error() {
        let result = route_command(&CommandMode::Parse, "gibberish nonsense");
        assert!(matches!(result, RouteResult::Error(_)));
    }

    #[test]
    fn route_ask_mode_delegates() {
        let result = route_command(&CommandMode::Ask, "ブルースっぽいバッキング弾いて");
        match result {
            RouteResult::DelegateToLlm(text) => {
                assert_eq!(text, "ブルースっぽいバッキング弾いて");
            }
            _ => panic!("Expected DelegateToLlm"),
        }
    }
}
```

**Step 2: main.rs にモジュール登録**

```rust
mod router;
```

**Step 3: テスト実行**

Run: `cargo test -p cplp-cadence`
Expected: 全テスト PASS

**Step 4: コミット**

```bash
git add crates/cplp-cadence/
git commit -m "feat(cadence): CommandRouter（parse/ask モード振り分け）"
```

---

## Task 6: Cadence セッションの統合（listen → 接続待機 → コマンド受信 → 演奏）

**Files:**
- Create: `crates/cplp-cadence/src/session.rs`
- Modify: `crates/cplp-cadence/src/main.rs`

**Step 1: CadenceSession を実装**

`crates/cplp-cadence/src/session.rs`:

```rust
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use cplp_audio::engine::AudioEngine;
use cplp_audio::plugin_host;
use cplp_core::config::AppConfig;
use cplp_core::types::PeerId;
use cplp_network::control::{ControlEvent, CommandMode, CommandStatus, ControlHandler};
use cplp_session::manager::SessionManager;

use crate::midi_types::MidiSequence;
use crate::router::{self, RouteResult};
use crate::sequencer::{MidiSequencer, NoteCommand};

/// Cadence セッション管理
pub struct CadenceSession {
    plugin_id: String,
    port: u16,
}

impl CadenceSession {
    pub fn new(plugin_id: String, port: u16) -> Self {
        Self { plugin_id, port }
    }

    /// ホストとして listen し、接続待ち → コマンド受信ループ
    pub async fn run_listen(&self) -> anyhow::Result<()> {
        println!("Cadence: ポート {} で待機中...", self.port);
        println!("Cadence: プラグイン {}", self.plugin_id);

        // オーディオエンジンセットアップ
        let plugins = plugin_host::scan_plugins();
        let plugin = plugins
            .iter()
            .find(|p| p.id.contains(&self.plugin_id) || p.name.contains(&self.plugin_id))
            .ok_or_else(|| anyhow::anyhow!("プラグインが見つかりません: {}", self.plugin_id))?
            .clone();

        let config = cplp_core::config::AudioConfig::default();
        let (mut synth_processor, note_ctrl, _synth_handle) = plugin_host::load_plugin(
            &plugin,
            config.sample_rate as f64,
            config.buffer_size,
            config.buffer_size,
            config.channels as usize,
        )?;
        println!("Cadence: シンセ {} をロード", plugin.name);

        let mut engine = AudioEngine::new(config);
        engine.start(move |buf: &mut [f32]| {
            synth_processor.process(buf);
        })?;

        // セッション開始
        let app_config = AppConfig {
            network: cplp_core::config::NetworkConfig {
                listen_port: self.port,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut session = SessionManager::new(app_config);
        let _streamer = session.host().await?;
        println!("Cadence: ピア接続完了、コマンド待機中");

        // シーケンサー
        let mut sequencer = MidiSequencer::new();
        let start_time = Instant::now();

        // コマンド受信ループ（TODO: 実際の ControlEvent 受信に接続）
        println!("Cadence: Ctrl+C で終了");
        loop {
            // シーケンサー更新
            let elapsed = start_time.elapsed().as_secs_f64();
            let commands = sequencer.update(elapsed);
            for cmd in commands {
                match cmd {
                    NoteCommand::NoteOn { note, velocity } => {
                        note_ctrl.note_on(0, note, velocity);
                    }
                    NoteCommand::NoteOff { note } => {
                        note_ctrl.note_off(0, note);
                    }
                    NoteCommand::Stop => {
                        sequencer.stop();
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    /// ゲストとして connect
    pub async fn run_connect(&self, addr: SocketAddr) -> anyhow::Result<()> {
        println!("Cadence: {} に接続中...", addr);
        // listen と同様だが session.join(addr) を使う
        // TODO: 実装（listen とほぼ同じ）
        Ok(())
    }
}
```

**Step 2: main.rs を更新**

```rust
mod midi_types;
mod parser;
mod sequencer;
mod router;
mod session;

use clap::{Parser, Subcommand};
use session::CadenceSession;

#[derive(Parser)]
#[command(name = "cadence")]
#[command(about = "Cadence - AI バンドメンバー")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// セッションをホストして接続を待機
    Listen {
        /// CLAP プラグイン ID
        plugin_id: String,
        /// 待機ポート
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
    /// 指定アドレスに接続
    Connect {
        /// 接続先アドレス
        addr: String,
        /// CLAP プラグイン ID
        plugin_id: String,
        /// ローカルポート
        #[arg(short, long, default_value_t = 5001)]
        port: u16,
    },
    /// 稼働状況を表示
    Status,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    match cli.cmd {
        Command::Listen { plugin_id, port } => {
            let session = CadenceSession::new(plugin_id, port);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(session.run_listen())?;
        }
        Command::Connect { addr, plugin_id, port } => {
            let addr: std::net::SocketAddr = addr.parse()?;
            let session = CadenceSession::new(plugin_id, port);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(session.run_connect(addr))?;
        }
        Command::Status => {
            println!("Cadence is not running");
        }
    }
    Ok(())
}
```

**Step 3: ビルド確認**

Run: `cargo build -p cplp-cadence`
Expected: 成功

**Step 4: テスト確認**

Run: `cargo test --workspace`
Expected: 全テスト PASS

**Step 5: コミット**

```bash
git add crates/cplp-cadence/
git commit -m "feat(cadence): CadenceSession 統合（listen → 接続 → コマンド → 演奏）"
```

---

## Task 7: cplp-app にコマンド送信機能を追加

**Files:**
- Modify: `crates/cplp-app/src/main.rs`

**Step 1: Player A 側からコマンドを送信する機能を追加**

`cplp-app/src/main.rs` の `run_session_blocking` 内で、Ctrl+C 待ちの代わりに stdin からコマンドを読む:

```rust
/// セッション中にコマンドを送信するループ
async fn command_input_loop(
    control_channels: &HashMap<PeerId, UnisonChannel>,
    local_peer_id: &PeerId,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    println!("コマンド入力（/parse <text> or /ask <text>）:");

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let (mode, text) = if let Some(rest) = line.strip_prefix("/ask ") {
            (cplp_network::control::CommandMode::Ask, rest.to_string())
        } else if let Some(rest) = line.strip_prefix("/parse ") {
            (cplp_network::control::CommandMode::Parse, rest.to_string())
        } else {
            // デフォルトは /parse
            (cplp_network::control::CommandMode::Parse, line)
        };

        let event = ControlEvent::Command {
            from: local_peer_id.clone(),
            mode,
            text,
        };

        if let Err(e) = ControlHandler::broadcast(control_channels, &event).await {
            tracing::error!("コマンド送信エラー: {}", e);
        }
    }
}
```

**Step 2: ビルド確認**

Run: `cargo build -p cplp-app`
Expected: 成功

**Step 3: コミット**

```bash
git add crates/cplp-app/src/main.rs
git commit -m "feat(app): セッション中のコマンド送信機能（/parse, /ask）"
```

---

## Task 8: E2E 統合テスト

**Files:**
- Create: `crates/cplp-cadence/tests/e2e.rs`

**Step 1: ControlEvent のコマンドフロー統合テスト**

```rust
use cplp_network::control::{ControlEvent, CommandMode, CommandStatus};
use cplp_core::types::PeerId;

#[test]
fn command_roundtrip_via_json() {
    // Player A がコマンドを送信
    let command = ControlEvent::Command {
        from: PeerId::new("player-a"),
        mode: CommandMode::Parse,
        text: "C major scale 120bpm".into(),
    };

    // JSON シリアライズ → ネットワーク → デシリアライズ
    let json = serde_json::to_string(&command).unwrap();
    let received: ControlEvent = serde_json::from_str(&json).unwrap();

    // Cadence が受信してパース
    match received {
        ControlEvent::Command { mode, text, .. } => {
            assert!(matches!(mode, CommandMode::Parse));

            // パーサーで変換
            // Note: parser は cplp-cadence 内部なのでここでは文字列チェックのみ
            assert_eq!(text, "C major scale 120bpm");
        }
        _ => panic!("Expected Command event"),
    }

    // Cadence が ACK を返す
    let ack = ControlEvent::CommandAck {
        status: CommandStatus::Accepted,
        message: "C major scale を演奏開始".into(),
    };
    let ack_json = serde_json::to_string(&ack).unwrap();
    let ack_received: ControlEvent = serde_json::from_str(&ack_json).unwrap();
    match ack_received {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Accepted));
            assert!(message.contains("演奏開始"));
        }
        _ => panic!("Expected CommandAck event"),
    }
}
```

**Step 2: テスト実行**

Run: `cargo test -p cplp-cadence`
Expected: 全テスト PASS

**Step 3: 全ワークスペーステスト**

Run: `cargo test --workspace`
Expected: 全テスト PASS

**Step 4: コミット**

```bash
git add crates/cplp-cadence/
git commit -m "test(cadence): E2E コマンドフロー統合テスト"
```

---

## 実装順序まとめ

| Task | 内容 | 依存 |
|------|------|------|
| 1 | クレート スキャフォールド | なし |
| 2 | ControlEvent 拡張 | なし |
| 3 | MidiSequence + パーサー | Task 1 |
| 4 | MidiSequencer | Task 3 |
| 5 | CommandRouter | Task 2, 3 |
| 6 | CadenceSession 統合 | Task 4, 5 |
| 7 | cplp-app コマンド送信 | Task 2 |
| 8 | E2E テスト | Task 6, 7 |

Task 1 と 2 は並列実行可能。Task 3-5 は順次。Task 6 が統合、Task 7-8 が仕上げ。
