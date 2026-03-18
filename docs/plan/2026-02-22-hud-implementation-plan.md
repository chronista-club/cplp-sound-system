# cplp-hud 実装計画

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** wgpu 直上でライブ演奏向け HUD を構築する。2D レンダラー、UI ウィジェット、ビジュアル描画をフルスクラッチで実装。

**Architecture:** cplp-hud クレートを新規作成。winit でウィンドウ管理、wgpu (Metal) で GPU レンダリング、glyphon でテキスト描画。矩形・ポリライン等の 2D プリミティブ、UI ウィジェット（ボタン、スライダー、リスト、テキスト入力）、ビジュアル（ウェーブフォーム、スペクトラム、グロー）をすべて自作。

**Tech Stack:** Rust, wgpu 28, winit 0.30, glyphon 0.10, atomic_float, triple_buffer, ringbuf

**設計書:** `docs/plans/2026-02-22-hud-gui-design.md`

---

## Task 1: cplp-hud クレートの足場作り

**Files:**
- Create: `crates/cplp-hud/Cargo.toml`
- Create: `crates/cplp-hud/src/lib.rs`
- Modify: `Cargo.toml` (workspace members + dependencies)

**Step 1: ワークスペースに依存クレートを追加**

`Cargo.toml` の `[workspace.dependencies]` に追加:

```toml
# GUI / レンダリング
wgpu = "28"
glyphon = "0.10"

# Lock-free（追加）
atomic_float = "1"
triple_buffer = "8"
```

`[workspace]` の `members` に `"crates/cplp-hud"` を追加。

**Step 2: cplp-hud の Cargo.toml を作成**

```toml
[package]
name = "cplp-hud"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
description = "ライブ演奏向け HUD（wgpu 直上フルスクラッチ）"

[dependencies]
cplp-core.workspace = true
wgpu.workspace = true
winit.workspace = true
glyphon.workspace = true
atomic_float.workspace = true
triple_buffer.workspace = true
ringbuf.workspace = true
tracing.workspace = true
anyhow.workspace = true
```

**Step 3: lib.rs に空のモジュール構成を作成**

```rust
pub mod app;
pub mod state;
pub mod renderer;
pub mod ui;
pub mod visuals;
```

各モジュールは空ファイルで OK（`pub mod` のみ、またはスタブ）。コンパイルが通ることが目標。

**Step 4: ビルド確認**

Run: `cargo check -p cplp-hud`
Expected: PASS（warning は OK）

**Step 5: コミット**

```bash
git add crates/cplp-hud/ Cargo.toml Cargo.lock
git commit -m "feat(hud): cplp-hud クレートの足場作り"
```

---

## Task 2: wgpu + winit の初期化と空ウィンドウ表示

**Files:**
- Create: `crates/cplp-hud/src/app.rs`
- Create: `crates/cplp-hud/src/renderer/mod.rs`
- Create: `crates/cplp-hud/src/renderer/pipeline.rs`

**Step 1: Renderer 構造体を作成**

`renderer/pipeline.rs` に wgpu の初期化コードを書く:

```rust
pub struct GpuContext {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
}

impl GpuContext {
    pub async fn new(window: Arc<winit::window::Window>) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("No suitable GPU adapter found"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await?;
        let size = window.inner_size();
        let config = surface.get_default_config(&adapter, size.width, size.height)
            .ok_or_else(|| anyhow::anyhow!("Surface not compatible"))?;
        surface.configure(&device, &config);
        Ok(Self { surface, device, queue, config, size })
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }
}
```

**Step 2: app.rs にイベントループを実装**

```rust
pub fn run() -> anyhow::Result<()> {
    let event_loop = winit::event_loop::EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let window = Arc::new(
        event_loop.create_window(
            winit::window::WindowAttributes::default()
                .with_title("cplp")
                .with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0)),
        )?
    );

    let mut gpu = pollster::block_on(GpuContext::new(window.clone()))?;

    event_loop.run(move |event, target| {
        match event {
            winit::event::Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(size) => gpu.resize(size),
                WindowEvent::RedrawRequested => {
                    // 暗い背景でクリア
                    let output = gpu.surface.get_current_texture().unwrap();
                    let view = output.texture.create_view(&Default::default());
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.05, g: 0.05, b: 0.08, a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    gpu.queue.submit(std::iter::once(encoder.finish()));
                    output.present();
                }
                _ => {}
            },
            winit::event::Event::AboutToWait => window.request_redraw(),
            _ => {}
        }
    })?;
    Ok(())
}
```

**Step 3: 実行して暗い背景のウィンドウが表示されることを確認**

`cplp-app/src/main.rs` に一時的な起動コードを追加するか、`cplp-hud` に `examples/window.rs` を作成。

Run: `cargo run -p cplp-hud --example window`（または `cargo run -p cplp-app`）
Expected: 640x480 の暗い背景ウィンドウが表示される

