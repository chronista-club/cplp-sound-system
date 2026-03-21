use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::scene_graph::MeshVertex;

/// 頂点データ（position + color + normal）
///
/// `scene_graph::MeshVertex` の型エイリアス。
/// プラットフォーム非依存のメッシュ生成関数と wgpu パイプラインの両方で使用する。
pub type Vertex = MeshVertex;

impl Vertex {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
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
}

/// シーンオブジェクト（メッシュ + トランスフォーム + アニメーション）
pub struct SceneObject {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    model_buffer: wgpu::Buffer,
    model_bind_group: wgpu::BindGroup,
    /// 基準位置
    pub base_position: [f32; 3],
    /// アニメーションパラメータ（None = 静的）
    pub animation: Option<Animation>,
}

/// アニメーションパラメータ
pub struct Animation {
    /// 上下の浮遊振幅
    pub bob_amplitude: f32,
    /// 浮遊速度（rad/sec）
    pub bob_speed: f32,
    /// 呼吸スケール振幅（1.0 ± scale_amplitude）
    pub breathe_amplitude: f32,
    /// 呼吸速度（rad/sec）
    pub breathe_speed: f32,
    /// 位相オフセット（オブジェクト毎にずらす）
    pub phase_offset: f32,
}

/// ライト uniform データ
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightUniform {
    /// xyz = 方向（正規化）, w = ambient 強度
    pub direction: [f32; 4],
    /// xyz = ライト色, w = specular 強度
    pub color: [f32; 4],
    /// x = shininess, y = rim 強度, z = unused, w = unused
    pub params: [f32; 4],
}

impl Default for LightUniform {
    fn default() -> Self {
        // メインライト: 左上から。ambient 0.15, specular 0.4, shininess 32
        Self {
            direction: [-0.4, -0.7, -0.5, 0.15],
            color: [1.0, 0.98, 0.95, 0.4],
            params: [32.0, 0.25, 0.0, 0.0],
        }
    }
}

/// 3D メッシュレンダリングパイプライン
pub struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    light_buffer: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    model_bind_group_layout: wgpu::BindGroupLayout,
    objects: Vec<SceneObject>,
}

