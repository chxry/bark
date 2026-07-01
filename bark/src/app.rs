use crate::ecs::World;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

#[cfg(feature = "tracy")]
#[global_allocator]
static GLOBAL: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 100);

pub struct Startup;
pub struct Update;
pub struct Render;

pub type WindowHandle = Arc<Window>;

pub struct App {
    pub world: World,
}

impl App {
    pub fn new() -> Self {
        let registry = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer().with_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("info,bark=debug")),
            ),
        );

        #[cfg(feature = "tracy")]
        let registry = registry.with(tracing_tracy::TracyLayer::default());

        registry.init();

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
        self.world.clear_events(); // todo: this is not sustainable. fixed timestep loop ruins this
        self.world.run_schedule(Update);
        self.world.run_schedule(Render);

        #[cfg(feature = "tracy")]
        tracy_client::frame_mark();

        if let Some(window) = self.world.get_resource::<WindowHandle>() {
            window.request_redraw();
        }
    }
}

pub struct ResizeEvent {
    pub width: u32,
    pub height: u32,
}
