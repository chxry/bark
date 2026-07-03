use crate::app::{self, App};
use crate::ecs::{Commands, IntoSystem, Query, Res, ResMut};
use crate::gfx::mesh::{INDEX_FORMAT, MeshHandle, MeshManager, Vertex};
use crate::gfx::texture::{TextureHandle, TextureManager};
use crate::gfx::{
    self, DEFAULT_BUFFER_SIZE, Framebuffer, RenderContext, RenderFrame, SAMPLES, resize_buffer,
};
use crate::math::{EulerRot, Mat3A, Mat4, Quat, Vec3};
use crate::{cast_bytes, cast_bytes_slice};
use std::mem;

pub const UP: Vec3 = Vec3::Y;
pub const FORWARD: Vec3 = Vec3::NEG_Z;

pub fn init(app: &mut App) {
    app.world
        .insert_system::<app::Startup>(init_pipeline.after(gfx::init_renderer));
    app.world
        .insert_system::<app::Render>(main_pass.after(gfx::begin_frame).before(gfx::submit_frame));
    app.world.insert_system::<app::Render>(
        extract_lights
            .after(gfx::begin_frame)
            .before(gfx::submit_frame),
    );
}

pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub fn position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    pub fn rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn rotation_euler(mut self, yaw: f32, pitch: f32, roll: f32) -> Self {
        self.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
        self
    }

    pub fn scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    pub fn as_mat4(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

pub struct Camera {
    pub fov: f32,
}

impl Camera {
    pub fn new(fov: f32) -> Self {
        Camera { fov }
    }

    pub fn as_mat4(&self, aspect_ratio: f32, transform: &Transform) -> Mat4 {
        Mat4::perspective_rh(self.fov, aspect_ratio, 0.01, 100.0)
            * Mat4::look_to_rh(transform.position, transform.rotation * FORWARD, UP)
    }
}

pub struct RenderObject {
    pub mesh: MeshHandle,
    pub diffuse_color: Vec3,
    pub diffuse_tex: Option<TextureHandle>,
    pub normal_tex: Option<TextureHandle>,
    pub pbr: PbrMode,
}

impl RenderObject {
    pub fn new(mesh: MeshHandle) -> Self {
        Self {
            mesh,
            diffuse_color: Vec3::ONE,
            diffuse_tex: None,
            normal_tex: None,
            pbr: PbrMode::Values {
                roughness: 0.5,
                metallic: 0.0,
            },
        }
    }

    pub fn diffuse_color(mut self, color: Vec3) -> Self {
        self.diffuse_color = color;
        self
    }

    pub fn diffuse_texture(mut self, tex: TextureHandle) -> Self {
        self.diffuse_tex = Some(tex);
        self
    }

    pub fn normal_texture(mut self, tex: TextureHandle) -> Self {
        self.normal_tex = Some(tex);
        self
    }

    pub fn pbr_texture(mut self, tex: TextureHandle) -> Self {
        self.pbr = PbrMode::Sampled(tex);
        self
    }

    pub fn pbr_values(mut self, roughness: f32, metallic: f32) -> Self {
        self.pbr = PbrMode::Values {
            roughness,
            metallic,
        };
        self
    }
}

#[derive(Clone)]
pub enum PbrMode {
    Sampled(TextureHandle),
    Values { roughness: f32, metallic: f32 },
}

impl PbrMode {
    fn get_tex(&self) -> Option<TextureHandle> {
        match self {
            Self::Sampled(t) => Some(*t),
            _ => None,
        }
    }

    fn get_arm_values(&self) -> Vec3 {
        match self {
            Self::Values {
                roughness,
                metallic,
            } => Vec3::new(1.0, *roughness, *metallic),
            _ => Vec3::ONE,
        }
    }
}

pub struct DirectionalLight {}

struct RenderPipeline {
    uniform_buffer: wgpu::Buffer,
    light_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

fn init_pipeline(ctx: Res<RenderContext>, textures: Res<TextureManager>, mut commands: Commands) {
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
                buffers: &[Vertex::LAYOUT],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
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

    commands.insert_resource(RenderPipeline {
        uniform_buffer,
        light_buffer,
        scene_bind_group,
        pipeline,
    });
}

fn main_pass(
    ctx: Res<RenderContext>,
    mut frame: ResMut<RenderFrame>,
    mut pipeline: ResMut<RenderPipeline>,
    framebuffer: Res<Framebuffer>,
    textures: Res<TextureManager>,
    meshes: Res<MeshManager>,
    mut cameras: Query<(&Transform, &Camera)>,
    mut scene_objects: Query<(&Transform, &RenderObject)>,
) {
    let Some(frame) = frame.as_mut() else {
        return;
    };

    let Some((_, (camera_transform, camera))) = cameras.iter().next() else {
        return;
    };

    let aspect_ratio = frame.surface.texture.width() as f32 / frame.surface.texture.height() as f32;
    let frame_globals = GPUFrameGlobals {
        camera: camera.as_mat4(aspect_ratio, camera_transform),
        camera_pos: camera_transform.position,
    };
    ctx.queue.write_buffer(&pipeline.uniform_buffer, 0, unsafe {
        cast_bytes(&frame_globals)
    });

    let mut main_pass = frame
        .encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &framebuffer.color_view,
                resolve_target: Some(&frame.surface_view),
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
}

fn extract_lights(
    ctx: Res<RenderContext>,
    mut frame: ResMut<RenderFrame>,
    mut pipeline: ResMut<RenderPipeline>,
    mut lights: Query<(&Transform, &DirectionalLight)>,
) {
    let Some(frame) = frame.as_mut() else {
        return;
    };

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
        resize_buffer(
            &ctx.device,
            &mut frame.encoder,
            &mut pipeline.light_buffer,
            light_buf_size,
            None,
        );
    }
    ctx.queue.write_buffer(&pipeline.light_buffer, 0, unsafe {
        cast_bytes_slice(&lights)
    });
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