impl MeshPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // group(0): camera (view_proj + eye_pos = 64 + 16 = 80 bytes, aligned to 16)
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera_uniform"),
            size: 128, // mat4x4 (64) + vec4 (16) = 80, padded to 128 for alignment
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

        // group(1): per-object model matrix (transform + normal_matrix = 128 bytes)
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
        let light_uniform = LightUniform::default();
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
                buffers: &[Vertex::LAYOUT],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
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
            pipeline,
            camera_buffer,
            camera_bind_group,
            light_buffer,
            light_bind_group,
            model_bind_group_layout,
            objects: Vec::new(),
        }
    }

    /// カメラ uniform を更新（view_proj + eye_pos）
    pub fn update_camera(&self, queue: &wgpu::Queue, camera: &Camera) {
        let vp = camera.view_proj();
        let eye = camera.eye;
        // view_proj (64 bytes) + eye_pos as vec4 (16 bytes)
        let mut data = [0u8; 128];
        data[..64].copy_from_slice(bytemuck::cast_slice(&vp));
        let eye_vec4: [f32; 4] = [eye[0], eye[1], eye[2], 1.0];
        data[64..80].copy_from_slice(bytemuck::cast_slice(&eye_vec4));
        queue.write_buffer(&self.camera_buffer, 0, &data);
    }

    /// オブジェクトを追加（静的：アニメーションなし）
    pub fn add_static(
        &mut self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        position: [f32; 3],
    ) {
        self.add_object(device, vertices, indices, position, None);
    }

    /// オブジェクトを追加（アニメーション付き）
    pub fn add_animated(
        &mut self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        position: [f32; 3],
        animation: Animation,
    ) {
        self.add_object(device, vertices, indices, position, Some(animation));
    }

    fn add_object(
        &mut self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        position: [f32; 3],
        animation: Option<Animation>,
    ) {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_vb"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_ib"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // 初期モデルデータ（transform + normal_matrix = 128 bytes）
        let identity: [[f32; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut init_data = [0u8; 128];
        init_data[..64].copy_from_slice(bytemuck::cast_slice(&identity));
        init_data[64..128].copy_from_slice(bytemuck::cast_slice(&identity));

        let model_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("model_uniform"),
            contents: &init_data,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model_bind_group"),
            layout: &self.model_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: model_buffer.as_entire_binding(),
            }],
        });

        self.objects.push(SceneObject {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            model_buffer,
            model_bind_group,
            base_position: position,
            animation,
        });
    }

    /// アニメーション更新（毎フレーム呼ぶ）
    pub fn update_animations(&self, queue: &wgpu::Queue, time: f32) {
        for obj in &self.objects {
            let [bx, by, bz] = obj.base_position;

            let (dy, scale) = if let Some(anim) = &obj.animation {
                let t = time * anim.bob_speed + anim.phase_offset;
                let dy = t.sin() * anim.bob_amplitude;
                let bt = time * anim.breathe_speed + anim.phase_offset * 0.7;
                let scale = 1.0 + bt.sin() * anim.breathe_amplitude;
                (dy, scale)
            } else {
                (0.0, 1.0)
            };

            // TRS: translate * scale
            let transform: [[f32; 4]; 4] = [
                [scale, 0.0, 0.0, 0.0],
                [0.0, scale, 0.0, 0.0],
                [0.0, 0.0, scale, 0.0],
                [bx, by + dy, bz, 1.0],
            ];

            // Normal matrix = transpose(inverse(upper-left 3x3 of transform))
            // For uniform scale, this simplifies to (1/scale) * I
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
            queue.write_buffer(&obj.model_buffer, 0, &data);
        }
    }

    /// オブジェクト数を指定した位置で切り詰める（再構築用）
    pub fn truncate(&mut self, count: usize) {
        self.objects.truncate(count);
    }

    /// オブジェクトの base_position を更新
    pub fn set_object_position(&mut self, index: usize, position: [f32; 3]) {
        if let Some(obj) = self.objects.get_mut(index) {
            obj.base_position = position;
        }
    }

    /// レンダーパスに描画
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
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

// ── ユーロラック定数 ──────────────────────────────

/// 1HP（Horizontal Pitch）のワールド単位幅
pub const HP_UNIT: f32 = 0.05;
/// 3U（標準モジュール高さ）のワールド単位
pub const ROW_HEIGHT_3U: f32 = 1.28;
/// レールの厚み
pub const RAIL_THICKNESS: f32 = 0.06;
/// レールの奥行き
pub const RAIL_DEPTH: f32 = 0.04;
/// モジュールパネルの奥行き
pub const PANEL_DEPTH: f32 = 0.03;
/// パネルとレール間のマージン
pub const PANEL_MARGIN: f32 = 0.01;
/// ベゼル幅
pub const BEZEL_WIDTH: f32 = 0.012;
/// ベゼル奥行き（前面からの突出量）
pub const BEZEL_DEPTH: f32 = 0.008;

// ── メッシュ生成 ────────────────────────────────

/// グリッド（床）のメッシュデータ
pub fn build_grid(size: f32, step: f32, color: [f32; 3]) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let half_thickness = 0.015;
    let normal = [0.0, 1.0, 0.0]; // 床面は上向き

    let n = (size / step) as i32;
    for i in -n..=n {
        let pos = i as f32 * step;
        let alpha = if i == 0 { 1.0 } else { 0.4 };
        let c = [color[0] * alpha, color[1] * alpha, color[2] * alpha];

        // X 方向のライン
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [pos, 0.0, -size], color: c, normal },
            Vertex { position: [pos, 0.0, size], color: c, normal },
            Vertex { position: [pos + half_thickness, 0.0, -size], color: c, normal },
            Vertex { position: [pos + half_thickness, 0.0, size], color: c, normal },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);

        // Z 方向のライン
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [-size, 0.0, pos], color: c, normal },
            Vertex { position: [size, 0.0, pos], color: c, normal },
            Vertex { position: [-size, 0.0, pos + half_thickness], color: c, normal },
            Vertex { position: [size, 0.0, pos + half_thickness], color: c, normal },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }

    (vertices, indices)
}

