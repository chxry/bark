use super::Transform;
use crate::assets::Asset;
use crate::math::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;

pub const MAX_BONES: u32 = 100;

#[derive(Clone, Serialize, Deserialize)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

impl Asset for Skeleton {
    fn read<R: Read>(reader: R) -> Self {
        bincode::deserialize_from(reader).unwrap()
    }
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct Bone {
    pub parent: Option<u32>,
    pub default_transform: Transform,
    pub offset: Mat4, // todo: only needed for deform bones
}

#[derive(Serialize, Deserialize)]
pub struct AnimationClip {
    pub duration_ticks: u32,
    pub ticks_per_second: f32,
    pub channels: HashMap<u32, AnimChannel>,
}

impl Asset for AnimationClip {
    fn read<R: Read>(reader: R) -> Self {
        bincode::deserialize_from(reader).unwrap()
    }
}

#[derive(Serialize, Deserialize)]
pub struct AnimChannel {
    pub positions: Vec<(u32, Vec3)>,
    pub rotations: Vec<(u32, Quat)>,
    pub scales: Vec<(u32, Vec3)>,
}
