use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::renderer::pipeline::GpuContext;

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
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
            .with_title("cplp")
            .with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("failed to create window"));
        let gpu = pollster::block_on(GpuContext::new(window.clone()))
            .expect("failed to initialize GPU context");
        self.window = Some(window);
        self.gpu = Some(gpu);
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
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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

pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