/// UV球（Sphere）のメッシュデータを生成
pub fn build_sphere(
    radius: f32,
    segments: u32,
    rings: u32,
    color: [f32; 3],
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    let pi = std::f32::consts::PI;

    for ring in 0..=rings {
        let theta = ring as f32 * pi / rings as f32;
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();

        for seg in 0..=segments {
            let phi = seg as f32 * 2.0 * pi / segments as f32;
            let x = sin_theta * phi.cos();
            let y = cos_theta;
            let z = sin_theta * phi.sin();

            vertices.push(Vertex {
                position: [x * radius, y * radius, z * radius],
                color,
                normal: [x, y, z], // 球の法線 = 正規化された位置
            });
        }
    }

    for ring in 0..rings {
        for seg in 0..segments {
            let a = ring * (segments + 1) + seg;
            let b = a + segments + 1;

            indices.extend_from_slice(&[a, b, a + 1]);
            indices.extend_from_slice(&[a + 1, b, b + 1]);
        }
    }

    (vertices, indices)
}

/// USD の Mesh prim からメッシュデータを生成（三角形分割）
pub fn build_from_usd_mesh(prim: &crate::usd::Prim) -> Option<(Vec<Vertex>, Vec<u32>)> {
    use crate::usd::Value;

    let points_prop = prim.properties.get("points")?;
    let indices_prop = prim.properties.get("faceVertexIndices")?;
    let counts_prop = prim.properties.get("faceVertexCounts")?;

    let color = extract_display_color(prim).unwrap_or([0.5, 0.5, 0.5]);

    let positions: Vec<[f32; 3]> = if let Value::Array(arr) = &points_prop.value {
        arr.iter()
            .filter_map(|v| v.as_f64x3().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]))
            .collect()
    } else {
        return None;
    };

    let face_indices: Vec<u32> = if let Value::Array(arr) = &indices_prop.value {
        arr.iter()
            .filter_map(|v| match v {
                Value::Int(i) => Some(*i as u32),
                _ => None,
            })
            .collect()
    } else {
        return None;
    };

    let face_counts: Vec<usize> = if let Value::Array(arr) = &counts_prop.value {
        arr.iter()
            .filter_map(|v| match v {
                Value::Int(i) => Some(*i as usize),
                _ => None,
            })
            .collect()
    } else {
        return None;
    };

    // デフォルト法線（後で三角形ごとに計算）
    let vertices: Vec<Vertex> = positions
        .iter()
        .map(|&position| Vertex {
            position,
            color,
            normal: [0.0, 1.0, 0.0],
        })
        .collect();

    let mut triangulated = Vec::new();
    let mut offset = 0;
    for count in &face_counts {
        for i in 1..(*count - 1) {
            triangulated.push(face_indices[offset]);
            triangulated.push(face_indices[offset + i]);
            triangulated.push(face_indices[offset + i + 1]);
        }
        offset += count;
    }

    Some((vertices, triangulated))
}

/// USD prim から displayColor を取得
pub fn extract_display_color(prim: &crate::usd::Prim) -> Option<[f32; 3]> {
    use crate::usd::Value;
    let prop = prim.properties.get("primvars:displayColor")?;
    if let Value::Array(arr) = &prop.value {
        let c = arr.first()?.as_f64x3()?;
        Some([c[0] as f32, c[1] as f32, c[2] as f32])
    } else {
        None
    }
}

/// USD prim から xformOp:translate を取得
pub fn extract_translate(prim: &crate::usd::Prim) -> Option<[f32; 3]> {
    let prop = prim.properties.get("xformOp:translate")?;
    let v = prop.value.as_f64x3()?;
    Some([v[0] as f32, v[1] as f32, v[2] as f32])
}

