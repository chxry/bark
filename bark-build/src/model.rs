use crate::{AssetProcessor, build_warn, create_frag};
use bark::bark3d::Transform;
use bark::bark3d::model::{AnimChannel, Animation, Bone, MAX_BONES, Skeleton};
use bark::cast_bytes_vec;
use bark::gfx::mesh::{Mesh, SkinnedVertex, StaticVertex};
use bark::math::{Mat4, Quat, UVec4, Vec2, Vec3, Vec4};
use russimp_ng::animation::Quaternion;
use russimp_ng::node::Node;
use russimp_ng::scene::{PostProcess, Scene};
use russimp_ng::{Matrix4x4, Vector3D};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;

pub struct ModelProcessor;

#[derive(Serialize, Deserialize)]
pub struct ModelOptions {
    skinned: bool,
}

impl AssetProcessor for ModelProcessor {
    type Options = ModelOptions;

    fn process(&self, src_data: &[u8], src_path: PathBuf, out_path: PathBuf, opts: Self::Options) {
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

        let mut bone_names = HashMap::new();
        let mut bones = vec![];
        if opts.skinned {
            traverse_nodes(
                &mut bone_names,
                &mut bones,
                scene.root.as_ref().unwrap(),
                None,
            );
            if bones.len() > MAX_BONES as _ {
                build_warn(&format!(
                    "model {:?} has {} bones, exceeding limit {}",
                    src_path,
                    bones.len(),
                    MAX_BONES
                ));
                panic!();
            }
        }

        for mesh in scene.meshes {
            let mut vertices = Vec::with_capacity(mesh.vertices.len());
            let mut indices = Vec::with_capacity(mesh.faces.len() * 3);
            for i in 0..mesh.vertices.len() {
                let pos = convert_vec3(mesh.vertices[i]);
                let uv = mesh.texture_coords[0].as_ref().unwrap()[i];
                let normal = convert_vec3(mesh.normals[i]);
                let tangent = convert_vec3(mesh.tangents[i]);
                let bitangent = convert_vec3(mesh.bitangents[i]);

                let handedness = if normal.cross(tangent).dot(bitangent) < 0.0 {
                    -1.0
                } else {
                    1.0
                };

                vertices.push(StaticVertex {
                    pos,
                    uv: Vec2::new(uv.x, 1.0 - uv.y),
                    normal,
                    tangent: tangent.extend(handedness),
                });
            }
            for f in &mesh.faces {
                indices.extend(f.0.clone());
            }
            let mesh_asset = if opts.skinned {
                let mut vertices = vertices
                    .iter()
                    .map(|v| SkinnedVertex {
                        pos: v.pos,
                        uv: v.uv,
                        tangent: v.tangent,
                        normal: v.normal,
                        bone_indices: UVec4::ZERO,
                        bone_weights: Vec4::ZERO,
                    })
                    .collect::<Vec<_>>();

                for bone in mesh.bones {
                    let bone_idx = bone_names.get(&bone.name).unwrap();
                    bones[*bone_idx as usize].offset = convert_mat4(bone.offset_matrix);
                    for weight in &bone.weights {
                        let vertex = &mut vertices[weight.vertex_id as usize];
                        let mut i = 0;
                        while i < 4 {
                            if vertex.bone_indices[i] != 0 {
                                i += 1;
                                continue;
                            }
                            vertex.bone_indices[i] = (bone_idx + 1) as _;
                            vertex.bone_weights[i] = weight.weight;
                            break;
                        }
                    }
                }

                unsafe {
                    Mesh {
                        vertex_data: cast_bytes_vec(vertices),
                        index_data: cast_bytes_vec(indices),
                        vertex_stride: mem::size_of::<SkinnedVertex>() as _,
                    }
                }
            } else {
                unsafe {
                    Mesh {
                        vertex_data: cast_bytes_vec(vertices),
                        index_data: cast_bytes_vec(indices),
                        vertex_stride: mem::size_of::<StaticVertex>() as _,
                    }
                }
            };

            bincode::serialize_into(create_frag(&out_path, &mesh.name), &mesh_asset).unwrap();
        }

        if opts.skinned {
            let skeleton = Skeleton { bones };
            bincode::serialize_into(create_frag(&out_path, "skel"), &skeleton).unwrap();

            for anim in scene.animations {
                let mut animation = Animation {
                    channels: HashMap::new(),
                };

                for channel in anim.channels {
                    let idx = bone_names.get(&channel.name).unwrap();

                    animation.channels.insert(
                        *idx,
                        AnimChannel {
                            positions: channel
                                .position_keys
                                .iter()
                                .map(|k| (k.time as _, convert_vec3(k.value)))
                                .collect(),
                            rotations: channel
                                .rotation_keys
                                .iter()
                                .map(|k| (k.time as _, convert_quat(k.value)))
                                .collect(),
                            scales: channel
                                .scaling_keys
                                .iter()
                                .map(|k| (k.time as _, convert_vec3(k.value)))
                                .collect(),
                        },
                    );
                }

                bincode::serialize_into(create_frag(&out_path, &anim.name), &animation).unwrap();
            }
        }
    }
}

fn traverse_nodes(
    bone_names: &mut HashMap<String, u32>,
    bones: &mut Vec<Bone>,
    node: &Node,
    parent: Option<u32>,
) {
    let idx = bones.len() as _;
    bone_names.insert(node.name.clone(), idx);
    bones.push(Bone {
        parent,
        default_transform: Transform::from_mat4(convert_mat4(node.transformation)),
        offset: Mat4::IDENTITY,
    });
    for node in node.children.borrow().iter() {
        traverse_nodes(bone_names, bones, node, Some(idx));
    }
}

fn convert_mat4(m: Matrix4x4) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(m.a1, m.b1, m.c1, m.d1),
        Vec4::new(m.a2, m.b2, m.c2, m.d2),
        Vec4::new(m.a3, m.b3, m.c3, m.d3),
        Vec4::new(m.a4, m.b4, m.c4, m.d4),
    )
}

fn convert_vec3(v: Vector3D) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn convert_quat(q: Quaternion) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}
