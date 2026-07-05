use super::{DEFAULT_BUFFER_SIZE, RenderContext, RenderFrame, extend_buffer};
use crate::assets::{Asset, Handle};
use crate::ecs::{Res, ResMut};
use crate::math::{Vec2, Vec3, Vec4};
use std::collections::HashMap;
use std::io::{Read, Write};
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
    asset_map: HashMap<String, MeshHandle>,
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

    pub fn add(&mut self, handle: Handle<Mesh>) -> MeshHandle {
        *self
            .asset_map
            .entry(handle.id().to_owned())
            .or_insert_with(|| {
                self.allocations.push(MeshSlot::Pending(handle));
                MeshHandle((self.allocations.len() - 1) as _)
            })
    }

    pub fn get(&self, handle: &MeshHandle) -> Option<&MeshAllocation> {
        self.allocations[handle.0 as usize].get_allocation()
    }

    fn upload_pending(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let mut to_upload = vec![];
        let mut upload_vertex_size = 0;
        let mut upload_index_size = 0;
        for (i, slot) in self.allocations.iter_mut().enumerate() {
            if let MeshSlot::Pending(handle) = slot
                && let Some(mesh) = handle.try_get()
            {
                debug!("upload mesh {:?}", handle.id());
                to_upload.push(i);
                upload_vertex_size += mesh.vertex_data.len();
                upload_index_size += mesh.index_data.len();
            }
        }

        if !to_upload.is_empty() {
            let mut vertex_view = extend_buffer(
                device,
                queue,
                encoder,
                &mut self.vertex_buffer,
                self.vertex_end,
                upload_vertex_size as _,
            );
            let mut index_view = extend_buffer(
                device,
                queue,
                encoder,
                &mut self.index_buffer,
                self.index_end,
                upload_index_size as _,
            );

            for i in to_upload {
                let mesh = self.allocations[i].get_pending().unwrap().get();
                let allocation = MeshAllocation {
                    vertex_start: self.vertex_end,
                    vertex_len: mesh.vertex_data.len() as _,
                    index_start: self.index_end,
                    index_len: mesh.index_data.len() as _,
                };
                self.vertex_end += allocation.vertex_len;
                self.index_end += allocation.index_len;
                vertex_view
                    .slice(allocation.vertex_start as usize..self.vertex_end as _)
                    .copy_from_slice(&mesh.vertex_data);
                index_view
                    .slice(allocation.index_start as usize..self.index_end as _)
                    .copy_from_slice(&mesh.index_data);

                self.allocations[i] = MeshSlot::Uploaded(allocation);
            }
        }
    }
}

enum MeshSlot {
    Pending(Handle<Mesh>),
    Uploaded(MeshAllocation),
}

impl MeshSlot {
    fn get_pending(&self) -> Option<&Handle<Mesh>> {
        match self {
            Self::Pending(h) => Some(h),
            _ => None,
        }
    }

    fn get_allocation(&self) -> Option<&MeshAllocation> {
        match self {
            Self::Uploaded(a) => Some(a),
            _ => None,
        }
    }
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
    mut frame: ResMut<RenderFrame>,
    mut meshes: ResMut<MeshManager>,
) {
    let Some(frame) = frame.as_mut() else {
        return;
    };

    meshes.upload_pending(&ctx.device, &ctx.queue, &mut frame.encoder);
}

pub struct Mesh {
    pub vertex_data: Vec<u8>,
    pub index_data: Vec<u8>,
}

impl Mesh {
    pub fn vertex_count(&self) -> u32 {
        (self.vertex_data.len() / mem::size_of::<Vertex>()) as _
    }

    pub fn index_count(&self) -> u32 {
        (self.index_data.len() / mem::size_of::<Index>()) as _
    }

    pub fn write<W: Write>(&self, mut writer: W) {
        writer
            .write_all(&self.vertex_count().to_le_bytes())
            .unwrap();
        writer.write_all(&self.index_count().to_le_bytes()).unwrap();
        writer.write_all(&self.vertex_data).unwrap();
        writer.write_all(&self.index_data).unwrap();
    }
}

impl Asset for Mesh {
    fn read<R: Read>(mut reader: R) -> Self {
        let mut header = [0; 8];
        reader.read_exact(&mut header).unwrap();

        let vertex_count = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let index_count = u32::from_le_bytes(header[4..8].try_into().unwrap());

        let mut vertex_data = vec![0; vertex_count as usize * mem::size_of::<Vertex>()];
        let mut index_data = vec![0; index_count as usize * mem::size_of::<Index>()];
        reader.read_exact(&mut vertex_data).unwrap();
        reader.read_exact(&mut index_data).unwrap();

        Self {
            vertex_data,
            index_data,
        }
    }
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
