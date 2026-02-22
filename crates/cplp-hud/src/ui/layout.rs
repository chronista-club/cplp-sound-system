use super::event::{EventResponse, UiEvent};
use super::widget::Widget;
use crate::renderer::Renderer;
use crate::renderer::primitives::{Rect, Vec2};

/// 子ウィジェットを垂直方向に並べるレイアウト
pub struct VStack {
    pub spacing: f32,
    pub children: Vec<Box<dyn Widget>>,
    child_sizes: Vec<Vec2>,
}

/// 子ウィジェットを水平方向に並べるレイアウト
pub struct HStack {
    pub spacing: f32,
    pub children: Vec<Box<dyn Widget>>,
    child_sizes: Vec<Vec2>,
}

/// 子ウィジェットにパディングを付与するラッパー
pub struct Padded {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
    pub child: Box<dyn Widget>,
}

/// 子ウィジェットを固定サイズに制約するラッパー
pub struct Fixed {
    pub size: Vec2,
    pub child: Box<dyn Widget>,
}

// ── VStack ───────────────────────────────────────────

impl Widget for VStack {
    fn measure(&mut self, available: Vec2) -> Vec2 {
        let mut max_w: f32 = 0.0;
        let mut total_h: f32 = 0.0;

        self.child_sizes.clear();
        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = child.measure(available);
            self.child_sizes.push(child_size);
            max_w = max_w.max(child_size.x);
            total_h += child_size.y;
            if i > 0 {
                total_h += self.spacing;
            }
        }

        Vec2 {
            x: max_w,
            y: total_h,
        }
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let mut y_offset = rect.y;

        for (i, child) in self.children.iter().enumerate() {
            let h = self.child_sizes.get(i).map(|s| s.y).unwrap_or(0.0);
            let child_rect = Rect {
                x: rect.x,
                y: y_offset,
                w: rect.w,
                h,
            };
            child.draw(renderer, child_rect);
            y_offset += h + self.spacing;
        }
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        let mut y_offset = rect.y;

        for (i, child) in self.children.iter_mut().enumerate() {
            let h = self.child_sizes.get(i).map(|s| s.y).unwrap_or(0.0);
            let child_rect = Rect {
                x: rect.x,
                y: y_offset,
                w: rect.w,
                h,
            };
            if let EventResponse::Consumed = child.event(event, child_rect) {
                return EventResponse::Consumed;
            }
            y_offset += h + self.spacing;
        }

        EventResponse::Ignored
    }
}

// ── HStack ───────────────────────────────────────────

impl Widget for HStack {
    fn measure(&mut self, available: Vec2) -> Vec2 {
        let mut total_w: f32 = 0.0;
        let mut max_h: f32 = 0.0;

        self.child_sizes.clear();
        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = child.measure(available);
            self.child_sizes.push(child_size);
            total_w += child_size.x;
            max_h = max_h.max(child_size.y);
            if i > 0 {
                total_w += self.spacing;
            }
        }

        Vec2 {
            x: total_w,
            y: max_h,
        }
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let mut x_offset = rect.x;

        for (i, child) in self.children.iter().enumerate() {
            let w = self.child_sizes.get(i).map(|s| s.x).unwrap_or(0.0);
            let child_rect = Rect {
                x: x_offset,
                y: rect.y,
                w,
                h: rect.h,
            };
            child.draw(renderer, child_rect);
            x_offset += w + self.spacing;
        }
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        let mut x_offset = rect.x;

        for (i, child) in self.children.iter_mut().enumerate() {
            let w = self.child_sizes.get(i).map(|s| s.x).unwrap_or(0.0);
            let child_rect = Rect {
                x: x_offset,
                y: rect.y,
                w,
                h: rect.h,
            };
            if let EventResponse::Consumed = child.event(event, child_rect) {
                return EventResponse::Consumed;
            }
            x_offset += w + self.spacing;
        }

        EventResponse::Ignored
    }
}

// ── Padded ───────────────────────────────────────────

impl Widget for Padded {
    fn measure(&mut self, available: Vec2) -> Vec2 {
        let inner_available = Vec2 {
            x: available.x - self.left - self.right,
            y: available.y - self.top - self.bottom,
        };
        let child_size = self.child.measure(inner_available);
        Vec2 {
            x: child_size.x + self.left + self.right,
            y: child_size.y + self.top + self.bottom,
        }
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let inner_rect = Rect {
            x: rect.x + self.left,
            y: rect.y + self.top,
            w: rect.w - self.left - self.right,
            h: rect.h - self.top - self.bottom,
        };
        self.child.draw(renderer, inner_rect);
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        let inner_rect = Rect {
            x: rect.x + self.left,
            y: rect.y + self.top,
            w: rect.w - self.left - self.right,
            h: rect.h - self.top - self.bottom,
        };
        self.child.event(event, inner_rect)
    }
}

// ── Fixed ────────────────────────────────────────────

impl Widget for Fixed {
    fn measure(&mut self, _available: Vec2) -> Vec2 {
        self.size
    }

    fn draw(&self, renderer: &mut Renderer, rect: Rect) {
        let inner_rect = Rect {
            x: rect.x,
            y: rect.y,
            w: self.size.x,
            h: self.size.y,
        };
        self.child.draw(renderer, inner_rect);
    }

    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse {
        let inner_rect = Rect {
            x: rect.x,
            y: rect.y,
            w: self.size.x,
            h: self.size.y,
        };
        self.child.event(event, inner_rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyWidget {
        size: Vec2,
    }

    impl Widget for DummyWidget {
        fn measure(&mut self, _available: Vec2) -> Vec2 {
            self.size
        }
        fn draw(&self, _renderer: &mut Renderer, _rect: Rect) {}
        fn event(&mut self, _event: &UiEvent, _rect: Rect) -> EventResponse {
            EventResponse::Ignored
        }
    }

    #[test]
    fn vstack_layout_sizes() {
        let mut stack = VStack {
            spacing: 10.0,
            child_sizes: Vec::new(),
            children: vec![
                Box::new(DummyWidget {
                    size: Vec2 { x: 100.0, y: 50.0 },
                }),
                Box::new(DummyWidget {
                    size: Vec2 { x: 80.0, y: 50.0 },
                }),
            ],
        };
        let result = stack.measure(Vec2 { x: 640.0, y: 480.0 });
        assert!((result.x - 100.0).abs() < f32::EPSILON); // 最大幅
        assert!((result.y - 110.0).abs() < f32::EPSILON); // 50 + 10 + 50
    }

    #[test]
    fn hstack_layout_sizes() {
        let mut stack = HStack {
            spacing: 10.0,
            child_sizes: Vec::new(),
            children: vec![
                Box::new(DummyWidget {
                    size: Vec2 { x: 100.0, y: 50.0 },
                }),
                Box::new(DummyWidget {
                    size: Vec2 { x: 80.0, y: 60.0 },
                }),
            ],
        };
        let result = stack.measure(Vec2 { x: 640.0, y: 480.0 });
        assert!((result.x - 190.0).abs() < f32::EPSILON); // 100 + 10 + 80
        assert!((result.y - 60.0).abs() < f32::EPSILON); // 最大高さ
    }
}
