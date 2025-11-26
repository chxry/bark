use std::num::NonZero;

use crate::app;
use crate::ecs::World;
use winit::window::Window;

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub fn init(world: &mut World) {
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
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::TEXTURE_BINDING_ARRAY | wgpu::Features::PUSH_CONSTANTS,
        required_limits: wgpu::Limits {
            max_binding_array_elements_per_shader_stage: 2048,
            max_push_constant_size: 256,
            max_buffer_size: 4 * 1024 * 1024 * 1024,
            ..Default::default()
        },
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .unwrap();

    world.insert_resource(Renderer {
        surface,
        device,
        queue,
    });

    world.insert_system_with(app::render, render);
    world.add_event_handler(resize_configure_surface);
}

pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
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

    world.queue_system(submit_frame);
}

pub fn submit_frame(world: &mut World) {
    let frame = world.remove_resource::<RenderFrame>().unwrap();
    let renderer = world.get_resource::<Renderer>().unwrap();
    renderer.queue.submit([frame.encoder.finish()]);
    frame.surface.present();
}

fn resize_configure_surface(world: &mut World, event: &app::ResizeEvent) {
    let renderer = world.get_resource::<Renderer>().unwrap();
    renderer.surface.configure(
        &renderer.device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: FORMAT,
            width: event.0.width,
            height: event.0.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        },
    );
}

pub fn resized_buffer(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    new_size: wgpu::BufferAddress,
) -> wgpu::Buffer {
    let new_size = wgpu::util::align_to(
        (buffer.size() * 3 / 2).max(new_size),
        wgpu::COPY_BUFFER_ALIGNMENT,
    );
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: new_size,
        mapped_at_creation: false,
        usage: buffer.usage(),
    })
}

pub fn grow_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    buffer: &mut wgpu::Buffer,
    current_usage: wgpu::BufferAddress,
    needed: wgpu::BufferAddress,
) -> wgpu::QueueWriteBufferView {
    if needed > buffer.size() - current_usage {
        let new_buffer = resized_buffer(device, buffer, current_usage + needed);
        encoder.copy_buffer_to_buffer(buffer, 0, &new_buffer, 0, current_usage);
        *buffer = new_buffer;
    }
    queue
        .write_buffer_with(buffer, current_usage, NonZero::new(needed).unwrap())
        .unwrap()
}
