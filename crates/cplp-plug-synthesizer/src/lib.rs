//! Human / Unpattern — 生演奏・即興シンセモジュール
//!
//! MIDI キーボードやコントローラーからの生演奏入力を受け取り、
//! リアルタイムに音声を生成する。

pub struct Synthesizer {
    sample_rate: f32,
}

impl Synthesizer {
    pub fn new(sample_rate: f32) -> Self {
        Self { sample_rate }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesizer_new() {
        let synth = Synthesizer::new(44100.0);
        assert_eq!(synth.sample_rate(), 44100.0);
    }
}
