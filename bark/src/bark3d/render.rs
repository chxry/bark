use super::{Camera, DirectionalLight, FORWARD, RenderObject, Transform};
use crate::app::ResizeEvent;
use crate::bark3d::UP;
use crate::ecs::{Commands, Observer, Query, Res, ResMut};
use crate::gfx::mesh::{INDEX_FORMAT, MeshManager, Vertex};
use crate::gfx::texture::TextureManager;
use crate::gfx::{
    DEFAULT_BUFFER_SIZE, RenderContext, RenderFrame, SAMPLES, SURFACE_FORMAT, resize_buffer,
};
use crate::math::{Mat3A, Mat4, Quat, Vec3};
use crate::{cast_bytes, cast_bytes_slice};
use std::mem;
use std::num::NonZero;

const SHADOW_RESOLUTION: u32 = 4096;
const NUM_SHADOW_CASCADES: u32 = 4;
const SHADOW_MULTIVIEW_MASK: NonZero<u32> =
    NonZero::new(2u32.pow(NUM_SHADOW_CASCADES) - 1).unwrap();

// todo: consider seperating out scene stuff
pub struct RenderPipeline {
    uniform_buffer: wgpu::Buffer,
    light_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    shadow_map_view: wgpu::TextureView,
    shadow_map_bind_group: wgpu::BindGroup,
    main_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
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

