//! Looper — リアルタイムオーディオルーパーモジュール
//!
//! 演奏をリアルタイムに録音・ループ再生する。
//! オーバーダブ（再生中に重ね録り）対応。
//!
//! ## 状態遷移
//!
//! ```text
//! Empty ──(REC)──► Recording ──(STOP)──► Stopped ──(PLAY)──► Playing
//!                                                    ▲           │
//!                                                    └──(STOP)───┘
//! Playing ──(REC)──► Recording  (オーバーダブ: ループ再生しながら録音)
//! ```

use cplp_core::{AudioModule, MidiEvent, ModuleCategory, ModuleInfo};

/// set_param で使用するパラメータ ID
pub mod params {
    /// 録音トリガー: 0.0→1.0 で録音開始/オーバーダブ
    pub const RECORD: u32 = 0;
    /// 停止: 0.0→1.0 で録音停止 or 再生停止
    pub const STOP: u32 = 1;
    /// 再生: 0.0→1.0 で再生開始
    pub const PLAY: u32 = 2;
    /// クリア: 0.0→1.0 でバッファ消去→Empty
    pub const CLEAR: u32 = 3;
    /// 入力ボリューム (0.0〜1.0)
    pub const INPUT_GAIN: u32 = 4;
    /// ループ再生ボリューム (0.0〜1.0)
    pub const LOOP_GAIN: u32 = 5;
}

/// MIDI ノート → 状態制御マッピング
fn note_to_action(note: u8) -> Option<u32> {
    match note {
        60 => Some(params::RECORD), // C3 = Record / Overdub
        62 => Some(params::STOP),   // D3 = Stop
        64 => Some(params::PLAY),   // E3 = Play
        65 => Some(params::CLEAR),  // F3 = Clear
        _ => None,
    }
}

/// ルーパーの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LooperState {
    /// バッファ空 — 初期状態
    Empty,
    /// 初回録音中（ループ長を決定する）
    Recording,
    /// 再生停止中（ループは保持）
    Stopped,
    /// ループ再生中
    Playing,
    /// オーバーダブ中（再生しながら重ね録り）
    Overdubbing,
}

/// 最大録音時間（秒）
const MAX_DURATION_SECS: f32 = 30.0;

pub struct Looper {
    sample_rate: f32,
    state: LooperState,
    /// 録音/再生バッファ
    buffer: Vec<f32>,
    /// バッファ内の現在位置（録音時: 書き込み位置、再生時: 読み出し位置）
    position: usize,
    /// ループ長（初回録音完了で確定）
    loop_length: usize,
    /// 入力ゲイン
    input_gain: f32,
    /// ループ再生ゲイン
    loop_gain: f32,
}

impl Looper {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            state: LooperState::Empty,
            buffer: Vec::new(),
            position: 0,
            loop_length: 0,
            input_gain: 1.0,
            loop_gain: 1.0,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn state(&self) -> LooperState {
        self.state
    }

    pub fn loop_length(&self) -> usize {
        self.loop_length
    }

    /// ループの長さを秒数で返す
    pub fn loop_duration_secs(&self) -> f32 {
        if self.sample_rate > 0.0 {
            self.loop_length as f32 / self.sample_rate
        } else {
            0.0
        }
    }

    /// 録音開始
    fn start_recording(&mut self) {
        match self.state {
            LooperState::Empty => {
                // 初回録音: バッファを確保
                let max_samples = (self.sample_rate * MAX_DURATION_SECS) as usize;
                self.buffer = vec![0.0; max_samples];
                self.position = 0;
                self.loop_length = 0;
                self.state = LooperState::Recording;
            }
            LooperState::Playing | LooperState::Stopped => {
                // オーバーダブ: 既存ループの上に重ね録り
                self.position = if self.state == LooperState::Playing {
                    self.position // 現在の再生位置から継続
                } else {
                    0 // 停止中なら先頭から
                };
                self.state = LooperState::Overdubbing;
            }
            _ => {} // Recording/Overdubbing 中は無視
        }
    }

