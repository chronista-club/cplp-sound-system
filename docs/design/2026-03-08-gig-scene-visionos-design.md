# Gig Scene – visionOS 対応アーキテクチャ設計

**作成日**: 2026-03-08
**対象イシュー**: #26
**ラベル**: gig, infra

---

## 概要

`cplp-gig` クレートは、ライブセッション中の機材ラック・ケーブルパッチを 3D で可視化する
「Gig Scene」機能を担う。将来的に Apple Vision Pro（visionOS）でも動作させるため、
**シーングラフ**・**メッシュ生成**・**GPU パイプライン** を明確に分離した設計とする。

---

## 問題設定

単一の `app.rs` / `mesh.rs` にシーンデータと GPU 依存コードを混在させると、
visionOS ポーティング時に次の問題が生じる。

| 問題 | 影響 |
|------|------|
| `wgpu` の `Surface` 作成が winit ウィンドウに依存 | visionOS では `CompositorLayer` を使う必要がある |
| メッシュデータと GPU バッファが同一型に紐づく | バックエンド切替でシーンデータの書き直しが必要 |
| テストが GPU デバイス初期化を必要とする | CI での単体テストが困難 |

---

## アーキテクチャ

```
cplp-gig
├── scene.rs          ← SceneGraph（GPU 非依存）
├── mesh.rs           ← メッシュ生成（GPU 非依存）
├── render_backend.rs ← RenderBackend trait（抽象層）
└── wgpu_backend.rs   ← wgpu 実装（feature = "wgpu" でのみコンパイル）
```

### レイヤー図

```
┌─────────────────────────────┐
│       SceneGraph            │  platform-agnostic
│  (scene.rs)                 │  ノード・トランスフォーム
└──────────┬──────────────────┘
           │ build_draw_calls()
┌──────────▼──────────────────┐
│       MeshData              │  platform-agnostic
│  (mesh.rs)                  │  頂点・インデックス生成
└──────────┬──────────────────┘
           │ upload_mesh() / submit_frame()
┌──────────▼──────────────────┐
│     RenderBackend trait     │  抽象層
│  (render_backend.rs)        │
├─────────────────────────────┤
│  WgpuBackend (wgpu feat.)   │  macOS / Linux / Windows
│  [将来] MetalBackend        │  visionOS / RealityKit
└─────────────────────────────┘
```

---

## モジュール詳細

### `scene.rs` – SceneGraph

プラットフォーム非依存のシーンデータ構造。`wgpu` への依存ゼロ。

```
SceneGraph
  └── Vec<SceneNode>
        ├── NodeId      (u32 newtype)
        ├── Transform   { position, rotation(quaternion), scale }
        └── NodeKind
              ├── RackUnit { name, active, rack_units }
              ├── Cable    { from: NodeId, to: NodeId }
              └── Background
```

**設計方針**:
- `Clone + Debug` を実装し、テスト・スナップショットを容易にする
- GPU オブジェクトへの参照を一切持たない

### `mesh.rs` – メッシュ生成

シーンノードから `MeshData`（頂点・インデックス列）を生成するロジック。
GPU バッファアップロードは行わない。

```rust
pub struct Vertex {
    pub position: [f32; 3],
    pub normal:   [f32; 3],
    pub uv:       [f32; 2],
    pub color:    [f32; 4],
}

pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices:  Vec<u32>,
}
```

**設計方針**:
- 純粋関数で実装 → 単体テストがシンプル
- `bytemuck::Pod` を `Vertex` に実装し、将来のバッファ書き込みを効率化

### `render_backend.rs` – RenderBackend trait

バックエンド非依存の描画インターフェース。

```rust
pub trait RenderBackend {
    fn upload_mesh(&mut self, mesh: &MeshData) -> MeshHandle;
    fn free_mesh(&mut self, handle: MeshHandle);
    fn submit_frame(&mut self, draw_calls: &[DrawCall]);
    fn resize(&mut self, width: u32, height: u32);
}
```

**`DrawCall`**:
```rust
pub struct DrawCall {
    pub mesh:      MeshHandle,
    pub transform: [[f32; 4]; 4],  // 列優先4×4行列
}
```

### `wgpu_backend.rs` – wgpu 実装

`feature = "wgpu"` でのみコンパイルされる。`RenderBackend` を `wgpu` で実装する。
visionOS 向けには別途 `metal_backend.rs` / `realitykit_backend.rs` を追加する想定。

---

## Cargo.toml 方針

```toml
[features]
default = ["wgpu"]
wgpu = ["dep:wgpu", "dep:winit", "dep:pollster", "dep:bytemuck"]
```

visionOS ビルド時は `default-features = false` として `wgpu` を外し、
プラットフォーム固有バックエンドのみをリンクする。

---

## テスト方針

- `scene.rs` / `mesh.rs` のユニットテストは GPU デバイス不要
- `wgpu_backend.rs` の統合テストは `#[cfg(feature = "wgpu")]` でガード

---

## 将来の拡張

| フェーズ | 内容 |
|----------|------|
| D-1 | 本設計（シーン/レンダリング分離） |
| D-2 | visionOS CompositorLayer 上の MetalBackend 実装 |
| D-3 | RealityKit アンカー連携（空間音響マッピング） |
