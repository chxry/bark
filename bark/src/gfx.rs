use crate::app::{self, App, ResizeEvent, WindowHandle};
use crate::assets::{AssetProcessor, AssetProcessors};
use crate::ecs::{Commands, Events, MainThread, Res, ResMut};
use glam::{Vec2, Vec3, Vec4};
use image::imageops::{self, FilterType};
use intel_tex_2::{RSurface, RgSurface, RgbaSurface, bc4, bc5, bc7};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Read, Write};
use tracing::error;

const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

pub fn init(app: &mut App) {
    app.world.insert_system(app::Startup, init_renderer);
    app.world.insert_system(app::Render, render);
}

struct RenderContext {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn init_renderer(window: Res<WindowHandle>, mut commands: Commands, _: MainThread) {
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
}

fn render(ctx: ResMut<RenderContext>, resize_events: Events<ResizeEvent>) {
    if let Some(event) = resize_events.iter().last() {
        ctx.surface.configure(
            &ctx.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: SURFACE_FORMAT,
                width: event.width,
                height: event.height,
                present_mode: wgpu::PresentMode::AutoVsync,
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
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    let main_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &surface_view,
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
    drop(main_pass);

    ctx.queue.submit([encoder.finish()]);
    surface.present();
}

pub fn init_build(assets: &mut AssetProcessors) {
    assets.register("texture", TextureProcessor);
    assets.register("mesh", MeshProcessor);
}

struct TextureProcessor;

#[derive(Serialize, Deserialize)]
struct TextureOptions {
    mode: TextureMode,
    compression: CompressionFormat,
    generate_mipmaps: bool,
}

#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TextureMode {
    Srgb,
    Linear,
    Normal,
}

#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CompressionFormat {
    Bc7,
    Bc5,
    Bc4,
    None,
}

struct TextureHeader {
    width: u32,
    height: u32,
    mip_count: u8,
    mode: TextureMode,
    compression: CompressionFormat,
}

impl TextureHeader {
    pub fn write<W: Write>(&self, mut out: W) {
        out.write_all(&self.width.to_le_bytes()).unwrap();
        out.write_all(&self.height.to_le_bytes()).unwrap();
        out.write_all(&[self.mip_count]).unwrap();
        out.write_all(&[self.mode as u8]).unwrap();
        out.write_all(&[self.compression as u8]).unwrap();
    }
}

impl AssetProcessor for TextureProcessor {
    type Options = TextureOptions;

    // todo: respect TextureMode, only downsample needed channels
    fn process<R: Read, W: Write>(&self, mut src: R, mut out: W, opts: Self::Options) {
        let mut bytes = Vec::new();
        src.read_to_end(&mut bytes).unwrap();

        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();

        let mut mip_levels = vec![image];
        if opts.generate_mipmaps {
            let mut w = mip_levels[0].width();
            let mut h = mip_levels[0].height();
            while w > 4 && h > 4 {
                w = (w / 2).max(4);
                h = (h / 2).max(4);
                mip_levels.push(imageops::resize(
                    mip_levels.last().unwrap(),
                    w,
                    h,
                    FilterType::Lanczos3,
                ));
            }
        }

        let header = TextureHeader {
            width: mip_levels[0].width(),
            height: mip_levels[0].height(),
            mip_count: mip_levels.len() as u8,
            mode: opts.mode,
            compression: opts.compression,
        };
        header.write(&mut out);

        for level in mip_levels {
            let compressed = match opts.compression {
                CompressionFormat::Bc7 => &bc7::compress_blocks(
                    &bc7::opaque_ultra_fast_settings(),
                    &RgbaSurface {
                        data: &level,
                        width: level.width(),
                        height: level.height(),
                        stride: level.width() * 4,
                    },
                ),
                CompressionFormat::Bc5 => &bc5::compress_blocks(&RgSurface {
                    data: &extract_rg(&level),
                    width: level.width(),
                    height: level.height(),
                    stride: level.height() * 2,
                }),
                CompressionFormat::Bc4 => &bc4::compress_blocks(&RSurface {
                    data: &extract_r(&level),
                    width: level.width(),
                    height: level.height(),
                    stride: level.width(),
                }),

                CompressionFormat::None => level.as_raw(),
            };
            out.write_all(&compressed).unwrap();
        }
    }
}

fn extract_rg(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height() * 2) as usize);
    for pixel in img.pixels() {
        out.push(pixel[0]);
        out.push(pixel[1]);
    }
    out
}

fn extract_r(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height()) as usize);
    for pixel in img.pixels() {
        out.push(pixel[0]);
    }
    out
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vertex {
    pub pos: Vec3,
    pub uv: Vec2,
    pub normal: Vec3,
    pub tangent: Vec4,
}

struct MeshProcessor;

struct MeshHeader {
    vertices_len: u32,
    indices_len: u32,
}

impl MeshHeader {
    pub fn write<W: Write>(&self, mut out: W) {
        out.write_all(&self.vertices_len.to_le_bytes()).unwrap();
        out.write_all(&self.indices_len.to_le_bytes()).unwrap();
    }
}

impl AssetProcessor for MeshProcessor {
    type Options = ();

    fn process<R: Read, W: Write>(&self, src: R, out: W, _: Self::Options) {
        let obj = obj::load_obj::<obj::TexturedVertex, _, u32>(BufReader::new(src)).unwrap();
        let mut vertices = obj
            .vertices
            .into_iter()
            .map(|v| Vertex {
                pos: Vec3::from(v.position),
                uv: Vec2::new(v.texture[0], 1.0 - v.texture[1]),
                normal: Vec3::from(v.normal),
                tangent: Vec4::ZERO,
            })
            .collect::<Vec<_>>();
        let indices = obj.indices;

        let header = MeshHeader {
            vertices_len: vertices.len() as _,
            indices_len: indices.len() as _,
        };
        header.write(out);

        let mut tangents = vec![Vec3::ZERO; vertices.len()];
        let mut bitangents = vec![Vec3::ZERO; vertices.len()];

        for tri in indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;

            let v0 = &vertices[i0];
            let v1 = &vertices[i1];
            let v2 = &vertices[i2];

            let dp1 = v1.pos - v0.pos;
            let dp2 = v2.pos - v0.pos;

            let duv1 = v1.uv - v0.uv;
            let duv2 = v2.uv - v0.uv;

            let r = 1.0 / (duv1.x * duv2.y - duv1.y * duv2.x);

            let tangent = (dp1 * duv2.y - dp2 * duv1.y) * r;
            let bitangent = (dp2 * duv1.x - dp1 * duv2.x) * r;

            tangents[i0] += tangent;
            tangents[i1] += tangent;
            tangents[i2] += tangent;

            bitangents[i0] += bitangent;
            bitangents[i1] += bitangent;
            bitangents[i2] += bitangent;
        }

        for (i, v) in vertices.iter_mut().enumerate() {
            let n = v.normal;
            let t = tangents[i];

            let tangent = (t - n * n.dot(t)).normalize();

            let b = bitangents[i];
            let handedness = if n.cross(tangent).dot(b) < 0.0 {
                -1.0
            } else {
                1.0
            };

            v.tangent = tangent.extend(handedness);
        }
    }
}