    /// 停止
    fn stop(&mut self) {
        match self.state {
            LooperState::Recording => {
                // 初回録音完了: ループ長を確定
                self.loop_length = self.position;
                self.position = 0;
                self.state = LooperState::Stopped;
            }
            LooperState::Overdubbing => {
                // オーバーダブ終了 → 再生に戻る
                self.state = LooperState::Playing;
            }
            LooperState::Playing => {
                self.position = 0;
                self.state = LooperState::Stopped;
            }
            _ => {}
        }
    }

    /// 再生開始
    fn start_playing(&mut self) {
        if self.state == LooperState::Stopped {
            self.position = 0;
            self.state = LooperState::Playing;
        }
    }

    /// バッファクリア → Empty に戻る
    fn clear(&mut self) {
        self.buffer.clear();
        self.position = 0;
        self.loop_length = 0;
        self.state = LooperState::Empty;
    }

    /// アクションをトリガー（set_param / MIDI 共通）
    fn trigger_action(&mut self, param_id: u32) {
        match param_id {
            params::RECORD => self.start_recording(),
            params::STOP => self.stop(),
            params::PLAY => self.start_playing(),
            params::CLEAR => self.clear(),
            _ => {}
        }
    }
}

impl AudioModule for Looper {
    fn process(&mut self, output: &mut [f32]) {
        // 入力なしで process() が呼ばれた場合: 再生のみ
        match self.state {
            LooperState::Playing | LooperState::Overdubbing => {
                if self.loop_length == 0 {
                    output.fill(0.0);
                    return;
                }
                for sample in output.iter_mut() {
                    *sample = self.buffer[self.position] * self.loop_gain;
                    self.position = (self.position + 1) % self.loop_length;
                }
            }
            _ => {
                output.fill(0.0);
            }
        }
    }

    fn process_replacing(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());

        match self.state {
            LooperState::Empty | LooperState::Stopped => {
                // パススルー: 入力をそのままスルー
                output[..len].copy_from_slice(&input[..len]);
            }
            LooperState::Recording => {
                // 初回録音: 入力をバッファに書き込み + スルー
                for i in 0..len {
                    if self.position < self.buffer.len() {
                        self.buffer[self.position] = input[i] * self.input_gain;
                        self.position += 1;
                    } else {
                        // 最大長に達したら自動停止
                        self.stop();
                        // 残りはパススルー
                        output[i..len].copy_from_slice(&input[i..len]);
                        return;
                    }
                    output[i] = input[i];
                }
            }
            LooperState::Playing => {
                // 再生: ループバッファ + 入力をミックス
                if self.loop_length == 0 {
                    output[..len].copy_from_slice(&input[..len]);
                    return;
                }
                for i in 0..len {
                    let loop_sample = self.buffer[self.position] * self.loop_gain;
                    output[i] = input[i] + loop_sample;
                    self.position = (self.position + 1) % self.loop_length;
                }
            }
            LooperState::Overdubbing => {
                // オーバーダブ: 再生しながら入力を重ね録り
                if self.loop_length == 0 {
                    output[..len].copy_from_slice(&input[..len]);
                    return;
                }
                for i in 0..len {
                    let existing = self.buffer[self.position];
                    let new_input = input[i] * self.input_gain;
                    // バッファに加算
                    self.buffer[self.position] = existing + new_input;
                    // 出力: 既存ループ + 入力
                    output[i] = existing * self.loop_gain + input[i];
                    self.position = (self.position + 1) % self.loop_length;
                }
            }
        }
    }

    fn handle_midi(&mut self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn { note, .. } => {
                if let Some(action) = note_to_action(note) {
                    self.trigger_action(action);
                }
            }
            MidiEvent::NoteOff { .. } | MidiEvent::ControlChange { .. } => {}
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        match id {
            params::RECORD | params::STOP | params::PLAY | params::CLEAR => {
                // トリガー: 0.0→1.0 の立ち上がりで発火
                if value >= 0.5 {
                    self.trigger_action(id);
                }
            }
            params::INPUT_GAIN => self.input_gain = value.clamp(0.0, 2.0),
            params::LOOP_GAIN => self.loop_gain = value.clamp(0.0, 2.0),
            _ => {}
        }
    }

    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "cplp.looper".to_string(),
            name: "Echo Chamber".to_string(),
            vendor: "cplp".to_string(),
            category: ModuleCategory::Effect,
        }
    }
}

// ─── マルチトラックルーパー ──────────────────────────────────

