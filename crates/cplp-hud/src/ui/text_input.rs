use super::event::{EventResponse, Key, MouseButton, UiEvent};
use super::theme;
use super::widget::Widget;
use crate::renderer::Renderer;
use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;

const FOCUSED_BG: Color = Color {
    r: 0.15,
    g: 0.15,
    b: 0.2,
    a: 0.9,
};
const CURSOR_COLOR: Color = Color {
    r: 0.85,
    g: 0.85,
    b: 0.85,
    a: 1.0,
};

pub struct TextInput {
    text: String,
    pub(crate) cursor: usize,
    pub(crate) focused: bool,
    placeholder: String,
    size: Vec2,
}

impl TextInput {
    pub fn new(placeholder: &str) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            focused: false,
            placeholder: placeholder.to_string(),
            size: Vec2 {
                x: theme::HALF_W,
                y: theme::INPUT_H,
            },
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, s: &str) {
        self.text = s.to_string();
        self.cursor = self.text.chars().count();
    }

    /// カーソル位置（文字数）をバイトオフセットに変換
    fn cursor_byte_offset(&self) -> usize {
        self.text
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }
}

impl Widget for TextInput {
    fn measure(&mut self, _available: Vec2) -> Vec2 {
        self.size
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let bg = if self.focused { FOCUSED_BG } else { theme::BG };
        renderer.rect(rect, bg);

        let ty = rect.y + (rect.h - theme::TEXT_SM) / 2.0;

        if self.text.is_empty() && !self.focused {
            renderer.text(TextEntry {
                text: self.placeholder.clone(),
                x: rect.x + theme::PAD_LEFT,
                y: ty,
                size: theme::TEXT_SM,
                color: theme::PLACEHOLDER,
            });
        } else {
            renderer.text(TextEntry {
                text: self.text.clone(),
                x: rect.x + theme::PAD_LEFT,
                y: ty,
                size: theme::TEXT_SM,
                color: theme::TEXT_COLOR,
            });
        }

        if self.focused {
            let cursor_x = rect.x + theme::PAD_LEFT + self.cursor as f32 * theme::CHAR_W;
            renderer.rect(
                Rect {
                    x: cursor_x,
                    y: ty,
                    w: theme::CURSOR_W,
                    h: theme::TEXT_SM,
                },
                CURSOR_COLOR,
            );
        }
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        match event {
            UiEvent::MouseDown(pos, MouseButton::Left) => {
                if rect.contains(*pos) {
                    self.focused = true;
                    EventResponse::Consumed
                } else {
                    self.focused = false;
                    EventResponse::Ignored
                }
            }
            UiEvent::KeyDown(key) if self.focused => {
                match key {
                    Key::Char(c) => {
                        let byte_pos = self.cursor_byte_offset();
                        self.text.insert(byte_pos, *c);
                        self.cursor += 1;
                    }
                    Key::Backspace => {
                        if self.cursor > 0 {
                            self.cursor -= 1;
                            let byte_pos = self.cursor_byte_offset();
                            self.text.remove(byte_pos);
                        }
                    }
                    Key::Left => {
                        if self.cursor > 0 {
                            self.cursor -= 1;
                        }
                    }
                    Key::Right => {
                        if self.cursor < self.text.chars().count() {
                            self.cursor += 1;
                        }
                    }
                    Key::Enter => {
                        self.focused = false;
                    }
                    _ => return EventResponse::Ignored,
                }
                EventResponse::Consumed
            }
            _ => EventResponse::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_typing() {
        let mut input = TextInput::new("Enter session ID");
        input.focused = true;
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 32.0,
        };
        input.event(&UiEvent::KeyDown(Key::Char('a')), rect);
        input.event(&UiEvent::KeyDown(Key::Char('b')), rect);
        assert_eq!(input.text(), "ab");
        input.event(&UiEvent::KeyDown(Key::Backspace), rect);
        assert_eq!(input.text(), "a");
    }

    #[test]
    fn text_input_cursor_movement() {
        let mut input = TextInput::new("");
        input.focused = true;
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 32.0,
        };

        input.event(&UiEvent::KeyDown(Key::Char('a')), rect);
        input.event(&UiEvent::KeyDown(Key::Char('b')), rect);
        input.event(&UiEvent::KeyDown(Key::Char('c')), rect);
        assert_eq!(input.cursor, 3);

        input.event(&UiEvent::KeyDown(Key::Left), rect);
        input.event(&UiEvent::KeyDown(Key::Left), rect);
        assert_eq!(input.cursor, 1);

        input.event(&UiEvent::KeyDown(Key::Char('x')), rect);
        assert_eq!(input.text(), "axbc");
    }

    #[test]
    fn text_input_focus_on_click() {
        let mut input = TextInput::new("placeholder");
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 32.0,
        };
        assert!(!input.focused);

        input.event(
            &UiEvent::MouseDown(Vec2 { x: 50.0, y: 16.0 }, MouseButton::Left),
            rect,
        );
        assert!(input.focused);

        input.event(&UiEvent::KeyDown(Key::Enter), rect);
        assert!(!input.focused);
    }

    #[test]
    fn text_input_unfocused_ignores_keys() {
        let mut input = TextInput::new("");
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 32.0,
        };
        let r = input.event(&UiEvent::KeyDown(Key::Char('a')), rect);
        assert_eq!(r, EventResponse::Ignored);
        assert_eq!(input.text(), "");
    }
}
