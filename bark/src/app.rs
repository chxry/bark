use crate::ecs::World;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

pub struct Startup;
pub struct Update;
pub struct Render;

pub type WindowHandle = Arc<Window>;

pub struct App {
    pub world: World,
}

impl App {
    pub fn new() -> Self {
        Self {
            world: World::new(),
        }
    }

    pub fn run(mut self) {
        let event_loop = EventLoop::new().unwrap();
        event_loop.run_app(&mut self).unwrap();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(WindowAttributes::default())
                .unwrap(),
        );
        self.world.insert_resource(window);
        self.world.run_schedule(Startup);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.world.queue_event(ResizeEvent {
                width: size.width,
                height: size.height,
            }),
            WindowEvent::RedrawRequested => {}
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        self.world.clear_events();
        self.world.run_schedule(Update);
        self.world.run_schedule(Render);
        if let Some(window) = self.world.get_resource::<WindowHandle>() {
            window.request_redraw();
        }
    }
}

pub struct ResizeEvent {
    pub width: u32,
    pub height: u32,
}
