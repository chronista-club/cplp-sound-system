# Gig Scene + visionOS 展開設計

**日付**: 2026-03-08
**ステータス**: Draft
**概要**: ユーロラック的 3D シーン（Gig Scene）のアーキテクチャと visionOS 展開パス

---

## 背景

cplp-sound-system にライブパフォーマンス可視化のための 3D シーン（Gig Scene）を構築中。
将来的に Apple Vision Pro (visionOS) への展開を視野に入れ、アーキテクチャを設計する。

### 現在の実装状況

- `cplp-scene` クレート: wgpu + winit ベースの 3D レンダリング
- ユーロラック的なフレーム + モジュールパネルの配置
- 自作 .usda パーサー（最小限のサブセット）
- 手書き行列演算（外部クレート不使用）
- `cplp gig start` でウィンドウ起動

---

## 3 つの開発ライン

### 1. ユーロラック 3D シーン（メインライン）

ユーロラックシンセサイザーのメタファーを 3D 空間に構築。

- **HP（Horizontal Pitch）ベースのグリッド**: モジュールを HP 単位でスナップ配置
- **モジュール = ビューコンポーネント**: ルーパー、ミキサー、エフェクト等の機材を 3D パネルとして表現
- **ラックフレーム**: レール + サイドパネルで物理的なラック構造を模倣

### 2. Story DSL（カスタムシーン記述言語）

3D フォーマット非依存の抽象レイヤー。

```
Story DSL（cplp 独自）
  │  ライブシーン・モジュール配置・パッチング・遷移を記述
  │
  ├── USD (.usda/.usdz) バックエンド
  ├── FBX バックエンド
  └── 将来の形式...
```

**設計思想**: USD や FBX は「形」を記述するフォーマット。Story は「意味」を記述する。
- 「このモジュールはルーパーで、ここに配置して、このパッチでミキサーに繋がる」
- シーン遷移、セットリスト、パフォーマンス演出も Story で記述

### 3. 3D モデリングエディタ

シーン内のオブジェクトを編集する機能（スコープ未確定）。

---

## visionOS 展開パス

### レンダリングスタック比較

visionOS には 3 つのレンダリングパスが存在する:

| パス | 概要 | Rust 親和性 | ユースケース |
|------|------|-------------|-------------|
| **RealityKit + SwiftUI** | USD ネイティブ読み、自動レンダリング | 低（Swift API） | 早期プロトタイプ |
| **CompositorServices + Metal** | Metal で直接 GPU 描画、フルコントロール | 中〜高（wgpu Metal） | 最終ターゲット |
| **WebGPU (Safari 26+)** | ブラウザ経由 | 中 | Web 展開時 |

### wgpu の visionOS サポート状況

- wgpu visionOS サポートは 2025年1月にマージ済み（PR #6611）
- Metal バックエンドを使用
- **制約**: winit が visionOS 未対応（`ImmersiveSpace` が必要）
- **制約**: ステレオレンダリング（左右眼）への拡張が必要
- Rust ターゲットは Tier 3（`aarch64-apple-visionos`、nightly + `-Zbuild-std`）

### Rust → visionOS ブリッジ

| 手法 | 成熟度 | 推奨度 | 備考 |
|------|--------|--------|------|
| **UniFFI (Mozilla)** | 高 | 最推奨 | Firefox Mobile で実績。Swift バインディング自動生成 |
| **swift-bridge** | 中 | 推奨 | 型安全な FFI 自動生成 |
| **objc2 直接** | 高 | Metal レイヤーのみ | visionOS 1.0〜2.5 対応済み |

---

## 推奨アーキテクチャ

```
┌─────────────────────────────────────┐
│  Story DSL（cplp 独自記述言語）       │
│  シーン・モジュール・パッチ・遷移       │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  cplp-scene-core（Rust、純ロジック）  │
│  シーングラフ / 行列演算 / アニメ     │
│  USD パーサー / メッシュ生成          │
└──────┬────────────────┬─────────────┘
       │                │
┌──────▼──────┐  ┌──────▼──────────────┐
│ macOS       │  │ visionOS             │
│ wgpu+winit  │  │ CompositorServices   │
│ (現行)      │  │ + Metal (objc2 経由) │
└─────────────┘  └──────────────────────┘
```

### コア分離の方針

現行の `app.rs` はシーンロジックとレンダリングが密結合している。分離すべき層:

- **SceneGraph**: モジュール配置、ラック構造、パッチング（プラットフォーム非依存）
- **MeshGenerator**: 頂点データ生成（プラットフォーム非依存）
- **RenderBackend**: GPU パイプライン管理（プラットフォーム依存）