**Step 4: コミット**

```bash
git commit -m "feat(hud): wgpu + winit 初期化、空ウィンドウ表示"
```

**注意:** `pollster` クレートを `Cargo.toml` に追加する必要がある（async → sync ブリッジ用）。

---

## Task 3: Quad パイプライン（矩形描画）

**Files:**
- Create: `crates/cplp-hud/src/renderer/primitives.rs`
- Create: `crates/cplp-hud/src/renderer/shaders/quad.wgsl`

**Step 1: 頂点フォーマットと型定義**

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct QuadVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}
```

**Step 2: WGSL シェーダーを書く**

`shaders/quad.wgsl`:

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: vec2<f32>;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // ピクセル座標 → クリップ座標変換
    let x = (in.position.x / viewport.x) * 2.0 - 1.0;
    let y = 1.0 - (in.position.y / viewport.y) * 2.0;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
```

**Step 3: QuadPipeline を実装**

矩形を頂点バッファに積み、`draw()` で一括描画:

```rust
pub struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
    vertices: Vec<QuadVertex>,
    indices: Vec<u16>,
}

impl QuadPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self { /* ... */ }
    pub fn rect(&mut self, rect: Rect, color: Color) { /* 6 vertices (2 triangles) を push */ }
    pub fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass) { /* draw call */ }
}
```

**Step 4: 画面に色付き矩形を描画して確認**

example を更新して、赤い矩形を 1 つ描画。
Expected: 暗い背景に赤い矩形が表示される

**Step 5: コミット**

```bash
git commit -m "feat(hud): Quad パイプライン（矩形描画）"
```

**注意:** `bytemuck` クレートを依存に追加する必要がある。

---

## Task 4: テキスト描画（glyphon 統合）

**Files:**
- Create: `crates/cplp-hud/src/renderer/text.rs`

**Step 1: TextRenderer ラッパーを実装**

```rust
use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
};

pub struct TextEngine {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    renderer: TextRenderer,
}

impl TextEngine {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self;
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, texts: &[TextEntry], size: (u32, u32));
    pub fn render(&self, pass: &mut wgpu::RenderPass);
}

pub struct TextEntry {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Color,
}
```

**Step 2: 画面にテキストを描画して確認**

example を更新して "cplp" とモノスペースフォントで表示。
Expected: 暗い背景に白いテキストが表示される

**Step 3: コミット**

```bash
git commit -m "feat(hud): glyphon によるテキスト描画"
```

---

## Task 5: Line パイプライン（ポリライン描画）

**Files:**
- Create: `crates/cplp-hud/src/renderer/shaders/line.wgsl`
- Modify: `crates/cplp-hud/src/renderer/primitives.rs`

**Step 1: LineVertex と LinePipeline を実装**

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

pub struct LinePipeline {
    pipeline: wgpu::RenderPipeline,
    vertices: Vec<LineVertex>,
}

impl LinePipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self;
    pub fn polyline(&mut self, points: &[Vec2], color: Color, thickness: f32);
    pub fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass);
}
```

**Step 2: サイン波のポリラインを描画して確認**

テスト用にサイン波のデータを生成して描画。
Expected: 暗い背景に緑のサイン波ラインが表示される

**Step 3: コミット**

```bash
git commit -m "feat(hud): Line パイプライン（ポリライン描画）"
```

---

## Task 6: 統合 Renderer API

**Files:**
- Modify: `crates/cplp-hud/src/renderer/mod.rs`

**Step 1: QuadPipeline + LinePipeline + TextEngine を統合**

```rust
pub struct Renderer {
    gpu: GpuContext,
    quads: QuadPipeline,
    lines: LinePipeline,
    text: TextEngine,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Self;
    pub fn rect(&mut self, rect: Rect, color: Color);
    pub fn polyline(&mut self, points: &[Vec2], color: Color);
    pub fn text(&mut self, entry: TextEntry);
    pub fn render_frame(&mut self);
}
```

**Step 2: 矩形 + テキスト + ポリラインを同一フレームに描画**

Expected: 3 種類のプリミティブが同時に正しく表示される

**Step 3: コミット**

```bash
git commit -m "feat(hud): 統合 Renderer API"
```

---

## Task 7: HudState と lock-free データ構造

**Files:**
- Create: `crates/cplp-hud/src/state.rs`

**Step 1: テストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    #[test]
    fn audio_meters_atomic_write_read() {
        let meters = AudioMeters::default();
        meters.local_level.store(0.75, Relaxed);
        assert!((meters.local_level.load(Relaxed) - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn session_snapshot_triple_buffer() {
        let (mut input, mut output) = triple_buffer::triple_buffer(&SessionSnapshot::default());
        input.write(SessionSnapshot {
            peer_name: "Player B".into(),
            connected: true,
            latency_ms: 8.2,
            ..Default::default()
        });
        let snap = output.read();
        assert_eq!(snap.peer_name, "Player B");
        assert!(snap.connected);
    }
}
```

