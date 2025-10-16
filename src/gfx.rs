use crate::app;
use crate::ecs::World;
use winit::window::Window;

pub fn init(world: &mut World) {
    world.queue_system(init_renderer);
    world.insert_system_with(app::resized, handle_resize);
    world.insert_system_with(app::render, render);
}

struct Renderer<'a> {
    surface: wgpu::Surface<'a>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn init_renderer(world: &mut World) {
    let window = world.get_resource::<Window>().unwrap();

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    // todo fix lifetimes
    let surface = unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(window).unwrap())
            .unwrap()
    };
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();

    world.insert_resource(Renderer {
        surface,
        device,
        queue,
    });
}

pub struct RenderFrame {
    pub surface: wgpu::SurfaceTexture,
    pub surface_view: wgpu::TextureView,
    pub encoder: wgpu::CommandEncoder,
}

fn render(world: &mut World) {
    let renderer = world.get_resource::<Renderer>().unwrap();
    let surface = renderer.surface.get_current_texture().unwrap();
    let surface_view = surface
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let encoder = renderer
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    world.insert_resource(RenderFrame {
        surface,
        surface_view,
        encoder,
    });

    world.queue_system_once(submit_frame);
}

pub fn submit_frame(world: &mut World) {
    let frame = world.remove_resource::<RenderFrame>().unwrap();
    let renderer = world.get_resource::<Renderer>().unwrap();
    renderer.queue.submit([frame.encoder.finish()]);
    frame.surface.present();
}

fn handle_resize(world: &mut World) {
    let app::ResizeEvent(size) = world.get_resource().unwrap();
    let renderer = world.get_resource::<Renderer>().unwrap();
    renderer.surface.configure(
        &renderer.device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        },
    );
}
