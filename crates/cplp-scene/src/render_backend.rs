//! RenderBackend — プラットフォーム抽象化レイヤー
//!
//! wgpu（macOS/Linux）と RealityKit（visionOS）を同じインターフェースで扱う。
//! 各バックエンドは SceneGraph を受け取り、GPU リソースの管理とフレーム描画を行う。

use crate::scene_graph::SceneGraph;

/// レンダリングバックエンド trait
///
/// # 実装ガイド
///
/// - `WgpuBackend`: macOS / Linux（現行実装）
/// - `RealityKitBackend`: visionOS（将来実装）
///   - CompositorServices + Metal で ImmersiveSpace にレンダリング
///   - ステレオレンダリング（左右眼）対応が必要
///
/// # ライフサイクル
///
/// ```text
/// init() → submit_scene() → [render_frame() ...] → resize() → [render_frame() ...]
/// ```
pub trait RenderBackend {
    /// バックエンド固有のエラー型
    type Error: std::error::Error + Send + Sync + 'static;

    /// シーングラフを GPU リソースに変換してサブミット
    ///
    /// 初回および SceneGraph 変更時に呼ぶ。
    /// バックエンドはメッシュデータから GPU バッファを作成する。
    fn submit_scene(&mut self, scene: &SceneGraph) -> Result<(), Self::Error>;

    /// カメラの view-projection 行列を更新
    ///
    /// `view_proj`: 列優先の 4x4 行列
    /// `eye_position`: カメラのワールド座標（スペキュラー計算用）
    fn update_camera(
        &mut self,
        view_proj: [[f32; 4]; 4],
        eye_position: [f32; 3],
    ) -> Result<(), Self::Error>;

    /// 1 フレームを描画
    ///
    /// `time`: シーン開始からの経過秒数（アニメーション用）
    fn render_frame(&mut self, time: f32) -> Result<(), Self::Error>;

    /// リサイズ処理
    fn resize(&mut self, width: u32, height: u32) -> Result<(), Self::Error>;
}

/// ライト設定（バックエンド非依存）
#[derive(Clone, Debug)]
pub struct LightConfig {
    /// 方向 [x, y, z]（正規化ベクトル）
    pub direction: [f32; 3],
    /// アンビエント強度
    pub ambient: f32,
    /// ライト色 [r, g, b]
    pub color: [f32; 3],
    /// スペキュラー強度
    pub specular: f32,
    /// シャイネス（Phong exponent）
    pub shininess: f32,
    /// リムライト強度
    pub rim: f32,
}

impl Default for LightConfig {
    fn default() -> Self {
        Self {
            direction: [-0.4, -0.7, -0.5],
            ambient: 0.15,
            color: [1.0, 0.98, 0.95],
            specular: 0.4,
            shininess: 32.0,
            rim: 0.25,
        }
    }
}

// ── 空間オーディオ対応メモ ──────────────────────────
//
// visionOS の空間オーディオ統合に向けた調査事項:
//
// 1. PHASE (Physical Audio Spatialization Engine)
//    - Apple の空間オーディオフレームワーク
//    - PHASESpatialMixer で 3D 位置にオーディオソースを配置
//    - SceneGraph のノード位置と PHASE リスナー位置を同期させる
//
// 2. Rust → PHASE ブリッジ
//    - objc2 経由で PHASEEngine, PHASESource を操作
//    - cpal のオーディオ出力と PHASE の共存方法を調査中
//    - 候補: cpal → AudioUnit → PHASE spatial mixer
//
// 3. 設計方針
//    - SceneNode に `spatial_audio: Option<SpatialAudioSource>` を追加予定
//    - SpatialAudioSource: gain, rolloff モデル, directivity パターン
//    - RenderBackend に `update_listener_position()` を追加予定
//    - バックエンド非依存の SpatialAudioMixer trait で抽象化
//
// 4. 参考
//    - WWDC22: "Discover PHASE" (spatial audio framework)
//    - WWDC23: "Enhance spatial computing with RealityKit audio"
//    - visionOS 2.0+: RealityKit SpatialAudioComponent
//    - Apple Spatial Audio は head-tracking + HRTF を自動適用
//
// TODO: Phase 4 で SpatialAudioSource / SpatialAudioMixer を実装
