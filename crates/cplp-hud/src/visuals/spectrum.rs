use rustfft::{num_complex::Complex, FftPlanner};

use crate::renderer::Renderer;
use crate::renderer::primitives::{Color, Rect};

/// FFT サイズ
const FFT_SIZE: usize = 1024;
/// 表示するバンド数
const NUM_BANDS: usize = 32;

pub struct Spectrum {
    planner: FftPlanner<f32>,
    fft_buffer: Vec<Complex<f32>>,
    pub(crate) magnitudes: Vec<f32>,
    pub(crate) smoothed: Vec<f32>,
    label: String,
    color: Color,
}

impl Spectrum {
    pub fn new(label: &str, color: Color) -> Self {
        Self {
            planner: FftPlanner::new(),
            fft_buffer: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            magnitudes: vec![0.0; NUM_BANDS],
            smoothed: vec![0.0; NUM_BANDS],
            label: label.to_string(),
            color,
        }
    }

    /// PCM サンプルを受け取り FFT を実行、マグニチュードを更新
    pub fn update(&mut self, samples: &[f32]) {
        // 1. サンプルを FFT バッファにコピー（足りなければゼロパディング）
        for (i, c) in self.fft_buffer.iter_mut().enumerate() {
            let sample = samples.get(i).copied().unwrap_or(0.0);
            // ハニング窓を適用
            let window =
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos());
            *c = Complex::new(sample * window, 0.0);
        }

        // 2. FFT 実行
        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        fft.process(&mut self.fft_buffer);

        // 3. マグニチュードを NUM_BANDS に集約（対数スケール）
        //    低周波帯は少ないビン、高周波帯は多いビンを集約
        let half = FFT_SIZE / 2;
        for band in 0..NUM_BANDS {
            let lo = band_to_bin(band, NUM_BANDS, half);
            let hi = band_to_bin(band + 1, NUM_BANDS, half);
            let hi = hi.max(lo + 1); // 最低 1 ビン

            let mut sum = 0.0f32;
            for i in lo..hi {
                let mag = self.fft_buffer[i].norm();
                sum = sum.max(mag); // ピーク値を使用
            }
            // 正規化（dB スケール、-60dB〜0dB → 0.0〜1.0）
            let db = 20.0 * sum.max(1e-10).log10();
            self.magnitudes[band] = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        }

        // 4. スムージング（指数移動平均）
        for i in 0..NUM_BANDS {
            self.smoothed[i] = self.smoothed[i] * 0.7 + self.magnitudes[i] * 0.3;
        }
    }

    /// スペクトラムバーを描画
    pub fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        // 背景
        renderer.rect(
            rect,
            Color {
                r: 0.08,
                g: 0.08,
                b: 0.1,
                a: 0.8,
            },
        );

        // バー描画
        let gap = 2.0;
        let bar_w = (rect.w - gap * (NUM_BANDS as f32 - 1.0)) / NUM_BANDS as f32;

        for (i, &mag) in self.smoothed.iter().enumerate() {
            let x = rect.x + i as f32 * (bar_w + gap);
            let bar_h = mag * rect.h;
            let y = rect.y + rect.h - bar_h;

            // バーの色: 高さに応じてグラデーション
            let color = bar_color(mag, &self.color);
            renderer.rect(Rect { x, y, w: bar_w, h: bar_h }, color);
        }
    }
}

/// バンドインデックスを FFT ビンインデックスに変換（対数スケール）
fn band_to_bin(band: usize, num_bands: usize, half_fft: usize) -> usize {
    let ratio = band as f32 / num_bands as f32;
    // 対数スケール: 低周波帯ほど細かく、高周波帯ほど粗く
    let bin = (half_fft as f32).powf(ratio);
    (bin as usize).min(half_fft)
}

/// バーの色（マグニチュードに応じてベースカラーの明度を変化）
fn bar_color(magnitude: f32, base: &Color) -> Color {
    let brightness = 0.3 + magnitude * 0.7;
    Color {
        r: base.r * brightness,
        g: base.g * brightness,
        b: base.b * brightness,
        a: base.a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_to_bin_monotonic() {
        let half = FFT_SIZE / 2;
        let mut prev = 0;
        for band in 0..=NUM_BANDS {
            let bin = band_to_bin(band, NUM_BANDS, half);
            assert!(bin >= prev, "band_to_bin must be monotonically increasing");
            prev = bin;
        }
    }

    #[test]
    fn band_to_bin_range() {
        let half = FFT_SIZE / 2;
        assert_eq!(band_to_bin(0, NUM_BANDS, half), 1); // 最低ビン
        assert_eq!(band_to_bin(NUM_BANDS, NUM_BANDS, half), half); // 最大ビン
    }

    #[test]
    fn spectrum_update_with_silence() {
        let mut spec = Spectrum::new(
            "Test",
            Color {
                r: 0.2,
                g: 0.8,
                b: 0.9,
                a: 1.0,
            },
        );
        let silence = vec![0.0f32; FFT_SIZE];
        spec.update(&silence);
        // 無音なら全バンドほぼゼロ
        for &mag in &spec.smoothed {
            assert!(mag < 0.01, "silence should produce near-zero magnitudes");
        }
    }

    #[test]
    fn spectrum_update_with_tone() {
        let mut spec = Spectrum::new(
            "Test",
            Color {
                r: 0.2,
                g: 0.8,
                b: 0.9,
                a: 1.0,
            },
        );
        // 440Hz のサイン波（サンプルレート 44100Hz 想定）
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        spec.update(&samples);
        // 何らかのバンドにエネルギーがあるはず
        let max_mag = spec.smoothed.iter().cloned().fold(0.0f32, f32::max);
        assert!(
            max_mag > 0.1,
            "tone should produce visible energy, got {}",
            max_mag
        );
    }

    #[test]
    fn bar_color_brightness() {
        let base = Color {
            r: 0.2,
            g: 0.8,
            b: 0.9,
            a: 1.0,
        };
        let low = bar_color(0.0, &base);
        let high = bar_color(1.0, &base);
        assert!(high.r > low.r);
        assert!(high.g > low.g);
    }
}