```rust
// 概念的な設計
trait RenderBackend {
    fn submit_scene(&mut self, scene: &SceneGraph);
    fn update_camera(&mut self, view_matrix: Mat4, proj_matrix: Mat4);
    fn render_frame(&mut self);
}
```

### 現行コードの再利用性

| ファイル | 再利用可能性 | 備考 |
|---------|-------------|------|
| `camera.rs` | 高 | 行列演算はプラットフォーム非依存。ステレオ拡張が必要 |
| `mesh.rs`（メッシュ生成部分） | 高 | 頂点データ生成はそのまま共有 |
| `mesh.rs`（MeshPipeline） | 低 | wgpu 依存。バックエンド別に実装 |
| `usd.rs` | 高 | パーサーは純 Rust。USDZ 出力拡張の余地 |
| `app.rs` | 低 | winit + wgpu に密結合。要分離 |
| `gpu.rs` | 低 | wgpu Surface 管理。visionOS では不要 |

---

## 段階的移行パス

### Phase 1: 基盤整備（現在〜）

- シーンロジックとレンダリングの分離
- Story DSL の初期設計
- ユーロラック UI の作り込み（macOS wgpu）

### Phase 2: visionOS 最小動作

- USD → USDZ 変換パイプライン構築
- UniFFI で Rust ロジック → Swift バインディング
- USDZ → RealityKit で visionOS 表示（最も低リスク）

### Phase 3: カスタムレンダリング

- CompositorServices + Metal 統合
- ステレオレンダリング対応
- ハンドトラッキング（ARKit）

### Phase 4: 体験の磨き込み

- 空間オーディオ統合
- モジュール操作 UI（ノブ、フェーダー、パッチケーブル）
- パフォーマンスチューニング

---

## 3D フォーマット比較

| | USD | glTF | 独自のみ |
|---|---|---|---|
| **visionOS** | ネイティブ（RealityKit 直読み） | 変換が必要 | 変換が必要 |
| **Rust エコシステム** | 公式クレートなし（自作パーサー） | `gltf` クレートが成熟 | 自由 |
| **仕様の複雑さ** | 高（Pixar 由来） | 低（JSON + バイナリ） | 自由 |
| **Web** | 弱い | 事実上の標準 | — |
| **シーングラフ** | 強い（階層・合成・バリアント） | 基本的 | 自由 |
| **ツール連携** | Blender, Houdini, Reality Composer Pro | Blender, three.js, ほぼ全部 | — |

**判断**: visionOS を狙うなら USD を継続。Story が吸収するため、将来 glTF 対応も可能。

---

## 学習ロードマップ

```
[現在地] wgpu + WGSL シェーダー + 手書き行列演算
    │
    ▼
[Step 1] 3D 基礎の強化
    - ライティングモデル（Phong/PBR）
    - テクスチャリング
    - ノーマルマッピング
    │
    ▼
[Step 2] Metal の理解
    - Metal Shading Language (MSL)
    - リソース管理（MTLBuffer, MTLTexture）
    - Apple "Metal Best Practices Guide"
    │
    ▼
[Step 3] visionOS 固有概念
    - CompositorServices API / LayerRenderer
    - ステレオレンダリング（Vertex Amplification）
    - WWDC23 "Discover Metal for immersive apps"
    - WWDC24 "Render Metal with passthrough in visionOS"
    │
    ▼
[Step 4] 空間インタラクション
    - ARKit ハンドトラッキング
    - Spatial Tap / Direct Pinch ジェスチャー
    - Entity Component System (RealityKit)
```

---

## 参考リソース

- [wgpu visionOS support PR #6611](https://github.com/gfx-rs/wgpu/pull/6611)
- [Rust visionOS target (rustc book)](https://doc.rust-lang.org/rustc/platform-support/apple-visionos.html)
- [objc2 - Apple frameworks bindings for Rust](https://github.com/madsmtm/objc2)
- [UniFFI - multi-language bindings generator](https://github.com/mozilla/uniffi-rs)
- [CompositorServices (Apple Docs)](https://developer.apple.com/documentation/compositorservices)
- [metal-spatial-rendering (参考実装)](https://github.com/metal-by-example/metal-spatial-rendering)
- [WWDC25: WebGPU on Apple Platforms](https://developer.apple.com/videos/play/wwdc2025/294/)

---

## 未解決課題

1. wgpu v28 の visionOS ビルドが実機で通るか検証が必要
2. naga の WGSL → MSL 変換のカバー範囲
3. cpal（オーディオ）の visionOS 動作確認
4. ハンドジェスチャー → ユーロラック操作のマッピング設計
5. Vision Pro M2 GPU で 90fps ステレオ維持の性能検証