**Step 2: テストが fail することを確認**

Run: `cargo test -p cplp-hud`
Expected: FAIL（構造体が未定義）

**Step 3: AudioMeters, SessionSnapshot, HudView を実装**

設計書セクション 6 の通り実装。`SessionSnapshot` に `Default` derive を追加。

**Step 4: テストが pass することを確認**

Run: `cargo test -p cplp-hud`
Expected: PASS

**Step 5: コミット**

```bash
git commit -m "feat(hud): HudState と lock-free データ構造"
```

---

## Task 8: Widget トレイトとレイアウトエンジン

**Files:**
- Create: `crates/cplp-hud/src/ui/widget.rs`
- Create: `crates/cplp-hud/src/ui/layout.rs`
- Create: `crates/cplp-hud/src/ui/event.rs`
- Create: `crates/cplp-hud/src/ui/mod.rs`

**Step 1: Widget トレイト定義**

設計書セクション 4.1 の通り。`UiEvent`, `EventResponse`, `Widget` トレイト。

**Step 2: Layout 列挙型（VStack, HStack, Padded, Fixed）**

設計書セクション 4.2 の通り。各バリアントに `layout()`, `draw()`, `event()` を実装。

**Step 3: winit イベント → UiEvent 変換**

`event.rs` に `WindowEvent` → `UiEvent` の変換関数を実装。

**Step 4: テスト**

VStack の layout 計算が正しいことをユニットテスト:

```rust
#[test]
fn vstack_layout_sizes() {
    // 2 つの Fixed(100x50) を spacing=10 で VStack
    // 期待: 幅 100, 高さ 50 + 10 + 50 = 110
}
```

Run: `cargo test -p cplp-hud`
Expected: PASS

**Step 5: コミット**

```bash
git commit -m "feat(hud): Widget トレイトとレイアウトエンジン"
```

---

## Task 9: 基本ウィジェット（Button, Slider, TextInput, List）

**Files:**
- Create: `crates/cplp-hud/src/ui/button.rs`
- Create: `crates/cplp-hud/src/ui/slider.rs`
- Create: `crates/cplp-hud/src/ui/text_input.rs`
- Create: `crates/cplp-hud/src/ui/list.rs`

**Step 1: Button**

- Widget トレイト実装
- ホバー時の色変化
- クリック時のコールバック

**Step 2: Slider**

- 横方向のドラッグスライダー
- 0.0〜1.0 の値を保持
- ミックスバランス操作に使用

**Step 3: TextInput**

- キーボード入力でテキスト編集
- カーソル位置管理
- セッション ID 入力に使用

**Step 4: List**

- スクロール可能な選択リスト
- 選択アイテムのハイライト
- プラグイン・MIDI デバイス選択に使用

**Step 5: 全ウィジェットを画面に並べて動作確認**

example で全ウィジェットを VStack に配置して操作テスト。

**Step 6: コミット**

```bash
git commit -m "feat(hud): 基本ウィジェット（Button, Slider, TextInput, List）"
```

---

## Task 10: ビジュアル — レベルメーター

**Files:**
- Create: `crates/cplp-hud/src/visuals/mod.rs`
- Create: `crates/cplp-hud/src/visuals/meters.rs`

**Step 1: LevelMeter を実装**

- 横バー表示（Quad パイプライン使用）
- 値に応じた色グラデーション（緑 → 黄 → 赤）
- ピークホールド表示（ピーク値が一定時間残る）
- dB 値テキスト表示

**Step 2: AudioMeters → LevelMeter の接続**

`AtomicF32` からレベル値を読み出して LevelMeter に渡す。

**Step 3: ダミーデータで動作確認**

時間経過で変動するダミーレベル値を生成して表示。
Expected: レベルメーターがリアルタイムで動く

**Step 4: コミット**

```bash
git commit -m "feat(hud): レベルメーター描画"
```

---

## Task 11: ビジュアル — ウェーブフォーム

**Files:**
- Create: `crates/cplp-hud/src/visuals/waveform.rs`

**Step 1: Waveform を実装**

- ringbuf の Consumer から直近 N サンプルを読み出し
- Line パイプラインでポリライン描画
- 色はチャンネルごとに分ける（You: シアン系、Peer: マゼンタ系）

**Step 2: ダミーのサイン波データで動作確認**

Expected: スクロールするウェーブフォームが表示される

**Step 3: コミット**

```bash
git commit -m "feat(hud): ウェーブフォーム描画"
```

---

## Task 12: ビジュアル — 接続状態インジケーター

**Files:**
- Create: `crates/cplp-hud/src/visuals/connection.rs`

