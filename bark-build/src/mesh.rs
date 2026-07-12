use crate::{AssetProcessor, build_warn};
use bark::cast_bytes_vec;
use bark::gfx::mesh::{Mesh, Vertex};
use bark::math::{Vec2, Vec3};
use russimp_ng::scene::{PostProcess, Scene};
use std::fs::File;
use std::path::Path;

pub struct MeshProcessor;

impl AssetProcessor for MeshProcessor {
    type Options = ();

    fn process(&self, src_data: &[u8], src_path: &Path, out: File, _: Self::Options) {
        let post_process_flags = vec![
            PostProcess::CalculateTangentSpace,
            PostProcess::Triangulate,
            PostProcess::JoinIdenticalVertices,
        ];
        let scene = Scene::from_buffer(
            src_data,
            post_process_flags,
            src_path.extension().unwrap().to_str().unwrap(),
        )
        .unwrap();

        if scene.meshes.len() > 1 {
            build_warn(&format!(
                "{:?} has multiple meshes, ignoring extras.",
                src_path.file_name().unwrap()
            ));
        }
        let mesh = &scene.meshes[0];

        let mut vertices = Vec::with_capacity(mesh.vertices.len());
        let mut indices = Vec::with_capacity(mesh.faces.len() * 3);
        for i in 0..mesh.vertices.len() {
            let pos = mesh.vertices[i];
            let uv = mesh.texture_coords[0].as_ref().unwrap()[i];
            let normal = mesh.normals[i];
            let tangent = mesh.tangents[i];
            let bitangent = mesh.bitangents[i];

            let normal = Vec3::new(normal.x, normal.y, normal.z);
            let tangent = Vec3::new(tangent.x, tangent.y, tangent.z);
            let bitangent = Vec3::new(bitangent.x, bitangent.y, bitangent.z);
            let handedness = if normal.cross(tangent).dot(bitangent) < 0.0 {
                -1.0
            } else {
                1.0
            };

            vertices.push(Vertex {
                pos: Vec3::new(pos.x, pos.y, pos.z),
                uv: Vec2::new(uv.x, 1.0 - uv.y),
                normal,
                tangent: tangent.extend(handedness),
            });
        }
        for f in &mesh.faces {
            indices.extend(f.0.clone());
        }

        // safety: `Vertex` and `Index` are valid for `cast_bytes_vec`
        let mesh = unsafe {
            Mesh {
                vertex_data: cast_bytes_vec(vertices),
                index_data: cast_bytes_vec(indices),
            }
        };

        mesh.write(out);
    }
}
