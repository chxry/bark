pub mod mesh;
pub mod texture;

use self::mesh::{MeshManager, MeshSource, Vertex, load_mesh};
use self::texture::{MAX_BOUND_TEXTURES, TextureManager, TextureSource, load_image};
use crate::app::ResizeEvent;
use crate::assets::{self, Assets};
use crate::ecs::World;
use crate::{app, cast_bytes, gfx, intersect};
use glam::{Mat3A, Mat4, Quat, Vec3};
use std::collections::HashSet;
use std::mem;

pub fn bark3d(world: &mut World) {
    world.insert_system_before(gfx::init, app::init);
    world.insert_system_before(init, gfx::init);
    world.insert_system_before(init, assets::init);
    world.queue_system(init);
}

#[derive(Copy, Clone, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

pub struct RenderObject {
    pub mesh: MeshSource,
    pub diffuse: TextureSource,
    pub normal: Option<TextureSource>,
}

#[repr(C)]
struct GPUObject {
    transform: Mat4,
    normal_transform: Mat3A,
    diffuse_id: u32,
    normal_id: u32,
}

struct RenderPipeline {
    frame_globals: FrameGlobals,
    texture_manager: TextureManager,
    mesh_manager: MeshManager,
    pipeline: wgpu::RenderPipeline,
}

pub fn init(world: &mut World) {
    let assets = world.get_resource_mut::<Assets>().unwrap();
    assets.register_loader(load_mesh);
    assets.register_loader(load_image);

    let renderer = world.get_resource_mut::<gfx::Renderer>().unwrap();

    let frame_globals = FrameGlobals::new(&renderer.device);
    let texture_manager = TextureManager::new(&renderer.device, &renderer.queue);
    let mesh_manager = MeshManager::new(&renderer.device);

    let pipeline_layout = renderer
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&frame_globals.layout, &texture_manager.layout],
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
        frame_globals,
        texture_manager,
        mesh_manager,
        pipeline,
    });

    world.insert_system_before(gfx::submit_frame, main_pass);
    world.add_event_handler(resize_framebuffer);
}

fn main_pass(world: &mut World) {
    let renderer = world.get_resource_mut::<gfx::Renderer>().unwrap();
    let frame = world.get_resource_mut::<gfx::RenderFrame>().unwrap();
    let pipeline = world.get_resource_mut::<RenderPipeline>().unwrap();
    let framebuffer = world.get_resource::<Framebuffer>().unwrap();

    let camera_pos = Vec3::new(0.0, 6.0, 10.0);
    let camera = Mat4::perspective_rh(
        1.0,
        frame.surface.texture.width() as f32 / frame.surface.texture.height() as f32,
        0.01,
        100.0,
    ) * Mat4::look_at_rh(camera_pos, Vec3::new(0.0, 2.5, 0.0), Vec3::Y);
    renderer.queue.write_buffer(
        &pipeline.frame_globals.buffer,
        0,
        cast_bytes(&GPUFrameGlobals { camera, camera_pos }),
    );

    let mut scene = intersect(world.get::<Transform>(), world.get::<RenderObject>())
        .map(|(_, (t, o))| (true, t, o))
        .collect::<Vec<_>>();

    let mut scene_textures = HashSet::new();
    let mut scene_meshes = HashSet::new();
    for (render, _, object) in &mut scene {
        if !object.mesh.ready() || !object.diffuse.ready() {
            *render = false;
            continue;
        }
        scene_textures.insert(object.diffuse.clone());
        if let Some(normal) = object.normal.clone().filter(|s| s.ready()) {
            scene_textures.insert(normal);
        }
        scene_meshes.insert(object.mesh.clone());
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
        main_pass.set_bind_group(0, &pipeline.frame_globals.bind_group, &[]);
        main_pass.set_bind_group(1, &pipeline.texture_manager.bind_group, &[]);
        main_pass.set_vertex_buffer(0, pipeline.mesh_manager.vertex_buffer.slice(..));
        main_pass.set_index_buffer(
            pipeline.mesh_manager.index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        for (render, transform, object) in scene {
            if !render {
                continue;
            }

            main_pass.set_push_constants(
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                0,
                cast_bytes(&GPUObject {
                    transform: Mat4::from_scale_rotation_translation(
                        transform.scale,
                        transform.rotation,
                        transform.position,
                    ),
                    normal_transform: Mat3A::from_quat(transform.rotation),
                    diffuse_id: pipeline.texture_manager.get_slot(&object.diffuse).unwrap(),
                    normal_id: object
                        .normal
                        .as_ref()
                        .and_then(|h| pipeline.texture_manager.get_slot(h))
                        .unwrap_or(MAX_BOUND_TEXTURES),
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

struct FrameGlobals {
    buffer: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
}

impl FrameGlobals {
    fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: mem::size_of::<GPUFrameGlobals>() as _,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            layout,
            buffer,
            bind_group,
        }
    }
}

#[repr(C)]
struct GPUFrameGlobals {
    camera: Mat4,
    camera_pos: Vec3,
}
