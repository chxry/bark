use super::Transform;
use crate::assets::Asset;
use crate::gfx::mesh::Mesh;
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Serialize, Deserialize)]
pub struct Model {
    // todo: bad idea
    pub nodes: Vec<ModelNode>,
    pub meshes: Vec<Mesh>,
}

impl Asset for Model {
    fn read<R: Read>(reader: R) -> Self {
        bincode::deserialize_from(reader).unwrap()
    }
}

#[derive(Serialize, Deserialize)]
pub struct ModelNode {
    parent: Option<u32>,
    transform: Transform,
}