/// CC 70 でアクティブトラックを切り替える（キーボード MIDI 操作用）
const CC_ACTIVE_TRACK: u8 = 70;

/// トラック数
const NUM_TRACKS: usize = 5;

/// LPD8 パッドマッピング
mod lpd8 {
    /// Pad 1-5 (Note 36-40): Track 1-5 Rec/Play トグル
    pub const PAD_TRACK_START: u8 = 36;
    pub const PAD_TRACK_END: u8 = 40;
    /// Pad 6 (Note 41): Stop All
    pub const PAD_STOP_ALL: u8 = 41;
    /// Pad 7 (Note 42): Clear All
    pub const PAD_CLEAR_ALL: u8 = 42;
    /// CC 1-5: Track 1-5 ゲイン
    pub const CC_TRACK_GAIN_START: u8 = 1;
    pub const CC_TRACK_GAIN_END: u8 = 5;
    /// CC 6: マスターゲイン（将来用）
    pub const CC_MASTER_GAIN: u8 = 6;
}

/// マルチトラックルーパー — 最大5トラックを独立操作
///
/// active_track で操作対象を切り替え、全トラックの出力をミックスする。
/// CC 70 (値 0-4) でアクティブトラックを変更。
pub struct MultiTrackLooper {
    tracks: Vec<Looper>,
    active_track: usize,
    sample_rate: f32,
}

