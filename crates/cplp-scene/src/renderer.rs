//! SceneRenderer — winit 非依存のレンダリングエンジン
//!
//! MeshPipeline + Camera を保持し、任意の TextureView に描画する。
//! Device/Queue は借用で受け取り、所有しない。
//! CLI (winit) と macOS (CAMetalLayer) の両方から利用可能。

use std::time::Instant;

use crate::camera::{Camera, OrbitController};
use crate::editor::{PlacedModule, PlacementEditor};
use crate::input::InputState;
use crate::mesh::{self, MeshPipeline};
use crate::selection::{GizmoState, Ray, SelectionState};

/// winit 非依存のシーンレンダラー
pub struct SceneRenderer {
    pipeline: MeshPipeline,
    camera: Camera,
    orbit: OrbitController,
    input: InputState,
    selection: SelectionState,
    gizmo: GizmoState,
    editor: PlacementEditor,
    start_time: Instant,
    /// フレームパーツの数（エディタがモジュールオブジェクトを識別するため）
    frame_object_count: usize,
    /// ゴーストパネルの数
    ghost_count: usize,
}

impl SceneRenderer {
    /// デバイスとサーフェスフォーマットからレンダラーを作成
    ///
    /// `graph` が Some なら AudioGraph のノードをモジュールとして配置する。
    /// None ならデフォルトの AudioGraph を使用する。
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        Self::with_graph(device, surface_format, width, height, None)
    }

    /// AudioGraph を指定してレンダラーを作成
    pub fn with_graph(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        graph: Option<&cplp_core::audio_graph::AudioGraph>,
    ) -> Self {
        let mut pipeline = MeshPipeline::new(device, surface_format);

        let aspect = if height > 0 {
            width as f32 / height as f32
        } else {
            16.0 / 9.0
        };

        let camera = Camera::rack_view(aspect);
        let orbit = OrbitController::from_camera(&camera);
        let input = InputState::new(width, height);
        let mut selection = SelectionState::new();
        let gizmo = GizmoState::new();

        let rack_hp: u32 = 84;
        let rack_rows: u32 = 2;

        let mut editor = PlacementEditor::new(rack_hp, rack_rows, width, height);
        let default_graph = cplp_core::audio_graph::AudioGraph::default_setup();
        let graph_ref = graph.unwrap_or(&default_graph);
        let frame_object_count = build_rack_scene(&mut pipeline, &mut selection, &mut editor, device, graph_ref);

        // ゴーストパネルを追加
        let ghost_count = add_ghost_panels(&mut pipeline, &editor, device);

        Self {
            pipeline,
            camera,
            orbit,
            input,
            selection,
            gizmo,
            editor,
            start_time: Instant::now(),
            frame_object_count,
            ghost_count,
        }
    }

    /// リサイズ時にカメラのアスペクト比を更新
    pub fn resize(&mut self, width: u32, height: u32) {
        self.camera.set_aspect(width, height);
        self.input.window_size = [width as f32, height as f32];
        self.editor.resize(width, height);
    }

    /// winit イベントを処理（カメラ操作・選択）
    ///
    /// イベントを消費した場合 `true` を返す。
    pub fn process_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        self.input.process_event(event)
    }

    /// フレーム更新（カメラ操作・選択判定・エディタドラッグ）
    pub fn update(&mut self, device: &wgpu::Device) {
        // --- エディタのドラッグ処理 ---

        // 左ドラッグ: エディタ選択中はモジュール移動、それ以外はカメラ回転
        if self.editor.is_dragging() {
            // ドラッグ中はモジュール移動
            let pos = self.input.cursor_pos;
            self.editor.on_mouse_move(
                pos[0],
                pos[1],
                self.camera.fov_y,
                self.camera.aspect,
                self.camera.eye[2],
                self.camera.target[2],
            );
            // ドラッグデルタを消費（カメラ回転させない）
            let _ = self.input.consume_left_drag_delta();
        } else {
            // 左ドラッグ → カメラ回転
            if let Some(delta) = self.input.consume_left_drag_delta() {
                self.orbit.rotate(delta[0], delta[1]);
            }
        }

        // 左ボタン解放 → ドラッグ終了
        if !self.input.left.pressed && self.editor.is_dragging() {
            self.editor.on_mouse_up();
        }

        // 中ドラッグ → パン
        if let Some(delta) = self.input.consume_middle_drag_delta() {
            self.orbit.pan(delta[0], delta[1]);
        }

        // 右ドラッグ → パン（代替）
        if let Some(delta) = self.input.consume_right_drag_delta() {
            self.orbit.pan(delta[0], delta[1]);
        }

        // スクロール → ズーム
        if self.input.scroll_delta.abs() > 0.0 {
            self.orbit.zoom(self.input.scroll_delta);
        }

        // カメラに反映
        self.orbit.apply(&mut self.camera);

        // --- 選択 ---
        if self.input.clicked {
            let ndc = self.input.screen_to_ndc(self.input.click_pos);
            let ray = Ray::from_ndc(ndc, &self.camera);
            let pick_result = self.selection.pick(&ray);

            // SelectionState の選択結果をエディタに反映
            if let Some(sel_idx) = pick_result {
                // selectables のインデックスがモジュールインデックスに対応
                self.editor.select(sel_idx);
            } else {
                self.editor.deselect();
            }

            // クリックで選択されたモジュールがあれば、次の左ドラッグでモジュール移動を開始
            if self.editor.selected().is_some() {
                let pos = self.input.click_pos;
                self.editor.on_mouse_down(pos[0], pos[1]);
            }
        }

        // --- エディタ dirty → シーンオブジェクト更新 ---
        if self.editor.take_dirty() {
            self.rebuild_module_objects(device);
        }

        // フレーム終了
        self.input.begin_frame();
    }

    /// エディタの状態変更に合わせてモジュールとゴーストのシーンオブジェクトを再構築
    fn rebuild_module_objects(&mut self, device: &wgpu::Device) {
        // フレームオブジェクト以降を全削除
        self.pipeline.truncate(self.frame_object_count);
        self.selection.selectables.clear();

        // モジュールを再追加
        for (i, m) in self.editor.modules().iter().enumerate() {
            let (verts, idxs) = mesh::build_module_panel(m.hp_width, m.color);
            let pos = m.world_position(self.editor.rack_hp);
            self.pipeline.add_static(device, &verts, &idxs, pos);

            let aabb = m.aabb(self.editor.rack_hp);
            self.selection.add_selectable(&m.name, aabb, self.frame_object_count + i);
        }

        // ゴーストパネルを再追加
        self.ghost_count = add_ghost_panels(&mut self.pipeline, &self.editor, device);
    }

    /// エディタへの参照
    pub fn editor(&self) -> &PlacementEditor {
        &self.editor
    }

    /// エディタへの可変参照
    pub fn editor_mut(&mut self) -> &mut PlacementEditor {
        &mut self.editor
    }

    /// 1 フレームを描画
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) {
        let time = self.start_time.elapsed().as_secs_f32();

        self.pipeline.update_animations(queue, time);
        self.pipeline.update_camera(queue, &self.camera);

        // 選択ハイライト: 選択オブジェクトの色を変更（将来的に専用 uniform に移行）
        // 現時点では pipeline の描画で対応せず、ログのみ
        // TODO: 選択オブジェクト用の highlight uniform を追加

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.012,
                            g: 0.014,
                            b: 0.028,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            self.pipeline.render(&mut pass);
        }

        queue.submit(std::iter::once(encoder.finish()));
    }

    /// 選択状態への参照
    pub fn selection(&self) -> &SelectionState {
        &self.selection
    }

    /// ギズモ状態への参照
    pub fn gizmo(&self) -> &GizmoState {
        &self.gizmo
    }

    /// ギズモ状態への可変参照
    pub fn gizmo_mut(&mut self) -> &mut GizmoState {
        &mut self.gizmo
    }
}

