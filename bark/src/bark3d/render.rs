use super::{Camera, DirectionalLight, FORWARD, RenderObject, Transform};
use crate::app::ResizeEvent;
use crate::ecs::{Commands, Observer, Query, Res, ResMut};
use crate::gfx::mesh::{INDEX_FORMAT, MeshManager, Vertex};
use crate::gfx::texture::TextureManager;
use crate::gfx::{
    DEFAULT_BUFFER_SIZE, RenderContext, RenderFrame, SAMPLES, SURFACE_FORMAT, resize_buffer,
};
use crate::math::{Mat3A, Mat4, Vec3};
use crate::{cast_bytes, cast_bytes_slice};
use std::mem;

// todo: consider seperating out scene stuff
pub struct RenderPipeline {
    uniform_buffer: wgpu::Buffer,
    light_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

pub fn init_pipeline(
    ctx: Res<RenderContext>,
    textures: Res<TextureManager>,
    mut commands: Commands,
) {
    let uniform_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        size: mem::size_of::<GPUFrameGlobals>() as _,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        label: None,
    });

    let light_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        size: DEFAULT_BUFFER_SIZE,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        label: None,
    });

    let scene_bind_group_layout =
        ctx.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
                label: None,
            });
    let scene_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
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

    let pipeline_layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            bind_group_layouts: &[Some(&scene_bind_group_layout), Some(&textures.layout)],
            immediate_size: mem::size_of::<GPUObject>() as _,
            label: None,
        });

    let shader = ctx
        .device
        .create_shader_module(wgpu::include_wgsl!("bark3d.wgsl"));
    let pipeline = ctx
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(Vertex::LAYOUT)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SURFACE_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
            label: None,
        });

    let surface_config = ctx.surface.get_configuration().unwrap();
    commands.insert_resource(Framebuffer::new(
        &ctx.device,
        surface_config.width,
        surface_config.height,
    ));
    commands.insert_resource(RenderPipeline {
        uniform_buffer,
        light_buffer,
        scene_bind_group,
        pipeline,
    });
}

pub struct Framebuffer {
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
}

impl Framebuffer {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let desc = wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: SURFACE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
            label: None,
        };
        let color_texture = device.create_texture(&desc);
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            format: wgpu::TextureFormat::Depth32Float,
            ..desc
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            color_view,
            depth_view,
        }
    }
}

pub fn resize_framebuffer(
    resize: Observer<ResizeEvent>,
    ctx: Res<RenderContext>,
    mut framebuffer: ResMut<Framebuffer>,
) {
    *framebuffer = Framebuffer::new(&ctx.device, resize.width, resize.height);
}

pub fn main_pass(
    ctx: Res<RenderContext>,
    frame: Res<RenderFrame>,
    pipeline: Res<RenderPipeline>,
    framebuffer: Res<Framebuffer>,
    textures: Res<TextureManager>,
    meshes: Res<MeshManager>,
    mut cameras: Query<(&Transform, &Camera)>,
    mut scene_objects: Query<(&Transform, &RenderObject)>,
) {
    let Some((surface, surface_view)) = frame.surface.as_ref() else {
        return;
    };

    let Some((_, (camera_transform, camera))) = cameras.iter().next() else {
        return;
    };

    let aspect_ratio = surface.texture.width() as f32 / surface.texture.height() as f32;
    let frame_globals = GPUFrameGlobals {
        camera: camera.as_mat4(aspect_ratio, camera_transform),
        camera_pos: camera_transform.position,
    };
    ctx.queue.write_buffer(&pipeline.uniform_buffer, 0, unsafe {
        cast_bytes(&frame_globals)
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    let mut main_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &framebuffer.color_view,
            resolve_target: Some(&surface_view),
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
        multiview_mask: None,
        label: None,
    });
    main_pass.set_pipeline(&pipeline.pipeline);
    main_pass.set_bind_group(0, &pipeline.scene_bind_group, &[]);
    main_pass.set_bind_group(1, &textures.bind_group, &[]);
    main_pass.set_vertex_buffer(0, meshes.vertex_buffer.slice(..));
    main_pass.set_index_buffer(meshes.index_buffer.slice(..), INDEX_FORMAT);
    for (_, (transform, object)) in scene_objects.iter() {
        if let Some(mesh) = meshes.get(&object.mesh) {
            let transform_mat = transform.as_mat4();
            let gpu_object = GPUObject {
                transform: transform_mat,
                normal_transform: Mat3A::from_mat4(transform_mat).inverse().transpose(),
                diffuse_color: object.diffuse_color,
                diffuse_id: object.diffuse_tex.map_or(0, |t| textures.get(t)),
                pbr_values: object.pbr.get_arm_values(),
                pbr_id: object.pbr.get_tex().map_or(0, |t| textures.get(t)),
                normal_id: object.normal_tex.map_or(0, |t| textures.get(t)),
            };
            main_pass.set_immediates(0, unsafe { cast_bytes(&gpu_object) });
            main_pass.draw_indexed(mesh.index_range(), mesh.vertex_range().start as _, 0..1);
        }
    }
    drop(main_pass);

    frame.submit(encoder.finish());
}

pub fn extract_lights(
    ctx: Res<RenderContext>,
    frame: Res<RenderFrame>,
    mut pipeline: ResMut<RenderPipeline>,
    mut lights: Query<(&Transform, &DirectionalLight)>,
) {
    let lights = lights
        .iter()
        .map(|(_, (t, _))| GPULight {
            direction: t.rotation * FORWARD,
            tag: 1,
        })
        .chain([GPULight {
            direction: Vec3::ZERO,
            tag: 0,
        }])
        .collect::<Vec<_>>();
    let light_buf_size = (lights.len() * mem::size_of::<GPULight>()) as _;
    if pipeline.light_buffer.size() < light_buf_size {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        resize_buffer(
            &ctx.device,
            &mut encoder,
            &mut pipeline.light_buffer,
            light_buf_size,
            None,
        );

        frame.submit(encoder.finish());
    }
    ctx.queue.write_buffer(&pipeline.light_buffer, 0, unsafe {
        cast_bytes_slice(&lights)
    });
}

pub fn shadow_pass(
    ctx: Res<RenderContext>,
    pipeline: Res<RenderPipeline>,
    mut lights: Query<(&Transform, &DirectionalLight)>,
) {
}

#[repr(C)]
struct GPUObject {
    transform: Mat4,
    normal_transform: Mat3A,
    diffuse_color: Vec3,
    diffuse_id: u32,
    pbr_values: Vec3,
    pbr_id: u32,
    normal_id: u32,
}

#[repr(C)]
struct GPUFrameGlobals {
    camera: Mat4,
    camera_pos: Vec3,
}

#[repr(C)]
struct GPULight {
    direction: Vec3,
    tag: u32,
}
