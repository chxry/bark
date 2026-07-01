pub mod mesh;
pub mod texture;

use self::mesh::MeshProcessor;
use self::texture::TextureProcessor;
use crate::app::{self, App, ResizeEvent, WindowHandle};
use crate::assets::AssetProcessors;
use crate::ecs::{Commands, IntoSystem, MainThread, Observer, Res, ResMut};
use crate::gfx::texture::TextureManager;
use tracing::error;

const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

pub fn init(app: &mut App) {
    app.world
        .insert_system::<app::Startup>(init_renderer.into_system());
    app.world
        .insert_system::<app::Render>(begin_frame.into_system());
    app.world
        .insert_system::<app::Render>(submit_frame.after(begin_frame));
    app.world
        .insert_system::<app::ResizeEvent>(on_resize.into_system());
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
    }))
    .unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::TEXTURE_COMPRESSION_BC,
        required_limits: wgpu::Limits {
            max_binding_array_elements_per_shader_stage: 2048,
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

    commands.insert_resource(RenderContext {
        surface,
        device,
        queue,
    });
    commands.insert_resource(textures);
    commands.insert_resource(None as RenderFrame);
}

pub type RenderFrame = Option<RenderFrameInner>;

pub struct RenderFrameInner {
    pub surface: wgpu::SurfaceTexture,
    pub surface_view: wgpu::TextureView,
    pub encoder: wgpu::CommandEncoder,
}

pub fn begin_frame(ctx: Res<RenderContext>, mut frame: ResMut<RenderFrame>) {
    // if let Some(event) = resize_events.iter().last() {
    //     configure_surface(&ctx.device, &ctx.surface, event.width, event.height);
    // }

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
    let encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    *frame = Some(RenderFrameInner {
        surface,
        surface_view,
        encoder,
    });
}

pub fn submit_frame(ctx: Res<RenderContext>, mut frame: ResMut<RenderFrame>) {
    let Some(frame) = (*frame).take() else {
        return;
    };

    ctx.queue.submit([frame.encoder.finish()]);
    frame.surface.present();
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
            view_formats: vec![],
        },
    );
}

pub fn on_resize(ctx: Res<RenderContext>, resize: Observer<ResizeEvent>) {
    configure_surface(&ctx.device, &ctx.surface, resize.width, resize.height);
}

pub fn init_build(assets: &mut AssetProcessors) {
    assets.register("texture", TextureProcessor);
    assets.register("mesh", MeshProcessor);
}
