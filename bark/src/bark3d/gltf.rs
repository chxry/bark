use crate::assets::{Asset, Assets};
use crate::bark3d::{PbrMode, RenderObject, Transform};
use crate::ecs::Commands;
use crate::gfx::{mesh::MeshManager, texture::TextureManager};
use crate::math::Vec3;
use serde::{Deserialize, Serialize};
use std::io::Read;

pub fn spawn_gltf(
    id: &str,
    assets: &mut Assets,
    textures: &mut TextureManager,
    meshes: &mut MeshManager,
    commands: &mut Commands,
) {
    let obj = assets.load_blocking::<GltfObject>(id);
    for node in &obj.get().0 {
        commands
            .spawn()
            .insert(node.transform)
            .insert(RenderObject {
                mesh: meshes.add(assets.load(&node.mesh)),
                diffuse_color: Vec3::ONE,
                diffuse_tex: node
                    .diffuse_tex
                    .as_ref()
                    .map(|x| textures.add(assets.load(x))),
                normal_tex: node
                    .normal_tex
                    .as_ref()
                    .map(|x| textures.add(assets.load(x))),
                pbr: node
                    .pbr_tex
                    .as_ref()
                    .map(|x| PbrMode::Sampled(textures.add(assets.load(x))))
                    .unwrap_or(PbrMode::Values {
                        roughness: 0.5,
                        metallic: 0.0,
                    }),
            });
    }
}

#[derive(Serialize, Deserialize)]
pub struct GltfObject(pub Vec<GltfNode>);

#[derive(Serialize, Deserialize)]
pub struct GltfNode {
    pub transform: Transform,
    pub mesh: String,
    pub diffuse_tex: Option<String>,
    pub normal_tex: Option<String>,
    pub pbr_tex: Option<String>,
}

impl Asset for GltfObject {
    fn read<R: Read>(reader: R) -> Self {
        serde_json::from_reader(reader).unwrap()
    }
}
