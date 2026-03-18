//! SceneRenderer — winit 非依存のレンダリングエンジン
//!
//! MeshPipeline + Camera を保持し、任意の TextureView に描画する。
//! Device/Queue は借用で受け取り、所有しない。
//! CLI (winit) と macOS (CAMetalLayer) の両方から利用可能。

use std::time::Instant;

use crate::camera::Camera;
use crate::mesh::{self, MeshPipeline};

/// winit 非依存のシーンレンダラー
pub struct SceneRenderer {
    pipeline: MeshPipeline,
    camera: Camera,
    start_time: Instant,
}

impl SceneRenderer {
    /// デバイスとサーフェスフォーマットからレンダラーを作成
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let mut pipeline = MeshPipeline::new(device, surface_format);
        build_rack_scene(&mut pipeline, device);

        let aspect = if height > 0 {
            width as f32 / height as f32
        } else {
            16.0 / 9.0
        };

        Self {
            pipeline,
            camera: Camera::rack_view(aspect),
            start_time: Instant::now(),
        }
    }

    /// リサイズ時にカメラのアスペクト比を更新
    pub fn resize(&mut self, width: u32, height: u32) {
        self.camera.set_aspect(width, height);
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
                        store: wgpu::StoreOp::Discard, // 下流で使わないので Discard（Metal tile-based GPU 最適化）
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            self.pipeline.render(&mut pass);
        }

        queue.submit(std::iter::once(encoder.finish()));
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

/// ユーロラックシーンを構築
fn build_rack_scene(pipeline: &mut MeshPipeline, device: &wgpu::Device) {
    let rack_hp: u32 = 84;
    let rack_rows: u32 = 2;

    // メタリックなアルミフレーム色
    let frame_color = [0.42, 0.42, 0.46];
    let frame_parts = mesh::build_rack_frame(rack_hp, rack_rows, frame_color);
    for (verts, idxs, pos) in frame_parts {
        pipeline.add_static(device, &verts, &idxs, pos);
    }

    struct ModuleDef {
        name: &'static str,
        hp_width: u32,
        hp_pos: u32,
        row: u32,
        color: [f32; 3],
    }

    let modules = [
        ModuleDef { name: "Looper",    hp_width: 16, hp_pos: 0,  row: 0, color: [0.1, 0.55, 0.9] },
        ModuleDef { name: "Mixer",     hp_width: 20, hp_pos: 16, row: 0, color: [0.2, 0.7, 0.3] },
        ModuleDef { name: "FX",        hp_width: 12, hp_pos: 36, row: 0, color: [0.9, 0.5, 0.1] },
        ModuleDef { name: "Sequencer", hp_width: 24, hp_pos: 48, row: 0, color: [0.7, 0.3, 0.8] },
        ModuleDef { name: "Monitor",   hp_width: 12, hp_pos: 72, row: 0, color: [0.8, 0.8, 0.2] },
        ModuleDef { name: "OSC",       hp_width: 10, hp_pos: 0,  row: 1, color: [0.9, 0.2, 0.3] },
        ModuleDef { name: "Filter",    hp_width: 14, hp_pos: 10, row: 1, color: [0.3, 0.8, 0.8] },
        ModuleDef { name: "Env",       hp_width: 8,  hp_pos: 24, row: 1, color: [0.6, 0.4, 0.2] },
        ModuleDef { name: "LFO",       hp_width: 8,  hp_pos: 32, row: 1, color: [0.5, 0.5, 0.9] },
    ];

    for m in &modules {
        let (verts, idxs) = mesh::build_module_panel(m.hp_width, m.color);
        let pos = mesh::module_world_position(m.hp_pos, m.hp_width, rack_hp, m.row);
        tracing::info!(
            "Module '{}': {}HP at hp={}, row={}, pos={:?}",
            m.name, m.hp_width, m.hp_pos, m.row, pos
        );
        pipeline.add_static(device, &verts, &idxs, pos);
    }
}
