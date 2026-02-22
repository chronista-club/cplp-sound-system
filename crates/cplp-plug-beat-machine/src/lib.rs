//! History / Pattern — ビートマシン・リズムシーケンスモジュール
//!
//! パターンベースのリズムシーケンスを管理・再生する。
//! ステップシーケンサー、パターンチェイン、テンポ同期を担う。

pub struct BeatMachine {
    bpm: f32,
    _sample_rate: f32,
}

impl BeatMachine {
    pub fn new(bpm: f32, sample_rate: f32) -> Self {
        Self {
            bpm,
            _sample_rate: sample_rate,
        }
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.bpm = bpm;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_machine_new() {
        let bm = BeatMachine::new(120.0, 44100.0);
        assert_eq!(bm.bpm(), 120.0);
    }

    #[test]
    fn beat_machine_set_bpm() {
        let mut bm = BeatMachine::new(120.0, 44100.0);
        bm.set_bpm(140.0);
        assert_eq!(bm.bpm(), 140.0);
    }
}
