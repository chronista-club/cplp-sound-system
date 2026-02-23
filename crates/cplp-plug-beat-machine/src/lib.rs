//! History / Pattern — ビートマシン・リズムシーケンスモジュール
//!
//! 16ステップシーケンサー × 3トラック（Kick / Snare / HiHat）。
//! BPM 同期でパターンをループ再生し、合成ドラム音源で発音する。

use cplp_core::{AudioModule, MidiEvent, ModuleCategory, ModuleInfo};

const STEPS: usize = 16;
const TRACKS: usize = 3;

/// set_param で使用するパラメータ ID
pub mod params {
    pub const BPM: u32 = 0;
    pub const PLAY_STOP: u32 = 1; // 0.0 = stop, 1.0 = play
    pub const SWING: u32 = 2;
}

/// GM ドラムマップに準拠した MIDI ノート → トラック対応
fn note_to_track(note: u8) -> Option<usize> {
    match note {
        36 => Some(0), // Kick  (C1)
        38 => Some(1), // Snare (D1)
        42 => Some(2), // HiHat (F#1)
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ドラム合成エンジン
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct DrumVoice {
    /// 経過時間（秒）
    time: f32,
    /// ベロシティ（0.0〜1.0）
    velocity: f32,
    /// アクティブかどうか
    active: bool,
}

impl DrumVoice {
    fn new() -> Self {
        Self {
            time: 0.0,
            velocity: 0.0,
            active: false,
        }
    }

    fn trigger(&mut self, velocity: f32) {
        self.time = 0.0;
        self.velocity = velocity;
        self.active = true;
    }
}

/// Kick: 低周波サイン波 + ピッチ急降下エンベロープ
fn kick_sample(time: f32, velocity: f32) -> (f32, bool) {
    let duration = 0.3;
    if time >= duration {
        return (0.0, false);
    }
    // ピッチ: 150Hz → 50Hz に急速に降下
    let pitch = 50.0 + 100.0 * (-time * 30.0).exp();
    // 振幅エンベロープ: 指数減衰
    let amp = (-time * 10.0).exp() * velocity;
    let phase = time * pitch * std::f32::consts::TAU;
    (phase.sin() * amp, true)
}

/// Snare: ノイズ + 低いサイン波のミックス
fn snare_sample(time: f32, velocity: f32) -> (f32, bool) {
    let duration = 0.2;
    if time >= duration {
        return (0.0, false);
    }
    let amp = (-time * 20.0).exp() * velocity;
    // 簡易ノイズ（線形合同法）
    let noise = simple_noise(time);
    // ボディ（200Hz サイン波）
    let body = (time * 200.0 * std::f32::consts::TAU).sin() * (-time * 40.0).exp();
    ((noise * 0.7 + body * 0.3) * amp, true)
}

/// HiHat: 高周波ノイズの短いバースト
fn hihat_sample(time: f32, velocity: f32) -> (f32, bool) {
    let duration = 0.08;
    if time >= duration {
        return (0.0, false);
    }
    let amp = (-time * 60.0).exp() * velocity * 0.6;
    let noise = simple_noise(time + 0.5); // オフセットでスネアと区別
    (noise * amp, true)
}

/// 決定論的な疑似ノイズ（-1.0 〜 1.0）
fn simple_noise(t: f32) -> f32 {
    // 複数の高周波サイン波を重ねてノイズ風に
    let s1 = (t * 3567.0).sin();
    let s2 = (t * 7919.0).sin();
    let s3 = (t * 12113.0).sin();
    (s1 + s2 + s3) / 3.0
}

// ---------------------------------------------------------------------------
// BeatMachine 本体
// ---------------------------------------------------------------------------

pub struct BeatMachine {
    sample_rate: f32,
    bpm: f32,
    /// パターン: patterns[track][step] = velocity (0 = off)
    patterns: [[u8; STEPS]; TRACKS],
    /// 現在のステップ位置
    current_step: usize,
    /// ステップ間のサンプルカウンター
    sample_counter: f32,
    /// ドラムボイス（トラックごと）
    voices: [DrumVoice; TRACKS],
    /// 再生中かどうか
    playing: bool,
    /// スイング量（0.0 = ストレート, 1.0 = 最大スイング）
    swing: f32,
}

impl BeatMachine {
    pub fn new(bpm: f32, sample_rate: f32) -> Self {
        Self {
            sample_rate,
            bpm,
            patterns: [[0; STEPS]; TRACKS],
            current_step: 0,
            sample_counter: 0.0,
            voices: [DrumVoice::new(); TRACKS],
            playing: false,
            swing: 0.0,
        }
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.bpm = bpm.clamp(20.0, 300.0);
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn current_step(&self) -> usize {
        self.current_step
    }

    /// パターンのステップを設定
    pub fn set_step(&mut self, track: usize, step: usize, velocity: u8) {
        if track < TRACKS && step < STEPS {
            self.patterns[track][step] = velocity;
        }
    }

    /// パターンのステップを取得
    pub fn get_step(&self, track: usize, step: usize) -> u8 {
        if track < TRACKS && step < STEPS {
            self.patterns[track][step]
        } else {
            0
        }
    }

    /// デフォルトの 4つ打ちパターンをロード
    pub fn load_four_on_floor(&mut self) {
        self.patterns = [[0; STEPS]; TRACKS];
        // Kick: 1, 5, 9, 13
        for &step in &[0, 4, 8, 12] {
            self.patterns[0][step] = 100;
        }
        // Snare: 5, 13
        for &step in &[4, 12] {
            self.patterns[1][step] = 100;
        }
        // HiHat: 全ステップ（8分音符）
        for step in (0..STEPS).step_by(2) {
            self.patterns[2][step] = 80;
        }
    }

    /// 1ステップあたりのサンプル数（16分音符 = 1拍の1/4）
    fn samples_per_step(&self) -> f32 {
        // 1拍 = 60/bpm 秒、16分音符 = 1拍/4
        (60.0 / self.bpm) * self.sample_rate / 4.0
    }

    /// スイング込みのステップ長（奇数ステップを遅らせる）
    fn step_length(&self, step: usize) -> f32 {
        let base = self.samples_per_step();
        if step % 2 == 1 {
            // 奇数ステップ（裏拍）を遅延
            base * (1.0 + self.swing * 0.33)
        } else {
            base * (1.0 - self.swing * 0.33 * (STEPS as f32 - 1.0) / STEPS as f32)
        }
    }

    /// 現在のステップでトリガーすべきドラムを発音
    fn trigger_step(&mut self) {
        for track in 0..TRACKS {
            let vel = self.patterns[track][self.current_step];
            if vel > 0 {
                self.voices[track].trigger(vel as f32 / 127.0);
            }
        }
    }

    /// 1サンプルのドラムミックスを生成
    fn render_sample(&mut self, dt: f32) -> f32 {
        let mut mix = 0.0_f32;

        for (track_idx, voice) in self.voices.iter_mut().enumerate() {
            if !voice.active {
                continue;
            }

            let (sample, still_active) = match track_idx {
                0 => kick_sample(voice.time, voice.velocity),
                1 => snare_sample(voice.time, voice.velocity),
                2 => hihat_sample(voice.time, voice.velocity),
                _ => (0.0, false),
            };

            mix += sample;
            voice.time += dt;
            voice.active = still_active;
        }

        mix
    }
}

impl AudioModule for BeatMachine {
    fn process(&mut self, output: &mut [f32]) {
        let dt = 1.0 / self.sample_rate;

        for sample in output.iter_mut() {
            if self.playing {
                if self.sample_counter <= 0.0 {
                    self.trigger_step();
                    self.sample_counter = self.step_length(self.current_step);
                    self.current_step = (self.current_step + 1) % STEPS;
                }
                self.sample_counter -= 1.0;
            }

            *sample = self.render_sample(dt);
        }
    }

    fn handle_midi(&mut self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn { note, velocity } => {
                // 即座にドラムをトリガー（ライブ演奏用）
                if let Some(track) = note_to_track(note) {
                    self.voices[track].trigger(velocity as f32 / 127.0);
                }
            }
            MidiEvent::NoteOff { .. } => {
                // ドラムは自然減衰なので NoteOff は無視
            }
            MidiEvent::ControlChange { .. } => {}
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        match id {
            params::BPM => self.set_bpm(value),
            params::PLAY_STOP => {
                let should_play = value >= 0.5;
                if should_play && !self.playing {
                    self.current_step = 0;
                    self.sample_counter = 0.0;
                }
                self.playing = should_play;
            }
            params::SWING => self.swing = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "cplp.beat-machine".to_string(),
            name: "History / Pattern".to_string(),
            vendor: "cplp".to_string(),
            category: ModuleCategory::Instrument,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_machine_new() {
        let bm = BeatMachine::new(120.0, 44100.0);
        assert_eq!(bm.bpm(), 120.0);
        assert!(!bm.is_playing());
        assert_eq!(bm.current_step(), 0);
    }

    #[test]
    fn beat_machine_set_bpm() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.set_bpm(140.0);
        assert_eq!(bm.bpm(), 140.0);
    }

    #[test]
    fn bpm_clamped_to_range() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.set_bpm(0.0);
        assert_eq!(bm.bpm(), 20.0);
        bm.set_bpm(999.0);
        assert_eq!(bm.bpm(), 300.0);
    }

    #[test]
    fn set_and_get_step() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.set_step(0, 0, 100);
        assert_eq!(bm.get_step(0, 0), 100);
        assert_eq!(bm.get_step(0, 1), 0);
    }

    #[test]
    fn out_of_range_step_is_safe() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.set_step(99, 99, 100); // no panic
        assert_eq!(bm.get_step(99, 99), 0);
    }

    #[test]
    fn four_on_floor_pattern() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.load_four_on_floor();
        // Kick on beats 1, 5, 9, 13
        assert_eq!(bm.get_step(0, 0), 100);
        assert_eq!(bm.get_step(0, 4), 100);
        assert_eq!(bm.get_step(0, 1), 0);
        // Snare on 5, 13
        assert_eq!(bm.get_step(1, 4), 100);
        assert_eq!(bm.get_step(1, 12), 100);
        // HiHat on even steps
        assert_eq!(bm.get_step(2, 0), 80);
        assert_eq!(bm.get_step(2, 2), 80);
        assert_eq!(bm.get_step(2, 1), 0);
    }

    #[test]
    fn silence_when_stopped() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.load_four_on_floor();
        // playing = false なのでパターンが進まない
        let mut buf = vec![0.0_f32; 4410]; // 0.1秒
        bm.process(&mut buf);
        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn produces_audio_when_playing() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.load_four_on_floor();
        bm.set_param(params::PLAY_STOP, 1.0);

        let mut buf = vec![0.0_f32; 4410];
        bm.process(&mut buf);

        let max = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(max > 0.0, "再生中なのに無音");
    }

    #[test]
    fn midi_note_on_triggers_drum() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        // Kick (note 36) を MIDI でトリガー
        bm.handle_midi(MidiEvent::NoteOn {
            note: 36,
            velocity: 100,
        });

        let mut buf = vec![0.0_f32; 512];
        bm.process(&mut buf);

        let max = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(max > 0.0, "MIDI NoteOn 後に無音");
    }

    #[test]
    fn unknown_midi_note_is_ignored() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.handle_midi(MidiEvent::NoteOn {
            note: 60, // C3 — ドラムマップ外
            velocity: 100,
        });

        let mut buf = vec![0.0_f32; 512];
        bm.process(&mut buf);

        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn implements_audio_module() {
        let bm = BeatMachine::new(120.0, 44100.0);
        let info = bm.info();
        assert_eq!(info.id, "cplp.beat-machine");
        assert_eq!(info.category, ModuleCategory::Instrument);
    }

    #[test]
    fn step_advances_when_playing() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.set_param(params::PLAY_STOP, 1.0);

        // 120BPM → 1拍 = 0.5秒 → 16分音符 = 0.125秒 = 5512.5 samples
        // 1秒分処理すれば 8ステップ以上進むはず
        let mut buf = vec![0.0_f32; 44100];
        bm.process(&mut buf);

        assert!(bm.current_step() > 0, "ステップが進んでいない");
    }

    #[test]
    fn play_stop_resets_position() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.set_param(params::PLAY_STOP, 1.0);

        let mut buf = vec![0.0_f32; 44100];
        bm.process(&mut buf);

        // Stop → Play で位置リセット
        bm.set_param(params::PLAY_STOP, 0.0);
        bm.set_param(params::PLAY_STOP, 1.0);
        assert_eq!(bm.current_step(), 0);
    }

    #[test]
    fn process_replacing_passthrough() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        let input = [1.0_f32, 2.0, 3.0];
        let mut output = [0.0_f32; 3];
        bm.process_replacing(&input, &mut output);
        assert_eq!(output, input);
    }
}
