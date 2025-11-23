use crate::app::ResizeEvent;
use crate::assets::{self, AssetId, Assets, Handle};
use crate::ecs::World;
use crate::{app, cast_bytes, cast_bytes_slice, gfx, intersect};
use glam::{Mat3A, Mat4, Quat, Vec2, Vec3, Vec4};
use image::DynamicImage;
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read};
use std::mem;
use std::num::NonZero;
use tracing::{debug, error};
use wgpu::util::DeviceExt;

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

#[repr(C)]
struct GPUObject {
    transform: Mat4,
    normal_transform: Mat3A,
    diffuse_id: u32,
    normal_id: u32,
}

pub struct MeshRenderer {
    pub mesh: Handle<Mesh>,
    pub diffuse: Handle<DynamicImage>,
    pub normal: Option<Handle<DynamicImage>>,
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

    // todo this is all wacky + there is a race condition here, if textures load after process_handles
    let objects =
        intersect(world.get::<Transform>(), world.get::<MeshRenderer>()).collect::<Vec<_>>();

    pipeline.texture_manager.process_handles(
        &renderer.device,
        &renderer.queue,
        objects
            .iter()
            .flat_map(|o| {
                [Some(o.1.1.diffuse.clone()), o.1.1.normal.clone()]
                    .into_iter()
                    .flatten()
            })
            .collect(),
    );

    pipeline.mesh_manager.process_handles(
        &renderer.device,
        &renderer.queue,
        &mut frame.encoder,
        objects.iter().map(|o| o.1.1.mesh.clone()).collect(),
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
        for (_, (transform, mesh_renderer)) in objects {
            if !mesh_renderer.mesh.loaded() || !mesh_renderer.diffuse.loaded() {
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
                    diffuse_id: pipeline.texture_manager.get_slot(&mesh_renderer.diffuse),
                    normal_id: mesh_renderer
                        .normal
                        .as_ref()
                        .filter(|h| h.loaded())
                        .map(|h| pipeline.texture_manager.get_slot(h))
                        .unwrap_or(MAX_BOUND_TEXTURES),
                }),
            );
            let mesh_handle = pipeline.mesh_manager.get_handle(&mesh_renderer.mesh);
            main_pass.draw_indexed(
                mesh_handle.index_start as _
                    ..mesh_handle.index_start as u32 + mesh_handle.mesh.get().indices.len() as u32,
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

const MAX_BOUND_TEXTURES: TextureSlotIndex = 2048;

struct TextureManager {
    undefined_texture_view: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    slots: [Option<TextureSlot>; MAX_BOUND_TEXTURES as _],
    handle_map: HashMap<AssetId, TextureSlotIndex>,
}

impl TextureManager {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let undefined_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::default(),
            &[255, 0, 255, 255],
        );
        let undefined_texture_view =
            undefined_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: NonZero::new(MAX_BOUND_TEXTURES),
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let slots = [const { None }; MAX_BOUND_TEXTURES as _];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let bind_group =
            Self::build_bind_group(device, &layout, &sampler, &slots, &undefined_texture_view);

        Self {
            undefined_texture_view,
            layout,
            sampler,
            bind_group,
            slots,
            handle_map: HashMap::new(),
        }
    }

