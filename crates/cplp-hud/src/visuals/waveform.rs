use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::Renderer;

/// 表示するサンプル数
const DISPLAY_SAMPLES: usize = 512;

pub struct Waveform {
    /// 内部バッファ（リングバッファから読み出したサンプルをコピー）
    pub(crate) samples: Vec<f32>,
    /// 描画色
    color: Color,
    /// ラベル（"You" or "Peer"）— テキスト描画で使用予定
    #[allow(dead_code)]
    label: String,
}

impl Waveform {
    pub fn new(label: &str, color: Color) -> Self {
        Self {
            samples: Vec::new(),
            color,
            label: label.to_string(),
        }
    }

    /// サンプルデータを更新
    pub fn update(&mut self, new_samples: &[f32]) {
        self.samples.clear();
        if new_samples.len() > DISPLAY_SAMPLES {
            // 末尾 DISPLAY_SAMPLES 分だけ保持
            self.samples
                .extend_from_slice(&new_samples[new_samples.len() - DISPLAY_SAMPLES..]);
        } else {
            self.samples.extend_from_slice(new_samples);
        }
    }

    /// 描画
    pub fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        // 1. 背景矩形
        let bg_color = Color {
            r: 0.05,
            g: 0.05,
            b: 0.08,
            a: 0.6,
        };
        renderer.rect(rect, bg_color);

        // 2. ゼロライン（中央の水平線）
        let center_y = rect.y + rect.h / 2.0;
        let grey = Color {
            r: 0.3,
            g: 0.3,
            b: 0.3,
            a: 0.5,
        };
        renderer.polyline(
            &[
                Vec2 {
                    x: rect.x,
                    y: center_y,
                },
                Vec2 {
                    x: rect.x + rect.w,
                    y: center_y,
                },
            ],
            grey,
        );

        // 3. ウェーブフォーム本体
        if self.samples.len() < 2 {
            return;
        }

        let step = rect.w / (self.samples.len() as f32 - 1.0).max(1.0);
        let points: Vec<Vec2> = self
            .samples
            .iter()
            .enumerate()
            .map(|(i, &s)| Vec2 {
                x: rect.x + i as f32 * step,
                y: center_y - s * (rect.h / 2.0),
            })
            .collect();
        renderer.polyline(&points, self.color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_update_replaces_samples() {
        let mut wf = Waveform::new(
            "Test",
            Color {
                r: 0.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
        );
        wf.update(&[0.1, 0.2, 0.3]);
        assert_eq!(wf.samples.len(), 3);
        assert!((wf.samples[0] - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn waveform_clamps_to_display_samples() {
        let mut wf = Waveform::new(
            "Test",
            Color {
                r: 0.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
        );
        let large = vec![0.5; DISPLAY_SAMPLES + 100];
        wf.update(&large);
        assert_eq!(wf.samples.len(), DISPLAY_SAMPLES);
    }
}
