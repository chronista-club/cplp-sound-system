use wgpu::util::DeviceExt;

/// ポストプロセス・グローエフェクト
///
/// 3ステップで動作:
/// 1. シーンを中間テクスチャに描画
/// 2. ガウシアンブラー（水平→垂直の2パス、半分サイズ）
/// 3. 元シーン + ブラー結果を加算合成して出力
pub struct GlowPipeline {
    blur_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    // 中間テクスチャ（元サイズ）
    scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,
    // ブラー用テクスチャ（半分サイズ、パフォーマンスのため）
    blur_texture_a: wgpu::Texture,
    blur_view_a: wgpu::TextureView,
    blur_texture_b: wgpu::Texture,
    blur_view_b: wgpu::TextureView,
    // Uniform buffers（ブラー方向）
    h_direction_buffer: wgpu::Buffer,
    v_direction_buffer: wgpu::Buffer,
    // Bind groups
    blur_h_bind_group: wgpu::BindGroup,
    blur_v_bind_group: wgpu::BindGroup,
    composite_bind_group: wgpu::BindGroup,
    // Bind group layouts（リサイズ時の再生成用）
    blur_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    // テクスチャフォーマット
    format: wgpu::TextureFormat,
    // 現在のサイズ
    width: u32,
    height: u32,
}