/// Depth テクスチャを作成（ユーティリティ関数）
pub fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// ユーロラックシーンを構築（フレーム + モジュール + 選択 AABB 登録）
///
/// フレームオブジェクト数を返す。
fn build_rack_scene(
    pipeline: &mut MeshPipeline,
    selection: &mut SelectionState,
    editor: &mut PlacementEditor,
    device: &wgpu::Device,
    graph: &cplp_core::audio_graph::AudioGraph,
) -> usize {
    let rack_hp = editor.rack_hp;
    let rack_rows = editor.rack_rows;

    // メタリックなアルミフレーム色
    let frame_color = [0.42, 0.42, 0.46];
    let frame_parts = mesh::build_rack_frame(rack_hp, rack_rows, frame_color);
    let frame_count = frame_parts.len();
    for (verts, idxs, pos) in frame_parts {
        pipeline.add_static(device, &verts, &idxs, pos);
    }

    // AudioGraph からモジュールを生成
    let mut hp_pos: u32 = 0;
    let row: u32 = 0;

    for node in graph.nodes() {
        let (hp_width, color) = node_to_module_params(&node.node_type);
        let module = PlacedModule {
            name: node.name.clone(),
            hp_width,
            hp_pos,
            row,
            color,
        };
        if let Err(e) = editor.add_module(module) {
            tracing::warn!("AudioGraph モジュール '{}' の追加に失敗: {}", node.name, e);
        }
        hp_pos += hp_width;
    }

    // エディタのモジュールをパイプラインに追加
    for (i, m) in editor.modules().iter().enumerate() {
        let (verts, idxs) = mesh::build_module_panel(m.hp_width, m.color);
        let pos = m.world_position(rack_hp);
        tracing::info!(
            "Module '{}': {}HP at hp={}, row={}, pos={:?}",
            m.name, m.hp_width, m.hp_pos, m.row, pos
        );
        pipeline.add_static(device, &verts, &idxs, pos);

        // モジュールの AABB を登録（選択用）
        let aabb = m.aabb(rack_hp);
        selection.add_selectable(&m.name, aabb, frame_count + i);
    }

    // dirty フラグをクリア（初期構築分）
    editor.take_dirty();

    frame_count
}