    fn process_handles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        handles: Vec<Handle<DynamicImage>>,
    ) {
        if handles.len() > MAX_BOUND_TEXTURES as _ {
            error!(
                "amount of bound textures exceeds limit ({})",
                MAX_BOUND_TEXTURES
            );
            return;
        }
        let mut bindings_changed = false;

        let active_ids = handles.iter().map(|h| h.id()).collect::<HashSet<_>>();
        for s in &mut self.slots {
            if let Some(slot) = s {
                let id = slot.image.id();
                if !active_ids.contains(&id) {
                    debug!("free texture {:?}", id);
                    *s = None;
                    bindings_changed = true;
                }
            }
        }

        let mut i = 0;
        for h in handles {
            if let Some(mesh) = h.try_get() {
                let id = h.id();
                if !self.handle_map.contains_key(&id) {
                    debug!("uploading texture {:?}", id);
                    while self.slots[i].is_some() {
                        i += 1;
                    }
                    let texture = device.create_texture_with_data(
                        queue,
                        &wgpu::TextureDescriptor {
                            label: None,
                            size: wgpu::Extent3d {
                                width: mesh.width(),
                                height: mesh.height(),
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm, // todo
                            usage: wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        },
                        wgpu::util::TextureDataOrder::default(),
                        &mesh.to_rgba8(),
                    );
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    self.slots[i] = Some(TextureSlot {
                        image: h.clone(),
                        view,
                    });
                    self.handle_map.insert(id, i as _);
                    bindings_changed = true;
                }
            }
        }

        if bindings_changed {
            self.bind_group = Self::build_bind_group(
                device,
                &self.layout,
                &self.sampler,
                &self.slots,
                &self.undefined_texture_view,
            );
        }
    }

    fn get_slot(&self, handle: &Handle<DynamicImage>) -> TextureSlotIndex {
        *self.handle_map.get(&handle.id()).unwrap()
    }

    fn build_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        slots: &[Option<TextureSlot>; MAX_BOUND_TEXTURES as _],
        fallback: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        debug!("rebuilding texture bindings");
        let views = slots
            .each_ref()
            .map(|s| s.as_ref().map(|s| &s.view).unwrap_or(fallback));
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureViewArray(&views),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

type TextureSlotIndex = u32;

struct TextureSlot {
    image: Handle<DynamicImage>,
    view: wgpu::TextureView,
}

struct MeshManager {
    vertex_buffer: wgpu::Buffer,
    vertex_end: wgpu::BufferAddress,
    index_buffer: wgpu::Buffer,
    index_end: wgpu::BufferAddress,
    handles: HashMap<AssetId, MeshHandle>,
}

impl MeshManager {
    fn new(device: &wgpu::Device) -> Self {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 0,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 0,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::INDEX
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            vertex_buffer,
            vertex_end: 0,
            index_buffer,
            index_end: 0,
            handles: HashMap::new(),
        }
    }

    fn process_handles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        handles: Vec<Handle<Mesh>>,
    ) {
        let to_upload = handles
            .iter()
            .filter_map(|h| {
                (!self.handles.contains_key(&h.id()) && h.loaded()).then_some((h.id(), h))
            })
            .collect::<HashMap<_, _>>();

        if !to_upload.is_empty() {
            let mut vertex_resize = 0;
            let mut index_resize = 0;
            for h in to_upload.values() {
                let mesh = h.get();
                vertex_resize += mesh.vertices.len() as wgpu::BufferAddress;
                index_resize += mesh.indices.len() as wgpu::BufferAddress;
            }

            let mut vertex_view = grow_buffer(
                device,
                queue,
                encoder,
                &mut self.vertex_buffer,
                self.vertex_end * mem::size_of::<Vertex>() as wgpu::BufferAddress,
                vertex_resize * mem::size_of::<Vertex>() as wgpu::BufferAddress,
            );
            let mut index_view = grow_buffer(
                device,
                queue,
                encoder,
                &mut self.index_buffer,
                self.index_end * mem::size_of::<u32>() as wgpu::BufferAddress,
                index_resize * mem::size_of::<u32>() as wgpu::BufferAddress,
            );

            let mut current_vertex_offset = 0;
            let mut current_index_offset = 0;

            for (id, h) in to_upload {
                debug!("uploading mesh {:?}", id);
                let mesh = h.get();

                let handle = MeshHandle {
                    mesh: h.clone(),
                    vertex_start: self.vertex_end,
                    index_start: self.index_end,
                };

                self.vertex_end += mesh.vertices.len() as wgpu::BufferAddress;
                self.index_end += mesh.indices.len() as wgpu::BufferAddress;

                let vertex_end =
                    current_vertex_offset + mesh.vertices.len() * mem::size_of::<Vertex>();
                vertex_view[current_vertex_offset..vertex_end]
                    .copy_from_slice(cast_bytes_slice(&mesh.vertices));
                current_vertex_offset = vertex_end;

                let index_end = current_index_offset + mesh.indices.len() * mem::size_of::<u32>();
                index_view[current_index_offset..index_end]
                    .copy_from_slice(cast_bytes_slice(&mesh.indices));
                current_index_offset = index_end;

                self.handles.insert(id, handle);
            }
        }
    }

    fn get_handle(&self, handle: &Handle<Mesh>) -> &MeshHandle {
        self.handles.get(&handle.id()).unwrap()
    }
}

struct MeshHandle {
    mesh: Handle<Mesh>,
    vertex_start: wgpu::BufferAddress,
    index_start: wgpu::BufferAddress,
}

fn resized_buffer(
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

fn grow_buffer(
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

pub struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

#[repr(C)]
struct Vertex {
    pos: Vec3,
    uv: Vec2,
    normal: Vec3,
    tangent: Vec4,
}

fn load_mesh(reader: impl Read) -> Mesh {
    let obj = obj::load_obj(BufReader::new(reader)).unwrap();
    let mut mesh = Mesh {
        vertices: obj
            .vertices
            .into_iter()
            .map(|v: obj::TexturedVertex| Vertex {
                pos: Vec3::from(v.position),
                uv: Vec2::new(v.texture[0], 1.0 - v.texture[1]),
                normal: Vec3::from(v.normal),
                tangent: Vec4::ZERO,
            })
            .collect(),
        indices: obj.indices,
    };

    let mut tangents = vec![Vec3::ZERO; mesh.vertices.len()];
    let mut bitangents = vec![Vec3::ZERO; mesh.vertices.len()];

    for tri in mesh.indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;

        let v0 = &mesh.vertices[i0];
        let v1 = &mesh.vertices[i1];
        let v2 = &mesh.vertices[i2];

        let p0 = v0.pos;
        let p1 = v1.pos;
        let p2 = v2.pos;

        let uv0 = v0.uv;
        let uv1 = v1.uv;
        let uv2 = v2.uv;

        let dp1 = p1 - p0;
        let dp2 = p2 - p0;

        let duv1 = uv1 - uv0;
        let duv2 = uv2 - uv0;

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

    for (i, v) in mesh.vertices.iter_mut().enumerate() {
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
    mesh
}

fn load_image(mut reader: impl Read) -> DynamicImage {
    let mut buf = vec![];
    reader.read_to_end(&mut buf).unwrap();
    image::load_from_memory(&buf).unwrap()
}
