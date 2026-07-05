pub mod mesh;
pub mod texture;

use self::mesh::MeshManager;
use self::texture::TextureManager;
use crate::app::{self, App, ResizeEvent, WindowHandle};
use crate::ecs::{Commands, IntoSystem, MainThread, Observer, Res, ResMut, System};
use std::num::NonZero;
use std::sync::Mutex;
use tracing::error;

pub const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub const SAMPLES: u32 = 4;
pub const DEFAULT_BUFFER_SIZE: wgpu::BufferAddress = 1024 * 1024;

pub fn init(app: &mut App) {
    app.world
        .insert_system::<app::Startup>(init_renderer.into_system());
    app.world
        .insert_system::<app::Render>(begin_frame.into_system());
    app.world
        .insert_system::<app::Render>(submit_frame.after(begin_frame));
    app.world
        .insert_system::<app::Render>(texture::upload_textures.before(submit_frame));
    app.world
        .insert_system::<app::Render>(mesh::upload_meshes.with(during_frame));
    app.world
        .insert_system::<app::ResizeEvent>(resize_surface.into_system());
}

pub struct RenderContext {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

pub fn init_renderer(window: Res<WindowHandle>, mut commands: Commands, _: MainThread) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = instance.create_surface(window.clone()).unwrap();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::TEXTURE_COMPRESSION_BC
            | wgpu::Features::IMMEDIATES,
        required_limits: wgpu::Limits {
            max_binding_array_elements_per_shader_stage: 2048,
            max_immediate_size: 256,
            ..Default::default()
        },
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .unwrap();

    let size = window.inner_size();
    configure_surface(&device, &surface, size.width, size.height);

    let textures = TextureManager::new(&device, &queue);
    let meshes = MeshManager::new(&device);

    commands.insert_resource(RenderContext {
        surface,
        device,
        queue,
    });
    commands.insert_resource(textures);
    commands.insert_resource(meshes);
    commands.insert_resource(RenderFrame::new());
}

pub struct RenderFrame {
    pub surface: Option<(wgpu::SurfaceTexture, wgpu::TextureView)>,
    pub command_buffers: Mutex<Vec<wgpu::CommandBuffer>>,
}

impl RenderFrame {
    fn new() -> Self {
        Self {
            surface: None,
            command_buffers: Mutex::new(vec![]),
        }
    }

    pub fn submit(&self, command_buffer: wgpu::CommandBuffer) {
        self.command_buffers.lock().unwrap().push(command_buffer);
    }
}

pub fn begin_frame(ctx: Res<RenderContext>, mut frame: ResMut<RenderFrame>) {
    let surface = match ctx.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(surface) => surface,
        wgpu::CurrentSurfaceTexture::Occluded => return,
        e => {
            error!("couldn't get surface texture: {:?}", e);
            return;
        }
    };
    let surface_view = surface
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    frame.surface = Some((surface, surface_view));
}

pub fn submit_frame(ctx: Res<RenderContext>, mut frame: ResMut<RenderFrame>) {
    let Some((surface, _)) = frame.surface.take() else {
        return;
    };

    ctx.queue
        .submit(frame.command_buffers.get_mut().unwrap().drain(..));
    ctx.queue.present(surface);
}

fn configure_surface(device: &wgpu::Device, surface: &wgpu::Surface, width: u32, height: u32) {
    surface.configure(
        device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: SURFACE_FORMAT,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        },
    );
}

pub fn resize_surface(resize: Observer<ResizeEvent>, ctx: Res<RenderContext>) {
    configure_surface(&ctx.device, &ctx.surface, resize.width, resize.height);
}

pub fn resize_buffer(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    buffer: &mut wgpu::Buffer,
    size: wgpu::BufferAddress,
    usage: Option<wgpu::BufferAddress>,
) {
    let new_size = wgpu::util::align_to(
        (buffer.size() * 3 / 2).max(size),
        wgpu::COPY_BUFFER_ALIGNMENT,
    );
    let new_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        size: new_size,
        usage: buffer.usage(),
        mapped_at_creation: false,
        label: None,
    });
    encoder.copy_buffer_to_buffer(buffer, 0, &new_buffer, 0, usage.unwrap_or(buffer.size()));
    *buffer = new_buffer;
}

pub fn extend_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    buffer: &mut wgpu::Buffer,
    offset: wgpu::BufferAddress,
    size: wgpu::BufferAddress,
) -> wgpu::QueueWriteBufferView {
    let needed_size = size + offset;
    if buffer.size() < needed_size {
        resize_buffer(device, encoder, buffer, needed_size, Some(offset));
    }
    queue
        .write_buffer_with(buffer, offset, NonZero::new(size).unwrap())
        .unwrap()
}

pub fn during_frame(sys: Box<dyn System>) -> Box<dyn System> {
    sys.after(begin_frame).before(submit_frame)
}
