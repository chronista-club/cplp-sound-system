use crate::renderer::primitives::Vec2;
use winit::event::{ElementState, MouseButton as WinitMouseButton, WindowEvent};
use winit::keyboard::{Key as WinitKey, NamedKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    Backspace,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Char(char),
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    MouseMove(Vec2),
    MouseDown(Vec2, MouseButton),
    MouseUp(Vec2, MouseButton),
    Scroll(Vec2),
    KeyDown(Key),
    TextInput(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResponse {
    Ignored,
    Consumed,
}

/// winit の `WindowEvent` を `UiEvent` に変換する。
/// 対応しないイベントは `None` を返す。
pub fn from_window_event(event: &WindowEvent) -> Option<UiEvent> {
    match event {
        WindowEvent::CursorMoved { position, .. } => Some(UiEvent::MouseMove(Vec2 {
            x: position.x as f32,
            y: position.y as f32,
        })),

        WindowEvent::MouseInput { state, button, .. } => {
            let btn = match button {
                WinitMouseButton::Left => MouseButton::Left,
                WinitMouseButton::Right => MouseButton::Right,
                _ => return None,
            };
            // マウス座標は CursorMoved で追跡する想定。
            // ここでは (0,0) を仮置き。実際のシステムでは最新カーソル位置を使う。
            let pos = Vec2 { x: 0.0, y: 0.0 };
            match state {
                ElementState::Pressed => Some(UiEvent::MouseDown(pos, btn)),
                ElementState::Released => Some(UiEvent::MouseUp(pos, btn)),
            }
        }

        WindowEvent::MouseWheel { delta, .. } => {
            let d = match delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => Vec2 { x: *x, y: *y },
                winit::event::MouseScrollDelta::PixelDelta(pos) => Vec2 {
                    x: pos.x as f32,
                    y: pos.y as f32,
                },
            };
            Some(UiEvent::Scroll(d))
        }

        WindowEvent::KeyboardInput { event, .. } => {
            if event.state != ElementState::Pressed {
                return None;
            }
            match &event.logical_key {
                WinitKey::Named(named) => {
                    let key = match named {
                        NamedKey::Enter => Key::Enter,
                        NamedKey::Escape => Key::Escape,
                        NamedKey::Backspace => Key::Backspace,
                        NamedKey::Tab => Key::Tab,
                        NamedKey::ArrowLeft => Key::Left,
                        NamedKey::ArrowRight => Key::Right,
                        NamedKey::ArrowUp => Key::Up,
                        NamedKey::ArrowDown => Key::Down,
                        _ => return None,
                    };
                    Some(UiEvent::KeyDown(key))
                }
                WinitKey::Character(s) => {
                    s.chars().next().map(|ch| UiEvent::KeyDown(Key::Char(ch)))
                }
                _ => None,
            }
        }

        _ => None,
    }
}
