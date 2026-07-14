use crate::ecs::World;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

pub use winit;

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
    occluded: bool,
    last_frame: Instant,
    accumulator: Duration,
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
            last_frame: Instant::now(),
            accumulator: Duration::ZERO,
            occluded: true,
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
        self.world.insert_resource(Input::new());
        self.world.insert_resource(UpdateTarget(60.0));
        self.world.run_schedule(Startup);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.world.run_schedule(ResizeEvent {
                width: size.width,
                height: size.height,
            }),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let input = self.world.get_resource_mut::<Input>().unwrap();
                    if event.state.is_pressed() {
                        if input.keys_down.insert(code) {
                            input.keys_pressed.insert(code);
                        }
                    } else {
                        input.keys_down.remove(&code);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let input = self.world.get_resource_mut::<Input>().unwrap();
                match button {
                    MouseButton::Left => input.left_mouse = state.is_pressed(),
                    MouseButton::Right => input.right_mouse = state.is_pressed(),
                    _ => {}
                }
            }
            WindowEvent::Occluded(occluded) => self.occluded = occluded,
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let delta = now - self.last_frame;
                self.last_frame = now;
                self.accumulator += delta;

                let timestep = Duration::from_secs_f32(
                    1.0 / self.world.get_resource::<UpdateTarget>().unwrap().0,
                );
                while self.accumulator >= timestep {
                    self.world.run_schedule(Update);
                    self.accumulator -= timestep;
                    let input = self.world.get_resource_mut::<Input>().unwrap();
                    input.end_frame();
                }

                if !self.occluded {
                    self.world.run_schedule(Render);

                    #[cfg(feature = "tracy")]
                    tracy_client::frame_mark();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        match event {
            DeviceEvent::MouseMotion { delta } => {
                let input = self.world.get_resource_mut::<Input>().unwrap();
                input.mouse_delta = (delta.0 as _, delta.1 as _)
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(window) = self.world.get_resource::<WindowHandle>() {
            window.request_redraw();
        }
    }
}

pub struct ResizeEvent {
    pub width: u32,
    pub height: u32,
}

pub struct UpdateTarget(pub f32);

pub struct Input {
    keys_down: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    mouse_delta: (f32, f32),
    left_mouse: bool,
    right_mouse: bool,
}

impl Input {
    fn new() -> Self {
        Self {
            keys_down: HashSet::new(),
            keys_pressed: HashSet::new(),
            mouse_delta: (0.0, 0.0),
            left_mouse: false,
            right_mouse: false,
        }
    }

    fn end_frame(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.keys_pressed.clear();
    }

    pub fn key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }

    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.keys_pressed.contains(&key)
    }

    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    pub fn left_mouse_down(&self) -> bool {
        self.left_mouse
    }

    pub fn right_mouse_down(&self) -> bool {
        self.right_mouse
    }
}
