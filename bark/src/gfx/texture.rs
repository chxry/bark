use crate::assets::{Asset, AssetProcessor, Handle};
use image::imageops::{self, FilterType};
use intel_tex_2::{RSurface, RgSurface, RgbaSurface, bc4, bc5, bc7};
use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::num::NonZero;
use strum::FromRepr;
use tracing::{debug, error};
use wgpu::util::DeviceExt;

type TextureSlotIndex = u32;

pub const MAX_BOUND_TEXTURES: TextureSlotIndex = 2048;

pub struct TextureManager {
    undefined_texture_view: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    slots: [Option<TextureSlot>; MAX_BOUND_TEXTURES as _],
    asset_map: HashMap<String, TextureSlotIndex>,
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
        let slots = [const { None }; MAX_BOUND_TEXTURES as _];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let bind_group =
            Self::build_bind_group(device, &layout, &sampler, &slots, &undefined_texture_view);

        Self {
            undefined_texture_view,
            layout,
            sampler,
            bind_group,
            slots,
            asset_map: HashMap::new(),
        }
    }

    pub fn process_sources(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        handles: HashSet<Handle<Texture>>,
    ) {
        if handles.len() > MAX_BOUND_TEXTURES as _ {
            error!(
                "amount of bound textures exceeds limit ({} > {})",
                handles.len(),
                MAX_BOUND_TEXTURES
            );
            return;
        }
        let mut bindings_changed = false;

        for s in &mut self.slots {
            if let Some(slot) = s
                && !handles.contains(&slot.asset)
            {
                *s = None;
                bindings_changed = true;
            }
        }

        let mut i = 1;
        for tex in handles {
            if !self.asset_map.contains_key(tex.id()) {
                debug!("upload texture {:?}", tex.id());
                while self.slots[i].is_some() {
                    i += 1;
                }

                let mut tex_format = match tex.header.compression {
                    CompressionFormat::Bc7 => wgpu::TextureFormat::Bc7RgbaUnorm,
                    CompressionFormat::Bc5 => wgpu::TextureFormat::Bc5RgUnorm,
                    CompressionFormat::Bc4 => wgpu::TextureFormat::Bc4RUnorm,
                    CompressionFormat::None => wgpu::TextureFormat::Rgba8Unorm,
                };
                if tex.header.mode == TextureMode::Srgb {
                    tex_format = tex_format.add_srgb_suffix();
                }

                let texture = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        size: wgpu::Extent3d {
                            width: tex.header.width,
                            height: tex.header.height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: tex.header.mip_count as _,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: tex_format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                        label: None,
                    },
                    wgpu::util::TextureDataOrder::default(),
                    tex.data(),
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.slots[i] = Some(TextureSlot {
                    asset: tex.clone(),
                    view,
                });
                self.asset_map.insert(tex.id().to_owned(), i as _);
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
        slots: &[Option<TextureSlot>; MAX_BOUND_TEXTURES as _],
        fallback: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        debug!("rebuilding texture bindings");
        let views = slots
            .each_ref()
            .map(|s| s.as_ref().map(|s| &s.view).unwrap_or(fallback));
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

struct TextureSlot {
    asset: Handle<Texture>,
    view: wgpu::TextureView,
}

pub struct Texture {
    header: TextureHeader,
    data: Mmap,
}

impl Texture {
    fn data(&self) -> &[u8] {
        &self.data[TextureHeader::OFFSET..]
    }
}

impl Asset for Texture {
    fn read(data: Mmap) -> Self {
        Self {
            header: TextureHeader::read(&*data),
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

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TextureHeader {
    pub width: u32,
    pub height: u32,
    pub mip_count: u8,
    pub mode: TextureMode,
    pub compression: CompressionFormat,
}

impl TextureHeader {
    pub const OFFSET: usize = 11;

    pub fn write<W: Write>(&self, mut writer: W) {
        writer.write_all(&self.width.to_le_bytes()).unwrap();
        writer.write_all(&self.height.to_le_bytes()).unwrap();
        writer.write_all(&[self.mip_count]).unwrap();
        writer.write_all(&[self.mode as u8]).unwrap();
        writer.write_all(&[self.compression as u8]).unwrap();
    }

    pub fn read<R: Read>(mut reader: R) -> Self {
        let mut buf = [0; 11];
        reader.read_exact(&mut buf).unwrap();
        Self {
            width: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            height: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
            mip_count: buf[8],
            mode: TextureMode::from_repr(buf[9]).unwrap(),
            compression: CompressionFormat::from_repr(buf[10]).unwrap(),
        }
    }
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
    fn process<R: Read, W: Write>(&self, mut src: R, mut out: W, opts: Self::Options) {
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

        let header = TextureHeader {
            width: mip_levels[0].width(),
            height: mip_levels[0].height(),
            mip_count: mip_levels.len() as u8,
            mode: opts.mode,
            compression: opts.compression,
        };
        header.write(&mut out);

        for level in mip_levels {
            let compressed = match opts.compression {
                CompressionFormat::Bc7 => &bc7::compress_blocks(
                    &bc7::opaque_ultra_fast_settings(),
                    &RgbaSurface {
                        data: &level,
                        width: level.width(),
                        height: level.height(),
                        stride: level.width() * 4,
                    },
                ),
                CompressionFormat::Bc5 => &bc5::compress_blocks(&RgSurface {
                    data: &extract_rg(&level),
                    width: level.width(),
                    height: level.height(),
                    stride: level.width() * 2,
                }),
                CompressionFormat::Bc4 => &bc4::compress_blocks(&RSurface {
                    data: &extract_r(&level),
                    width: level.width(),
                    height: level.height(),
                    stride: level.width(),
                }),

                CompressionFormat::None => level.as_raw(),
            };
            out.write_all(compressed).unwrap();
        }
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