impl GlowPipeline {
    pub fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        // シェーダー
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glow blur shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/glow.wgsl").into()),
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glow composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });

        // サンプラー（Linear でスムーズなブラー）
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // ブラー方向 uniform buffers
        let h_direction_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("glow h direction"),
            contents: bytemuck::cast_slice(&[1.0_f32, 0.0_f32]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let v_direction_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("glow v direction"),
            contents: bytemuck::cast_slice(&[0.0_f32, 1.0_f32]),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // ブラー bind group layout
        let blur_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("glow blur bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        // コンポジット bind group layout
        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("glow composite bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // ブラーパイプライン
        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glow blur pipeline layout"),
            bind_group_layouts: &[&blur_bind_group_layout],
            immediate_size: 0,
        });
        let blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glow blur pipeline"),
            layout: Some(&blur_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blur_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blur_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // コンポジットパイプライン
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("glow composite pipeline layout"),
                bind_group_layouts: &[&composite_bind_group_layout],
                immediate_size: 0,
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glow composite pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // テクスチャ + bind groups を生成
        let (
            scene_texture,
            scene_view,
            blur_texture_a,
            blur_view_a,
            blur_texture_b,
            blur_view_b,
            blur_h_bind_group,
            blur_v_bind_group,
            composite_bind_group,
        ) = Self::create_textures_and_bind_groups(
            device,
            format,
            width,
            height,
            &sampler,
            &h_direction_buffer,
            &v_direction_buffer,
            &blur_bind_group_layout,
            &composite_bind_group_layout,
        );

        Self {
            blur_pipeline,
            composite_pipeline,
            sampler,
            scene_texture,
            scene_view,
            blur_texture_a,
            blur_view_a,
            blur_texture_b,
            blur_view_b,
            h_direction_buffer,
            v_direction_buffer,
            blur_h_bind_group,
            blur_v_bind_group,
            composite_bind_group,
            blur_bind_group_layout,
            composite_bind_group_layout,
            format,
            width,
            height,
        }
    }

    /// ウィンドウリサイズ時にテクスチャを再生成
    pub fn resize(&mut self, device: &wgpu::Device, _queue: &wgpu::Queue, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;

        let (
            scene_texture,
            scene_view,
            blur_texture_a,
            blur_view_a,
            blur_texture_b,
            blur_view_b,
            blur_h_bind_group,
            blur_v_bind_group,
            composite_bind_group,
        ) = Self::create_textures_and_bind_groups(
            device,
            self.format,
            width,
            height,
            &self.sampler,
            &self.h_direction_buffer,
            &self.v_direction_buffer,
            &self.blur_bind_group_layout,
            &self.composite_bind_group_layout,
        );

        self.scene_texture = scene_texture;
        self.scene_view = scene_view;
        self.blur_texture_a = blur_texture_a;
        self.blur_view_a = blur_view_a;
        self.blur_texture_b = blur_texture_b;
        self.blur_view_b = blur_view_b;
        self.blur_h_bind_group = blur_h_bind_group;
        self.blur_v_bind_group = blur_v_bind_group;
        self.composite_bind_group = composite_bind_group;
    }

    /// シーン描画先のテクスチャビューを返す
    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.scene_view
    }

    /// ブラー + コンポジットを実行
    /// encoder に 3 つの render pass を追加: blur_h -> blur_v -> composite
    pub fn apply(&self, encoder: &mut wgpu::CommandEncoder, output_view: &wgpu::TextureView) {
        // パス1: 水平ブラー（scene -> blur_a）
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glow blur horizontal"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.blur_view_a,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &self.blur_h_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // パス2: 垂直ブラー（blur_a -> blur_b）
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glow blur vertical"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.blur_view_b,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &self.blur_v_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // パス3: コンポジット（scene + blur_b -> output）
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glow composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &self.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// テクスチャと bind groups を生成（初期化・リサイズ共通）
    #[allow(clippy::too_many_arguments)]
    fn create_textures_and_bind_groups(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        sampler: &wgpu::Sampler,
        h_direction_buffer: &wgpu::Buffer,
        v_direction_buffer: &wgpu::Buffer,
        blur_bind_group_layout: &wgpu::BindGroupLayout,
        composite_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> (
        wgpu::Texture,
        wgpu::TextureView,
        wgpu::Texture,
        wgpu::TextureView,
        wgpu::Texture,
        wgpu::TextureView,
        wgpu::BindGroup,
        wgpu::BindGroup,
        wgpu::BindGroup,
    ) {
        let tex_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;

        // シーンテクスチャ（元サイズ）
        let scene_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glow scene texture"),
            dimension: wgpu::TextureDimension::D2,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            format,
            usage: tex_usage,
            view_formats: &[],
        });
        let scene_view = scene_texture.create_view(&Default::default());

        // ブラーテクスチャ（半分サイズ）
        let blur_w = (width / 2).max(1);
        let blur_h = (height / 2).max(1);

        let blur_texture_a = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glow blur texture a"),
            dimension: wgpu::TextureDimension::D2,
            size: wgpu::Extent3d {
                width: blur_w,
                height: blur_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            format,
            usage: tex_usage,
            view_formats: &[],
        });
        let blur_view_a = blur_texture_a.create_view(&Default::default());

        let blur_texture_b = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glow blur texture b"),
            dimension: wgpu::TextureDimension::D2,
            size: wgpu::Extent3d {
                width: blur_w,
                height: blur_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            format,
            usage: tex_usage,
            view_formats: &[],
        });
        let blur_view_b = blur_texture_b.create_view(&Default::default());

        // 水平ブラー bind group: scene -> blur_a
        let blur_h_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glow blur h bind group"),
            layout: blur_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: h_direction_buffer.as_entire_binding(),
                },
            ],
        });

        // 垂直ブラー bind group: blur_a -> blur_b
        let blur_v_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glow blur v bind group"),
            layout: blur_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&blur_view_a),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: v_direction_buffer.as_entire_binding(),
                },
            ],
        });

        // コンポジット bind group: scene + blur_b -> output
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glow composite bind group"),
            layout: composite_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&blur_view_b),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        (
            scene_texture,
            scene_view,
            blur_texture_a,
            blur_view_a,
            blur_texture_b,
            blur_view_b,
            blur_h_bind_group,
            blur_v_bind_group,
            composite_bind_group,
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fullscreen_triangle_covers_screen() {
        // idx=0: x=-1, y=-1 -> bottom-left
        // idx=1: x=-1, y=3  -> beyond top-left
        // idx=2: x=3,  y=-1 -> beyond bottom-right
        // この三角形が [-1,1]x[-1,1] の画面全体をカバーすることを確認
        let verts: Vec<(f32, f32)> = (0..3)
            .map(|idx: u32| {
                let x = (idx / 2) as f32 * 4.0 - 1.0;
                let y = (idx % 2) as f32 * 4.0 - 1.0;
                (x, y)
            })
            .collect();
        assert_eq!(verts[0], (-1.0, -1.0));
        assert_eq!(verts[1], (-1.0, 3.0));
        assert_eq!(verts[2], (3.0, -1.0));
    }
}
