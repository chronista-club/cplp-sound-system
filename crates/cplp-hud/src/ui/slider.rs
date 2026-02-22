use super::event::{EventResponse, MouseButton, UiEvent};
use super::theme;
use super::widget::Widget;
use crate::renderer::Renderer;
use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;

const KNOB_COLOR: Color = Color {
    r: 0.85,
    g: 0.85,
    b: 0.85,
    a: 1.0,
};

pub struct Slider {
    value: f32,
    pub(crate) dragging: bool,
    pub(crate) hovered: bool,
    label: String,
    size: Vec2,
}

impl Slider {
    pub fn new(label: &str) -> Self {
        Self {
            value: 0.0,
            dragging: false,
            hovered: false,
            label: label.to_string(),
            size: Vec2 {
                x: theme::SLIDER_W,
                y: theme::SLIDER_H,
            },
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn set_value(&mut self, v: f32) {
        self.value = v.clamp(0.0, 1.0);
    }

    /// マウス x 座標から value を計算
    fn value_from_pos(&self, x: f32, rect: Rect) -> f32 {
        ((x - rect.x) / rect.w).clamp(0.0, 1.0)
    }
}

impl Widget for Slider {
    fn measure(&mut self, _available: Vec2) -> Vec2 {
        self.size
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        // 背景トラック
        let track_y = rect.y + (rect.h - theme::TRACK_H) / 2.0;
        renderer.rect(
            Rect {
                x: rect.x,
                y: track_y,
                w: rect.w,
                h: theme::TRACK_H,
            },
            theme::BG,
        );

        // フィル部分（value 分の幅）
        let fill_w = rect.w * self.value;
        renderer.rect(
            Rect {
                x: rect.x,
                y: track_y,
                w: fill_w,
                h: theme::TRACK_H,
            },
            theme::ACTIVE,
        );

        // ノブ（value 位置に小さい白矩形）
        let knob_x = rect.x + fill_w - theme::KNOB_W / 2.0;
        let knob_y = rect.y + (rect.h - theme::KNOB_H) / 2.0;
        renderer.rect(
            Rect {
                x: knob_x,
                y: knob_y,
                w: theme::KNOB_W,
                h: theme::KNOB_H,
            },
            KNOB_COLOR,
        );

        // ラベル（左端）
        renderer.text(TextEntry {
            text: self.label.clone(),
            x: rect.x,
            y: rect.y,
            size: theme::TEXT_SM,
            color: theme::TEXT_COLOR,
        });

        // 値テキスト（右端）
        let pct = format!("{:.0}%", self.value * 100.0);
        let pct_w = pct.len() as f32 * theme::CHAR_W;
        renderer.text(TextEntry {
            text: pct,
            x: rect.x + rect.w - pct_w,
            y: rect.y,
            size: theme::TEXT_SM,
            color: theme::TEXT_COLOR,
        });
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        match event {
            UiEvent::MouseMove(pos) => {
                self.hovered = rect.contains(*pos);
                if self.dragging {
                    self.value = self.value_from_pos(pos.x, rect);
                    EventResponse::Consumed
                } else {
                    EventResponse::Ignored
                }
            }
            UiEvent::MouseDown(pos, MouseButton::Left) => {
                if rect.contains(*pos) {
                    self.dragging = true;
                    self.value = self.value_from_pos(pos.x, rect);
                    EventResponse::Consumed
                } else {
                    EventResponse::Ignored
                }
            }
            UiEvent::MouseUp(_pos, MouseButton::Left) => {
                if self.dragging {
                    self.dragging = false;
                    EventResponse::Consumed
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_drag_updates_value() {
        let mut slider = Slider::new("Mix");
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 28.0,
        };
        slider.event(
            &UiEvent::MouseDown(Vec2 { x: 100.0, y: 14.0 }, MouseButton::Left),
            rect,
        );
        assert!((slider.value() - 0.5).abs() < 0.01);
    }

    #[test]
    fn slider_clamps_value() {
        let mut slider = Slider::new("Vol");
        slider.set_value(1.5);
        assert!((slider.value() - 1.0).abs() < f32::EPSILON);
        slider.set_value(-0.5);
        assert!(slider.value().abs() < f32::EPSILON);
    }

    #[test]
    fn slider_drag_and_release() {
        let mut slider = Slider::new("Pan");
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 28.0,
        };

        slider.event(
            &UiEvent::MouseDown(Vec2 { x: 50.0, y: 14.0 }, MouseButton::Left),
            rect,
        );
        assert!(slider.dragging);
        assert!((slider.value() - 0.25).abs() < 0.01);

        slider.event(&UiEvent::MouseMove(Vec2 { x: 150.0, y: 14.0 }), rect);
        assert!((slider.value() - 0.75).abs() < 0.01);

        slider.event(
            &UiEvent::MouseUp(Vec2 { x: 150.0, y: 14.0 }, MouseButton::Left),
            rect,
        );
        assert!(!slider.dragging);
    }
}
