//! WgpuBackend — wgpu ベースの RenderBackend 実装
//!
//! macOS / Linux / Web(WebGPU) で動作する。
//! 既存の MeshPipeline + SceneRenderer のロジックを RenderBackend trait で包む。

use wgpu::util::DeviceExt;

use crate::render_backend::{LightConfig, RenderBackend};
use crate::scene_graph::{MeshVertex, SceneGraph};

/// wgpu GPU オブジェクト（MeshPipeline 内部で管理するもの）
struct GpuObject {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    model_buffer: wgpu::Buffer,
    model_bind_group: wgpu::BindGroup,
    /// 基準位置
    base_position: [f32; 3],
    /// アニメーションパラメータ
    animation: Option<crate::scene_graph::AnimationParams>,
}

/// wgpu バックエンド
pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    light_buffer: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    model_bind_group_layout: wgpu::BindGroupLayout,
    objects: Vec<GpuObject>,
    /// 現在の描画サイズ
    width: u32,
    height: u32,
}

/// wgpu バックエンドのエラー
#[derive(Debug, thiserror::Error)]
pub enum WgpuBackendError {
    #[error("wgpu error: {0}")]
    Wgpu(String),
}

/// MeshVertex の wgpu 頂点レイアウト
const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &[
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: (std::mem::size_of::<[f32; 3]>() * 2) as wgpu::BufferAddress,
            shader_location: 2,
            format: wgpu::VertexFormat::Float32x3,
        },
    ],
};

impl WgpuBackend {
    /// デバイス・キュー・サーフェスフォーマットから WgpuBackend を作成
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        light_config: &LightConfig,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // group(0): camera (view_proj + eye_pos)
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera_uniform"),
            size: 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // group(1): per-object model matrix
        let model_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("model_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // group(2): light
        let light_uniform = light_config_to_uniform(light_config);
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("light_uniform"),
            contents: bytemuck::cast_slice(&[light_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light_bind_group"),
            layout: &light_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene_pipeline_layout"),
            bind_group_layouts: &[&camera_layout, &model_bind_group_layout, &light_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[VERTEX_LAYOUT],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            device,
            queue,
            pipeline,
            camera_buffer,
            camera_bind_group,
            light_buffer,
            light_bind_group,
            model_bind_group_layout,
            objects: Vec::new(),
            width: 0,
            height: 0,
        }
    }

    /// device の参照を取得
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// queue の参照を取得
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// テクスチャビューに描画（SceneRenderer 互換）
    pub fn render_to_texture(
        &self,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        time: f32,
    ) {
        self.update_animations(time);

        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
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

            self.draw(&mut pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// GPU オブジェクトを追加（内部用）
    fn add_gpu_object(
        &mut self,
        vertices: &[MeshVertex],
        indices: &[u32],
        position: [f32; 3],
        animation: Option<crate::scene_graph::AnimationParams>,
    ) {
        let vertex_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_vb"),
                    contents: bytemuck::cast_slice(vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_ib"),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        let identity: [[f32; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut init_data = [0u8; 128];
        init_data[..64].copy_from_slice(bytemuck::cast_slice(&identity));
        init_data[64..128].copy_from_slice(bytemuck::cast_slice(&identity));

        let model_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("model_uniform"),
                    contents: &init_data,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
        let model_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model_bind_group"),
            layout: &self.model_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: model_buffer.as_entire_binding(),
            }],
        });

        self.objects.push(GpuObject {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            model_buffer,
            model_bind_group,
            base_position: position,
            animation,
        });
    }

    /// アニメーション更新
    fn update_animations(&self, time: f32) {
        for obj in &self.objects {
            let [bx, by, bz] = obj.base_position;

            let (dy, scale) = if let Some(anim) = &obj.animation {
                anim.evaluate(time)
            } else {
                (0.0, 1.0)
            };

            let transform: [[f32; 4]; 4] = [
                [scale, 0.0, 0.0, 0.0],
                [0.0, scale, 0.0, 0.0],
                [0.0, 0.0, scale, 0.0],
                [bx, by + dy, bz, 1.0],
            ];

            let inv_scale = 1.0 / scale;
            let normal_matrix: [[f32; 4]; 4] = [
                [inv_scale, 0.0, 0.0, 0.0],
                [0.0, inv_scale, 0.0, 0.0],
                [0.0, 0.0, inv_scale, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ];

            let mut data = [0u8; 128];
            data[..64].copy_from_slice(bytemuck::cast_slice(&transform));
            data[64..128].copy_from_slice(bytemuck::cast_slice(&normal_matrix));
            self.queue.write_buffer(&obj.model_buffer, 0, &data);
        }
    }

    /// レンダーパスに描画
    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(2, &self.light_bind_group, &[]);
        for obj in &self.objects {
            pass.set_bind_group(1, &obj.model_bind_group, &[]);
            pass.set_vertex_buffer(0, obj.vertex_buffer.slice(..));
            pass.set_index_buffer(obj.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..obj.index_count, 0, 0..1);
        }
    }
}

impl RenderBackend for WgpuBackend {
    type Error = WgpuBackendError;

    fn submit_scene(&mut self, scene: &SceneGraph) -> Result<(), Self::Error> {
        // 既存のオブジェクトをクリア
        self.objects.clear();

        // SceneGraph のノードを走査して GPU オブジェクトを作成
        for node in scene.iter_nodes() {
            if let Some(mesh) = &node.mesh {
                self.add_gpu_object(
                    &mesh.vertices,
                    &mesh.indices,
                    node.transform.position,
                    node.animation.clone(),
                );
            }
        }

        tracing::info!(
            "WgpuBackend: submitted {} objects from SceneGraph ({} nodes)",
            self.objects.len(),
            scene.node_count()
        );

        Ok(())
    }

    fn update_camera(
        &mut self,
        view_proj: [[f32; 4]; 4],
        eye_position: [f32; 3],
    ) -> Result<(), Self::Error> {
        let mut data = [0u8; 128];
        data[..64].copy_from_slice(bytemuck::cast_slice(&view_proj));
        let eye_vec4: [f32; 4] = [eye_position[0], eye_position[1], eye_position[2], 1.0];
        data[64..80].copy_from_slice(bytemuck::cast_slice(&eye_vec4));
        self.queue.write_buffer(&self.camera_buffer, 0, &data);
        Ok(())
    }

    fn render_frame(&mut self, time: f32) -> Result<(), Self::Error> {
        self.update_animations(time);
        // NOTE: 実際の描画は render_to_texture() で行う（Surface 管理は呼び出し側）
        // この trait メソッドは将来的に独自 Surface 管理する場合に使う
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
        self.width = width;
        self.height = height;
        Ok(())
    }
}

/// Depth テクスチャを作成（ユーティリティ関数）
pub fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
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

/// LightConfig → GPU uniform 変換
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniform {
    direction: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
}

fn light_config_to_uniform(config: &LightConfig) -> LightUniform {
    LightUniform {
        direction: [
            config.direction[0],
            config.direction[1],
            config.direction[2],
            config.ambient,
        ],
        color: [
            config.color[0],
            config.color[1],
            config.color[2],
            config.specular,
        ],
        params: [config.shininess, config.rim, 0.0, 0.0],
    }
}