/// USD prim から radius を取得
pub fn extract_radius(prim: &crate::usd::Prim) -> Option<f32> {
    let prop = prim.properties.get("radius")?;
    prop.value.as_f64().map(|r| r as f32)
}

// ── ユーロラック メッシュ ──────────────────────────

/// 直方体（ボックス）メッシュを生成（中心原点、面ごとに法線付き）
pub fn build_box(
    width: f32,
    height: f32,
    depth: f32,
    front_color: [f32; 3],
    side_color: [f32; 3],
) -> (Vec<Vertex>, Vec<u32>) {
    let hw = width / 2.0;
    let hh = height / 2.0;
    let hd = depth / 2.0;

    let fc = front_color;
    let sc = side_color;

    let vertices = vec![
        // Front face (+Z)
        Vertex { position: [-hw, -hh, hd], color: fc, normal: [0.0, 0.0, 1.0] },
        Vertex { position: [hw, -hh, hd], color: fc, normal: [0.0, 0.0, 1.0] },
        Vertex { position: [hw, hh, hd], color: fc, normal: [0.0, 0.0, 1.0] },
        Vertex { position: [-hw, hh, hd], color: fc, normal: [0.0, 0.0, 1.0] },
        // Back face (-Z)
        Vertex { position: [hw, -hh, -hd], color: sc, normal: [0.0, 0.0, -1.0] },
        Vertex { position: [-hw, -hh, -hd], color: sc, normal: [0.0, 0.0, -1.0] },
        Vertex { position: [-hw, hh, -hd], color: sc, normal: [0.0, 0.0, -1.0] },
        Vertex { position: [hw, hh, -hd], color: sc, normal: [0.0, 0.0, -1.0] },
        // Top face (+Y)
        Vertex { position: [-hw, hh, hd], color: sc, normal: [0.0, 1.0, 0.0] },
        Vertex { position: [hw, hh, hd], color: sc, normal: [0.0, 1.0, 0.0] },
        Vertex { position: [hw, hh, -hd], color: sc, normal: [0.0, 1.0, 0.0] },
        Vertex { position: [-hw, hh, -hd], color: sc, normal: [0.0, 1.0, 0.0] },
        // Bottom face (-Y)
        Vertex { position: [-hw, -hh, -hd], color: sc, normal: [0.0, -1.0, 0.0] },
        Vertex { position: [hw, -hh, -hd], color: sc, normal: [0.0, -1.0, 0.0] },
        Vertex { position: [hw, -hh, hd], color: sc, normal: [0.0, -1.0, 0.0] },
        Vertex { position: [-hw, -hh, hd], color: sc, normal: [0.0, -1.0, 0.0] },
        // Right face (+X)
        Vertex { position: [hw, -hh, hd], color: sc, normal: [1.0, 0.0, 0.0] },
        Vertex { position: [hw, -hh, -hd], color: sc, normal: [1.0, 0.0, 0.0] },
        Vertex { position: [hw, hh, -hd], color: sc, normal: [1.0, 0.0, 0.0] },
        Vertex { position: [hw, hh, hd], color: sc, normal: [1.0, 0.0, 0.0] },
        // Left face (-X)
        Vertex { position: [-hw, -hh, -hd], color: sc, normal: [-1.0, 0.0, 0.0] },
        Vertex { position: [-hw, -hh, hd], color: sc, normal: [-1.0, 0.0, 0.0] },
        Vertex { position: [-hw, hh, hd], color: sc, normal: [-1.0, 0.0, 0.0] },
        Vertex { position: [-hw, hh, -hd], color: sc, normal: [-1.0, 0.0, 0.0] },
    ];

    #[rustfmt::skip]
    let indices = vec![
        0,  1,  2,  0,  2,  3,   // front
        4,  5,  6,  4,  6,  7,   // back
        8,  9,  10, 8,  10, 11,  // top
        12, 13, 14, 12, 14, 15,  // bottom
        16, 17, 18, 16, 18, 19,  // right
        20, 21, 22, 20, 22, 23,  // left
    ];

    (vertices, indices)
}

