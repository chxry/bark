use crate::app::ResizeEvent;
use crate::assets::{self, AssetId, Assets, Handle};
use crate::ecs::World;
use crate::{app, cast_bytes, cast_bytes_slice, gfx, intersect};
use glam::{Mat4, Vec2, Vec3};
use image::DynamicImage;
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read};
use std::mem;
use std::num::NonZero;
use tracing::{debug, error, trace};
use wgpu::util::DeviceExt;

pub fn start(_: &mut World) {}

pub fn init(world: &mut World) {
    world.queue_system(app::init);
    world.queue_system(gfx::init);
    world.queue_system(assets::init);
    world.queue_system(init_pipeline);
    world.queue_system(start);
}

pub struct Transform {
    pub mat: Mat4,
}

pub struct MeshRenderer {
    pub mesh: Handle<Mesh>,
    pub texture: Handle<DynamicImage>,
}

struct RenderPipeline {
    frame_globals: FrameGlobals,
    texture_manager: TextureManager,
    mesh_manager: MeshManager,
    pipeline: wgpu::RenderPipeline,
}

fn init_pipeline(world: &mut World) {
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
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
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

    let camera = Mat4::perspective_rh(
        1.2,
        frame.surface.texture.width() as f32 / frame.surface.texture.height() as f32,
        0.01,
        100.0,
    ) * Mat4::look_at_rh(Vec3::new(5.0, 2.0, 5.0), Vec3::new(0.0, 2.0, 0.0), Vec3::Y);
    renderer
        .queue
        .write_buffer(&pipeline.frame_globals.buffer, 0, cast_bytes(&camera));

    let objects =
        intersect(world.get::<Transform>(), world.get::<MeshRenderer>()).collect::<Vec<_>>();

    pipeline.texture_manager.process_handles(
        &renderer.device,
        &renderer.queue,
        objects.iter().map(|o| o.1.1.texture.clone()).collect(),
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
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
    if !objects.is_empty() {
        main_pass.set_pipeline(&pipeline.pipeline);
        main_pass.set_bind_group(0, &pipeline.frame_globals.bind_group, &[]);
        main_pass.set_bind_group(1, &pipeline.texture_manager.bind_group, &[]);
        main_pass.set_vertex_buffer(0, pipeline.mesh_manager.vertex_buffer.slice(..));
        main_pass.set_index_buffer(
            pipeline.mesh_manager.index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        for (_, (transform, mesh)) in objects {
            main_pass.set_push_constants(
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                0,
                cast_bytes(&GPUObject {
                    transform: transform.mat,
                    texture_id: pipeline.texture_manager.get_slot(&mesh.texture),
                }),
            );
            let mesh_handle = pipeline.mesh_manager.get_handle(&mesh.mesh);
            main_pass.draw_indexed(
                mesh_handle.index_start as _..mesh.mesh.indices.len() as _,
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
            size: mem::size_of::<Mat4>() as _,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
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

        let active_ids = handles.iter().map(AssetId::of).collect::<HashSet<_>>();
        for s in &mut self.slots {
            if let Some(slot) = s {
                let id = AssetId::of(&slot.image);
                if !active_ids.contains(&id) {
                    trace!("free texture {:?}", id);
                    *s = None;
                    bindings_changed = true;
                }
            }
        }

        let mut i = 0;
        for h in handles {
            let id = AssetId::of(&h);
            if !self.handle_map.contains_key(&id) {
                trace!("uploading texture {:?}", id);
                while self.slots[i].is_some() {
                    i += 1;
                }
                let texture = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: h.width(),
                            height: h.height(),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::default(),
                    &h.to_rgba8(),
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
        *self.handle_map.get(&AssetId::of(handle)).unwrap()
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
                let id = AssetId::of(h);
                (!self.handles.contains_key(&AssetId::of(h))).then_some((id, h))
            })
            .collect::<HashMap<_, _>>();
        if !to_upload.is_empty() {
            let mut vertex_resize = 0;
            let mut index_resize = 0;
            for m in to_upload.values() {
                vertex_resize +=
                    (m.vertices.len() * mem::size_of::<Vertex>()) as wgpu::BufferAddress;
                index_resize += (m.indices.len() * mem::size_of::<u16>()) as wgpu::BufferAddress;
            }

            let mut vertex_view = grow_buffer(
                device,
                queue,
                encoder,
                &mut self.vertex_buffer,
                self.vertex_end,
                vertex_resize,
            );
            let mut index_view = grow_buffer(
                device,
                queue,
                encoder,
                &mut self.index_buffer,
                self.index_end,
                index_resize,
            );

            for (id, m) in to_upload {
                trace!("uploading mesh {:?}", id);
                let handle = MeshHandle {
                    mesh: m.clone(),
                    vertex_start: self.vertex_end,
                    index_start: self.index_end,
                };
                self.vertex_end +=
                    (m.vertices.len() * mem::size_of::<Vertex>()) as wgpu::BufferAddress;
                self.index_end += (m.indices.len() * mem::size_of::<u16>()) as wgpu::BufferAddress;

                vertex_view[handle.vertex_start as _..self.vertex_end as _]
                    .copy_from_slice(cast_bytes_slice(&m.vertices));
                index_view[handle.index_start as _..self.index_end as _]
                    .copy_from_slice(cast_bytes_slice(&m.indices));

                self.handles.insert(id, handle);
            }
        }
    }

    fn get_handle(&self, handle: &Handle<Mesh>) -> &MeshHandle {
        self.handles.get(&AssetId::of(handle)).unwrap()
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
        let new_buffer = resized_buffer(device, buffer, buffer.size() + needed);
        encoder.copy_buffer_to_buffer(buffer, 0, &new_buffer, 0, buffer.size());
        *buffer = new_buffer;
    }
    queue
        .write_buffer_with(buffer, current_usage, NonZero::new(buffer.size()).unwrap())
        .unwrap()
}

#[repr(C, align(16))]
struct GPUObject {
    transform: Mat4,
    texture_id: u32,
}

pub struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
}

#[repr(C, align(16))]
struct Vertex {
    pos: Vec3,
    uv: Vec2,
}

fn load_mesh(reader: impl Read) -> Mesh {
    let obj = obj::load_obj(BufReader::new(reader)).unwrap();
    Mesh {
        vertices: obj
            .vertices
            .into_iter()
            .map(|v: obj::TexturedVertex| Vertex {
                pos: Vec3::from(v.position),
                uv: Vec2::new(v.texture[0], 1.0 - v.texture[1]),
            })
            .collect(),
        indices: obj.indices,
    }
}

fn load_image(mut reader: impl Read) -> DynamicImage {
    let mut buf = vec![];
    reader.read_to_end(&mut buf).unwrap();
    image::load_from_memory(&buf).unwrap()
}
