use super::{DEFAULT_BUFFER_SIZE, RenderContext, RenderFrame, extend_buffer};
use crate::assets::{Asset, Assets, Handle};
use crate::ecs::{Res, ResMut};
use crate::math::{UVec4, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::mem;
use std::ops::Range;
use tracing::debug;

#[derive(Copy, Clone)]
pub struct MeshHandle(u32);

pub struct MeshManager {
    pub vertex_buf: Allocator,
    pub index_buf: Allocator,
    allocations: Vec<MeshSlot>,
    asset_map: HashMap<String, MeshHandle>,
}

impl MeshManager {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            vertex_buf: Allocator::new(device, wgpu::BufferUsages::VERTEX),
            index_buf: Allocator::new(device, wgpu::BufferUsages::INDEX),
            allocations: vec![],
            asset_map: HashMap::new(),
        }
    }

    pub fn add(&mut self, id: &str) -> MeshHandle {
        *self.asset_map.entry(id.to_owned()).or_insert_with(|| {
            self.allocations.push(MeshSlot::PendingId(id.to_owned()));
            MeshHandle((self.allocations.len() - 1) as _)
        })
    }

    pub fn get(&self, handle: &MeshHandle) -> Option<&MeshAllocation> {
        match &self.allocations[handle.0 as usize] {
            MeshSlot::Uploaded(h) => Some(h),
            _ => None,
        }
    }

    fn upload_pending(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets: &mut Assets,
    ) -> Option<wgpu::CommandBuffer> {
        let mut to_upload = vec![];
        let vertex_base = self.vertex_buf.end;
        let index_base = self.index_buf.end;
        for (i, slot) in self.allocations.iter_mut().enumerate() {
            match slot {
                MeshSlot::PendingId(id) => *slot = MeshSlot::PendingAsset(assets.load(id)),
                MeshSlot::PendingAsset(handle) if let Some(mesh) = handle.try_get() => {
                    debug!("upload mesh {:?}", handle.id());
                    to_upload.push((
                        i,
                        MeshAllocation {
                            vertex: self
                                .vertex_buf
                                .allocate(mesh.vertex_data.len() as _, mesh.vertex_stride),
                            index: self
                                .index_buf
                                .allocate(mesh.index_data.len() as _, mem::size_of::<Index>() as _),
                        },
                    ));
                }
                _ => {}
            }
        }

        if to_upload.is_empty() {
            None
        } else {
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

            let mut vertex_view =
                self.vertex_buf
                    .prepare_upload(device, queue, &mut encoder, vertex_base);
            let mut index_view =
                self.index_buf
                    .prepare_upload(device, queue, &mut encoder, index_base);

            for (i, alloc) in to_upload {
                let MeshSlot::PendingAsset(handle) = &self.allocations[i] else {
                    panic!();
                };
                let mesh = &handle.get();
                self.vertex_buf.upload(
                    vertex_view.as_mut().unwrap(),
                    &alloc.vertex,
                    &mesh.vertex_data,
                );
                self.index_buf
                    .upload(index_view.as_mut().unwrap(), &alloc.index, &mesh.index_data);

                self.allocations[i] = MeshSlot::Uploaded(alloc);
            }
            Some(encoder.finish())
        }
    }
}

enum MeshSlot {
    PendingId(String),
    PendingAsset(Handle<Mesh>),
    Uploaded(MeshAllocation),
}

pub struct MeshAllocation {
    pub vertex: Allocation,
    pub index: Allocation,
}

pub fn upload_meshes(
    ctx: Res<RenderContext>,
    frame: Res<RenderFrame>,
    mut meshes: ResMut<MeshManager>,
    mut assets: ResMut<Assets>,
) {
    if let Some(buffer) = meshes.upload_pending(&ctx.device, &ctx.queue, &mut assets) {
        frame.submit(buffer);
    }
}

pub struct Allocator {
    pub buffer: wgpu::Buffer,
    end: wgpu::BufferAddress,
}

impl Allocator {
    fn new(device: &wgpu::Device, usage: wgpu::BufferUsages) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: DEFAULT_BUFFER_SIZE,
            mapped_at_creation: false,
            usage: usage | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        });
        Self { buffer, end: 0 }
    }

    fn allocate(&mut self, len: wgpu::BufferAddress, stride: wgpu::BufferAddress) -> Allocation {
        let start = wgpu::util::align_to(self.end, stride);
        self.end = start + len;
        Allocation { start, len, stride }
    }

    fn prepare_upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        base: wgpu::BufferAddress,
    ) -> Option<wgpu::QueueWriteBufferView> {
        extend_buffer(
            device,
            queue,
            encoder,
            &mut self.buffer,
            base,
            self.end - base,
        )
    }

    fn upload(&mut self, view: &mut wgpu::QueueWriteBufferView, alloc: &Allocation, data: &[u8]) {
        view.slice(alloc.start as usize..(alloc.start + alloc.len) as _)
            .copy_from_slice(data);
    }
}

pub struct Allocation {
    start: wgpu::BufferAddress,
    len: wgpu::BufferAddress,
    stride: wgpu::BufferAddress,
}

impl Allocation {
    pub fn range(&self) -> Range<u32> {
        (self.start / self.stride) as _..((self.start + self.len) / self.stride) as _
    }
}

#[derive(Serialize, Deserialize)]
pub struct Mesh {
    pub vertex_stride: wgpu::BufferAddress,
    #[serde(with = "serde_bytes")]
    pub vertex_data: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub index_data: Vec<u8>,
}

impl Asset for Mesh {
    fn read<R: Read>(reader: R) -> Self {
        bincode::deserialize_from(reader).unwrap()
    }
}

pub type Index = u32;
pub const INDEX_FORMAT: wgpu::IndexFormat = wgpu::IndexFormat::Uint32;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct StaticVertex {
    pub pos: Vec3,
    pub uv: Vec2,
    pub normal: Vec3,
    pub tangent: Vec4,
}

impl StaticVertex {
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: mem::size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32x4],
    };
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkinnedVertex {
    pub pos: Vec3,
    pub uv: Vec2,
    pub normal: Vec3,
    pub tangent: Vec4,
    pub bone_indices: UVec4,
    pub bone_weights: Vec4,
}

impl SkinnedVertex {
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: mem::size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32x4, 4 => Uint32x4, 5 => Float32x4],
    };
}
