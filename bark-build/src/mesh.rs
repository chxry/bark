use crate::{AssetProcessor, AssetProcessorContext};
use bark::cast_bytes_vec;
use bark::gfx::mesh::{Index, Mesh, Vertex};
use bark::math::{Vec2, Vec3, Vec4};

pub struct MeshProcessor;

impl AssetProcessor for MeshProcessor {
    type Options = ();

    fn process(&self, ctx: AssetProcessorContext, _: Self::Options) {
        let obj = obj::load_obj::<obj::TexturedVertex, _, Index>(ctx.src_data).unwrap();
        let vertices = obj
            .vertices
            .into_iter()
            .map(|v| Vertex {
                pos: Vec3::from(v.position),
                uv: Vec2::new(v.texture[0], 1.0 - v.texture[1]),
                normal: Vec3::from(v.normal),
                tangent: Vec4::ZERO,
            })
            .collect::<Vec<_>>();

        let mesh = process_mesh(vertices, obj.indices);
        mesh.write(ctx.emit_main());
    }
}

pub fn process_mesh(mut vertices: Vec<Vertex>, indices: Vec<Index>) -> Mesh {
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
    unsafe {
        Mesh {
            vertex_data: cast_bytes_vec(vertices),
            index_data: cast_bytes_vec(indices),
        }
    }
}
