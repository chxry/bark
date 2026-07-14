use super::RenderContext;
use crate::assets::{Asset, Assets, Handle};
use crate::ecs::{Res, ResMut};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::num::NonZero;
use tracing::{debug, error};
use wgpu::util::DeviceExt;

#[derive(Copy, Clone)]
pub struct TextureHandle(u32);

pub const MAX_BOUND_TEXTURES: u32 = 2048;

pub struct TextureManager {
    undefined_texture_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    slots: [TextureSlot; MAX_BOUND_TEXTURES as _],
    asset_map: HashMap<String, TextureHandle>,
}

impl TextureManager {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let undefined_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::default(),
            &[255; 4],
        );
        let undefined_texture_view =
            undefined_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: NonZero::new(MAX_BOUND_TEXTURES),
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let mut slots = [const { TextureSlot::Empty }; MAX_BOUND_TEXTURES as _];
        slots[0] = TextureSlot::Reserved;
        let bind_group =
            Self::build_bind_group(device, &layout, &sampler, &slots, &undefined_texture_view);

        Self {
            undefined_texture_view,
            sampler,
            layout,
            bind_group,
            slots,
            asset_map: HashMap::new(),
        }
    }

    pub fn add(&mut self, id: &str) -> TextureHandle {
        *self.asset_map.entry(id.to_owned()).or_insert_with(|| {
            let i = match self
                .slots
                .iter()
                .position(|s| matches!(s, TextureSlot::Empty))
            {
                Some(i) => i,
                None => {
                    error!("texture slots exhausted");
                    return TextureHandle(0);
                }
            };
            self.slots[i] = TextureSlot::PendingId(id.to_owned());
            TextureHandle(i as _)
        })
    }

    pub fn get(&self, handle: TextureHandle) -> u32 {
        handle.0
    }

    fn upload_pending(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &mut Assets) {
        let mut bindings_changed = false;
        for slot in &mut self.slots {
            match slot {
                TextureSlot::PendingId(id) => *slot = TextureSlot::PendingAsset(assets.load(id)),
                TextureSlot::PendingAsset(handle) if let Some(tex) = handle.try_get() => {
                    debug!("upload texture {:?}", handle.id());
                    let mut tex_format = match tex.compression {
                        CompressionFormat::Bc7 => wgpu::TextureFormat::Bc7RgbaUnorm,
                        CompressionFormat::Bc5 => wgpu::TextureFormat::Bc5RgUnorm,
                        CompressionFormat::Bc4 => wgpu::TextureFormat::Bc4RUnorm,
                        CompressionFormat::None => wgpu::TextureFormat::Rgba8Unorm,
                    };
                    if tex.mode == TextureMode::Srgb {
                        tex_format = tex_format.add_srgb_suffix();
                    }

                    let texture = device.create_texture_with_data(
                        queue,
                        &wgpu::TextureDescriptor {
                            size: wgpu::Extent3d {
                                width: tex.width,
                                height: tex.height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: tex.mip_count as _,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: tex_format,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                            label: None,
                        },
                        wgpu::util::TextureDataOrder::default(),
                        &tex.data,
                    );
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    *slot = TextureSlot::Uploaded(view);
                    bindings_changed = true;
                }
                _ => {}
            }
        }

        if bindings_changed {
            self.bind_group = Self::build_bind_group(
                device,
                &self.layout,
                &self.sampler,
                &self.slots,
                &self.undefined_texture_view,
            );
        }
    }

    fn build_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        slots: &[TextureSlot; MAX_BOUND_TEXTURES as _],
        fallback: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        debug!("rebuilding texture bindings");
        let views = slots.each_ref().map(|s| match s {
            TextureSlot::Uploaded(v) => v,
            _ => fallback,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureViewArray(&views),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

enum TextureSlot {
    Reserved,
    Empty,
    PendingId(String),
    PendingAsset(Handle<Texture>),
    Uploaded(wgpu::TextureView),
}

pub fn upload_textures(
    ctx: Res<RenderContext>,
    mut textures: ResMut<TextureManager>,
    mut assets: ResMut<Assets>,
) {
    textures.upload_pending(&ctx.device, &ctx.queue, &mut assets);
}

#[derive(Serialize, Deserialize)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub mip_count: u8,
    pub mode: TextureMode,
    pub compression: CompressionFormat,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

impl Asset for Texture {
    fn read<R: Read>(reader: R) -> Self {
        bincode::deserialize_from(reader).unwrap()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum TextureMode {
    Srgb,
    Linear,
    Normal,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum CompressionFormat {
    Bc7,
    Bc5,
    Bc4,
    None,
}