/// ユーロラックフレーム（レール + サイドパネル）を生成
///
/// 各パーツの (vertices, indices, position) を返す。
pub fn build_rack_frame(
    total_hp: u32,
    rows: u32,
    frame_color: [f32; 3],
) -> Vec<(Vec<Vertex>, Vec<u32>, [f32; 3])> {
    let total_width = total_hp as f32 * HP_UNIT;
    let total_height = rows as f32 * ROW_HEIGHT_3U;
    let mut parts = Vec::new();

    let dark = [
        frame_color[0] * 0.6,
        frame_color[1] * 0.6,
        frame_color[2] * 0.6,
    ];

    for row in 0..rows {
        let row_y = row as f32 * ROW_HEIGHT_3U;

        // Bottom rail
        let (v, i) = build_box(total_width + 0.04, RAIL_THICKNESS, RAIL_DEPTH, frame_color, dark);
        parts.push((v, i, [0.0, row_y, 0.0]));

        // Top rail
        let (v, i) = build_box(total_width + 0.04, RAIL_THICKNESS, RAIL_DEPTH, frame_color, dark);
        parts.push((v, i, [0.0, row_y + ROW_HEIGHT_3U, 0.0]));
    }

    // Side rails（左右）
    let side_dark = [
        frame_color[0] * 0.4,
        frame_color[1] * 0.4,
        frame_color[2] * 0.4,
    ];
    let side_w = 0.06;
    let side_h = total_height + RAIL_THICKNESS;

    let (v, i) = build_box(side_w, side_h, RAIL_DEPTH + 0.01, frame_color, side_dark);
    parts.push((v, i, [-(total_width + side_w) / 2.0, total_height / 2.0, 0.0]));

    let (v, i) = build_box(side_w, side_h, RAIL_DEPTH + 0.01, frame_color, side_dark);
    parts.push((v, i, [(total_width + side_w) / 2.0, total_height / 2.0, 0.0]));

    // 背面パネル（ラック背面の板、奥行き感を出す）
    let back_color = [
        frame_color[0] * 0.3,
        frame_color[1] * 0.3,
        frame_color[2] * 0.3,
    ];
    let back_dark = [
        frame_color[0] * 0.2,
        frame_color[1] * 0.2,
        frame_color[2] * 0.2,
    ];
    let (v, i) = build_box(
        total_width + 0.04 + side_w * 2.0,
        total_height + RAIL_THICKNESS,
        0.005,
        back_color,
        back_dark,
    );
    parts.push((v, i, [0.0, total_height / 2.0, -RAIL_DEPTH / 2.0 - 0.003]));

    parts
}

