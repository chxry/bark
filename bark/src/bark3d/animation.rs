use super::model::{AnimationClip, MAX_BONES, Skeleton};
use crate::app::DeltaTime;
use crate::assets::Handle;
use crate::cast_bytes_slice;
use crate::ecs::{Res, ResMut};
use crate::gfx::{DEFAULT_BUFFER_SIZE, RenderContext, RenderFrame, resize_buffer};
use crate::math::{Mat4, Quat, Vec3};
use std::mem;

#[derive(Copy, Clone)]
pub struct AnimationHandle(u32);

pub struct AnimationManager {
    buffer: wgpu::Buffer,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    animations: Vec<AnimationState>,
}

impl AnimationManager {
    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            size: DEFAULT_BUFFER_SIZE,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label: None,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            buffer,
            bind_group_layout,
            bind_group,
            animations: vec![],
        }
    }

    pub fn add(
        &mut self,
        skeleton: Handle<Skeleton>,
        clip: Handle<AnimationClip>,
    ) -> AnimationHandle {
        self.animations.push(AnimationState {
            skeleton,
            clip,
            progress_secs: 0.0,
        });
        AnimationHandle((self.animations.len() - 1) as _)
    }

    pub fn get_id(&self, handle: AnimationHandle) -> u32 {
        handle.0
    }
}

pub struct AnimationState {
    pub skeleton: Handle<Skeleton>,
    pub clip: Handle<AnimationClip>,
    pub progress_secs: f32,
}

pub fn extract_skeletons(
    ctx: Res<RenderContext>,
    frame: Res<RenderFrame>,
    mut animations: ResMut<AnimationManager>,
) {
    let skeletons = animations
        .animations
        .iter()
        .map(|a| match (a.skeleton.try_get(), a.clip.try_get()) {
            (Some(skel), Some(anim)) => GPUSkeleton::create(
                skel,
                anim,
                a.progress_secs * anim.ticks_per_second,
                // (a.progress_secs * anim.ticks_per_second).trunc(),
            ),
            _ => GPUSkeleton {
                bones: [Mat4::ZERO; _],
            },
        })
        .collect::<Vec<_>>();

    let skeleton_buf_size = (skeletons.len() * mem::size_of::<GPUSkeleton>()) as _;
    if animations.buffer.size() < skeleton_buf_size {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        resize_buffer(
            &ctx.device,
            &mut encoder,
            &mut animations.buffer,
            skeleton_buf_size,
            None,
        );

        frame.submit(encoder.finish());
    }
    ctx.queue.write_buffer(&animations.buffer, 0, unsafe {
        cast_bytes_slice(&skeletons)
    });
}

pub fn progress_animations(dt: Res<DeltaTime>, mut animations: ResMut<AnimationManager>) {
    for anim_state in &mut animations.animations {
        if let Some(clip) = anim_state.clip.try_get() {
            anim_state.progress_secs = (anim_state.progress_secs + dt.0.as_secs_f32())
                .rem_euclid(clip.duration_ticks as f32 / clip.ticks_per_second);
        }
    }
}

#[repr(C)]
struct GPUSkeleton {
    bones: [Mat4; MAX_BONES as _],
}

impl GPUSkeleton {
    fn create(skeleton: &Skeleton, clip: &AnimationClip, tick: f32) -> Self {
        let mut bones = [Mat4::IDENTITY; MAX_BONES as _];
        for i in 0..skeleton.bones.len() as u32 {
            let mut transform = skeleton.bones[i as usize].offset;

            let mut next = Some(i);
            while let Some(i) = next {
                let bone = skeleton.bones[i as usize];
                let bone_transform = match clip.channels.get(&i) {
                    Some(channel) => {
                        let (t_pos, pos_a, pos_b) = get_keyframes(&channel.positions, tick);
                        let (t_rot, rot_a, rot_b) = get_keyframes(&channel.rotations, tick);
                        let (t_scale, scale_a, scale_b) = get_keyframes(&channel.scales, tick);
                        Mat4::from_scale_rotation_translation(
                            Vec3::lerp(scale_a, scale_b, t_scale),
                            Quat::lerp(rot_a, rot_b, t_rot),
                            Vec3::lerp(pos_a, pos_b, t_pos),
                        )
                    }
                    None => bone.default_transform.as_mat4(),
                };
                transform = bone_transform * transform;
                next = bone.parent;
            }

            bones[(i + 1) as usize] = transform;
        }
        Self { bones }
    }
}

fn get_keyframes<T: Copy>(keyframes: &[(u32, T)], tick: f32) -> (f32, T, T) {
    match keyframes.partition_point(|(t, _)| *t <= tick as u32) {
        0 => (0.0, keyframes[0].1, keyframes[0].1),
        n if n == keyframes.len() => (0.0, keyframes[n - 1].1, keyframes[n - 1].1),
        n => {
            let (t0, a) = keyframes[n - 1];
            let (t1, b) = keyframes[n];
            ((tick - t0 as f32) / (t1 - t0) as f32, a, b)
        }
    }
}