    let shadow_map_texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SHADOW_RESOLUTION,
            height: SHADOW_RESOLUTION,
            depth_or_array_layers: 4,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let shadow_map_view = shadow_map_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let shadow_map_sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
        compare: Some(wgpu::CompareFunction::LessEqual),
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let shadow_map_bind_group_layout =
        ctx.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2Array,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                ],
            });
    let shadow_map_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &shadow_map_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&shadow_map_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&shadow_map_sampler),
            },
        ],
    });

    let main_shader = ctx
        .device
        .create_shader_module(wgpu::include_wgsl!("main.wgsl"));
    let main_pipeline_layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            bind_group_layouts: &[
                Some(&scene_bind_group_layout),
                Some(&textures.layout),
                Some(&shadow_map_bind_group_layout),
            ],
            immediate_size: mem::size_of::<GPUObject>() as _,
            label: None,
        });
    let main_pipeline = ctx
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            layout: Some(&main_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &main_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(Vertex::LAYOUT)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &main_shader,
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
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
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

    let shadow_shader = ctx
        .device
        .create_shader_module(wgpu::include_wgsl!("shadow.wgsl"));
    let shadow_pipeline_layout =
        ctx.device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                bind_group_layouts: &[Some(&scene_bind_group_layout)],
                immediate_size: mem::size_of::<GPUObject>() as _,
                label: None,
            });
    let shadow_pipeline = ctx
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_shadow"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(Vertex::LAYOUT)],
            },
            fragment: None,
            // primitive: wgpu::PrimitiveState {
            //     topology: wgpu::PrimitiveTopology::TriangleList,
            //     strip_index_format: None,
            //     front_face: wgpu::FrontFace::default(),
            //     cull_mode: Some(wgpu::Face::Front),
            //     unclipped_depth: false,
            //     polygon_mode: wgpu::PolygonMode::Fill,
            //     conservative: false,
            // },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: Some(SHADOW_MULTIVIEW_MASK),
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
        shadow_map_view,
        shadow_map_bind_group,
        main_pipeline,
        shadow_pipeline,
    });
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
    mut scene_objects: Query<(&Transform, &RenderObject)>,
) {
    let Some((_, surface_view)) = frame.surface.as_ref() else {
        return;
    };

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut main_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &framebuffer.color_view,
            resolve_target: Some(surface_view),
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
    main_pass.set_pipeline(&pipeline.main_pipeline);
    main_pass.set_bind_group(0, &pipeline.scene_bind_group, &[]);
    main_pass.set_bind_group(1, &textures.bind_group, &[]);
    main_pass.set_bind_group(2, &pipeline.shadow_map_bind_group, &[]);
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

pub fn shadow_pass(
    ctx: Res<RenderContext>,
    frame: Res<RenderFrame>,
    pipeline: Res<RenderPipeline>,
    meshes: Res<MeshManager>,
    lights: Query<(&Transform, &DirectionalLight)>,
    mut scene_objects: Query<(&Transform, &RenderObject)>,
) {
    if frame.surface.is_none() || get_shadow_source(lights).is_none() {
        return;
    }

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut shadow_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &pipeline.shadow_map_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: Some(SHADOW_MULTIVIEW_MASK),
        label: None,
    });
    shadow_pass.set_pipeline(&pipeline.shadow_pipeline);
    shadow_pass.set_bind_group(0, &pipeline.scene_bind_group, &[]);
    shadow_pass.set_vertex_buffer(0, meshes.vertex_buffer.slice(..));
    shadow_pass.set_index_buffer(meshes.index_buffer.slice(..), INDEX_FORMAT);
    for (_, (transform, object)) in scene_objects.iter() {
        if let Some(mesh) = meshes.get(&object.mesh) {
            let transform_mat = transform.as_mat4();
            shadow_pass.set_immediates(0, unsafe { cast_bytes(&transform_mat) });
            shadow_pass.draw_indexed(mesh.index_range(), mesh.vertex_range().start as _, 0..1);
        }
    }
    drop(shadow_pass);

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
        .map(|(_, (t, l))| GPULight {
            direction: t.rotation * FORWARD,
            tag: 1,
            color: l.color,
        })
        .chain([GPULight {
            direction: Vec3::ZERO,
            tag: 0,
            color: Vec3::ZERO,
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

pub fn extract_frame_globals(
    ctx: Res<RenderContext>,
    frame: Res<RenderFrame>,
    pipeline: ResMut<RenderPipeline>,
    mut cameras: Query<(&Transform, &Camera)>,
    lights: Query<(&Transform, &DirectionalLight)>,
) {
    let Some((surface, _)) = frame.surface.as_ref() else {
        return;
    };
    let Some((_, (camera_transform, camera))) = cameras.iter().next() else {
        return;
    };

    let aspect_ratio = surface.texture.width() as f32 / surface.texture.height() as f32;
    let camera_view = glam::camera::rh::view::look_to_mat4(
        camera_transform.position,
        camera_transform.rotation * FORWARD,
        UP,
    );
    let camera_proj = glam::camera::rh::proj::directx::perspective(
        camera.fov,
        aspect_ratio,
        camera.clip_range.start,
        camera.clip_range.end,
    );
    let cascades = [5.0, 10.0, 25.0, 100.0];

    let shadow_source_mats = match get_shadow_source(lights) {
        Some(transform) => cascades.map(|x| {
            fit_shadow_source_mat(
                glam::camera::rh::proj::directx::perspective(
                    camera.fov,
                    aspect_ratio,
                    camera.clip_range.start,
                    x,
                ) * camera_view,
                transform.rotation,
            )
        }),
        None => [Mat4::ZERO; NUM_SHADOW_CASCADES as _],
    };

    let frame_globals = GPUFrameGlobals {
        camera_view,
        camera_proj,
        shadow_source_mats,
        camera_pos: camera_transform.position,
    };
    ctx.queue.write_buffer(&pipeline.uniform_buffer, 0, unsafe {
        cast_bytes(&frame_globals)
    });
}

fn get_shadow_source<'a>(
    mut lights: Query<(&'a Transform, &DirectionalLight)>,
) -> Option<&'a Transform> {
    lights
        .iter()
        .filter_map(|(_, (t, l))| l.shadows.then_some(t))
        .next()
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

fn fit_shadow_source_mat(camera_mat: Mat4, light_rotation: Quat) -> Mat4 {
    let ndc_corners = [
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(-1.0, -1.0, 1.0),
        Vec3::new(1.0, -1.0, 1.0),
        Vec3::new(-1.0, 1.0, 1.0),
        Vec3::new(1.0, 1.0, 1.0),
    ];

    let camera_inv = camera_mat.inverse();
    let world_corners = ndc_corners.map(|x| camera_inv.project_point3(x));
    let world_center = world_corners.iter().sum::<Vec3>() / ndc_corners.len() as f32;

    let light_view = glam::camera::rh::view::look_at_mat4(
        world_center - light_rotation * FORWARD * 100.0,
        world_center,
        UP,
    );

    let mut min = Vec3::INFINITY;
    let mut max = Vec3::NEG_INFINITY;
    for x in world_corners {
        let pos = light_view.transform_point3(x);
        min = min.min(pos);
        max = max.max(pos);
    }
    let pad = Vec3::new(0.0, 0.0, 10.0);
    min -= pad;
    max += pad;

    let light_proj =
        glam::camera::rh::proj::directx::orthographic(min.x, max.x, min.y, max.y, -max.z, -min.z);

    light_proj * light_view
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
    camera_view: Mat4,
    camera_proj: Mat4,
    shadow_source_mats: [Mat4; NUM_SHADOW_CASCADES as _],
    camera_pos: Vec3,
}

#[repr(C)]
struct GPULight {
    direction: Vec3,
    tag: u32,
    color: Vec3,
}
