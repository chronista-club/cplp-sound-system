use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;
use crate::renderer::Renderer;
use super::event::{EventResponse, Key, MouseButton, UiEvent};
use super::widget::Widget;

/// HUD 風デザイン定数
const BG_COLOR: Color = Color { r: 0.12, g: 0.12, b: 0.15, a: 0.9 };
const HOVER_COLOR: Color = Color { r: 0.2, g: 0.2, b: 0.25, a: 0.9 };
const ACTIVE_COLOR: Color = Color { r: 0.2, g: 0.6, b: 0.9, a: 0.9 };
const TEXT_COLOR: [f32; 4] = [0.85, 0.85, 0.85, 1.0];
const TEXT_SIZE: f32 = 14.0;
const ITEM_HEIGHT: f32 = 30.0;
const PADDING_LEFT: f32 = 8.0;

pub struct List {
    items: Vec<String>,
    selected: Option<usize>,
    scroll_offset: usize,
    visible_count: usize,
    item_height: f32,
    pub(crate) hovered_index: Option<usize>,
}

impl List {
    pub fn new(visible_count: usize) -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            scroll_offset: 0,
            visible_count,
            item_height: ITEM_HEIGHT,
            hovered_index: None,
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
        self.selected = None;
        self.scroll_offset = 0;
    }

    /// y 座標からアイテムの絶対インデックスを計算
    fn index_at(&self, y: f32, rect: Rect) -> Option<usize> {
        if y < rect.y || y > rect.y + rect.h {
            return None;
        }
        let relative_y = y - rect.y;
        let row = (relative_y / self.item_height) as usize;
        let idx = self.scroll_offset + row;
        if idx < self.items.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// スクロールオフセットの最大値
    fn max_scroll(&self) -> usize {
        self.items.len().saturating_sub(self.visible_count)
    }
}

impl Widget for List {
    fn measure(&mut self, available: Vec2) -> Vec2 {
        Vec2 {
            x: available.x,
            y: self.visible_count as f32 * self.item_height,
        }
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        // 背景
        renderer.rect(rect, BG_COLOR);

        // visible_count 分のアイテムを描画
        for i in 0..self.visible_count {
            let idx = self.scroll_offset + i;
            if idx >= self.items.len() {
                break;
            }

            let item_rect = Rect {
                x: rect.x,
                y: rect.y + i as f32 * self.item_height,
                w: rect.w,
                h: self.item_height,
            };

            // 選択・ホバー背景
            if self.selected == Some(idx) {
                renderer.rect(item_rect, ACTIVE_COLOR);
            } else if self.hovered_index == Some(idx) {
                renderer.rect(item_rect, HOVER_COLOR);
            }

            // テキスト描画
            let ty = item_rect.y + (self.item_height - TEXT_SIZE) / 2.0;
            renderer.text(TextEntry {
                text: self.items[idx].clone(),
                x: item_rect.x + PADDING_LEFT,
                y: ty,
                size: TEXT_SIZE,
                color: TEXT_COLOR,
            });
        }
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        match event {
            UiEvent::MouseMove(pos) => {
                self.hovered_index = self.index_at(pos.y, rect);
                EventResponse::Ignored
            }
            UiEvent::MouseDown(pos, MouseButton::Left) => {
                if let Some(idx) = self.index_at(pos.y, rect) {
                    self.selected = Some(idx);
                    EventResponse::Consumed
                } else {
                    EventResponse::Ignored
                }
            }
            UiEvent::Scroll(delta) => {
                if delta.y < 0.0 && self.scroll_offset < self.max_scroll() {
                    self.scroll_offset += 1;
                } else if delta.y > 0.0 && self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
                EventResponse::Consumed
            }
            UiEvent::KeyDown(Key::Up) => {
                if let Some(sel) = self.selected {
                    if sel > 0 {
                        self.selected = Some(sel - 1);
                        // スクロール追従
                        if sel - 1 < self.scroll_offset {
                            self.scroll_offset = sel - 1;
                        }
                    }
                } else if !self.items.is_empty() {
                    self.selected = Some(0);
                }
                EventResponse::Consumed
            }
            UiEvent::KeyDown(Key::Down) => {
                if let Some(sel) = self.selected {
                    if sel + 1 < self.items.len() {
                        self.selected = Some(sel + 1);
                        // スクロール追従
                        if sel + 1 >= self.scroll_offset + self.visible_count {
                            self.scroll_offset = sel + 1 - self.visible_count + 1;
                        }
                    }
                } else if !self.items.is_empty() {
                    self.selected = Some(0);
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
    fn list_selection() {
        let mut list = List::new(5);
        list.set_items(vec!["A".into(), "B".into(), "C".into()]);
        let rect = Rect { x: 0.0, y: 0.0, w: 200.0, h: 150.0 };
        // クリックで2番目を選択 (item_height = 30.0)
        list.event(&UiEvent::MouseDown(Vec2 { x: 50.0, y: 35.0 }, MouseButton::Left), rect);
        assert_eq!(list.selected(), Some(1));
    }

    #[test]
    fn list_keyboard_navigation() {
        let mut list = List::new(5);
        list.set_items(vec!["A".into(), "B".into(), "C".into()]);
        let rect = Rect { x: 0.0, y: 0.0, w: 200.0, h: 150.0 };

        // Down で初期選択
        list.event(&UiEvent::KeyDown(Key::Down), rect);
        assert_eq!(list.selected(), Some(0));

        // Down で次へ
        list.event(&UiEvent::KeyDown(Key::Down), rect);
        assert_eq!(list.selected(), Some(1));

        // Up で戻る
        list.event(&UiEvent::KeyDown(Key::Up), rect);
        assert_eq!(list.selected(), Some(0));

        // Up で境界チェック（0 以下にならない）
        list.event(&UiEvent::KeyDown(Key::Up), rect);
        assert_eq!(list.selected(), Some(0));
    }

    #[test]
    fn list_hover_tracking() {
        let mut list = List::new(5);
        list.set_items(vec!["A".into(), "B".into(), "C".into()]);
        let rect = Rect { x: 0.0, y: 0.0, w: 200.0, h: 150.0 };

        list.event(&UiEvent::MouseMove(Vec2 { x: 50.0, y: 65.0 }), rect);
        assert_eq!(list.hovered_index, Some(2));

        // 範囲外
        list.event(&UiEvent::MouseMove(Vec2 { x: 50.0, y: 200.0 }), rect);
        assert_eq!(list.hovered_index, None);
    }

    #[test]
    fn list_scroll() {
        let mut list = List::new(2);
        list.set_items(vec!["A".into(), "B".into(), "C".into(), "D".into()]);
        let rect = Rect { x: 0.0, y: 0.0, w: 200.0, h: 60.0 };

        assert_eq!(list.scroll_offset, 0);
        // 下スクロール（delta.y < 0）
        list.event(&UiEvent::Scroll(Vec2 { x: 0.0, y: -1.0 }), rect);
        assert_eq!(list.scroll_offset, 1);

        // 上スクロール（delta.y > 0）
        list.event(&UiEvent::Scroll(Vec2 { x: 0.0, y: 1.0 }), rect);
        assert_eq!(list.scroll_offset, 0);
    }
}
