//! 入力状態管理
//!
//! winit のイベントを内部表現に変換し、カメラ操作・選択に必要な
//! マウス位置・ボタン状態・ドラッグ情報を管理する。

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};

/// マウスボタンの状態
#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonState {
    pub pressed: bool,
}

/// ドラッグ操作の状態
#[derive(Debug, Clone, Copy)]
pub struct DragState {
    /// ドラッグ開始位置（ピクセル）
    pub start: [f32; 2],
    /// 前フレームの位置（ピクセル）
    pub prev: [f32; 2],
}

/// 入力状態
///
/// 毎フレーム `process_event` で winit イベントを受け取り、
/// カメラコントローラーやセレクション処理が参照する。
pub struct InputState {
    /// 現在のマウス位置（ピクセル）
    pub cursor_pos: [f32; 2],
    /// ウィンドウサイズ（ピクセル）
    pub window_size: [f32; 2],

    /// 左ボタン
    pub left: ButtonState,
    /// 中ボタン
    pub middle: ButtonState,
    /// 右ボタン
    pub right: ButtonState,

    /// 左ボタンドラッグ
    pub left_drag: Option<DragState>,
    /// 中ボタンドラッグ
    pub middle_drag: Option<DragState>,
    /// 右ボタンドラッグ
    pub right_drag: Option<DragState>,

    /// 今フレームのスクロール量
    pub scroll_delta: f32,

    /// 今フレームでクリック（press → release が短距離）されたか
    pub clicked: bool,
    /// クリック位置（ピクセル）
    pub click_pos: [f32; 2],
}

/// ドラッグと判定する最小移動距離（ピクセル）
const CLICK_THRESHOLD: f32 = 4.0;

impl InputState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            cursor_pos: [0.0; 2],
            window_size: [width as f32, height as f32],
            left: ButtonState::default(),
            middle: ButtonState::default(),
            right: ButtonState::default(),
            left_drag: None,
            middle_drag: None,
            right_drag: None,
            scroll_delta: 0.0,
            clicked: false,
            click_pos: [0.0; 2],
        }
    }

    /// フレーム開始時にリセットすべき一時値をクリア
    pub fn begin_frame(&mut self) {
        self.scroll_delta = 0.0;
        self.clicked = false;
    }

    /// winit のウィンドウイベントを処理
    ///
    /// カメラ操作に必要なイベントを消費した場合 `true` を返す。
    pub fn process_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = [position.x as f32, position.y as f32];

                // ドラッグ中なら前回位置を更新（デルタはフレーム処理で計算）
                // ここでは prev を更新しない — consume_drag_delta で更新する
                true
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = *state == ElementState::Pressed;
                let (btn, drag) = match button {
                    MouseButton::Left => (&mut self.left, &mut self.left_drag),
                    MouseButton::Middle => (&mut self.middle, &mut self.middle_drag),
                    MouseButton::Right => (&mut self.right, &mut self.right_drag),
                    _ => return false,
                };

                if pressed {
                    btn.pressed = true;
                    *drag = Some(DragState {
                        start: self.cursor_pos,
                        prev: self.cursor_pos,
                    });
                } else {
                    btn.pressed = false;
                    // クリック判定: ドラッグ距離が閾値以下
                    if let Some(d) = drag.take() {
                        let dx = self.cursor_pos[0] - d.start[0];
                        let dy = self.cursor_pos[1] - d.start[1];
                        if (dx * dx + dy * dy).sqrt() < CLICK_THRESHOLD
                            && matches!(button, MouseButton::Left)
                        {
                            self.clicked = true;
                            self.click_pos = self.cursor_pos;
                        }
                    }
                }
                true
            }

            WindowEvent::MouseWheel { delta, .. } => {
                self.scroll_delta += match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.0,
                };
                true
            }

            WindowEvent::Resized(size) => {
                self.window_size = [size.width as f32, size.height as f32];
                false // リサイズはカメラ操作ではないので false
            }

            _ => false,
        }
    }

    /// 左ドラッグのデルタを取得し、prev を更新
    pub fn consume_left_drag_delta(&mut self) -> Option<[f32; 2]> {
        self.consume_drag_delta(&mut self.left_drag.clone(), |s, d| s.left_drag = d)
    }

    /// 中ドラッグのデルタを取得し、prev を更新
    pub fn consume_middle_drag_delta(&mut self) -> Option<[f32; 2]> {
        self.consume_drag_delta(&mut self.middle_drag.clone(), |s, d| s.middle_drag = d)
    }

    /// 右ドラッグのデルタを取得し、prev を更新
    pub fn consume_right_drag_delta(&mut self) -> Option<[f32; 2]> {
        self.consume_drag_delta(&mut self.right_drag.clone(), |s, d| s.right_drag = d)
    }

    fn consume_drag_delta(
        &mut self,
        drag: &mut Option<DragState>,
        set: impl FnOnce(&mut Self, Option<DragState>),
    ) -> Option<[f32; 2]> {
        if let Some(d) = drag {
            let delta = [
                self.cursor_pos[0] - d.prev[0],
                self.cursor_pos[1] - d.prev[1],
            ];
            d.prev = self.cursor_pos;
            let updated = Some(*d);
            set(self, updated);
            if delta[0].abs() > 0.0 || delta[1].abs() > 0.0 {
                Some(delta)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// スクリーン座標を NDC（-1..1）に変換
    pub fn screen_to_ndc(&self, screen: [f32; 2]) -> [f32; 2] {
        [
            (screen[0] / self.window_size[0]) * 2.0 - 1.0,
            1.0 - (screen[1] / self.window_size[1]) * 2.0, // Y 反転
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_to_ndc_center() {
        let input = InputState::new(800, 600);
        let ndc = input.screen_to_ndc([400.0, 300.0]);
        assert!((ndc[0]).abs() < 0.01);
        assert!((ndc[1]).abs() < 0.01);
    }

    #[test]
    fn screen_to_ndc_corners() {
        let input = InputState::new(800, 600);

        // 左上
        let ndc = input.screen_to_ndc([0.0, 0.0]);
        assert!((ndc[0] - (-1.0)).abs() < 0.01);
        assert!((ndc[1] - 1.0).abs() < 0.01);

        // 右下
        let ndc = input.screen_to_ndc([800.0, 600.0]);
        assert!((ndc[0] - 1.0).abs() < 0.01);
        assert!((ndc[1] - (-1.0)).abs() < 0.01);
    }

    #[test]
    fn scroll_accumulates() {
        let mut input = InputState::new(800, 600);
        input.begin_frame();
        let ev1 = WindowEvent::MouseWheel {
            device_id: unsafe { std::mem::zeroed() },
            delta: MouseScrollDelta::LineDelta(0.0, 1.0),
            phase: winit::event::TouchPhase::Moved,
        };
        let ev2 = WindowEvent::MouseWheel {
            device_id: unsafe { std::mem::zeroed() },
            delta: MouseScrollDelta::LineDelta(0.0, 0.5),
            phase: winit::event::TouchPhase::Moved,
        };
        input.process_event(&ev1);
        input.process_event(&ev2);
        assert!((input.scroll_delta - 1.5).abs() < 0.01);
    }
}