**Step 1: ConnectionIndicator を実装**

- 接続状態：緑●（接続中）/ 赤●（切断）
- ピア名表示
- レイテンシ表示（色分け: <10ms 緑、<20ms 黄、>20ms 赤）
- ジッタ値表示

**Step 2: SessionSnapshot → ConnectionIndicator の接続**

TripleBuffer から読み出して表示。

**Step 3: コミット**

```bash
git commit -m "feat(hud): 接続状態インジケーター"
```

---

## Task 13: 画面遷移と Setup / Connecting / Live 画面

**Files:**
- Modify: `crates/cplp-hud/src/app.rs`

**Step 1: Screen enum と画面遷移ロジック**

```rust
pub enum Screen {
    Setup,
    Connecting,
    Live,
}
```

**Step 2: Setup 画面**

- List ウィジェットでプラグイン一覧
- Button で Create / Join Session
- TextInput でセッション ID
- List で MIDI デバイス

**Step 3: Connecting 画面**

- アニメーション付きインジケーター
- セッション ID 表示
- Cancel ボタン

**Step 4: Live 画面**

Task 10-12 のビジュアルを統合:
- ConnectionIndicator（上部）
- LevelMeter × 2（You / Peer）
- Waveform × 2
- Slider（ミックスバランス）

**Step 5: 画面遷移の動作確認**

Setup → (ボタン) → Connecting → (タイマー or ダミー) → Live の遷移を確認。

**Step 6: コミット**

```bash
git commit -m "feat(hud): 画面遷移と Setup / Connecting / Live 画面"
```

---

## Task 14: グローエフェクト（ポストプロセス）

**Files:**
- Create: `crates/cplp-hud/src/renderer/shaders/glow.wgsl`
- Modify: `crates/cplp-hud/src/renderer/primitives.rs`

**Step 1: Glow パイプラインを実装**

1. レンダーターゲットを中間テクスチャに描画
2. ガウシアンブラー（水平 + 垂直の 2 パス）
3. 元画像にブラー結果を加算合成

```wgsl
// glow.wgsl — ガウシアンブラー（1 パス分）
@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var<uniform> direction: vec2<f32>; // (1,0) or (0,1)

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let weights = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
    var result = textureSample(input_texture, tex_sampler, uv) * weights[0];
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    for (var i = 1; i < 5; i++) {
        let offset = direction * f32(i) / tex_size;
        result += textureSample(input_texture, tex_sampler, uv + offset) * weights[i];
        result += textureSample(input_texture, tex_sampler, uv - offset) * weights[i];
    }
    return result;
}
```

**Step 2: レベルメーターとウェーブフォームにグローを適用**

**Step 3: コミット**

```bash
git commit -m "feat(hud): グローエフェクト（ポストプロセス）"
```

---

## Task 15: スペクトラム描画

**Files:**
- Create: `crates/cplp-hud/src/visuals/spectrum.rs`

**Step 1: FFT 処理**

`rustfft` クレートを追加。ringbuf から読み出した PCM データに FFT を適用し、周波数スペクトラムを計算。

**Step 2: スペクトラムバー描画**

Quad パイプラインで周波数帯ごとのバーを描画。グローエフェクト適用。

**Step 3: コミット**

```bash
git commit -m "feat(hud): スペクトラム描画"
```

**注意:** `rustfft` クレートを依存に追加する必要がある。

---

## Task 16: cplp-app への統合

**Files:**
- Modify: `crates/cplp-app/Cargo.toml`
- Modify: `crates/cplp-app/src/main.rs`

**Step 1: cplp-app に cplp-hud 依存を追加**

**Step 2: CLI フラグで GUI 起動**

```rust
// --gui フラグで HUD モード起動
if args.gui {
    cplp_hud::app::run(hud_view)?;
} else {
    // 既存 CLI モード
}
```

**Step 3: AudioEngine / NetworkManager から HudView へのデータ接続**

- `AudioMeters` を `Arc` で共有、cpal コールバック内で `store`
- `SessionSnapshot` を TripleBuffer で共有
- waveform 用 ringbuf を追加

**Step 4: コミット**

```bash
git commit -m "feat(app): cplp-app に HUD 起動モードを統合"
```

---

## 依存関係の整理

タスク間の依存:

```
Task 1 (足場)
  → Task 2 (wgpu + winit)
    → Task 3 (Quad)
    → Task 4 (Text)
    → Task 5 (Line)
      → Task 6 (統合 Renderer)
        → Task 8 (Widget + Layout)
          → Task 9 (ウィジェット群)
        → Task 10 (メーター)
        → Task 11 (ウェーブフォーム)
        → Task 12 (接続状態)
          → Task 13 (画面遷移)
            → Task 14 (グロー)
            → Task 15 (スペクトラム)
              → Task 16 (統合)

Task 7 (HudState) は独立して並行可能
```
