pub mod mesh;
pub mod texture;

use self::mesh::MeshProcessor;
use self::texture::TextureProcessor;
use crate::app::{self, App, ResizeEvent, WindowHandle};
use crate::assets::AssetProcessors;
use crate::ecs::{Commands, Events, IntoSystem, MainThread, Res, ResMut};
use tracing::error;

const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

pub fn init(app: &mut App) {
    app.world.insert_system(app::Startup, init_renderer);
    app.world.insert_system(app::Render, begin_frame);
    app.world
        .insert_system(app::Render, submit_frame.after(begin_frame));
    app.world.insert_system(
        app::Render,
        main_pass.after(begin_frame).before(submit_frame),
    )
}

pub struct RenderContext {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
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
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::defaults(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .unwrap();

    commands.insert_resource(RenderContext {
        surface,
        device,
        queue,
    });
    commands.insert_resource(None as RenderFrame);
}

pub type RenderFrame = Option<RenderFrameInner>;

pub struct RenderFrameInner {
    surface: wgpu::SurfaceTexture,
    surface_view: wgpu::TextureView,
    encoder: wgpu::CommandEncoder,
}

pub fn begin_frame(
    ctx: Res<RenderContext>,
    mut frame: ResMut<RenderFrame>,
    resize_events: Events<ResizeEvent>,
) {
    if let Some(event) = resize_events.iter().last() {
        ctx.surface.configure(
            &ctx.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: SURFACE_FORMAT,
                width: event.width,
                height: event.height,
                present_mode: wgpu::PresentMode::Immediate,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
            },
        );
    }

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

pub fn main_pass(mut frame: ResMut<RenderFrame>) {
    let Some(frame) = (*frame).as_mut() else {
        return;
    };

    let main_pass = frame
        .encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.surface_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.5,
                        g: 0.6,
                        b: 0.8,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
            label: None,
        });
}

pub fn submit_frame(ctx: Res<RenderContext>, mut frame: ResMut<RenderFrame>) {
    let Some(frame) = (*frame).take() else {
        return;
    };

    ctx.queue.submit([frame.encoder.finish()]);
    frame.surface.present();
}

pub fn init_build(assets: &mut AssetProcessors) {
    assets.register("texture", TextureProcessor);
    assets.register("mesh", MeshProcessor);
}