impl MultiTrackLooper {
    pub fn new(sample_rate: f32) -> Self {
        let tracks = (0..NUM_TRACKS).map(|_| Looper::new(sample_rate)).collect();
        Self {
            tracks,
            active_track: 0,
            sample_rate,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn active_track(&self) -> usize {
        self.active_track
    }

    pub fn track(&self, index: usize) -> Option<&Looper> {
        self.tracks.get(index)
    }

    pub fn num_tracks(&self) -> usize {
        self.tracks.len()
    }

    /// Rec/Play トグル: Empty→Rec, Recording→Stop→Play, Playing→Stop, Stopped→Play
    fn toggle_rec_play(&mut self, track_idx: usize) {
        if track_idx >= self.tracks.len() {
            return;
        }
        match self.tracks[track_idx].state() {
            LooperState::Empty => {
                self.tracks[track_idx].trigger_action(params::RECORD);
            }
            LooperState::Recording | LooperState::Overdubbing => {
                self.tracks[track_idx].trigger_action(params::STOP);
            }
            LooperState::Stopped => {
                self.tracks[track_idx].trigger_action(params::PLAY);
            }
            LooperState::Playing => {
                self.tracks[track_idx].trigger_action(params::STOP);
            }
        }
    }

    /// 全トラック停止
    fn stop_all(&mut self) {
        for track in &mut self.tracks {
            track.trigger_action(params::STOP);
        }
    }

    /// 全トラッククリア
    fn clear_all(&mut self) {
        for track in &mut self.tracks {
            track.trigger_action(params::CLEAR);
        }
    }
}

impl AudioModule for MultiTrackLooper {
    fn process(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        let mut track_buf = vec![0.0f32; output.len()];
        for track in &mut self.tracks {
            track_buf.fill(0.0);
            track.process(&mut track_buf);
            for (o, t) in output.iter_mut().zip(track_buf.iter()) {
                *o += t;
            }
        }
    }

    fn process_replacing(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());

        // active_track のみに入力を送って録音
        // 他のトラックは再生のみ
        let mut active_out = vec![0.0f32; len];
        self.tracks[self.active_track].process_replacing(&input[..len], &mut active_out);

        // 残りのトラックの再生出力をミックス
        output[..len].copy_from_slice(&active_out);
        let mut track_buf = vec![0.0f32; len];
        for (i, track) in self.tracks.iter_mut().enumerate() {
            if i == self.active_track {
                continue;
            }
            track_buf.fill(0.0);
            track.process(&mut track_buf);
            for j in 0..len {
                output[j] += track_buf[j];
            }
        }
    }

    fn handle_midi(&mut self, event: MidiEvent) {
        match event {
            // CC: トラック切替 / ゲイン / マスターゲイン
            MidiEvent::ControlChange { cc, value } => {
                if cc == CC_ACTIVE_TRACK {
                    self.active_track = (value as usize).min(NUM_TRACKS - 1);
                } else if cc >= lpd8::CC_TRACK_GAIN_START && cc <= lpd8::CC_TRACK_GAIN_END
                {
                    let track_idx = (cc - lpd8::CC_TRACK_GAIN_START) as usize;
                    if track_idx < self.tracks.len() {
                        // CC value 0-127 → gain 0.0-2.0
                        let gain = (value as f32 / 127.0) * 2.0;
                        self.tracks[track_idx].set_param(params::LOOP_GAIN, gain);
                    }
                } else if cc == lpd8::CC_MASTER_GAIN {
                    // マスターゲイン: 全トラックの loop_gain を一括設定
                    let gain = (value as f32 / 127.0) * 2.0;
                    for track in &mut self.tracks {
                        track.set_param(params::LOOP_GAIN, gain);
                    }
                }
            }
            // NoteOn: LPD8 パッド or キーボード操作
            MidiEvent::NoteOn { note, velocity } if velocity > 0 => {
                if note >= lpd8::PAD_TRACK_START && note <= lpd8::PAD_TRACK_END {
                    // Pad 1-5: Rec/Play トグル
                    let track_idx = (note - lpd8::PAD_TRACK_START) as usize;
                    self.toggle_rec_play(track_idx);
                } else if note == lpd8::PAD_STOP_ALL {
                    self.stop_all();
                } else if note == lpd8::PAD_CLEAR_ALL {
                    self.clear_all();
                } else {
                    // C3/D3/E3/F3 → active_track に転送
                    self.tracks[self.active_track].handle_midi(event);
                }
            }
            _ => {
                self.tracks[self.active_track].handle_midi(event);
            }
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        self.tracks[self.active_track].set_param(id, value);
    }

    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "cplp.multitrack-looper".to_string(),
            name: "Echo Chamber MT".to_string(),
            vendor: "cplp".to_string(),
            category: ModuleCategory::Effect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looper_new() {
        let looper = Looper::new(44100.0);
        assert_eq!(looper.sample_rate(), 44100.0);
        assert_eq!(looper.state(), LooperState::Empty);
        assert_eq!(looper.loop_length(), 0);
    }

    #[test]
    fn empty_state_passthrough() {
        let mut looper = Looper::new(44100.0);
        let input = [0.5, -0.3, 0.7];
        let mut output = [0.0; 3];
        looper.process_replacing(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn record_and_stop_sets_loop_length() {
        let mut looper = Looper::new(44100.0);

        // 録音開始
        looper.set_param(params::RECORD, 1.0);
        assert_eq!(looper.state(), LooperState::Recording);

        // 100 サンプル録音
        let input = vec![0.5_f32; 100];
        let mut output = vec![0.0_f32; 100];
        looper.process_replacing(&input, &mut output);

        // 停止
        looper.set_param(params::STOP, 1.0);
        assert_eq!(looper.state(), LooperState::Stopped);
        assert_eq!(looper.loop_length(), 100);
    }

    #[test]
    fn play_loops_recorded_audio() {
        let mut looper = Looper::new(44100.0);

        // 4サンプル録音
        looper.set_param(params::RECORD, 1.0);
        let input = [1.0, 2.0, 3.0, 4.0];
        let mut output = [0.0; 4];
        looper.process_replacing(&input, &mut output);
        looper.set_param(params::STOP, 1.0);

        // 再生: 8サンプル（2周）
        looper.set_param(params::PLAY, 1.0);
        assert_eq!(looper.state(), LooperState::Playing);

        let silence = [0.0_f32; 8];
        let mut play_out = [0.0_f32; 8];
        looper.process_replacing(&silence, &mut play_out);

        // ループが2回繰り返される
        assert_eq!(play_out[0], 1.0);
        assert_eq!(play_out[1], 2.0);
        assert_eq!(play_out[2], 3.0);
        assert_eq!(play_out[3], 4.0);
        assert_eq!(play_out[4], 1.0);
        assert_eq!(play_out[5], 2.0);
    }

    #[test]
    fn play_mixes_with_input() {
        let mut looper = Looper::new(44100.0);

        // 2サンプル録音
        looper.set_param(params::RECORD, 1.0);
        let rec = [1.0, 2.0];
        let mut out = [0.0; 2];
        looper.process_replacing(&rec, &mut out);
        looper.set_param(params::STOP, 1.0);

        // 再生 + 入力ミックス
        looper.set_param(params::PLAY, 1.0);
        let input = [0.1, 0.2];
        let mut play_out = [0.0; 2];
        looper.process_replacing(&input, &mut play_out);

        // output = input + loop
        assert!((play_out[0] - 1.1).abs() < 1e-5);
        assert!((play_out[1] - 2.2).abs() < 1e-5);
    }

    #[test]
    fn overdub_adds_to_buffer() {
        let mut looper = Looper::new(44100.0);

        // 初回: 2サンプル [1.0, 2.0]
        looper.set_param(params::RECORD, 1.0);
        let rec = [1.0, 2.0];
        let mut out = [0.0; 2];
        looper.process_replacing(&rec, &mut out);
        looper.set_param(params::STOP, 1.0);

        // 再生 → オーバーダブ
        looper.set_param(params::PLAY, 1.0);
        looper.set_param(params::RECORD, 1.0);
        assert_eq!(looper.state(), LooperState::Overdubbing);

        let overdub_input = [0.5, 0.5];
        let mut overdub_out = [0.0; 2];
        looper.process_replacing(&overdub_input, &mut overdub_out);

        // オーバーダブ停止
        looper.set_param(params::STOP, 1.0);
        assert_eq!(looper.state(), LooperState::Playing);

        // 再度再生 → バッファに加算されている
        looper.set_param(params::STOP, 1.0);
        looper.set_param(params::PLAY, 1.0);
        let silence = [0.0; 2];
        let mut verify = [0.0; 2];
        looper.process_replacing(&silence, &mut verify);

        // 1.0 + 0.5 = 1.5, 2.0 + 0.5 = 2.5
        assert!((verify[0] - 1.5).abs() < 1e-5);
        assert!((verify[1] - 2.5).abs() < 1e-5);
    }

    #[test]
    fn clear_resets_to_empty() {
        let mut looper = Looper::new(44100.0);

        // 録音 → 停止
        looper.set_param(params::RECORD, 1.0);
        let input = [1.0; 10];
        let mut out = [0.0; 10];
        looper.process_replacing(&input, &mut out);
        looper.set_param(params::STOP, 1.0);
        assert_eq!(looper.loop_length(), 10);

        // クリア
        looper.set_param(params::CLEAR, 1.0);
        assert_eq!(looper.state(), LooperState::Empty);
        assert_eq!(looper.loop_length(), 0);
    }

    #[test]
    fn midi_controls_state() {
        let mut looper = Looper::new(44100.0);

        // C3 = Record
        looper.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        assert_eq!(looper.state(), LooperState::Recording);

        // 少し録音
        let input = [0.5; 50];
        let mut out = [0.0; 50];
        looper.process_replacing(&input, &mut out);

        // D3 = Stop
        looper.handle_midi(MidiEvent::NoteOn {
            note: 62,
            velocity: 100,
        });
        assert_eq!(looper.state(), LooperState::Stopped);

        // E3 = Play
        looper.handle_midi(MidiEvent::NoteOn {
            note: 64,
            velocity: 100,
        });
        assert_eq!(looper.state(), LooperState::Playing);

        // F3 = Clear
        looper.handle_midi(MidiEvent::NoteOn {
            note: 65,
            velocity: 100,
        });
        assert_eq!(looper.state(), LooperState::Empty);
    }

    #[test]
    fn play_without_record_is_noop() {
        let mut looper = Looper::new(44100.0);
        // Empty 状態で Play しても何も起きない
        looper.set_param(params::PLAY, 1.0);
        assert_eq!(looper.state(), LooperState::Empty);
    }

    #[test]
    fn process_only_plays_loop() {
        let mut looper = Looper::new(44100.0);

        // 録音
        looper.set_param(params::RECORD, 1.0);
        let input = [0.8, -0.8];
        let mut out = [0.0; 2];
        looper.process_replacing(&input, &mut out);
        looper.set_param(params::STOP, 1.0);

        // process() (入力なし) で再生
        looper.set_param(params::PLAY, 1.0);
        let mut play_buf = [0.0; 4];
        looper.process(&mut play_buf);

        assert!((play_buf[0] - 0.8).abs() < 1e-5);
        assert!((play_buf[1] - (-0.8)).abs() < 1e-5);
        assert!((play_buf[2] - 0.8).abs() < 1e-5); // ループ2周目
    }

    #[test]
    fn input_gain_scales_recording() {
        let mut looper = Looper::new(44100.0);
        looper.set_param(params::INPUT_GAIN, 0.5);

        looper.set_param(params::RECORD, 1.0);
        let input = [1.0; 4];
        let mut out = [0.0; 4];
        looper.process_replacing(&input, &mut out);
        looper.set_param(params::STOP, 1.0);

        // 再生: 0.5 ゲインで録音されている
        looper.set_param(params::PLAY, 1.0);
        let silence = [0.0; 4];
        let mut play_out = [0.0; 4];
        looper.process_replacing(&silence, &mut play_out);

        assert!((play_out[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn loop_gain_scales_playback() {
        let mut looper = Looper::new(44100.0);

        // 録音
        looper.set_param(params::RECORD, 1.0);
        let input = [1.0; 2];
        let mut out = [0.0; 2];
        looper.process_replacing(&input, &mut out);
        looper.set_param(params::STOP, 1.0);

        // ループゲインを0.5に
        looper.set_param(params::LOOP_GAIN, 0.5);
        looper.set_param(params::PLAY, 1.0);

        let silence = [0.0; 2];
        let mut play_out = [0.0; 2];
        looper.process_replacing(&silence, &mut play_out);

        assert!((play_out[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn loop_duration_secs() {
        let mut looper = Looper::new(44100.0);
        assert_eq!(looper.loop_duration_secs(), 0.0);

        looper.set_param(params::RECORD, 1.0);
        let input = vec![0.0; 44100]; // 1秒
        let mut out = vec![0.0; 44100];
        looper.process_replacing(&input, &mut out);
        looper.set_param(params::STOP, 1.0);

        assert!((looper.loop_duration_secs() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn auto_stop_at_max_duration() {
        let sr = 1000.0; // テスト用に低いサンプルレート
        let mut looper = Looper::new(sr);

        looper.set_param(params::RECORD, 1.0);

        // MAX_DURATION_SECS * sr + 余分なサンプルを入力
        let total = (MAX_DURATION_SECS * sr) as usize + 100;
        let input = vec![0.5_f32; total];
        let mut out = vec![0.0_f32; total];
        looper.process_replacing(&input, &mut out);

        // 最大長で自動停止している
        assert_eq!(looper.state(), LooperState::Stopped);
        assert_eq!(looper.loop_length(), (MAX_DURATION_SECS * sr) as usize);
    }

    #[test]
    fn info_returns_effect_category() {
        let looper = Looper::new(44100.0);
        let info = looper.info();
        assert_eq!(info.id, "cplp.looper");
        assert_eq!(info.category, ModuleCategory::Effect);
    }

    #[test]
    fn unknown_midi_note_ignored() {
        let mut looper = Looper::new(44100.0);
        looper.handle_midi(MidiEvent::NoteOn {
            note: 127,
            velocity: 100,
        });
        assert_eq!(looper.state(), LooperState::Empty);
    }

    #[test]
    fn stopped_play_overdub_cycle() {
        let mut looper = Looper::new(44100.0);

        // 録音 → 停止
        looper.set_param(params::RECORD, 1.0);
        let input = [1.0; 4];
        let mut out = [0.0; 4];
        looper.process_replacing(&input, &mut out);
        looper.set_param(params::STOP, 1.0);

        // 停止中にオーバーダブ開始 → position は 0 から
        looper.set_param(params::RECORD, 1.0);
        assert_eq!(looper.state(), LooperState::Overdubbing);

        let overdub = [0.25; 4];
        let mut od_out = [0.0; 4];
        looper.process_replacing(&overdub, &mut od_out);
        looper.set_param(params::STOP, 1.0);
        // Overdub → Playing
        assert_eq!(looper.state(), LooperState::Playing);
    }

    // ─── MultiTrackLooper テスト ──────────────────────────────

    #[test]
    fn multitrack_new() {
        let mt = MultiTrackLooper::new(44100.0);
        assert_eq!(mt.num_tracks(), 5);
        assert_eq!(mt.active_track(), 0);
        for i in 0..5 {
            assert_eq!(mt.track(i).unwrap().state(), LooperState::Empty);
        }
    }

    #[test]
    fn multitrack_record_on_active_track() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Track 0 に録音
        mt.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        }); // REC
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Recording);
        assert_eq!(mt.track(1).unwrap().state(), LooperState::Empty);

        let input = [1.0; 4];
        let mut out = [0.0; 4];
        mt.process_replacing(&input, &mut out);

        mt.handle_midi(MidiEvent::NoteOn {
            note: 62,
            velocity: 100,
        }); // STOP
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Stopped);
        assert_eq!(mt.track(0).unwrap().loop_length(), 4);
    }

    #[test]
    fn multitrack_switch_active_track() {
        let mut mt = MultiTrackLooper::new(44100.0);
        assert_eq!(mt.active_track(), 0);

        // CC 70 = 2 → Track 2 に切替
        mt.handle_midi(MidiEvent::ControlChange {
            cc: CC_ACTIVE_TRACK,
            value: 2,
        });
        assert_eq!(mt.active_track(), 2);

        // Track 2 に録音
        mt.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        assert_eq!(mt.track(2).unwrap().state(), LooperState::Recording);
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Empty);
    }

    #[test]
    fn multitrack_clamp_track_index() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // CC 70 = 127 → clamped to 4
        mt.handle_midi(MidiEvent::ControlChange {
            cc: CC_ACTIVE_TRACK,
            value: 127,
        });
        assert_eq!(mt.active_track(), 4);
    }

    #[test]
    fn multitrack_mix_all_tracks() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Track 0: 録音 [1.0, 1.0]
        mt.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        let input = [1.0; 2];
        let mut out = [0.0; 2];
        mt.process_replacing(&input, &mut out);
        mt.handle_midi(MidiEvent::NoteOn {
            note: 62,
            velocity: 100,
        }); // STOP
        mt.handle_midi(MidiEvent::NoteOn {
            note: 64,
            velocity: 100,
        }); // PLAY

        // Track 1 に切替 → 録音 [0.5, 0.5]
        mt.handle_midi(MidiEvent::ControlChange {
            cc: CC_ACTIVE_TRACK,
            value: 1,
        });
        mt.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        let input2 = [0.5; 2];
        let mut out2 = [0.0; 2];
        mt.process_replacing(&input2, &mut out2);
        mt.handle_midi(MidiEvent::NoteOn {
            note: 62,
            velocity: 100,
        }); // STOP
        mt.handle_midi(MidiEvent::NoteOn {
            note: 64,
            velocity: 100,
        }); // PLAY

        // 両方再生 → ミックスされる
        let silence = [0.0; 2];
        let mut mixed = [0.0; 2];
        mt.process_replacing(&silence, &mut mixed);

        // Track 0 (1.0) + Track 1 (0.5) = 1.5
        assert!((mixed[0] - 1.5).abs() < 1e-5);
        assert!((mixed[1] - 1.5).abs() < 1e-5);
    }

    #[test]
    fn multitrack_passthrough_when_empty() {
        let mut mt = MultiTrackLooper::new(44100.0);
        let input = [0.7, -0.3];
        let mut out = [0.0; 2];
        mt.process_replacing(&input, &mut out);
        assert_eq!(out, input);
    }

    // ─── LPD8 マッピングテスト ──────────────────────────────

    #[test]
    fn lpd8_pad_toggle_rec_play() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Pad 1 (Note 36) → Track 0: Empty → Recording
        mt.handle_midi(MidiEvent::NoteOn {
            note: 36,
            velocity: 100,
        });
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Recording);

        // 少し録音
        let input = [0.5; 4];
        let mut out = [0.0; 4];
        mt.process_replacing(&input, &mut out);

        // Pad 1 再度 → Recording → Stopped
        mt.handle_midi(MidiEvent::NoteOn {
            note: 36,
            velocity: 100,
        });
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Stopped);

        // Pad 1 再度 → Stopped → Playing
        mt.handle_midi(MidiEvent::NoteOn {
            note: 36,
            velocity: 100,
        });
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Playing);

        // Pad 1 再度 → Playing → Stopped
        mt.handle_midi(MidiEvent::NoteOn {
            note: 36,
            velocity: 100,
        });
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Stopped);
    }

    #[test]
    fn lpd8_pad_targets_correct_track() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Pad 3 (Note 38) → Track 2
        mt.handle_midi(MidiEvent::NoteOn {
            note: 38,
            velocity: 100,
        });
        assert_eq!(mt.track(2).unwrap().state(), LooperState::Recording);
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Empty);

        // Pad 5 (Note 40) → Track 4
        mt.handle_midi(MidiEvent::NoteOn {
            note: 40,
            velocity: 100,
        });
        assert_eq!(mt.track(4).unwrap().state(), LooperState::Recording);
    }

