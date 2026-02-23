//! Human / Unpattern — 生演奏・即興シンセモジュール
//!
//! MIDI キーボードやコントローラーからの生演奏入力を受け取り、
//! リアルタイムに音声を生成する。
//!
//! - 8ボイスポリフォニー
//! - サイン波 / ノコギリ波 選択可能
//! - ADSR エンベロープ

use cplp_core::{AudioModule, MidiEvent, ModuleCategory, ModuleInfo};
use std::f32::consts::PI;

const MAX_VOICES: usize = 8;
const TWO_PI: f32 = 2.0 * PI;

/// set_param で使用するパラメータ ID
pub mod params {
    pub const ATTACK: u32 = 0;
    pub const DECAY: u32 = 1;
    pub const SUSTAIN: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const WAVEFORM: u32 = 4;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
}

#[derive(Debug, Clone, Copy)]
enum EnvelopeState {
    Idle,
    Attack(f32),
    Decay(f32),
    Sustain,
    Release(f32),
}

#[derive(Debug, Clone)]
struct Voice {
    note: u8,
    velocity: f32,
    phase: f32,
    envelope: EnvelopeState,
    active: bool,
}

impl Voice {
    fn new() -> Self {
        Self {
            note: 0,
            velocity: 0.0,
            phase: 0.0,
            envelope: EnvelopeState::Idle,
            active: false,
        }
    }
}

pub struct Synthesizer {
    sample_rate: f32,
    voices: [Voice; MAX_VOICES],
    waveform: Waveform,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Synthesizer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            voices: std::array::from_fn(|_| Voice::new()),
            waveform: Waveform::Sine,
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.3,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn waveform(&self) -> Waveform {
        self.waveform
    }

    pub fn set_waveform(&mut self, wf: Waveform) {
        self.waveform = wf;
    }

    /// MIDI ノート番号 → 周波数 (Hz)
    fn note_to_freq(note: u8) -> f32 {
        440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
    }

    /// 波形生成（1サンプル）
    fn oscillator(waveform: Waveform, phase: f32) -> f32 {
        match waveform {
            Waveform::Sine => (phase * TWO_PI).sin(),
            Waveform::Saw => 2.0 * (phase - phase.floor()) - 1.0,
        }
    }

    /// エンベロープの現在のゲインを計算し、状態を進める
    fn advance_envelope(
        env: &mut EnvelopeState,
        dt: f32,
        attack: f32,
        decay: f32,
        sustain: f32,
        release: f32,
    ) -> f32 {
        match env {
            EnvelopeState::Idle => 0.0,
            EnvelopeState::Attack(t) => {
                let gain = if attack > 0.0 {
                    (*t / attack).min(1.0)
                } else {
                    1.0
                };
                *t += dt;
                if *t >= attack {
                    *env = EnvelopeState::Decay(0.0);
                }
                gain
            }
            EnvelopeState::Decay(t) => {
                let progress = if decay > 0.0 {
                    (*t / decay).min(1.0)
                } else {
                    1.0
                };
                let gain = 1.0 - (1.0 - sustain) * progress;
                *t += dt;
                if *t >= decay {
                    *env = EnvelopeState::Sustain;
                }
                gain
            }
            EnvelopeState::Sustain => sustain,
            EnvelopeState::Release(t) => {
                let progress = if release > 0.0 {
                    (*t / release).min(1.0)
                } else {
                    1.0
                };
                let gain = sustain * (1.0 - progress);
                *t += dt;
                if *t >= release {
                    *env = EnvelopeState::Idle;
                }
                gain
            }
        }
    }

    /// 空きボイスを探して割り当て（既に同ノートが鳴っていれば再利用）
    fn allocate_voice(&mut self, note: u8, velocity: u8) {
        // 同一ノートが既にあればリトリガー
        if let Some(v) = self.voices.iter_mut().find(|v| v.active && v.note == note) {
            v.velocity = velocity as f32 / 127.0;
            v.envelope = EnvelopeState::Attack(0.0);
            return;
        }
        // 空きボイスを探す
        if let Some(v) = self.voices.iter_mut().find(|v| !v.active) {
            v.note = note;
            v.velocity = velocity as f32 / 127.0;
            v.phase = 0.0;
            v.envelope = EnvelopeState::Attack(0.0);
            v.active = true;
            return;
        }
        // 空きがなければ最初のボイスをスチール
        let v = &mut self.voices[0];
        v.note = note;
        v.velocity = velocity as f32 / 127.0;
        v.phase = 0.0;
        v.envelope = EnvelopeState::Attack(0.0);
        v.active = true;
    }

    fn release_voice(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.active && v.note == note {
                if let EnvelopeState::Idle = v.envelope {
                    // 既に idle ならそのまま
                } else {
                    v.envelope = EnvelopeState::Release(0.0);
                }
            }
        }
    }
}

impl AudioModule for Synthesizer {
    fn process(&mut self, output: &mut [f32]) {
        let dt = 1.0 / self.sample_rate;

        for sample in output.iter_mut() {
            let mut mix = 0.0_f32;

            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }

                let freq = Self::note_to_freq(voice.note);
                let osc = Self::oscillator(self.waveform, voice.phase);

                let gain = Self::advance_envelope(
                    &mut voice.envelope,
                    dt,
                    self.attack,
                    self.decay,
                    self.sustain,
                    self.release,
                );

                mix += osc * gain * voice.velocity;

                // 位相を進める
                voice.phase += freq * dt;
                if voice.phase >= 1.0 {
                    voice.phase -= 1.0;
                }

                // エンベロープが Idle になったらボイスを解放
                if matches!(voice.envelope, EnvelopeState::Idle) && gain == 0.0 {
                    voice.active = false;
                }
            }

