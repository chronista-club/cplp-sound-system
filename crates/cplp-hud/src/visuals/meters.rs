use crate::renderer::Renderer;
use crate::renderer::primitives::{Color, Rect};
use crate::renderer::text::TextEntry;

/// レベル値 (0.0–1.0) を dB に変換
pub fn level_to_db(level: f32) -> f32 {
    20.0 * level.max(1e-6).log10()
}

/// レベル値に応じたメーターカラーを返す（緑→黄→赤）
pub fn meter_color(level: f32) -> Color {
    if level < 0.6 {
        Color {
            r: 0.2,
            g: 0.8,
            b: 0.4,
            a: 1.0,
        }
    } else if level < 0.85 {
        Color {
            r: 0.9,
            g: 0.8,
            b: 0.2,
            a: 1.0,
        }
    } else {
        Color {
            r: 0.9,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        }
    }
}

pub struct LevelMeter {
    /// 現在のレベル (0.0 ~ 1.0)
    level: f32,
    /// ピーク値 (0.0 ~ 1.0)
    pub(crate) peak: f32,
    /// ピーク減衰タイマー
    peak_hold_frames: u32,
    /// ラベル
    label: String,
}

impl LevelMeter {
    pub fn new(label: &str) -> Self {
        Self {
            level: 0.0,
            peak: 0.0,
            peak_hold_frames: 0,
            label: label.to_string(),
        }
    }

    /// 毎フレーム呼び出し: レベルとピークを更新
    pub fn update(&mut self, level: f32) {
        self.level = level.clamp(0.0, 1.0);

        if self.level > self.peak {
            self.peak = self.level;
            self.peak_hold_frames = 60;
        } else if self.peak_hold_frames > 0 {
            self.peak_hold_frames -= 1;
        } else {
            self.peak -= 0.01;
            self.peak = self.peak.max(0.0);
        }
    }

    /// 描画（Renderer の rect() と text() を使う）
    pub fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let bg_color = Color {
            r: 0.15,
            g: 0.15,
            b: 0.18,
            a: 1.0,
        };

        // 1. 背景矩形
        renderer.rect(rect, bg_color);

        // 2. メーターバー（level 分の幅）
        let bar_w = rect.w * self.level;
        if bar_w > 0.0 {
            renderer.rect(
                Rect {
                    x: rect.x,
                    y: rect.y,
                    w: bar_w,
                    h: rect.h,
                },
                meter_color(self.level),
            );
        }

        // 3. ピーク縦線（白、幅 2px）
        if self.peak > 0.0 {
            let peak_x = rect.x + rect.w * self.peak - 1.0;
            renderer.rect(
                Rect {
                    x: peak_x,
                    y: rect.y,
                    w: 2.0,
                    h: rect.h,
                },
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            );
        }

        // 4. ラベルテキスト（左端）
        renderer.text(TextEntry {
            text: self.label.clone(),
            x: rect.x + 4.0,
            y: rect.y + 2.0,
            size: 14.0,
            color: [1.0, 1.0, 1.0, 1.0],
        });

        // 5. dB テキスト（右端）
        let db = level_to_db(self.level);
        renderer.text(TextEntry {
            text: format!("{:.1} dB", db),
            x: rect.x + rect.w - 70.0,
            y: rect.y + 2.0,
            size: 14.0,
            color: [0.8, 0.8, 0.8, 1.0],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_to_db_conversion() {
        // 1.0 → 0.0 dB
        assert!((level_to_db(1.0) - 0.0).abs() < 0.01);
        // 0.5 → ~-6.02 dB
        assert!((level_to_db(0.5) - (-6.02)).abs() < 0.1);
    }

    #[test]
    fn peak_hold_and_decay() {
        let mut meter = LevelMeter::new("Test");
        meter.update(0.8);
        assert!((meter.peak - 0.8).abs() < f32::EPSILON);

        // レベルが下がってもピークはホールド
        meter.update(0.3);
        assert!((meter.peak - 0.8).abs() < f32::EPSILON);

        // 60 フレーム後にピーク減衰開始
        for _ in 0..60 {
            meter.update(0.0);
        }
        assert!(meter.peak < 0.8);
    }

    #[test]
    fn meter_color_gradient() {
        assert_eq!(
            meter_color(0.3),
            Color {
                r: 0.2,
                g: 0.8,
                b: 0.4,
                a: 1.0
            }
        ); // 緑
        assert_eq!(
            meter_color(0.7),
            Color {
                r: 0.9,
                g: 0.8,
                b: 0.2,
                a: 1.0
            }
        ); // 黄
        assert_eq!(
            meter_color(0.9),
            Color {
                r: 0.9,
                g: 0.2,
                b: 0.2,
                a: 1.0
            }
        ); // 赤
    }
}
