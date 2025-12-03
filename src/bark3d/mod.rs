pub mod mesh;
pub mod texture;

use self::mesh::{Mesh, MeshManager, Vertex, load_mesh};
use self::texture::{Texture, TextureManager, load_texture};
use crate::app::ResizeEvent;
use crate::assets::{self, Assets, Handle};
use crate::ecs::World;
use crate::gfx::resized_buffer;
use crate::{app, cast_bytes, cast_bytes_slice, gfx, intersect};
use glam::{Affine3A, Mat3A, Mat4, Quat, Vec3};
use std::collections::HashSet;
use std::mem;

pub const DEFAULT_BUFFER_SIZE: wgpu::BufferAddress = 1024 * 1024;

pub fn bark3d(world: &mut World) {
    world.insert_system_before(gfx::init, app::init);
    world.insert_system_before(init, gfx::init);
    world.insert_system_before(init, assets::init);
    world.queue_system(init);
}

pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

pub struct RenderObject {
    pub mesh: Handle<Mesh>,
    pub diffuse_colour: Vec3,
    pub diffuse: Option<Handle<Texture>>,
    pub normal: Option<Handle<Texture>>,
    pub pbr: PbrMode,
}

#[derive(Clone)]
pub enum PbrMode {
    Sampled(Handle<Texture>),
    Values { roughness: f32, metallic: f32 },
}

pub struct Camera {
    pub fov: f32,
}

pub enum Light {
    Directional(Vec3),
}

#[repr(C)]
struct GPUObject {
    transform: Mat4,
    normal_transform: Mat3A,
    diffuse_colour: Vec3,
    diffuse_id: u32,
    pbr_arm: Vec3,
    normal_id: u32,
    pbr_id: u32,
}

#[repr(C)]
struct GPULight {
    direction: Vec3,
    tag: u32,
}

struct RenderPipeline {
    texture_manager: TextureManager,
    mesh_manager: MeshManager,
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    light_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
}

pub fn init(world: &mut World) {
    let assets = world.get_resource_mut::<Assets>().unwrap();
    assets.register_loader(load_mesh);
    assets.register_loader(load_texture);

    let renderer = world.get_resource_mut::<gfx::Renderer>().unwrap();

    let texture_manager = TextureManager::new(&renderer.device, &renderer.queue);
    let mesh_manager = MeshManager::new(&renderer.device);

    let uniform_buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: mem::size_of::<GPUFrameGlobals>() as _,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let light_buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: DEFAULT_BUFFER_SIZE,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let scene_bind_group_layout =
        renderer
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
    let scene_bind_group = renderer
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &scene_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: light_buffer.as_entire_binding(),
                },
            ],
        });

    let pipeline_layout = renderer
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&scene_bind_group_layout, &texture_manager.layout],
            push_constant_ranges: &[wgpu::PushConstantRange {
                stages: wgpu::ShaderStages::VERTEX_FRAGMENT,
                range: 0..mem::size_of::<GPUObject>() as _,
            }],
        });

    let shader = renderer
        .device
        .create_shader_module(wgpu::include_wgsl!("test.wgsl"));
    let pipeline = renderer
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: (mem::size_of::<Vertex>()) as _,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: gfx::FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: gfx::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
            label: None,
        });

    world.insert_resource(RenderPipeline {
        texture_manager,
        mesh_manager,
        pipeline,
        uniform_buffer,
        light_buffer,
        scene_bind_group,
    });

    world.insert_system_before(gfx::submit_frame, main_pass);
    world.add_event_handler(resize_framebuffer);
}

