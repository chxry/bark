use std::collections::HashSet;

use crate::app::{self, App};
use crate::assets::Handle;
use crate::ecs::{IntoSystem, Query, Res, ResMut};
use crate::gfx::mesh::Mesh;
use crate::gfx::texture::{Texture, TextureManager};
use crate::gfx::{self, RenderContext, RenderFrame};
use crate::math::{Quat, Vec3};

pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
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

pub struct RenderObject {
    pub mesh: Handle<Mesh>,
    pub texture: Handle<Texture>,
}

pub fn init(app: &mut App) {
    app.world.insert_system(
        app::Render,
        main_pass.after(gfx::begin_frame).before(gfx::submit_frame),
    );
}

pub fn main_pass(
    ctx: Res<RenderContext>,
    mut frame: ResMut<RenderFrame>,
    mut textures: ResMut<TextureManager>,
    mut scene: Query<(&Transform, &RenderObject)>,
) {
    let Some(frame) = (*frame).as_mut() else {
        return;
    };

    frame
        .encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.surface_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.5,
                        g: 0.6,
                        b: 0.8,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
            label: None,
        });

    let mut scene_textures = HashSet::new();

    for (_, (transform, obj)) in scene.iter() {
        scene_textures.insert(obj.texture.clone());
    }

    textures.process_sources(&ctx.device, &ctx.queue, scene_textures);
}
