use crate::assets::{Asset, AssetProcessor};
use crate::math::{Vec2, Vec3, Vec4};
use memmap2::Mmap;
use std::io::{BufReader, Read, Write};

pub struct Mesh {
    pub header: MeshHeader,
    pub data: Mmap,
}

impl Asset for Mesh {
    fn read(data: Mmap) -> Self {
        Self {
            header: MeshHeader::read(&*data),
            data,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vertex {
    pub pos: Vec3,
    pub uv: Vec2,
    pub normal: Vec3,
    pub tangent: Vec4,
}

pub struct MeshHeader {
    pub vertices_len: u32,
    pub indices_len: u32,
}

impl MeshHeader {
    pub fn write<W: Write>(&self, mut writer: W) {
        writer.write_all(&self.vertices_len.to_le_bytes()).unwrap();
        writer.write_all(&self.indices_len.to_le_bytes()).unwrap();
    }

    pub fn read<R: Read>(mut reader: R) -> Self {
        let mut buf = [0; 8];
        reader.read_exact(&mut buf).unwrap();
        Self {
            vertices_len: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            indices_len: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
        }
    }
}

pub struct MeshProcessor;

impl AssetProcessor for MeshProcessor {
    type Options = ();

    fn process<R: Read, W: Write>(&self, src: R, out: W, _: Self::Options) {
        let obj = obj::load_obj::<obj::TexturedVertex, _, u32>(BufReader::new(src)).unwrap();
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

        let header = MeshHeader {
            vertices_len: vertices.len() as _,
            indices_len: indices.len() as _,
        };
        header.write(out);

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
    }
}
