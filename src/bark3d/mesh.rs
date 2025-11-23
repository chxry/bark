use crate::assets::Handle;
use crate::cast_bytes_slice;
use glam::{Vec2, Vec3, Vec4};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read};
use std::mem;
use std::num::NonZero;
use tracing::debug;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MeshSource {
    asset: Handle<Mesh>,
}

impl MeshSource {
    pub fn new(asset: Handle<Mesh>) -> Self {
        Self { asset }
    }

    pub fn ready(&self) -> bool {
        self.asset.loaded()
    }
}

pub struct MeshManager {
    pub vertex_buffer: wgpu::Buffer,
    pub vertex_end: wgpu::BufferAddress,
    pub index_buffer: wgpu::Buffer,
    pub index_end: wgpu::BufferAddress,
    pub source_map: HashMap<MeshSource, MeshHandle>,
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
            source_map: HashMap::new(),
        }
    }

    pub fn process_sources(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        sources: HashSet<MeshSource>,
    ) {
        let mut to_upload = vec![];
        for source in sources {
            if !self.source_map.contains_key(&source) {
                to_upload.push(source);
            }
        }

        if !to_upload.is_empty() {
            let mut vertex_resize = 0;
            let mut index_resize = 0;
            for source in &to_upload {
                let mesh = source.asset.get();
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

            for source in to_upload {
                debug!("upload mesh {:?}", source.asset);
                let mesh = source.asset.get();

                let handle = MeshHandle {
                    vertex_start: self.vertex_end,
                    index_start: self.index_end,
                    index_count: mesh.indices.len() as _,
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

                self.source_map.insert(source, handle);
            }
        }
    }

    pub fn get_handle(&self, source: &MeshSource) -> Option<MeshHandle> {
        self.source_map.get(source).copied()
    }
}

#[derive(Copy, Clone)]
pub struct MeshHandle {
    pub vertex_start: wgpu::BufferAddress,
    pub index_start: wgpu::BufferAddress,
    pub index_count: u32,
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

pub fn load_mesh(reader: impl Read) -> Mesh {
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
