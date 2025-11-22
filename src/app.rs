use crate::ecs::World;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

pub fn init(world: &mut World) {
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(world).unwrap();
}

pub fn update(_: &mut World) {}
pub fn render(_: &mut World) {}

pub struct ResizeEvent(pub PhysicalSize<u32>);

impl ApplicationHandler for World {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();
        self.insert_resource(window);
        self.insert_resource(Time::default());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.queue_and_run(render);
            }
            WindowEvent::Resized(size) => {
                self.handle_event(ResizeEvent(size));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let time = self.get_resource_mut::<Time>().unwrap();
        let now = Instant::now();
        time.accumulator += now - time.last_instant;
        time.last_instant = now;
        event_loop.set_control_flow(ControlFlow::WaitUntil(now + time.timestep));

        let updates = time.accumulator.div_duration_f32(time.timestep);
        time.accumulator = time.timestep.mul_f32(updates.fract());

        for _ in 0..(updates.trunc() as _) {
            self.queue_and_run(update);
        }

        if let Some(window) = self.get_resource::<Window>() {
            window.request_redraw();
        }
    }
}

struct Time {
    last_instant: Instant,
    accumulator: Duration,
    timestep: Duration,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            last_instant: Instant::now(),
            accumulator: Duration::ZERO,
            timestep: Duration::from_secs_f32(1.0 / 60.0),
        }
    }
}
