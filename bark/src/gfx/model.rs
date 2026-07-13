use super::mesh::Mesh;
use crate::assets::Asset;
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Serialize, Deserialize)]
pub struct Model {
    // todo: bad idea
    pub meshes: Vec<Mesh>,
}

impl Asset for Model {
    fn read<R: Read>(reader: R) -> Self {
        bincode::deserialize_from(reader).unwrap()
    }
}
