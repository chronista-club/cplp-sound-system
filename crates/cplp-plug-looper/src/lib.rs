//! Looper — リアルタイムオーディオルーパープラグイン
//!
//! 演奏をリアルタイムに録音・ループ再生する。
//! オーバーダブ、アンドゥ、レイヤー重ねに対応予定。

pub struct Looper {
    sample_rate: f32,
}

impl Looper {
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
    fn looper_new() {
        let looper = Looper::new(44100.0);
        assert_eq!(looper.sample_rate(), 44100.0);
    }
}
