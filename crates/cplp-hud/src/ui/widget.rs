use crate::renderer::primitives::{Rect, Vec2};
use crate::renderer::Renderer;
use super::event::{UiEvent, EventResponse};

/// UI ウィジェットの共通トレイト。
/// measure → draw → event のライフサイクルで駆動される。
pub trait Widget {
    /// レイアウト計算（希望サイズを返す）
    fn measure(&mut self, available: Vec2) -> Vec2;
    /// 描画
    fn draw(&self, renderer: &mut Renderer, rect: Rect);
    /// イベント処理
    fn event(&mut self, event: &UiEvent, rect: Rect) -> EventResponse;
}