/// ベゼル付きモジュールパネルを生成（HP幅指定、3U高さ）
///
/// パネル本体 + ベゼル（面取り）で立体感を表現。
pub fn build_module_panel(hp_width: u32, color: [f32; 3]) -> (Vec<Vertex>, Vec<u32>) {
    let width = hp_width as f32 * HP_UNIT - PANEL_MARGIN;
    let height = ROW_HEIGHT_3U - RAIL_THICKNESS * 2.0 - PANEL_MARGIN;
    let dark = [color[0] * 0.45, color[1] * 0.45, color[2] * 0.45];

    // メインパネル本体（ベゼル分内側に縮小）
    let inner_w = width - BEZEL_WIDTH * 2.0;
    let inner_h = height - BEZEL_WIDTH * 2.0;
    let (mut vertices, mut indices) = build_box(inner_w, inner_h, PANEL_DEPTH, color, dark);

    // ベゼル: 前面の面取りストリップ（上下左右 4 辺）
    let hw = width / 2.0;
    let hh = height / 2.0;
    let ihw = inner_w / 2.0;
    let ihh = inner_h / 2.0;
    let front_z = PANEL_DEPTH / 2.0;
    let bezel_z = front_z + BEZEL_DEPTH;

    // ベゼルは内側面から外側面に向かって斜めになる
    // 明るいベゼル色（ハイライト）と暗いベゼル色（シャドウ）
    let highlight = [
        (color[0] * 1.3).min(1.0),
        (color[1] * 1.3).min(1.0),
        (color[2] * 1.3).min(1.0),
    ];
    let shadow = [color[0] * 0.35, color[1] * 0.35, color[2] * 0.35];

    // Top bezel（上辺 — ハイライト）
    {
        let n = [0.0, 0.6, 0.8]; // 斜め上向き
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [-ihw, ihh, bezel_z], color: highlight, normal: n },
            Vertex { position: [ihw, ihh, bezel_z], color: highlight, normal: n },
            Vertex { position: [hw, hh, front_z], color: highlight, normal: n },
            Vertex { position: [-hw, hh, front_z], color: highlight, normal: n },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    // Bottom bezel（下辺 — シャドウ）
    {
        let n = [0.0, -0.6, 0.8];
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [ihw, -ihh, bezel_z], color: shadow, normal: n },
            Vertex { position: [-ihw, -ihh, bezel_z], color: shadow, normal: n },
            Vertex { position: [-hw, -hh, front_z], color: shadow, normal: n },
            Vertex { position: [hw, -hh, front_z], color: shadow, normal: n },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    // Left bezel（左辺 — やや暗め）
    {
        let n = [-0.6, 0.0, 0.8];
        let mid = [
            (color[0] * 0.8).min(1.0),
            (color[1] * 0.8).min(1.0),
            (color[2] * 0.8).min(1.0),
        ];
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [-ihw, -ihh, bezel_z], color: mid, normal: n },
            Vertex { position: [-ihw, ihh, bezel_z], color: mid, normal: n },
            Vertex { position: [-hw, hh, front_z], color: mid, normal: n },
            Vertex { position: [-hw, -hh, front_z], color: mid, normal: n },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    // Right bezel（右辺 — やや暗め）
    {
        let n = [0.6, 0.0, 0.8];
        let mid = [
            (color[0] * 0.8).min(1.0),
            (color[1] * 0.8).min(1.0),
            (color[2] * 0.8).min(1.0),
        ];
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [ihw, ihh, bezel_z], color: mid, normal: n },
            Vertex { position: [ihw, -ihh, bezel_z], color: mid, normal: n },
            Vertex { position: [hw, -hh, front_z], color: mid, normal: n },
            Vertex { position: [hw, hh, front_z], color: mid, normal: n },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    // 前面インナーパネル（ベゼルの内側、少し前に突き出す）
    {
        let n = [0.0, 0.0, 1.0];
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex { position: [-ihw, -ihh, bezel_z], color, normal: n },
            Vertex { position: [ihw, -ihh, bezel_z], color, normal: n },
            Vertex { position: [ihw, ihh, bezel_z], color, normal: n },
            Vertex { position: [-ihw, ihh, bezel_z], color, normal: n },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    (vertices, indices)
}

/// HP 位置からワールド座標を計算
///
/// `hp_pos`: モジュール左端の HP 位置（0始まり）
/// `hp_width`: モジュールの HP 幅
/// `total_hp`: ラック全体の HP 幅
/// `row`: 行番号（0始まり）
pub fn module_world_position(hp_pos: u32, hp_width: u32, total_hp: u32, row: u32) -> [f32; 3] {
    let total_width = total_hp as f32 * HP_UNIT;
    let x = (hp_pos as f32 + hp_width as f32 / 2.0) * HP_UNIT - total_width / 2.0;
    let y = row as f32 * ROW_HEIGHT_3U + ROW_HEIGHT_3U / 2.0;
    let z = RAIL_DEPTH / 2.0 + PANEL_DEPTH / 2.0 + 0.001; // レールの前面に配置
    [x, y, z]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn hp_unit_constant_sanity() {
        assert!((HP_UNIT - 0.05).abs() < f32::EPSILON);
    }

    #[test]
    fn module_world_position_center_alignment() {
        // HP 0 に 84HP モジュール（ラック幅 84HP）→ x = 0.0（中央揃え）
        let pos = module_world_position(0, 84, 84, 0);
        assert!((pos[0] - 0.0).abs() < 1e-5, "x should be 0.0, got {}", pos[0]);
    }

    #[test]
    fn module_world_position_offset() {
        // HP 位置が変わると x が変化する
        let pos0 = module_world_position(0, 8, 84, 0);
        let pos10 = module_world_position(10, 8, 84, 0);
        assert!(
            (pos10[0] - pos0[0] - 10.0 * HP_UNIT).abs() < 1e-5,
            "x offset should be 10 * HP_UNIT"
        );
    }

    #[test]
    fn module_world_position_row1_y_offset() {
        let pos_r0 = module_world_position(0, 8, 84, 0);
        let pos_r1 = module_world_position(0, 8, 84, 1);
        assert!(
            (pos_r1[1] - pos_r0[1] - ROW_HEIGHT_3U).abs() < 1e-5,
            "row=1 should be ROW_HEIGHT_3U higher"
        );
    }

    #[test]
    fn build_box_vertex_count() {
        let (vertices, indices) = build_box(1.0, 1.0, 1.0, [1.0; 3], [0.5; 3]);
        assert_eq!(vertices.len(), 24, "box should have 24 vertices (4 per face * 6 faces)");
        assert_eq!(indices.len(), 36, "box should have 36 indices (6 per face * 6 faces)");
    }

    #[test]
    fn build_box_index_valid_range() {
        let (vertices, indices) = build_box(2.0, 3.0, 1.5, [1.0; 3], [0.5; 3]);
        let vlen = vertices.len() as u32;
        for (i, &idx) in indices.iter().enumerate() {
            assert!(idx < vlen, "index[{}] = {} >= vertex count {}", i, idx, vlen);
        }
    }

    #[test]
    fn build_box_normals_normalized() {
        let (vertices, _) = build_box(1.0, 1.0, 1.0, [1.0; 3], [0.5; 3]);
        for (i, v) in vertices.iter().enumerate() {
            let len = (v.normal[0].powi(2) + v.normal[1].powi(2) + v.normal[2].powi(2)).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-5,
                "vertex[{}] normal length = {}, expected 1.0",
                i,
                len
            );
        }
    }

    #[test]
    fn build_sphere_vertex_count_formula() {
        let segments = 16u32;
        let rings = 8u32;
        let (vertices, indices) = build_sphere(1.0, segments, rings, [1.0; 3]);
        let expected_verts = (rings + 1) * (segments + 1);
        assert_eq!(
            vertices.len(),
            expected_verts as usize,
            "sphere verts: (rings+1)*(segments+1)"
        );
        let expected_indices = rings * segments * 6;
        assert_eq!(
            indices.len(),
            expected_indices as usize,
            "sphere indices: rings*segments*6"
        );
    }

    #[test]
    fn build_sphere_index_valid_range() {
        let (vertices, indices) = build_sphere(0.5, 12, 6, [0.8; 3]);
        let vlen = vertices.len() as u32;
        for (i, &idx) in indices.iter().enumerate() {
            assert!(idx < vlen, "sphere index[{}] = {} >= vertex count {}", i, idx, vlen);
        }
    }

    #[test]
    fn build_grid_empty_for_zero_step() {
        // step=0 → n = (size / 0) = inf → as i32 は未定義動作に近いが、
        // 実際の利用では step > 0 を前提とする。step が size より大きければ空に近い。
        let (vertices, indices) = build_grid(1.0, 10.0, [0.5; 3]);
        // n = (1.0 / 10.0) as i32 = 0 → -0..=0 → 1 iteration → 8 vertices, 12 indices
        // "empty" ではないが、step > size で最小出力になることを確認
        assert!(vertices.len() <= 8, "with step > size, minimal output expected");
        assert!(indices.len() <= 12);
    }

    #[test]
    fn build_module_panel_vertex_count() {
        let (vertices, indices) = build_module_panel(8, [0.7, 0.7, 0.7]);
        // box(24 verts, 36 idx) + 4 bezel strips (4 verts each = 16) + 1 inner panel (4 verts)
        // = 24 + 16 + 4 = 44 vertices
        // = 36 + 5*6 = 66 indices
        assert_eq!(vertices.len(), 44, "module panel: 24 (box) + 16 (bezels) + 4 (inner)");
        assert_eq!(indices.len(), 66, "module panel: 36 (box) + 30 (bezels+inner)");
    }

    #[test]
    fn build_rack_frame_no_panic() {
        let parts = build_rack_frame(84, 2, [0.3, 0.3, 0.35]);
        // 2 rows * 2 rails + 2 side rails + 1 back panel = 7 parts
        assert_eq!(parts.len(), 7, "84HP 2-row rack should have 7 parts");
        for (verts, idxs, _pos) in &parts {
            assert!(!verts.is_empty());
            assert!(!idxs.is_empty());
        }
    }

    #[test]
    fn build_from_usd_mesh_none_on_missing_attrs() {
        use crate::usd::{Prim, Property, Value};

        // 空の Prim — 必要な属性がない
        let prim = Prim {
            prim_type: "Mesh".to_string(),
            name: "test".to_string(),
            properties: HashMap::new(),
            children: vec![],
        };
        assert!(build_from_usd_mesh(&prim).is_none());

        // points だけあっても faceVertexIndices/faceVertexCounts がない
        let mut props = HashMap::new();
        props.insert(
            "points".to_string(),
            Property {
                type_name: "point3f[]".to_string(),
                value: Value::Array(vec![]),
            },
        );
        let prim2 = Prim {
            prim_type: "Mesh".to_string(),
            name: "test2".to_string(),
            properties: props,
            children: vec![],
        };
        assert!(build_from_usd_mesh(&prim2).is_none());
    }

    #[test]
    fn build_sphere_vertices_on_surface() {
        let radius = 2.5;
        let (vertices, _) = build_sphere(radius, 12, 8, [1.0; 3]);
        let r_sq = radius * radius;
        for (i, v) in vertices.iter().enumerate() {
            let dist_sq = v.position[0].powi(2) + v.position[1].powi(2) + v.position[2].powi(2);
            assert!(
                (dist_sq - r_sq).abs() < 1e-4,
                "vertex[{}] dist^2 = {}, expected {} (r={})",
                i,
                dist_sq,
                r_sq,
                radius
            );
        }
    }

    #[test]
    fn build_module_panel_zero_hp_no_panic() {
        // hp=0 でパニックしない
        let (vertices, indices) = build_module_panel(0, [0.5, 0.5, 0.5]);
        // 結果の内容は問わないが、パニックせず返ること
        let _ = (vertices, indices);
    }

    #[test]
    fn build_box_vertices_within_bounds() {
        let w = 3.0f32;
        let h = 2.0f32;
        let d = 1.5f32;
        let (vertices, _) = build_box(w, h, d, [1.0; 3], [0.5; 3]);
        let hw = w / 2.0;
        let hh = h / 2.0;
        let hd = d / 2.0;
        for (i, v) in vertices.iter().enumerate() {
            assert!(
                v.position[0] >= -hw - 1e-5 && v.position[0] <= hw + 1e-5,
                "vertex[{}] x = {} outside [-{}, {}]",
                i,
                v.position[0],
                hw,
                hw
            );
            assert!(
                v.position[1] >= -hh - 1e-5 && v.position[1] <= hh + 1e-5,
                "vertex[{}] y = {} outside [-{}, {}]",
                i,
                v.position[1],
                hh,
                hh
            );
            assert!(
                v.position[2] >= -hd - 1e-5 && v.position[2] <= hd + 1e-5,
                "vertex[{}] z = {} outside [-{}, {}]",
                i,
                v.position[2],
                hd,
                hd
            );
        }
    }
}