/// AudioGraph の NodeType から HP 幅とパネル色を決定
fn node_to_module_params(node_type: &cplp_core::audio_graph::NodeType) -> (u32, [f32; 3]) {
    use cplp_core::audio_graph::{AudioModuleType, NodeType};
    match node_type {
        NodeType::MidiInput => (8, [0.2, 0.15, 0.4]),              // 紫系
        NodeType::ClapInstrument { .. } => (16, [0.1, 0.55, 0.9]), // 青系
        NodeType::ClapEffect { .. } => (12, [0.9, 0.5, 0.1]),     // オレンジ系
        NodeType::AudioModule { module_type } => match module_type {
            AudioModuleType::Synthesizer => (14, [0.9, 0.2, 0.3]), // 赤系
            AudioModuleType::Looper => (16, [0.3, 0.8, 0.8]),     // シアン系
            AudioModuleType::BeatMachine => (12, [0.7, 0.3, 0.8]),// 紫系
        },
        NodeType::Mixer => (20, [0.2, 0.7, 0.3]),                  // 緑系
        NodeType::Output => (8, [0.8, 0.8, 0.2]),                  // 黄系
    }
}

/// ゴーストパネルをパイプラインに追加。追加した数を返す。
fn add_ghost_panels(
    pipeline: &mut MeshPipeline,
    editor: &PlacementEditor,
    device: &wgpu::Device,
) -> usize {
    let ghosts = editor.build_ghost_meshes();
    let count = ghosts.len();
    for (verts, idxs, pos) in ghosts {
        pipeline.add_static(device, &verts, &idxs, pos);
    }
    count
}