fn main_pass(world: &mut World) {
    let renderer = world.get_resource_mut::<gfx::Renderer>().unwrap();
    let frame = world.get_resource_mut::<gfx::RenderFrame>().unwrap();
    let pipeline = world.get_resource_mut::<RenderPipeline>().unwrap();
    let framebuffer = world.get_resource::<Framebuffer>().unwrap();

    let (_, (cam_transform, cam)) = intersect(world.get::<Transform>(), world.get::<Camera>())
        .next()
        .unwrap();

    let aspect_ratio = frame.surface.texture.width() as f32 / frame.surface.texture.height() as f32;
    renderer.queue.write_buffer(
        &pipeline.uniform_buffer,
        0,
        cast_bytes(&GPUFrameGlobals {
            camera: Mat4::perspective_rh(cam.fov, aspect_ratio, 0.01, 100.0)
                * Mat4::look_to_rh(
                    cam_transform.position,
                    cam_transform.rotation * Vec3::NEG_Z,
                    Vec3::Y,
                ),
            camera_pos: cam_transform.position,
        }),
    );

    let objects =
        intersect(world.get::<Transform>(), world.get::<RenderObject>()).collect::<Vec<_>>();

    let mut scene_textures = HashSet::new();
    let mut scene_meshes = HashSet::new();
    for (_, (_, object)) in &objects {
        scene_meshes.insert(object.mesh.clone());

        if let Some(diffuse) = object.diffuse.clone() {
            scene_textures.insert(diffuse);
        }
        if let Some(normal) = object.normal.clone() {
            scene_textures.insert(normal);
        }
        if let PbrMode::Sampled(pbr) = object.pbr.clone() {
            scene_textures.insert(pbr);
        }
    }

    pipeline
        .texture_manager
        .process_sources(&renderer.device, &renderer.queue, scene_textures);

    pipeline.mesh_manager.process_sources(
        &renderer.device,
        &renderer.queue,
        &mut frame.encoder,
        scene_meshes,
    );

    let lights = world
        .get::<Light>()
        .map(|(_, l)| match l {
            Light::Directional(dir) => GPULight {
                direction: dir.normalize(),
                tag: 1,
            },
        })
        .chain([GPULight {
            direction: Vec3::ZERO,
            tag: 0,
        }])
        .collect::<Vec<_>>();
    let light_buf_size = (lights.len() * mem::size_of::<GPULight>()) as wgpu::BufferAddress;
    if pipeline.light_buffer.size() < light_buf_size {
        pipeline.light_buffer =
            resized_buffer(&renderer.device, &pipeline.light_buffer, light_buf_size);
    }
    renderer
        .queue
        .write_buffer(&pipeline.light_buffer, 0, cast_bytes_slice(&lights));

    let mut main_pass = frame
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
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &framebuffer.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            label: None,
        });
    if pipeline.mesh_manager.vertex_buffer.size() > 0 {
        main_pass.set_pipeline(&pipeline.pipeline);
        main_pass.set_bind_group(0, &pipeline.scene_bind_group, &[]);
        main_pass.set_bind_group(1, &pipeline.texture_manager.bind_group, &[]);
        main_pass.set_vertex_buffer(0, pipeline.mesh_manager.vertex_buffer.slice(..));
        main_pass.set_index_buffer(
            pipeline.mesh_manager.index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        for (_, (transform, object)) in objects {
            let transform = Affine3A::from_scale_rotation_translation(
                transform.scale,
                transform.rotation,
                transform.position,
            );

            let (pbr_id, pbr_arm) = match &object.pbr {
                PbrMode::Sampled(tex) => (
                    pipeline.texture_manager.get_slot(tex).unwrap_or(0),
                    Vec3::ONE,
                ),
                PbrMode::Values {
                    roughness,
                    metallic,
                } => (0, Vec3::new(1.0, *roughness, *metallic)),
            };

            main_pass.set_push_constants(
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                0,
                cast_bytes(&GPUObject {
                    transform: transform.into(),
                    normal_transform: transform.matrix3.inverse().transpose(),
                    diffuse_colour: object.diffuse_colour,
                    diffuse_id: object
                        .diffuse
                        .as_ref()
                        .and_then(|x| pipeline.texture_manager.get_slot(x))
                        .unwrap_or(0),
                    pbr_arm,
                    normal_id: object
                        .normal
                        .as_ref()
                        .and_then(|x| pipeline.texture_manager.get_slot(x))
                        .unwrap_or(0),

                    pbr_id,
                }),
            );
            let mesh_handle = pipeline.mesh_manager.get_handle(&object.mesh).unwrap();
            main_pass.draw_indexed(
                mesh_handle.index_start as _
                    ..mesh_handle.index_start as u32 + mesh_handle.index_count,
                mesh_handle.vertex_start as _,
                0..1,
            );
        }
    }
}

struct Framebuffer {
    depth_view: wgpu::TextureView,
}

impl Framebuffer {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gfx::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { depth_view }
    }
}

fn resize_framebuffer(world: &mut World, event: &ResizeEvent) {
    let renderer = world.get_resource::<gfx::Renderer>().unwrap();
    world.insert_resource(Framebuffer::new(
        &renderer.device,
        event.0.width,
        event.0.height,
    ));
}

#[repr(C)]
struct GPUFrameGlobals {
    camera: Mat4,
    camera_pos: Vec3,
}
