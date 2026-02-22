use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;
use crate::renderer::Renderer;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            renderer: None,
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
        let renderer =
            Renderer::new(window.clone()).expect("failed to initialize renderer");
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(renderer) = &mut self.renderer else {
                    return;
                };

                // Red rectangle
                renderer.rect(
                    Rect {
                        x: 50.0,
                        y: 50.0,
                        w: 200.0,
                        h: 100.0,
                    },
                    Color {
                        r: 0.9,
                        g: 0.2,
                        b: 0.2,
                        a: 1.0,
                    },
                );

                // Green sine wave polyline
                let points: Vec<Vec2> = (0..200)
                    .map(|i| {
                        let x = 50.0 + i as f32 * 2.7;
                        let y = 250.0 + (i as f32 * 0.05).sin() * 40.0;
                        Vec2 { x, y }
                    })
                    .collect();
                renderer.polyline(
                    &points,
                    Color {
                        r: 0.2,
                        g: 0.9,
                        b: 0.4,
                        a: 1.0,
                    },
                );

                // White text
                renderer.text(TextEntry {
                    text: "cplp — Renderer OK".into(),
                    x: 50.0,
                    y: 350.0,
                    size: 24.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                });

                renderer.render_frame();
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
