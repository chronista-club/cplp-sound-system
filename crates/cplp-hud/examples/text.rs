use std::sync::Arc;

use cplp_hud::renderer::pipeline::GpuContext;
use cplp_hud::renderer::text::{TextEngine, TextEntry};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    text_engine: Option<TextEngine>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
            text_engine: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        event_loop.set_control_flow(ControlFlow::Poll);

        let attrs = Window::default_attributes()
            .with_title("cplp - Text Example")
            .with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        let gpu = pollster::block_on(GpuContext::new(window.clone()))
            .expect("failed to initialize GPU context");

        let text_engine = TextEngine::new(&gpu.device, &gpu.queue, gpu.config.format);

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.text_engine = Some(text_engine);
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
                let Some(text_engine) = &mut self.text_engine else {
                    return;
                };

                let entries = [
                    TextEntry {
                        text: "cplp - Live HUD".into(),
                        x: 20.0,
                        y: 20.0,
                        size: 32.0,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                    TextEntry {
                        text: "glyphon + wgpu text rendering".into(),
                        x: 20.0,
                        y: 70.0,
                        size: 20.0,
                        color: [0.6, 0.8, 1.0, 1.0],
                    },
                    TextEntry {
                        text: "Monospace font / Dark background".into(),
                        x: 20.0,
                        y: 110.0,
                        size: 16.0,
                        color: [0.5, 0.5, 0.5, 1.0],
                    },
                ];

                text_engine.prepare(
                    &gpu.device,
                    &gpu.queue,
                    &entries,
                    gpu.size.width,
                    gpu.size.height,
                );

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
                    text_engine.render(&mut pass);
                }
                gpu.queue.submit(std::iter::once(encoder.finish()));
                output.present();

                text_engine.trim();
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
    let mut app = App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