    #[test]
    fn lpd8_stop_all() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Track 0, 1 を録音 → 再生
        for track_note in [36u8, 37] {
            mt.handle_midi(MidiEvent::NoteOn {
                note: track_note,
                velocity: 100,
            });
            let input = [0.5; 4];
            let mut out = [0.0; 4];
            mt.process_replacing(&input, &mut out);
            // toggle: Recording → Stopped
            mt.handle_midi(MidiEvent::NoteOn {
                note: track_note,
                velocity: 100,
            });
            // toggle: Stopped → Playing
            mt.handle_midi(MidiEvent::NoteOn {
                note: track_note,
                velocity: 100,
            });
        }
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Playing);
        assert_eq!(mt.track(1).unwrap().state(), LooperState::Playing);

        // Pad 6 (Note 41) → Stop All
        mt.handle_midi(MidiEvent::NoteOn {
            note: 41,
            velocity: 100,
        });
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Stopped);
        assert_eq!(mt.track(1).unwrap().state(), LooperState::Stopped);
    }

    #[test]
    fn lpd8_clear_all() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Track 0 に録音
        mt.handle_midi(MidiEvent::NoteOn {
            note: 36,
            velocity: 100,
        });
        let input = [0.5; 4];
        let mut out = [0.0; 4];
        mt.process_replacing(&input, &mut out);
        mt.handle_midi(MidiEvent::NoteOn {
            note: 36,
            velocity: 100,
        }); // Stop

        // Pad 7 (Note 42) → Clear All
        mt.handle_midi(MidiEvent::NoteOn {
            note: 42,
            velocity: 100,
        });
        assert_eq!(mt.track(0).unwrap().state(), LooperState::Empty);
        assert_eq!(mt.track(0).unwrap().loop_length(), 0);
    }

    #[test]
    fn lpd8_cc_track_gain() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Track 0 に録音
        mt.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        }); // REC via C3
        let input = [1.0; 4];
        let mut out = [0.0; 4];
        mt.process_replacing(&input, &mut out);
        mt.handle_midi(MidiEvent::NoteOn {
            note: 62,
            velocity: 100,
        }); // STOP
        mt.handle_midi(MidiEvent::NoteOn {
            note: 64,
            velocity: 100,
        }); // PLAY

        // CC 1 = Track 0 ゲイン → 0 (ミュート)
        mt.handle_midi(MidiEvent::ControlChange { cc: 1, value: 0 });

        let silence = [0.0; 4];
        let mut play_out = [0.0; 4];
        mt.process_replacing(&silence, &mut play_out);
        // ゲイン 0 なので無音
        assert!(play_out[0].abs() < 1e-5);
    }

    #[test]
    fn lpd8_cc_master_gain() {
        let mut mt = MultiTrackLooper::new(44100.0);

        // Track 0 に録音
        mt.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        let input = [1.0; 2];
        let mut out = [0.0; 2];
        mt.process_replacing(&input, &mut out);
        mt.handle_midi(MidiEvent::NoteOn {
            note: 62,
            velocity: 100,
        });
        mt.handle_midi(MidiEvent::NoteOn {
            note: 64,
            velocity: 100,
        });

        // CC 6 = マスターゲイン → 64 (約半分)
        mt.handle_midi(MidiEvent::ControlChange { cc: 6, value: 64 });

        let silence = [0.0; 2];
        let mut play_out = [0.0; 2];
        mt.process_replacing(&silence, &mut play_out);
        // 64/127 * 2.0 ≈ 1.008 ゲイン → ほぼ 1.0
        assert!(play_out[0] > 0.9 && play_out[0] < 1.1);
    }
}
