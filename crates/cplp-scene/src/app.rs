use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::gpu::GpuContext;
use crate::renderer::{self, SceneRenderer};

/// Gig Scene の状態（winit ラッパー）
///
/// 内部で SceneRenderer を使用。CLI から winit で起動する場合に使う。
struct SceneApp {
    gpu: Option<GpuContext>,
    renderer: Option<SceneRenderer>,
    depth_view: Option<wgpu::TextureView>,
}

impl SceneApp {
    fn new() -> Self {
        Self {
            gpu: None,
            renderer: None,
            depth_view: None,
        }
    }
}

impl ApplicationHandler for SceneApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("Gig Scene")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));

        let window = Arc::new(event_loop.create_window(attrs).expect("window creation"));
        let gpu = pollster::block_on(GpuContext::new(window)).expect("GPU init");

        let scene_renderer =
            SceneRenderer::new(&gpu.device, gpu.config.format, gpu.size.width, gpu.size.height);
        let depth = renderer::create_depth_texture(&gpu.device, gpu.size.width, gpu.size.height);

        self.gpu = Some(gpu);
        self.depth_view = Some(depth);
        self.renderer = Some(scene_renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gpu) = &mut self.gpu else { return };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                gpu.resize(new_size);
                if new_size.width > 0 && new_size.height > 0 {
                    if let Some(r) = &mut self.renderer {
                        self.depth_view = Some(renderer::create_depth_texture(
                            &gpu.device,
                            new_size.width,
                            new_size.height,
                        ));
                        r.resize(new_size.width, new_size.height);
                    }
                }
                gpu.window().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let Some(r) = &self.renderer else { return };
                let Some(depth_view) = &self.depth_view else {
                    return;
                };

                let frame = match gpu.surface.get_current_texture() {
                    Ok(f) => f,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        gpu.resize(gpu.size);
                        if self.renderer.is_some() {
                            self.depth_view = Some(renderer::create_depth_texture(
                                &gpu.device,
                                gpu.size.width,
                                gpu.size.height,
                            ));
                        }
                        return;
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        event_loop.exit();
                        return;
                    }
                    Err(_) => return,
                };

                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                r.render(&gpu.device, &gpu.queue, &view, depth_view);
                frame.present();

                gpu.window().request_redraw();
            }
            _ => {}
        }
    }
}

/// Gig Scene ウィンドウを起動
pub fn run() -> anyhow::Result<()> {
    tracing::info!("Gig Scene を起動（ユーロラックビュー）");
    let event_loop = EventLoop::new()?;
    let mut app = SceneApp::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
