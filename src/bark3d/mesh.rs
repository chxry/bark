use super::DEFAULT_BUFFER_SIZE;
use crate::assets::Handle;
use crate::cast_bytes_slice;
use crate::gfx::grow_buffer;
use glam::{Vec2, Vec3, Vec4};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read};
use std::mem;
use tracing::debug;

pub struct MeshManager {
    pub vertex_buffer: wgpu::Buffer,
    pub vertex_end: wgpu::BufferAddress,
    pub index_buffer: wgpu::Buffer,
    pub index_end: wgpu::BufferAddress,
    pub handle_map: HashMap<Handle<Mesh>, MeshHandle>,
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
            handle_map: HashMap::new(),
        }
    }

    pub fn process_sources(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        handles: HashSet<Handle<Mesh>>,
    ) {
        let mut to_upload = vec![];
        for source in handles {
            if !self.handle_map.contains_key(&source) {
                to_upload.push(source);
            }
        }

        if !to_upload.is_empty() {
            let mut vertex_resize = 0;
            let mut index_resize = 0;
            for mesh in &to_upload {
                vertex_resize += mesh.vertices.len() as wgpu::BufferAddress;
                index_resize += mesh.indices.len() as wgpu::BufferAddress;
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

            let mut current_vertex_offset = 0;
            let mut current_index_offset = 0;

            for mesh in to_upload {
                debug!("upload mesh {:?}", mesh.id());

                let handle = MeshHandle {
                    vertex_start: self.vertex_end / mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    index_start: self.index_end / mem::size_of::<u32>() as wgpu::BufferAddress,
                    index_count: (mesh.indices.len() / mem::size_of::<u32>()) as _,
                };

                self.vertex_end += mesh.vertices.len() as wgpu::BufferAddress;
                self.index_end += mesh.indices.len() as wgpu::BufferAddress;

                let vertex_end = current_vertex_offset + mesh.vertices.len();
                vertex_view[current_vertex_offset..vertex_end].copy_from_slice(&mesh.vertices);
                current_vertex_offset = vertex_end;

                let index_end = current_index_offset + mesh.indices.len();
                index_view[current_index_offset..index_end].copy_from_slice(&mesh.indices);
                current_index_offset = index_end;

                self.handle_map.insert(mesh, handle);
            }
        }
    }

    pub fn get_handle(&self, handle: &Handle<Mesh>) -> Option<MeshHandle> {
        self.handle_map.get(handle).copied()
    }
}

#[derive(Copy, Clone)]
pub struct MeshHandle {
    pub vertex_start: wgpu::BufferAddress,
    pub index_start: wgpu::BufferAddress,
    pub index_count: u32,
}

pub struct Mesh {
    vertices: Vec<u8>,
    indices: Vec<u8>,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vertex {
    pub pos: Vec3,
    pub uv: Vec2,
    pub normal: Vec3,
    pub tangent: Vec4,
}

pub fn process_mesh(data: &[u8]) -> Mesh {
    let obj = obj::load_obj::<obj::TexturedVertex, _, u32>(data).unwrap();
    let mut vertices = obj
        .vertices
        .into_iter()
        .map(|v| Vertex {
            pos: Vec3::from(v.position),
            uv: Vec2::new(v.texture[0], 1.0 - v.texture[1]),
            normal: Vec3::from(v.normal),
            tangent: Vec4::ZERO,
        })
        .collect::<Vec<_>>();
    let indices = obj.indices;

    let mut tangents = vec![Vec3::ZERO; vertices.len()];
    let mut bitangents = vec![Vec3::ZERO; vertices.len()];

    for tri in indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;

        let v0 = &vertices[i0];
        let v1 = &vertices[i1];
        let v2 = &vertices[i2];

        let dp1 = v1.pos - v0.pos;
        let dp2 = v2.pos - v0.pos;

        let duv1 = v1.uv - v0.uv;
        let duv2 = v2.uv - v0.uv;

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

    for (i, v) in vertices.iter_mut().enumerate() {
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
    Mesh {
        vertices: cast_bytes_slice(&vertices).to_vec(),
        indices: cast_bytes_slice(&indices).to_vec(),
    }
}

pub fn save_mesh(mesh: &Mesh) -> Vec<u8> {
    let mut data = vec![];
    data.extend((mesh.vertices.len() as u32).to_le_bytes());
    data.extend((mesh.indices.len() as u32).to_le_bytes());
    data.extend(&mesh.vertices);
    data.extend(&mesh.indices);
    data
}

pub fn load_mesh(data: &[u8]) -> Mesh {
    let vertices_len = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let indices_len = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let vertex_end = 8 + vertices_len as usize;
    Mesh {
        vertices: data[8..vertex_end].to_vec(),
        indices: data[vertex_end..vertex_end + indices_len as usize].to_vec(),
    }
}
