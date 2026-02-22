use std::sync::Arc;

use cplp_hud::renderer::line::LinePipeline;
use cplp_hud::renderer::pipeline::GpuContext;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct LineApp {
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    lines: Option<LinePipeline>,
}

impl LineApp {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
            lines: None,
        }
    }
}

impl ApplicationHandler for LineApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        event_loop.set_control_flow(ControlFlow::Poll);

        let attrs = Window::default_attributes()
            .with_title("cplp — Line Demo")
            .with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        let gpu = pollster::block_on(GpuContext::new(window.clone()))
            .expect("failed to initialize GPU context");

        let lines = LinePipeline::new(&gpu.device, gpu.config.format);

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.lines = Some(lines);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size);
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(gpu) = &self.gpu else { return };
                let Some(lines) = &mut self.lines else {
                    return;
                };

                let w = gpu.size.width as f32;
                let h = gpu.size.height as f32;
                lines.set_viewport(w, h);

                // サイン波データを生成
                let num_points = 300;
                let points: Vec<[f32; 2]> = (0..num_points)
                    .map(|i| {
                        let t = i as f32 / (num_points - 1) as f32;
                        let x = t * w;
                        let y = h * 0.5 + (t * std::f32::consts::TAU * 3.0).sin() * h * 0.3;
                        [x, y]
                    })
                    .collect();

                lines.polyline(&points, [0.0, 1.0, 0.4, 1.0]); // 緑

                lines.prepare(&gpu.device, &gpu.queue);

                let output = match gpu.surface.get_current_texture() {
                    Ok(t) => t,
                    Err(wgpu::SurfaceError::Lost) => {
                        gpu.surface.configure(&gpu.device, &gpu.config);
                        return;
                    }
                    Err(_) => return,
                };
                let view = output.texture.create_view(&Default::default());
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
                    lines.render(&mut pass);
                }
                gpu.queue.submit(std::iter::once(encoder.finish()));
                output.present();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = LineApp::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
