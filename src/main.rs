mod core;
mod platform;
mod renderer;

use std::time::{Duration, Instant};

use anyhow::Context;
use core::{PetState, TickInput};
use platform::window::{PetWindow, WindowCommand};
use renderer::vulkan::VulkanRenderer;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalPosition;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let event_loop = EventLoop::new().context("failed to create event loop")?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()));

    let mut app = App::default();
    event_loop.run_app(&mut app).context("event loop failed")
}

struct App {
    window: Option<PetWindow>,
    renderer: Option<VulkanRenderer>,
    pet: PetState,
    last_tick: Instant,
    cursor_position: (f32, f32),
    dragging: bool,
    drag_offset: (f32, f32),
    next_redraw: Instant,
}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            renderer: None,
            pet: PetState::default(),
            last_tick: Instant::now(),
            cursor_position: (0.0, 0.0),
            dragging: false,
            drag_offset: (0.0, 0.0),
            next_redraw: Instant::now(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window = match PetWindow::new(event_loop) {
            Ok(window) => window,
            Err(error) => {
                log::error!("failed to create pet window: {error:#}");
                event_loop.exit();
                return;
            }
        };

        let renderer = match VulkanRenderer::new(window.raw()) {
            Ok(renderer) => Some(renderer),
            Err(error) => {
                log::warn!("Vulkan renderer is not ready yet: {error:#}");
                None
            }
        };

        window.raw().request_redraw();
        self.window = Some(window);
        self.renderer = renderer;
        self.last_tick = Instant::now();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        if window.raw().id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                let logical = position.to_logical::<f64>(window.raw().scale_factor());
                self.cursor_position = (logical.x as f32, logical.y as f32);
                self.pet.note_user_activity();
                if self.dragging {
                    window.drag_to_cursor(self.cursor_position, self.drag_offset);
                    self.pet.set_dragging(true, self.cursor_position);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.pet.note_user_activity();
                match state {
                    ElementState::Pressed => {
                        if self.pet.hit_test(self.cursor_position, window.outer_size()) {
                            self.dragging = true;
                            self.drag_offset = self.cursor_position;
                            self.pet.set_dragging(true, self.cursor_position);
                        }
                    }
                    ElementState::Released => {
                        self.dragging = false;
                        self.pet.set_dragging(false, self.cursor_position);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_tick).as_secs_f32().min(0.05);
                self.last_tick = now;

                let monitor = window.monitor_work_area();
                let window_pos = window.outer_position();
                let window_size = window.outer_size();

                self.pet.tick(TickInput {
                    dt,
                    window_pos,
                    window_size,
                    monitor,
                    dragging: self.dragging,
                });

                for command in self.pet.take_window_commands() {
                    match command {
                        WindowCommand::MoveTo { x, y } => {
                            window.set_outer_position(LogicalPosition::new(x, y))
                        }
                    }
                }

                let snapshot = self.pet.render_snapshot();
                window.apply_hit_region(&snapshot.points);
                if let Some(renderer) = self.renderer.as_mut()
                    && let Err(error) = renderer.render(&snapshot)
                {
                    log::warn!("render failed: {error:#}");
                }

                self.next_redraw = now + self.pet.next_frame_delay();
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_redraw));
            }
            WindowEvent::Resized(_) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.request_resize();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            let now = Instant::now();
            if now >= self.next_redraw {
                window.raw().request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_redraw));
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
        }
    }
}
