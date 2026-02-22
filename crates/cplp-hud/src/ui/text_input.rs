use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;
use crate::renderer::Renderer;
use super::event::{EventResponse, Key, MouseButton, UiEvent};
use super::widget::Widget;

/// HUD 風デザイン定数
const BG_COLOR: Color = Color { r: 0.12, g: 0.12, b: 0.15, a: 0.9 };
const FOCUSED_BG: Color = Color { r: 0.15, g: 0.15, b: 0.2, a: 0.9 };
const TEXT_COLOR: [f32; 4] = [0.85, 0.85, 0.85, 1.0];
const PLACEHOLDER_COLOR: [f32; 4] = [0.45, 0.45, 0.5, 1.0];
const CURSOR_COLOR: Color = Color { r: 0.85, g: 0.85, b: 0.85, a: 1.0 };
const TEXT_SIZE: f32 = 14.0;
const INPUT_WIDTH: f32 = 300.0;
const INPUT_HEIGHT: f32 = 32.0;
const PADDING_LEFT: f32 = 8.0;
const CHAR_WIDTH: f32 = 8.4;
const CURSOR_W: f32 = 2.0;

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
            size: Vec2 { x: INPUT_WIDTH, y: INPUT_HEIGHT },
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, s: &str) {
        self.text = s.to_string();
        self.cursor = self.text.len();
    }
}

impl Widget for TextInput {
    fn measure(&mut self, _available: Vec2) -> Vec2 {
        self.size
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        // 背景（focused で色変更）
        let bg = if self.focused { FOCUSED_BG } else { BG_COLOR };
        renderer.rect(rect, bg);

        let ty = rect.y + (rect.h - TEXT_SIZE) / 2.0;

        if self.text.is_empty() && !self.focused {
            // プレースホルダー表示
            renderer.text(TextEntry {
                text: self.placeholder.clone(),
                x: rect.x + PADDING_LEFT,
                y: ty,
                size: TEXT_SIZE,
                color: PLACEHOLDER_COLOR,
            });
        } else {
            // テキスト描画
            renderer.text(TextEntry {
                text: self.text.clone(),
                x: rect.x + PADDING_LEFT,
                y: ty,
                size: TEXT_SIZE,
                color: TEXT_COLOR,
            });
        }

        // カーソル描画（focused 時のみ）
        if self.focused {
            let cursor_x = rect.x + PADDING_LEFT + self.cursor as f32 * CHAR_WIDTH;
            renderer.rect(
                Rect { x: cursor_x, y: ty, w: CURSOR_W, h: TEXT_SIZE },
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
                        self.text.insert(self.cursor, *c);
                        self.cursor += 1;
                    }
                    Key::Backspace => {
                        if self.cursor > 0 {
                            self.cursor -= 1;
                            self.text.remove(self.cursor);
                        }
                    }
                    Key::Left => {
                        if self.cursor > 0 {
                            self.cursor -= 1;
                        }
                    }
                    Key::Right => {
                        if self.cursor < self.text.len() {
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
        let rect = Rect { x: 0.0, y: 0.0, w: 300.0, h: 32.0 };
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
        let rect = Rect { x: 0.0, y: 0.0, w: 300.0, h: 32.0 };

        // "abc" を入力
        input.event(&UiEvent::KeyDown(Key::Char('a')), rect);
        input.event(&UiEvent::KeyDown(Key::Char('b')), rect);
        input.event(&UiEvent::KeyDown(Key::Char('c')), rect);
        assert_eq!(input.cursor, 3);

        // 左に2つ移動
        input.event(&UiEvent::KeyDown(Key::Left), rect);
        input.event(&UiEvent::KeyDown(Key::Left), rect);
        assert_eq!(input.cursor, 1);

        // カーソル位置に挿入
        input.event(&UiEvent::KeyDown(Key::Char('x')), rect);
        assert_eq!(input.text(), "axbc");
    }

    #[test]
    fn text_input_focus_on_click() {
        let mut input = TextInput::new("placeholder");
        let rect = Rect { x: 0.0, y: 0.0, w: 300.0, h: 32.0 };
        assert!(!input.focused);

        // rect 内クリックでフォーカス
        input.event(&UiEvent::MouseDown(Vec2 { x: 50.0, y: 16.0 }, MouseButton::Left), rect);
        assert!(input.focused);

        // Enter で確定（フォーカス解除）
        input.event(&UiEvent::KeyDown(Key::Enter), rect);
        assert!(!input.focused);
    }

    #[test]
    fn text_input_unfocused_ignores_keys() {
        let mut input = TextInput::new("");
        let rect = Rect { x: 0.0, y: 0.0, w: 300.0, h: 32.0 };
        let r = input.event(&UiEvent::KeyDown(Key::Char('a')), rect);
        assert_eq!(r, EventResponse::Ignored);
        assert_eq!(input.text(), "");
    }
}
