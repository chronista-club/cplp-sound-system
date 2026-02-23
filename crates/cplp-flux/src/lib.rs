//! Flux — 全モジュールを統合・加工・出力するライブコントロールの司令塔
//!
//! Synthesizer（Human/Unpattern）と BeatMachine（History/Pattern）の出力を受け取り、
//! Cadence からの音声データやルーパーと合わせて、リアルタイムに加工・ミキシングして出力する。

use cplp_core::AudioModule;
use cplp_plug_beat_machine::BeatMachine;
use cplp_plug_looper::Looper;
use cplp_plug_synthesizer::Synthesizer;

/// 各モジュールの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Off,
    Ready,
    Playing,
}

/// ルーパーの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LooperState {
    Empty,
    Recording,
    Playing,
    Stopped,
}

/// HUD 表示用のスナップショット（lock-free で共有）
#[derive(Debug, Clone)]
pub struct FluxSnapshot {
    pub bpm: f32,
    pub synth_state: ModuleState,
    pub beat_machine_state: ModuleState,
    pub looper_state: LooperState,
    pub active_plugin: Option<String>,
}

impl Default for FluxSnapshot {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            synth_state: ModuleState::Off,
            beat_machine_state: ModuleState::Off,
            looper_state: LooperState::Empty,
            active_plugin: None,
        }
    }
}

pub struct Flux {
    synthesizer: Synthesizer,
    beat_machine: BeatMachine,
    looper: Looper,
    /// 動的に登録されたモジュール（将来の拡張用）
    modules: Vec<Box<dyn AudioModule>>,
    sample_rate: f32,
    synth_state: ModuleState,
    beat_machine_state: ModuleState,
    looper_state: LooperState,
    active_plugin: Option<String>,
}

impl Flux {
    pub fn new(sample_rate: f32, bpm: f32) -> Self {
        Self {
            synthesizer: Synthesizer::new(sample_rate),
            beat_machine: BeatMachine::new(bpm, sample_rate),
            looper: Looper::new(sample_rate),
            modules: Vec::new(),
            sample_rate,
            synth_state: ModuleState::Off,
            beat_machine_state: ModuleState::Off,
            looper_state: LooperState::Empty,
            active_plugin: None,
        }
    }

    pub fn snapshot(&self) -> FluxSnapshot {
        FluxSnapshot {
            bpm: self.beat_machine.bpm(),
            synth_state: self.synth_state,
            beat_machine_state: self.beat_machine_state,
            looper_state: self.looper_state,
            active_plugin: self.active_plugin.clone(),
        }
    }

    pub fn set_active_plugin(&mut self, name: Option<String>) {
        self.active_plugin = name;
    }

    pub fn set_synth_state(&mut self, state: ModuleState) {
        self.synth_state = state;
    }

    pub fn set_beat_machine_state(&mut self, state: ModuleState) {
        self.beat_machine_state = state;
    }

    pub fn set_looper_state(&mut self, state: LooperState) {
        self.looper_state = state;
    }

    pub fn synthesizer(&self) -> &Synthesizer {
        &self.synthesizer
    }

    pub fn synthesizer_mut(&mut self) -> &mut Synthesizer {
        &mut self.synthesizer
    }

    pub fn beat_machine(&self) -> &BeatMachine {
        &self.beat_machine
    }

    pub fn beat_machine_mut(&mut self) -> &mut BeatMachine {
        &mut self.beat_machine
    }

    pub fn looper(&self) -> &Looper {
        &self.looper
    }

    pub fn looper_mut(&mut self) -> &mut Looper {
        &mut self.looper
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// 動的モジュールを登録
    pub fn register_module(&mut self, module: Box<dyn AudioModule>) {
        self.modules.push(module);
    }

    /// 動的モジュールを ID で削除（最初に一致したもの）
    pub fn remove_module(&mut self, id: &str) -> bool {
        if let Some(pos) = self.modules.iter().position(|m| m.info().id == id) {
            self.modules.remove(pos);
            true
        } else {
            false
        }
    }

    /// 登録済み動的モジュール数
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flux_new() {
        let flux = Flux::new(44100.0, 120.0);
        assert_eq!(flux.sample_rate(), 44100.0);
        assert_eq!(flux.beat_machine().bpm(), 120.0);
        assert_eq!(flux.synthesizer().sample_rate(), 44100.0);
        assert_eq!(flux.looper().sample_rate(), 44100.0);
    }

    #[test]
    fn flux_snapshot_default() {
        let flux = Flux::new(44100.0, 120.0);
        let snap = flux.snapshot();
        assert_eq!(snap.bpm, 120.0);
        assert_eq!(snap.synth_state, ModuleState::Off);
        assert_eq!(snap.beat_machine_state, ModuleState::Off);
        assert_eq!(snap.looper_state, LooperState::Empty);
        assert!(snap.active_plugin.is_none());
    }

    #[test]
    fn flux_snapshot_reflects_state() {
        let mut flux = Flux::new(44100.0, 140.0);
        flux.set_synth_state(ModuleState::Playing);
        flux.set_active_plugin(Some("Diva".into()));
        flux.set_looper_state(LooperState::Recording);

        let snap = flux.snapshot();
        assert_eq!(snap.bpm, 140.0);
        assert_eq!(snap.synth_state, ModuleState::Playing);
        assert_eq!(snap.looper_state, LooperState::Recording);
        assert_eq!(snap.active_plugin.as_deref(), Some("Diva"));
    }
}
