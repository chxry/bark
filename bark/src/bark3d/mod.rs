pub mod animation;
pub mod model;
mod render;

use self::animation::AnimationHandle;
use crate::app::{self, App};
use crate::ecs::{Commands, EntityId, IntoSystem, Query};
use crate::gfx;
use crate::gfx::mesh::MeshHandle;
use crate::gfx::texture::TextureHandle;
use crate::math::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub const UP: Vec3 = Vec3::Y;
pub const FORWARD: Vec3 = Vec3::NEG_Z;
pub const RIGHT: Vec3 = Vec3::X;

pub fn init(app: &mut App) {
    app.world
        .insert_system::<app::Startup>(render::init_pipeline.after(gfx::init_renderer));

    app.world
        .insert_system::<app::Render>(render::sky_pass.with(gfx::during_frame));
    app.world.insert_system::<app::Render>(
        render::shadow_pass
            .with(gfx::during_frame)
            // .after(render::extract_skeletons) todo: very concerning
            .after(propagate_transforms),
    );
    app.world.insert_system::<app::Render>(
        render::main_pass
            .with(gfx::during_frame)
            .after(render::shadow_pass)
            .after(render::sky_pass)
            // .after(render::extract_skeletons)
            .after(propagate_transforms),
    );

    app.world.insert_system::<app::Render>(
        render::extract_lights
            .with(gfx::during_frame)
            .after(propagate_transforms),
    );
    app.world.insert_system::<app::Render>(
        render::extract_frame_globals
            .with(gfx::during_frame)
            .after(propagate_transforms),
    );
    app.world
        .insert_system::<app::Render>(animation::extract_skeletons.with(gfx::during_frame));

    app.world
        .insert_system::<app::Render>(propagate_transforms.into_system());
    app.world
        .insert_system::<app::ResizeEvent>(render::resize_framebuffer.into_system());

    app.world
        .insert_system::<app::Update>(animation::progress_animations.into_system());

    app.world.insert_resource(SkySettings {
        sun_dir: Vec3::new(1.0, -1.0, 1.0).normalize(),
    })
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub fn position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    pub fn rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn rotation_euler(mut self, yaw: f32, pitch: f32, roll: f32) -> Self {
        self.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
        self
    }

    pub fn scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    pub fn as_mat4(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    pub fn from_mat4(mat: Mat4) -> Self {
        let (scale, rotation, position) = mat.to_scale_rotation_translation();
        Self {
            scale,
            position,
            rotation,
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

pub struct GlobalTransform(Mat4);

impl GlobalTransform {
    pub fn position(&self) -> Vec3 {
        self.0.to_scale_rotation_translation().2
    }

    pub fn rotation(&self) -> Quat {
        self.0.to_scale_rotation_translation().1
    }

    pub fn scale(&self) -> Vec3 {
        self.0.to_scale_rotation_translation().0
    }
}

pub struct Parent(pub EntityId);

pub fn propagate_transforms(
    transforms: Query<(&Transform,)>,
    parented: Query<(&Transform, &Parent)>,
    mut commands: Commands,
) {
    // todo: optimize
    for (entity_id, (transform,)) in transforms.iter() {
        let mut transform = transform.as_mat4();
        if let Some((_, parent)) = parented.get(entity_id) {
            let mut parent_id = parent.0;
            loop {
                match parented.get(parent_id) {
                    Some((parent_transform, new_parent)) => {
                        parent_id = new_parent.0;
                        transform = parent_transform.as_mat4() * transform;
                    }
                    None if let Some((parent_transform,)) = transforms.get(parent_id) => {
                        transform = parent_transform.as_mat4() * transform;
                        break;
                    }

                    _ => break,
                }
            }
        }
        commands
            .entity(entity_id)
            .insert(GlobalTransform(transform));
    }
}

pub struct Camera {
    pub fov: f32,
    pub clip_range: Range<f32>,
}

impl Camera {
    pub fn new(fov: f32) -> Self {
        Camera {
            fov,
            clip_range: 0.01..100.0,
        }
    }
}

pub struct StaticMesh(pub MeshHandle);

pub struct SkinnedMesh(pub MeshHandle, pub AnimationHandle);

pub struct Material {
    pub diffuse_color: Vec3,
    pub diffuse_tex: Option<TextureHandle>,
    pub normal_tex: Option<TextureHandle>,
    pub pbr: PbrMode,
}

impl Material {
    pub fn diffuse_color(mut self, color: Vec3) -> Self {
        self.diffuse_color = color;
        self
    }

    pub fn diffuse_texture(mut self, tex: TextureHandle) -> Self {
        self.diffuse_tex = Some(tex);
        self
    }

    pub fn normal_texture(mut self, tex: TextureHandle) -> Self {
        self.normal_tex = Some(tex);
        self
    }

    pub fn pbr_texture(mut self, tex: TextureHandle) -> Self {
        self.pbr = PbrMode::Sampled(tex);
        self
    }

    pub fn pbr_values(mut self, roughness: f32, metallic: f32) -> Self {
        self.pbr = PbrMode::Values {
            roughness,
            metallic,
        };
        self
    }
}

impl Default for Material {
    fn default() -> Self {
        Self {
            diffuse_color: Vec3::ONE,
            diffuse_tex: None,
            normal_tex: None,
            pbr: PbrMode::Values {
                roughness: 0.5,
                metallic: 0.0,
            },
        }
    }
}

#[derive(Clone)]
pub enum PbrMode {
    Sampled(TextureHandle),
    Values { roughness: f32, metallic: f32 },
}

impl PbrMode {
    fn get_tex(&self) -> Option<TextureHandle> {
        match self {
            Self::Sampled(t) => Some(*t),
            _ => None,
        }
    }

    fn get_arm_values(&self) -> Vec3 {
        match self {
            Self::Values {
                roughness,
                metallic,
            } => Vec3::new(1.0, *roughness, *metallic),
            _ => Vec3::new(1.0, 1.0, 0.0),
        }
    }
}

pub struct DirectionalLight {
    pub color: Vec3,
}

impl DirectionalLight {
    pub fn color(mut self, color: Vec3) -> Self {
        self.color = color;
        self
    }
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self { color: Vec3::ONE }
    }
}

pub struct SkySettings {
    pub sun_dir: Vec3,
}
