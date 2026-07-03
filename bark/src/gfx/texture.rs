use super::RenderContext;
use crate::assets::{Asset, AssetProcessor, Handle};
use crate::ecs::{Res, ResMut};
use image::imageops::{self, FilterType};
use intel_tex_2::{RSurface, RgSurface, RgbaSurface, bc4, bc5, bc7};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::num::NonZero;
use strum::FromRepr;
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());

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

    pub fn add(&mut self, handle: Handle<Texture>) -> TextureHandle {
        *self
            .asset_map
            .entry(handle.id().to_owned())
            .or_insert_with(|| {
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
                self.slots[i] = TextureSlot::Pending(handle);
                TextureHandle(i as _)
            })
    }

    pub fn get(&self, handle: TextureHandle) -> u32 {
        handle.0
    }

    fn upload_pending(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut bindings_changed = false;
        for slot in &mut self.slots {
            if let TextureSlot::Pending(handle) = slot
                && let Some(tex) = handle.try_get()
            {
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
        let views = slots.each_ref().map(|s| s.get_view(fallback));
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
    Pending(Handle<Texture>),
    Uploaded(wgpu::TextureView),
}

impl TextureSlot {
    fn get_view<'a>(&'a self, fallback: &'a wgpu::TextureView) -> &'a wgpu::TextureView {
        match self {
            Self::Uploaded(view) => view,
            _ => fallback,
        }
    }
}

pub fn upload_textures(ctx: Res<RenderContext>, mut textures: ResMut<TextureManager>) {
    textures.upload_pending(&ctx.device, &ctx.queue);
}

pub struct Texture {
    width: u32,
    height: u32,
    mip_count: u8,
    mode: TextureMode,
    compression: CompressionFormat,
    data: Vec<u8>,
}

impl Texture {
    fn write<W: Write>(&self, mut writer: W) {
        writer.write_all(&self.width.to_le_bytes()).unwrap();
        writer.write_all(&self.height.to_le_bytes()).unwrap();
        writer.write_all(&[self.mip_count]).unwrap();
        writer.write_all(&[self.mode as u8]).unwrap();
        writer.write_all(&[self.compression as u8]).unwrap();
        writer.write_all(&self.data).unwrap();
    }
}

impl Asset for Texture {
    fn read<R: Read>(mut reader: R) -> Self {
        let mut header = [0; 11];
        let mut data = vec![];
        reader.read_exact(&mut header).unwrap();
        reader.read_to_end(&mut data).unwrap();
        Self {
            width: u32::from_le_bytes(header[0..4].try_into().unwrap()),
            height: u32::from_le_bytes(header[4..8].try_into().unwrap()),
            mip_count: header[8],
            mode: TextureMode::from_repr(header[9]).unwrap(),
            compression: CompressionFormat::from_repr(header[10]).unwrap(),
            data,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize, FromRepr)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum TextureMode {
    Srgb,
    Linear,
    Normal,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize, FromRepr)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum CompressionFormat {
    Bc7,
    Bc5,
    Bc4,
    None,
}

pub struct TextureProcessor;

#[derive(Serialize, Deserialize)]
pub struct TextureOptions {
    mode: TextureMode,
    compression: CompressionFormat,
    generate_mipmaps: bool,
}

impl AssetProcessor for TextureProcessor {
    type Options = TextureOptions;

    // todo: respect TextureMode, only downsample needed channels
    fn process<R: Read, W: Write>(&self, mut src: R, out: W, opts: Self::Options) {
        let mut bytes = Vec::new();
        src.read_to_end(&mut bytes).unwrap();

        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();

        let mut mip_levels = vec![image];
        if opts.generate_mipmaps {
            let mut w = mip_levels[0].width();
            let mut h = mip_levels[0].height();
            while w > 4 && h > 4 {
                w = (w / 2).max(4);
                h = (h / 2).max(4);
                mip_levels.push(imageops::resize(
                    mip_levels.last().unwrap(),
                    w,
                    h,
                    FilterType::Lanczos3,
                ));
            }
        }

        let mut data = vec![];
        for level in &mip_levels {
            let mip_data = match opts.compression {
                CompressionFormat::Bc7 => &bc7::compress_blocks(
                    &bc7::alpha_ultra_fast_settings(),
                    &RgbaSurface {
                        data: level,
                        width: level.width(),
                        height: level.height(),
                        stride: level.width() * 4,
                    },
                ),
                CompressionFormat::Bc5 => &bc5::compress_blocks(&RgSurface {
                    data: &extract_rg(level),
                    width: level.width(),
                    height: level.height(),
                    stride: level.width() * 2,
                }),
                CompressionFormat::Bc4 => &bc4::compress_blocks(&RSurface {
                    data: &extract_r(level),
                    width: level.width(),
                    height: level.height(),
                    stride: level.width(),
                }),

                CompressionFormat::None => level.as_raw(),
            };
            data.extend(mip_data);
        }

        let texture = Texture {
            width: mip_levels[0].width(),
            height: mip_levels[0].height(),
            mip_count: mip_levels.len() as u8,
            mode: opts.mode,
            compression: opts.compression,
            data,
        };
        texture.write(out);
    }
}

fn extract_rg(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height() * 2) as usize);
    for pixel in img.pixels() {
        out.push(pixel[0]);
        out.push(pixel[1]);
    }
    out
}

fn extract_r(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height()) as usize);
    for pixel in img.pixels() {
        out.push(pixel[0]);
    }
    out
}