            *sample = mix;
        }
    }

    fn handle_midi(&mut self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn { note, velocity } => {
                if velocity == 0 {
                    self.release_voice(note);
                } else {
                    self.allocate_voice(note, velocity);
                }
            }
            MidiEvent::NoteOff { note } => {
                self.release_voice(note);
            }
            MidiEvent::ControlChange { .. } => {
                // 将来: CC マッピング
            }
        }
    }

    fn set_param(&mut self, id: u32, value: f32) {
        match id {
            params::ATTACK => self.attack = value.max(0.001),
            params::DECAY => self.decay = value.max(0.0),
            params::SUSTAIN => self.sustain = value.clamp(0.0, 1.0),
            params::RELEASE => self.release = value.max(0.0),
            params::WAVEFORM => {
                self.waveform = if value < 0.5 {
                    Waveform::Sine
                } else {
                    Waveform::Saw
                };
            }
            _ => {}
        }
    }

    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "cplp.synthesizer".to_string(),
            name: "Human / Unpattern".to_string(),
            vendor: "cplp".to_string(),
            category: ModuleCategory::Instrument,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesizer_new() {
        let synth = Synthesizer::new(44100.0);
        assert_eq!(synth.sample_rate(), 44100.0);
        assert_eq!(synth.waveform(), Waveform::Sine);
    }

    #[test]
    fn synthesizer_implements_audio_module() {
        let synth = Synthesizer::new(44100.0);
        let info = synth.info();
        assert_eq!(info.id, "cplp.synthesizer");
        assert_eq!(info.category, ModuleCategory::Instrument);
    }

    #[test]
    fn note_on_produces_nonzero_output() {
        let mut synth = Synthesizer::new(44100.0);
        synth.handle_midi(MidiEvent::NoteOn {
            note: 69,
            velocity: 127,
        });

        let mut buf = vec![0.0_f32; 512];
        synth.process(&mut buf);

        let max_abs = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(max_abs > 0.0, "NoteOn 後の出力がゼロ");
    }

    #[test]
    fn silence_without_note() {
        let mut synth = Synthesizer::new(44100.0);

        let mut buf = vec![0.0_f32; 256];
        synth.process(&mut buf);

        assert!(buf.iter().all(|&s| s == 0.0), "ノートなしで音が出ている");
    }

    #[test]
    fn note_off_triggers_release() {
        let mut synth = Synthesizer::new(44100.0);
        synth.set_param(params::RELEASE, 0.01); // 短いリリース

        synth.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });

        // attack + sustain 中に少し鳴らす
        let mut buf = vec![0.0_f32; 2048];
        synth.process(&mut buf);

        // NoteOff → Release 開始
        synth.handle_midi(MidiEvent::NoteOff { note: 60 });

        // リリース完了まで十分なサンプル数を処理
        let mut release_buf = vec![0.0_f32; 44100]; // 1秒分
        synth.process(&mut release_buf);

        // 末尾はゼロに近づくはず
        let tail = &release_buf[release_buf.len() - 256..];
        let tail_max = tail.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(
            tail_max < 0.001,
            "リリース後も音が残っている: max={}",
            tail_max
        );
    }

    #[test]
    fn polyphony_multiple_notes() {
        let mut synth = Synthesizer::new(44100.0);

        // 3つのノートを同時に鳴らす
        synth.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        synth.handle_midi(MidiEvent::NoteOn {
            note: 64,
            velocity: 100,
        });
        synth.handle_midi(MidiEvent::NoteOn {
            note: 67,
            velocity: 100,
        });

        let active_count = synth.voices.iter().filter(|v| v.active).count();
        assert_eq!(active_count, 3, "3ボイスがアクティブであるべき");
    }

    #[test]
    fn set_param_changes_adsr() {
        let mut synth = Synthesizer::new(44100.0);

        synth.set_param(params::ATTACK, 0.5);
        assert_eq!(synth.attack, 0.5);

        synth.set_param(params::DECAY, 0.2);
        assert_eq!(synth.decay, 0.2);

        synth.set_param(params::SUSTAIN, 0.5);
        assert_eq!(synth.sustain, 0.5);

        synth.set_param(params::RELEASE, 1.0);
        assert_eq!(synth.release, 1.0);
    }

    #[test]
    fn set_param_waveform() {
        let mut synth = Synthesizer::new(44100.0);

        synth.set_param(params::WAVEFORM, 1.0);
        assert_eq!(synth.waveform(), Waveform::Saw);

        synth.set_param(params::WAVEFORM, 0.0);
        assert_eq!(synth.waveform(), Waveform::Sine);
    }

    #[test]
    fn velocity_zero_acts_as_note_off() {
        let mut synth = Synthesizer::new(44100.0);

        synth.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        assert!(synth.voices.iter().any(|v| v.active && v.note == 60));

        // velocity 0 は NoteOff と同等
        synth.handle_midi(MidiEvent::NoteOn {
            note: 60,
            velocity: 0,
        });
        let voice = synth.voices.iter().find(|v| v.note == 60).unwrap();
        assert!(matches!(voice.envelope, EnvelopeState::Release(_)));
    }

    #[test]
    fn process_replacing_passthrough() {
        let mut synth = Synthesizer::new(44100.0);
        let input = [1.0_f32, 2.0, 3.0, 4.0];
        let mut output = [0.0_f32; 4];
        synth.process_replacing(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn saw_waveform_produces_output() {
        let mut synth = Synthesizer::new(44100.0);
        synth.set_waveform(Waveform::Saw);
        synth.handle_midi(MidiEvent::NoteOn {
            note: 69,
            velocity: 127,
        });

        let mut buf = vec![0.0_f32; 512];
        synth.process(&mut buf);

        let max_abs = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(max_abs > 0.0, "Saw波形で出力がゼロ");
    }
}
