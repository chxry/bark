use crate::mesh::process_mesh;
use crate::texture::{TextureOptions, process_texture};
use crate::{AssetProcessor, AssetProcessorContext};
use bark::bark3d::Transform;
use bark::bark3d::gltf::{GltfNode, GltfObject};
use bark::gfx::mesh::Vertex;
use bark::math::{Quat, Vec2, Vec3, Vec4};
use gltf::Gltf;
use gltf::image::Source;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub struct GltfProcessor;

#[derive(Serialize, Deserialize)]
pub struct GltfOptions {
    textures: HashMap<usize, TextureOptions>,
}

impl AssetProcessor for GltfProcessor {
    type Options = GltfOptions;

    fn process(&self, ctx: AssetProcessorContext, opts: Self::Options) {
        let mut gltf = Gltf::from_slice(ctx.src_data).unwrap();
        let blob = gltf.blob.take();
        let buffers =
            gltf::import_buffers(&gltf, Some(ctx.src_path.parent().unwrap()), blob).unwrap();

        for (i, mesh) in gltf.meshes().enumerate() {
            let mut vertices = vec![];
            let mut indices = vec![];
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buf| Some(&buffers[buf.index()]));

                vertices.extend(
                    reader
                        .read_positions()
                        .unwrap()
                        .zip(reader.read_tex_coords(0).unwrap().into_f32())
                        .zip(reader.read_normals().unwrap())
                        .map(|((pos, uv), normal)| Vertex {
                            pos: Vec3::from(pos),
                            uv: Vec2::new(uv[0], uv[1]),
                            normal: Vec3::from(normal),
                            tangent: Vec4::ZERO,
                        }),
                );
                indices.extend(reader.read_indices().unwrap().into_u32());
            }

            let mesh = process_mesh(vertices, indices);
            mesh.write(ctx.emit_sub(&format!("mesh{}", i)));
        }

        for (i, texture_opts) in opts.textures {
            if let Source::View { view, .. } = gltf.textures().nth(i).unwrap().source().source() {
                let buf = &buffers[view.buffer().index()].0;
                let image =
                    image::load_from_memory(&buf[view.offset()..view.offset() + view.length()])
                        .unwrap()
                        .to_rgba8();

                let texture = process_texture(image, &texture_opts);
                texture.write(ctx.emit_sub(&format!("tex{}", i)));
            }
        }
        let mut obj = GltfObject(vec![]);
        for node in gltf.nodes() {
            if let Some(mesh) = node.mesh() {
                let mat = mesh.primitives().next().unwrap().material();
                let (pos, rot, scale) = node.transform().decomposed();
                obj.0.push(GltfNode {
                    transform: Transform {
                        position: pos.into(),
                        rotation: Quat::from_array(rot),
                        scale: scale.into(),
                    },
                    mesh: get_id(ctx.src_path, "mesh", mesh.index()),
                    diffuse_tex: mat
                        .pbr_metallic_roughness()
                        .base_color_texture()
                        .map(|x| get_texture_id(x.texture(), ctx.src_path)),
                    normal_tex: mat
                        .normal_texture()
                        .map(|x| get_texture_id(x.texture(), ctx.src_path)),
                    // todo: generate an arm texture from whatever bs
                    pbr_tex: mat
                        .pbr_metallic_roughness()
                        .metallic_roughness_texture()
                        .map(|x| get_texture_id(x.texture(), ctx.src_path)),
                })
            }
        }
        serde_json::to_writer(ctx.emit_main(), &obj).unwrap();
    }
}

fn get_texture_id(tex: gltf::Texture, src_path: &Path) -> String {
    match tex.source().source() {
        Source::View { .. } => get_id(src_path, "tex", tex.index()),
        Source::Uri { uri, .. } => uri.to_owned(),
    }
}

fn get_id(src_path: &Path, ty: &str, index: usize) -> String {
    // todo: probably wrong
    format!(
        "{}#{}{}",
        src_path.file_name().unwrap().display(),
        ty,
        index
    )
}
