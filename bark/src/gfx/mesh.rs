use crate::assets::{Asset, AssetProcessor};
use crate::cast_bytes_vec;
use crate::math::{Vec2, Vec3, Vec4};
use std::io::{BufReader, Read, Write};
use std::mem;

pub struct Mesh {
    vertex_data: Vec<u8>,
    index_data: Vec<u8>,
}

impl Mesh {
    fn vertex_count(&self) -> u32 {
        (self.vertex_data.len() / mem::size_of::<Vertex>()) as _
    }

    fn index_count(&self) -> u32 {
        (self.index_data.len() / mem::size_of::<u32>()) as _
    }

    fn write<W: Write>(&self, mut writer: W) {
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
        let mut index_data = vec![0; index_count as usize * mem::size_of::<u32>()];
        reader.read_exact(&mut vertex_data).unwrap();
        reader.read_exact(&mut index_data).unwrap();

        Self {
            vertex_data,
            index_data,
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

        // safety: `Vertex` is valid for `cast_bytes_vec`
        let mesh = unsafe {
            Mesh {
                vertex_data: cast_bytes_vec(vertices),
                index_data: cast_bytes_vec(indices),
            }
        };
        mesh.write(out);
    }
}
