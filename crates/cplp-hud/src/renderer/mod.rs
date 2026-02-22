pub mod glow;
pub mod line;
pub mod pipeline;
pub mod primitives;
pub mod text;

use std::sync::Arc;

use winit::window::Window;

use self::glow::GlowPipeline;
use self::line::LinePipeline;
use self::pipeline::GpuContext;
use self::primitives::{Color, QuadPipeline, Rect, Vec2};
use self::text::{TextEngine, TextEntry};

pub struct Renderer {
    gpu: GpuContext,
    quads: QuadPipeline,
    lines: LinePipeline,
    text: TextEngine,
    text_entries: Vec<TextEntry>,
    glow: GlowPipeline,
    glow_enabled: bool,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let gpu = pollster::block_on(GpuContext::new(window))?;
        let quads = QuadPipeline::new(&gpu.device, gpu.config.format);
        let lines = LinePipeline::new(&gpu.device, gpu.config.format);
        let text = TextEngine::new(&gpu.device, &gpu.queue, gpu.config.format);
        let glow = GlowPipeline::new(
            &gpu.device,
            &gpu.queue,
            gpu.config.format,
            gpu.size.width,
            gpu.size.height,
        );
        Ok(Self {
            gpu,
            quads,
            lines,
            text,
            text_entries: Vec::new(),
            glow,
            glow_enabled: true,
        })
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.glow
            .resize(&self.gpu.device, &self.gpu.queue, size.width, size.height);
    }

    pub fn set_glow_enabled(&mut self, enabled: bool) {
        self.glow_enabled = enabled;
    }

    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.quads.rect(rect, color);
    }

    pub fn polyline(&mut self, points: &[Vec2], color: Color) {
        let pts: Vec<[f32; 2]> = points.iter().map(|p| [p.x, p.y]).collect();
        let c = [color.r, color.g, color.b, color.a];
        self.lines.polyline(&pts, c);
    }

    pub fn text(&mut self, entry: TextEntry) {
        self.text_entries.push(entry);
    }

    pub fn render_frame(&mut self) {
        let (w, h) = (self.gpu.size.width, self.gpu.size.height);

        // Prepare quads
        self.quads.set_viewport(w as f32, h as f32);
        self.quads.prepare(&self.gpu.device, &self.gpu.queue);

        // Prepare lines viewport
        self.lines.set_viewport(w as f32, h as f32);

        // Prepare text
        let entries: Vec<_> = self.text_entries.drain(..).collect();
        self.text
            .prepare(&self.gpu.device, &self.gpu.queue, &entries, w, h);

        // Acquire surface texture
        let output = match self.gpu.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost) => {
                self.gpu
                    .surface
                    .configure(&self.gpu.device, &self.gpu.config);
                return;
            }
            Err(_) => return,
        };
        let view = output.texture.create_view(&Default::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&Default::default());

        // グロー有効時: 中間テクスチャに描画 → ブラー → コンポジット
        // グロー無効時: 直接 surface に描画
        let target_view = if self.glow_enabled {
            self.glow.scene_view()
        } else {
            &view
        };

        // Render pass: quads -> lines -> text
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
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

            self.quads.render(&mut pass);
            self.lines
                .flush(&self.gpu.device, &self.gpu.queue, &mut pass);
            self.text.render(&mut pass);
        }

        // グロー有効時: ブラー + コンポジットを適用
        if self.glow_enabled {
            self.glow.apply(&mut encoder, &view);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        self.text.trim();
    }
}
