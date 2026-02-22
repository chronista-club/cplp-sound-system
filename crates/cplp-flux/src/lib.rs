//! Flux — 全モジュールを統合・加工・出力するライブコントロールの司令塔
//!
//! Synthesizer（Human/Unpattern）と BeatMachine（History/Pattern）の出力を受け取り、
//! Cadence からの音声データやルーパーと合わせて、リアルタイムに加工・ミキシングして出力する。

use cplp_plug_beat_machine::BeatMachine;
use cplp_plug_synthesizer::Synthesizer;

pub struct Flux {
    synthesizer: Synthesizer,
    beat_machine: BeatMachine,
    sample_rate: f32,
}

impl Flux {
    pub fn new(sample_rate: f32, bpm: f32) -> Self {
        Self {
            synthesizer: Synthesizer::new(sample_rate),
            beat_machine: BeatMachine::new(bpm, sample_rate),
            sample_rate,
        }
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

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
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
    }
}
