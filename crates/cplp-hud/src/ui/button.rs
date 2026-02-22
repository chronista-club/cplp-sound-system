use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;
use crate::renderer::Renderer;
use super::event::{EventResponse, MouseButton, UiEvent};
use super::widget::Widget;

/// HUD 風デザイン定数
const BG_COLOR: Color = Color { r: 0.12, g: 0.12, b: 0.15, a: 0.9 };
const HOVER_COLOR: Color = Color { r: 0.2, g: 0.2, b: 0.25, a: 0.9 };
const ACTIVE_COLOR: Color = Color { r: 0.2, g: 0.6, b: 0.9, a: 0.9 };
const TEXT_COLOR: [f32; 4] = [0.85, 0.85, 0.85, 1.0];
const TEXT_SIZE: f32 = 14.0;
const BUTTON_HEIGHT: f32 = 36.0;
/// 1文字あたりの推定幅（モノスペース）
const CHAR_WIDTH: f32 = 8.4;
const PADDING_H: f32 = 16.0;

pub struct Button {
    label: String,
    pub(crate) hovered: bool,
    pub(crate) pressed: bool,
    on_click: Option<Box<dyn FnMut()>>,
    size: Vec2,
}

impl Button {
    pub fn new(label: &str) -> Self {
        let w = label.len() as f32 * CHAR_WIDTH + PADDING_H * 2.0;
        Self {
            label: label.to_string(),
            hovered: false,
            pressed: false,
            on_click: None,
            size: Vec2 { x: w, y: BUTTON_HEIGHT },
        }
    }

    pub fn on_click(mut self, f: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl Widget for Button {
    fn measure(&mut self, _available: Vec2) -> Vec2 {
        self.size
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        // 背景色: 状態に応じて切り替え
        let bg = if self.pressed {
            ACTIVE_COLOR
        } else if self.hovered {
            HOVER_COLOR
        } else {
            BG_COLOR
        };
        renderer.rect(rect, bg);

        // テキストを中央に配置
        let text_w = self.label.len() as f32 * CHAR_WIDTH;
        let tx = rect.x + (rect.w - text_w) / 2.0;
        let ty = rect.y + (rect.h - TEXT_SIZE) / 2.0;
        renderer.text(TextEntry {
            text: self.label.clone(),
            x: tx,
            y: ty,
            size: TEXT_SIZE,
            color: TEXT_COLOR,
        });
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        match event {
            UiEvent::MouseMove(pos) => {
                self.hovered = rect.contains(*pos);
                EventResponse::Ignored
            }
            UiEvent::MouseDown(pos, MouseButton::Left) => {
                if rect.contains(*pos) {
                    self.pressed = true;
                    EventResponse::Consumed
                } else {
                    EventResponse::Ignored
                }
            }
            UiEvent::MouseUp(pos, MouseButton::Left) => {
                if self.pressed && rect.contains(*pos) {
                    if let Some(cb) = &mut self.on_click {
                        cb();
                    }
                    self.pressed = false;
                    EventResponse::Consumed
                } else {
                    self.pressed = false;
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
    fn button_hover_detection() {
        let mut btn = Button::new("Test");
        let rect = Rect { x: 10.0, y: 10.0, w: 100.0, h: 36.0 };
        btn.event(&UiEvent::MouseMove(Vec2 { x: 50.0, y: 25.0 }), rect);
        assert!(btn.hovered);
        btn.event(&UiEvent::MouseMove(Vec2 { x: 200.0, y: 200.0 }), rect);
        assert!(!btn.hovered);
    }

    #[test]
    fn button_click_fires_callback() {
        use std::cell::Cell;
        use std::rc::Rc;

        let clicked = Rc::new(Cell::new(false));
        let clicked_clone = clicked.clone();
        let mut btn = Button::new("OK").on_click(move || {
            clicked_clone.set(true);
        });
        let rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 36.0 };

        // MouseDown → pressed
        let r = btn.event(&UiEvent::MouseDown(Vec2 { x: 50.0, y: 18.0 }, MouseButton::Left), rect);
        assert_eq!(r, EventResponse::Consumed);
        assert!(btn.pressed);

        // MouseUp → コールバック発火
        let r = btn.event(&UiEvent::MouseUp(Vec2 { x: 50.0, y: 18.0 }, MouseButton::Left), rect);
        assert_eq!(r, EventResponse::Consumed);
        assert!(clicked.get());
        assert!(!btn.pressed);
    }

    #[test]
    fn button_click_outside_ignored() {
        let mut btn = Button::new("X");
        let rect = Rect { x: 0.0, y: 0.0, w: 50.0, h: 36.0 };
        let r = btn.event(&UiEvent::MouseDown(Vec2 { x: 200.0, y: 200.0 }, MouseButton::Left), rect);
        assert_eq!(r, EventResponse::Ignored);
        assert!(!btn.pressed);
    }
}
