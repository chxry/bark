use super::{DEFAULT_BUFFER_SIZE, RenderContext, RenderFrame, extend_buffer};
use crate::assets::{Assets, Handle};
use crate::bark3d::model::Model; // TEMP
use crate::ecs::{Res, ResMut};
use crate::math::{Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::mem;
use std::ops::Range;
use tracing::debug;

#[derive(Copy, Clone)]
pub struct MeshHandle(u32);

pub struct MeshManager {
    pub vertex_buffer: wgpu::Buffer,
    vertex_end: wgpu::BufferAddress,
    pub index_buffer: wgpu::Buffer,
    index_end: wgpu::BufferAddress,
    allocations: Vec<MeshSlot>,
    asset_map: HashMap<(String, usize), MeshHandle>,
}

impl MeshManager {
    pub fn new(device: &wgpu::Device) -> Self {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: DEFAULT_BUFFER_SIZE,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: DEFAULT_BUFFER_SIZE,
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
            allocations: vec![],
            asset_map: HashMap::new(),
        }
    }

    pub fn add(&mut self, id: &str, index: usize) -> MeshHandle {
        *self
            .asset_map
            .entry((id.to_owned(), index))
            .or_insert_with(|| {
                self.allocations
                    .push(MeshSlot::Pending(id.to_owned(), index));
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
        let mut upload_vertex_size = 0;
        let mut upload_index_size = 0;
        for (i, slot) in self.allocations.iter_mut().enumerate() {
            match slot {
                MeshSlot::Pending(id, index) => {
                    *slot = MeshSlot::PendingAsset(assets.load(id), *index)
                }
                MeshSlot::PendingAsset(handle, index) if let Some(model) = handle.try_get() => {
                    debug!("upload mesh {:?}/{}", handle.id(), index);
                    to_upload.push(i);
                    let mesh = &model.meshes[*index];
                    upload_vertex_size += mesh.vertex_data.len();
                    upload_index_size += mesh.index_data.len();
                }
                _ => {}
            }
        }

        if to_upload.is_empty() {
            None
        } else {
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

            let vertex_base = self.vertex_end;
            let mut vertex_view = extend_buffer(
                device,
                queue,
                &mut encoder,
                &mut self.vertex_buffer,
                vertex_base,
                upload_vertex_size as _,
            );
            let index_base = self.index_end;
            let mut index_view = extend_buffer(
                device,
                queue,
                &mut encoder,
                &mut self.index_buffer,
                index_base,
                upload_index_size as _,
            );

            for i in to_upload {
                let MeshSlot::PendingAsset(handle, index) = &self.allocations[i] else {
                    panic!();
                };
                let mesh = &handle.get().meshes[*index];

                let allocation = MeshAllocation {
                    vertex_start: self.vertex_end,
                    vertex_len: mesh.vertex_data.len() as _,
                    index_start: self.index_end,
                    index_len: mesh.index_data.len() as _,
                };
                self.vertex_end += allocation.vertex_len;
                self.index_end += allocation.index_len;
                vertex_view
                    .slice(
                        (allocation.vertex_start - vertex_base) as usize
                            ..(self.vertex_end - vertex_base) as _,
                    )
                    .copy_from_slice(&mesh.vertex_data);
                index_view
                    .slice(
                        (allocation.index_start - index_base) as usize
                            ..(self.index_end - index_base) as _,
                    )
                    .copy_from_slice(&mesh.index_data);

                self.allocations[i] = MeshSlot::Uploaded(allocation);
            }
            Some(encoder.finish())
        }
    }
}

enum MeshSlot {
    Pending(String, usize),
    PendingAsset(Handle<Model>, usize),
    Uploaded(MeshAllocation),
}

pub struct MeshAllocation {
    pub vertex_start: wgpu::BufferAddress,
    pub vertex_len: wgpu::BufferAddress,
    pub index_start: wgpu::BufferAddress,
    pub index_len: wgpu::BufferAddress,
}

impl MeshAllocation {
    pub fn vertex_range(&self) -> Range<u32> {
        (self.vertex_start / mem::size_of::<Vertex>() as wgpu::BufferAddress) as _
            ..((self.vertex_start + self.vertex_len)
                / mem::size_of::<Vertex>() as wgpu::BufferAddress) as _
    }

    pub fn index_range(&self) -> Range<u32> {
        (self.index_start / mem::size_of::<Index>() as wgpu::BufferAddress) as _
            ..((self.index_start + self.index_len) / mem::size_of::<Index>() as wgpu::BufferAddress)
                as _
    }
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

#[derive(Serialize, Deserialize)]
pub struct Mesh {
    #[serde(with = "serde_bytes")]
    pub vertex_data: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub index_data: Vec<u8>,
}

pub type Index = u32;
pub const INDEX_FORMAT: wgpu::IndexFormat = wgpu::IndexFormat::Uint32;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vertex {
    pub pos: Vec3,
    pub uv: Vec2,
    pub normal: Vec3,
    pub tangent: Vec4,
}

impl Vertex {
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: mem::size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32x4],
    };
}
