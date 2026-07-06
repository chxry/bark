mod render;

use crate::app::{self, App};
use crate::ecs::IntoSystem;
use crate::gfx;
use crate::gfx::mesh::MeshHandle;
use crate::gfx::texture::TextureHandle;
use crate::math::{EulerRot, Mat4, Quat, Vec3};

pub const UP: Vec3 = Vec3::Y;
pub const FORWARD: Vec3 = Vec3::NEG_Z;
pub const RIGHT: Vec3 = Vec3::X;

pub fn init(app: &mut App) {
    app.world
        .insert_system::<app::Startup>(render::init_pipeline.after(gfx::init_renderer));
    app.world
        .insert_system::<app::Render>(render::shadow_pass.with(gfx::during_frame));
    app.world.insert_system::<app::Render>(
        render::main_pass
            .with(gfx::during_frame)
            .after(render::shadow_pass),
    );
    app.world
        .insert_system::<app::Render>(render::extract_lights.with(gfx::during_frame));
    app.world
        .insert_system::<app::Render>(render::extract_frame_globals.with(gfx::during_frame));
    app.world
        .insert_system::<app::ResizeEvent>(render::resize_framebuffer.into_system());
}

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

    pub fn as_transform_mat(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    pub fn as_view_mat(&self) -> Mat4 {
        glam::camera::rh::view::look_to_mat4(self.position, self.rotation * FORWARD, UP)
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

pub struct Camera {
    pub fov: f32,
}

impl Camera {
    pub fn new(fov: f32) -> Self {
        Camera { fov }
    }

    pub fn as_mat(&self, aspect_ratio: f32, transform: &Transform) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fov, aspect_ratio, 0.01, 100.0)
            * transform.as_view_mat()
    }
}

pub struct RenderObject {
    pub mesh: MeshHandle,
    pub diffuse_color: Vec3,
    pub diffuse_tex: Option<TextureHandle>,
    pub normal_tex: Option<TextureHandle>,
    pub pbr: PbrMode,
}

impl RenderObject {
    pub fn new(mesh: MeshHandle) -> Self {
        Self {
            mesh,
            diffuse_color: Vec3::ONE,
            diffuse_tex: None,
            normal_tex: None,
            pbr: PbrMode::Values {
                roughness: 0.5,
                metallic: 0.0,
            },
        }
    }

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
            _ => Vec3::ONE,
        }
    }
}

pub struct DirectionalLight {
    color: Vec3,
    shadows: bool,
}

impl DirectionalLight {
    pub fn color(mut self, color: Vec3) -> Self {
        self.color = color;
        self
    }

    pub fn shadows(mut self, shadows: bool) -> Self {
        self.shadows = shadows;
        self
    }
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            shadows: false,
        }
    }
}
