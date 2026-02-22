//! UI テーマ定数（全ウィジェット共通）
//!
//! サイズ・余白・フォントの基準値を一箇所で管理する。
//! 変更はここだけで全 UI に反映される。

use crate::renderer::primitives::Color;

// ── スケール ──────────────────────────────────────────
/// グローバル UI スケール（1.0 = デフォルト、2.0 = 2倍）
pub const SCALE: f32 = 2.0;

// ── フォント ──────────────────────────────────────────
/// 本文テキストサイズ
pub const TEXT_SM: f32 = 14.0 * SCALE;
/// セクションラベル
pub const TEXT_MD: f32 = 16.0 * SCALE;
/// 画面タイトル
pub const TEXT_LG: f32 = 24.0 * SCALE;
/// アプリタイトル
pub const TEXT_XL: f32 = 32.0 * SCALE;
/// モノスペース 1 文字幅（推定値）
pub const CHAR_W: f32 = 8.4 * SCALE;

// ── ウィジェットサイズ ─────────────────────────────────
/// ボタン高さ
pub const BUTTON_H: f32 = 36.0 * SCALE;
/// リストアイテム高さ
pub const ITEM_H: f32 = 30.0 * SCALE;
/// テキスト入力高さ
pub const INPUT_H: f32 = 32.0 * SCALE;
/// スライダー高さ
pub const SLIDER_H: f32 = 28.0 * SCALE;
/// スライダー幅
pub const SLIDER_W: f32 = 200.0 * SCALE;
/// スライダーつまみ幅
pub const KNOB_W: f32 = 4.0 * SCALE;
/// スライダーつまみ高さ
pub const KNOB_H: f32 = 20.0 * SCALE;
/// スライダートラック高さ
pub const TRACK_H: f32 = 6.0 * SCALE;
/// カーソル幅
pub const CURSOR_W: f32 = 2.0 * SCALE;

// ── 余白 ──────────────────────────────────────────────
/// 一般パディング
pub const PAD: f32 = 20.0 * SCALE;
/// ボタン水平パディング
pub const PAD_BTN_H: f32 = 16.0 * SCALE;
/// リスト・入力の左パディング
pub const PAD_LEFT: f32 = 8.0 * SCALE;

// ── レイアウト ────────────────────────────────────────
/// セットアップ画面: 左右カラム幅
pub const HALF_W: f32 = 300.0 * SCALE;
/// セットアップ画面: コンテンツ開始 Y
pub const CONTENT_Y: f32 = 70.0 * SCALE;
/// ウィンドウ初期サイズ
pub const WINDOW_W: f32 = 640.0 * SCALE;
pub const WINDOW_H: f32 = 480.0 * SCALE;

// ── カラー ────────────────────────────────────────────
pub const BG: Color = Color {
    r: 0.12,
    g: 0.12,
    b: 0.15,
    a: 0.9,
};
pub const HOVER: Color = Color {
    r: 0.2,
    g: 0.2,
    b: 0.25,
    a: 0.9,
};
pub const ACTIVE: Color = Color {
    r: 0.2,
    g: 0.6,
    b: 0.9,
    a: 0.9,
};
pub const DISABLED_BG: Color = Color {
    r: 0.08,
    g: 0.08,
    b: 0.10,
    a: 0.9,
};
pub const TEXT_COLOR: [f32; 4] = [0.85, 0.85, 0.85, 1.0];
pub const TEXT_DIM: [f32; 4] = [0.7, 0.7, 0.7, 1.0];
pub const TEXT_DISABLED: [f32; 4] = [0.4, 0.4, 0.4, 1.0];
pub const ACCENT: [f32; 4] = [0.2, 0.6, 0.9, 1.0];
pub const ERROR_COLOR: [f32; 4] = [0.9, 0.3, 0.3, 1.0];
pub const PLACEHOLDER: [f32; 4] = [0.45, 0.45, 0.5, 1.0];
