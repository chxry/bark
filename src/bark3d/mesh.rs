use crate::assets::{AssetId, Handle};
use crate::cast_bytes_slice;
use glam::{Vec2, Vec3, Vec4};
use std::collections::HashMap;
use std::mem;
use std::num::NonZero;
use tracing::debug;

pub struct MeshManager {
    pub vertex_buffer: wgpu::Buffer,
    pub vertex_end: wgpu::BufferAddress,
    pub index_buffer: wgpu::Buffer,
    pub index_end: wgpu::BufferAddress,
    pub handles: HashMap<AssetId, MeshHandle>,
}

impl MeshManager {
    pub fn new(device: &wgpu::Device) -> Self {
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

    pub fn process_handles(
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

    pub fn get_handle(&self, handle: &Handle<Mesh>) -> &MeshHandle {
        self.handles.get(&handle.id()).unwrap()
    }
}

pub struct MeshHandle {
    pub mesh: Handle<Mesh>,
    pub vertex_start: wgpu::BufferAddress,
    pub index_start: wgpu::BufferAddress,
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
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

#[repr(C)]
pub struct Vertex {
    pub pos: Vec3,
    pub uv: Vec2,
    pub normal: Vec3,
    pub tangent: Vec4,
}
