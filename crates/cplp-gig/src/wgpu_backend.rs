//! wgpu バックエンド – `feature = "wgpu"` でのみコンパイルされる
//!
//! `RenderBackend` トレイトの wgpu 実装。
//! macOS・Linux・Windows 向けのデフォルトバックエンド。
//! visionOS 向けは将来 `metal_backend.rs` / `realitykit_backend.rs` を追加する。

use std::collections::HashMap;
use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::mesh::{MeshData, Vertex};
use crate::render_backend::{DrawCall, MeshHandle, RenderBackend};

/// wgpu でアップロード済みのメッシュバッファ。
struct GpuMesh {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
}

/// wgpu を用いた `RenderBackend` 実装。
pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    meshes: HashMap<u32, GpuMesh>,
    next_handle: u32,
    width: u32,
    height: u32,
}

impl WgpuBackend {
    /// ウィンドウから WgpuBackend を初期化する。
    pub fn new(window: Arc<winit::window::Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance.create_surface(window)?;
        let adapter = pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        ))?;
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor::default())
        )?;

        let config = surface
            .get_default_config(&adapter, size.width, size.height)
            .ok_or_else(|| anyhow::anyhow!("Surface not compatible"))?;
        surface.configure(&device, &config);

        // ── ユニフォームバッファ（カメラ行列）──────────────────────────────
        let uniform_data = [0u8; 64]; // 4×4 f32 行列 = 64 バイト
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gig_uniform"),
            contents: &uniform_data,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("gig_bgl"),
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
            },
        );
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gig_bg"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        // ── シェーダー & パイプライン ────────────────────────────────────
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gig_shader"),
            source: wgpu::ShaderSource::Wgsl(GIG_SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gig_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gig_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            device,
            queue,
            surface,
            config,
            pipeline,
            uniform_buf,
            uniform_bind_group,
            meshes: HashMap::new(),
            next_handle: 0,
            width: size.width,
            height: size.height,
        })
    }

    /// カメラのビュー-プロジェクション行列をユニフォームバッファに書き込む。
    ///
    /// 現状は固定の俯瞰カメラ（簡易透視投影）。
    fn update_camera(&self) {
        // 簡易: 単位行列（将来は実際のカメラ行列を計算）
        let identity: [[f32; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let bytes = bytemuck::cast_slice(&identity);
        self.queue.write_buffer(&self.uniform_buf, 0, bytes);
    }
}

impl RenderBackend for WgpuBackend {
    fn upload_mesh(&mut self, mesh: &MeshData) -> MeshHandle {
        let handle_id = self.next_handle;
        self.next_handle += 1;

        let vertex_bytes: &[u8] = bytemuck::cast_slice(&mesh.vertices);
        let vertex_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gig_vb"),
            contents: vertex_bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_bytes: &[u8] = bytemuck::cast_slice(&mesh.indices);
        let index_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gig_ib"),
            contents: index_bytes,
            usage: wgpu::BufferUsages::INDEX,
        });

        self.meshes.insert(
            handle_id,
            GpuMesh {
                vertex_buf,
                index_buf,
                index_count: mesh.indices.len() as u32,
            },
        );

        MeshHandle(handle_id)
    }

    fn free_mesh(&mut self, handle: MeshHandle) {
        if let Some(gpu_mesh) = self.meshes.remove(&handle.0) {
            gpu_mesh.vertex_buf.destroy();
            gpu_mesh.index_buf.destroy();
        }
    }

    fn submit_frame(&mut self, draw_calls: &[DrawCall]) {
        self.update_camera();

        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => {
                tracing::warn!("gig surface error: {:?}", e);
                return;
            }
        };
        let view = output.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gig_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.05,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);

            for call in draw_calls {
                if let Some(gpu_mesh) = self.meshes.get(&call.mesh.0) {
                    pass.set_vertex_buffer(0, gpu_mesh.vertex_buf.slice(..));
                    pass.set_index_buffer(
                        gpu_mesh.index_buf.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }
}

fn vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            // position
            wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
            // normal
            wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
            // uv
            wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
            // color
            wgpu::VertexAttribute { offset: 32, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
        ],
    }
}

const GIG_SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
    @location(3) color:    vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    // 簡易ランバート照明（光源方向: 上方向固定）
    let light = vec3<f32>(0.0, 1.0, 0.5);
    let diff = max(dot(normalize(in.normal), normalize(light)), 0.15);
    out.color = vec4<f32>(in.color.rgb * diff, in.color.a);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;
