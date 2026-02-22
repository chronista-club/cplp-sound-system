use crate::renderer::Renderer;
use crate::renderer::primitives::{Color, Rect};
use crate::renderer::text::TextEntry;
use crate::state::SessionSnapshot;

/// 接続状態インジケーター
///
/// ピア名・接続状態ドット・レイテンシ・ジッタを表示する。
/// SessionSnapshot の内容を描画に反映する。
pub struct ConnectionIndicator {
    pub(crate) snapshot: SessionSnapshot,
}

impl Default for ConnectionIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionIndicator {
    pub fn new() -> Self {
        Self {
            snapshot: SessionSnapshot::default(),
        }
    }

    /// SessionSnapshot で状態更新
    pub fn update(&mut self, snapshot: &SessionSnapshot) {
        self.snapshot = snapshot.clone();
    }

    /// 指定された矩形領域に接続状態を描画する
    pub fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let padding = 8.0;
        let dot_size = 8.0;
        let font_size = 14.0;

        // 背景矩形
        renderer.rect(
            rect,
            Color {
                r: 0.1,
                g: 0.1,
                b: 0.12,
                a: 0.8,
            },
        );

        // ステータスドット（接続中: 緑 / 切断: 赤）
        let dot_color = if self.snapshot.connected {
            Color {
                r: 0.2,
                g: 0.9,
                b: 0.4,
                a: 1.0,
            }
        } else {
            Color {
                r: 0.9,
                g: 0.2,
                b: 0.2,
                a: 1.0,
            }
        };

        let dot_y = rect.y + (rect.h - dot_size) / 2.0;
        renderer.rect(
            Rect {
                x: rect.x + padding,
                y: dot_y,
                w: dot_size,
                h: dot_size,
            },
            dot_color,
        );

        // ピア名テキスト（ドットの右）
        let peer_text = if self.snapshot.connected {
            format!("● {}", self.snapshot.peer_name)
        } else {
            "● Disconnected".to_string()
        };
        let text_y = rect.y + (rect.h - font_size) / 2.0;
        renderer.text(TextEntry {
            text: peer_text,
            x: rect.x + padding + dot_size + padding,
            y: text_y,
            size: font_size,
            color: [dot_color.r, dot_color.g, dot_color.b, dot_color.a],
        });

        // ジッタテキスト（右端寄せ）
        let jitter_text = format!("±{:.1}ms", self.snapshot.jitter_ms);
        let jitter_width = jitter_text.len() as f32 * font_size * 0.6;
        renderer.text(TextEntry {
            text: jitter_text,
            x: rect.x + rect.w - padding - jitter_width,
            y: text_y,
            size: font_size,
            color: [0.7, 0.7, 0.7, 1.0],
        });

        // レイテンシテキスト（ジッタの左）
        let latency_text = format!("{:.1}ms", self.snapshot.latency_ms);
        let latency_width = latency_text.len() as f32 * font_size * 0.6;
        let lat_color = latency_color(self.snapshot.latency_ms);
        renderer.text(TextEntry {
            text: latency_text,
            x: rect.x + rect.w - padding - jitter_width - padding - latency_width,
            y: text_y,
            size: font_size,
            color: [lat_color.r, lat_color.g, lat_color.b, lat_color.a],
        });
    }
}

/// レイテンシ値に応じた色を返す
fn latency_color(latency_ms: f32) -> Color {
    if latency_ms < 10.0 {
        Color {
            r: 0.2,
            g: 0.9,
            b: 0.4,
            a: 1.0,
        } // 緑
    } else if latency_ms < 20.0 {
        Color {
            r: 0.9,
            g: 0.8,
            b: 0.2,
            a: 1.0,
        } // 黄
    } else {
        Color {
            r: 0.9,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        } // 赤
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_color_green() {
        let c = latency_color(5.0);
        assert!((c.g - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn latency_color_yellow() {
        let c = latency_color(15.0);
        assert!((c.r - 0.9).abs() < f32::EPSILON);
        assert!((c.g - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn latency_color_red() {
        let c = latency_color(25.0);
        assert!((c.r - 0.9).abs() < f32::EPSILON);
        assert!((c.g - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn update_snapshot() {
        let mut indicator = ConnectionIndicator::new();
        let snap = SessionSnapshot {
            peer_name: "Alice".into(),
            connected: true,
            latency_ms: 8.0,
            jitter_ms: 1.2,
            ..Default::default()
        };
        indicator.update(&snap);
        assert_eq!(indicator.snapshot.peer_name, "Alice");
        assert!(indicator.snapshot.connected);
    }
}
