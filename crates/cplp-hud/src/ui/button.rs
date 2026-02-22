use super::event::{EventResponse, MouseButton, UiEvent};
use super::theme;
use super::widget::Widget;
use crate::renderer::Renderer;
use crate::renderer::primitives::{Rect, Vec2};
use crate::renderer::text::TextEntry;

pub struct Button {
    label: String,
    pub(crate) hovered: bool,
    pub(crate) pressed: bool,
    enabled: bool,
    on_click: Option<Box<dyn FnMut()>>,
    size: Vec2,
}

impl Button {
    pub fn new(label: &str) -> Self {
        let w = label.len() as f32 * theme::CHAR_W + theme::PAD_BTN_H * 2.0;
        Self {
            label: label.to_string(),
            hovered: false,
            pressed: false,
            enabled: true,
            on_click: None,
            size: Vec2 {
                x: w,
                y: theme::BUTTON_H,
            },
        }
    }

    pub fn on_click(mut self, f: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.hovered = false;
            self.pressed = false;
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

impl Widget for Button {
    fn measure(&mut self, _available: Vec2) -> Vec2 {
        self.size
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let (bg, text_color) = if !self.enabled {
            (theme::DISABLED_BG, theme::TEXT_DISABLED)
        } else if self.pressed {
            (theme::ACTIVE, theme::TEXT_COLOR)
        } else if self.hovered {
            (theme::HOVER, theme::TEXT_COLOR)
        } else {
            (theme::BG, theme::TEXT_COLOR)
        };
        renderer.rect(rect, bg);

        let text_w = self.label.len() as f32 * theme::CHAR_W;
        let tx = rect.x + (rect.w - text_w) / 2.0;
        let ty = rect.y + (rect.h - theme::TEXT_SM) / 2.0;
        renderer.text(TextEntry {
            text: self.label.clone(),
            x: tx,
            y: ty,
            size: theme::TEXT_SM,
            color: text_color,
        });
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
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
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 36.0,
        };
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
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 36.0,
        };

        let r = btn.event(
            &UiEvent::MouseDown(Vec2 { x: 50.0, y: 18.0 }, MouseButton::Left),
            rect,
        );
        assert_eq!(r, EventResponse::Consumed);
        assert!(btn.pressed);

        let r = btn.event(
            &UiEvent::MouseUp(Vec2 { x: 50.0, y: 18.0 }, MouseButton::Left),
            rect,
        );
        assert_eq!(r, EventResponse::Consumed);
        assert!(clicked.get());
        assert!(!btn.pressed);
    }

    #[test]
    fn button_disabled_ignores_click() {
        use std::cell::Cell;
        use std::rc::Rc;

        let clicked = Rc::new(Cell::new(false));
        let clicked_clone = clicked.clone();
        let mut btn = Button::new("OK").on_click(move || {
            clicked_clone.set(true);
        });
        btn.set_enabled(false);
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 36.0,
        };

        let r = btn.event(
            &UiEvent::MouseDown(Vec2 { x: 50.0, y: 18.0 }, MouseButton::Left),
            rect,
        );
        assert_eq!(r, EventResponse::Ignored);
        assert!(!btn.pressed);

        let r = btn.event(
            &UiEvent::MouseUp(Vec2 { x: 50.0, y: 18.0 }, MouseButton::Left),
            rect,
        );
        assert_eq!(r, EventResponse::Ignored);
        assert!(!clicked.get());
    }

    #[test]
    fn button_click_outside_ignored() {
        let mut btn = Button::new("X");
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 36.0,
        };
        let r = btn.event(
            &UiEvent::MouseDown(Vec2 { x: 200.0, y: 200.0 }, MouseButton::Left),
            rect,
        );
        assert_eq!(r, EventResponse::Ignored);
        assert!(!btn.pressed);
    }
}
