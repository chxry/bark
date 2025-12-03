use crate::assets::Handle;
use image::imageops;
use intel_tex_2::{RgbaSurface, bc7};
use std::collections::{HashMap, HashSet};
use std::num::NonZero;
use tracing::{debug, error};
use wgpu::util::DeviceExt;

type TextureSlotIndex = u32;

pub const MAX_BOUND_TEXTURES: TextureSlotIndex = 2048;

pub struct TextureManager {
    pub undefined_texture_view: wgpu::TextureView,
    pub layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
    pub slots: [Option<TextureSlot>; MAX_BOUND_TEXTURES as _],
    pub handle_map: HashMap<Handle<Texture>, TextureSlotIndex>,
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
            handle_map: HashMap::new(),
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
                "amount of bound textures exceeds limit ({})",
                MAX_BOUND_TEXTURES
            );
            return;
        }
        let mut bindings_changed = false;

        for s in &mut self.slots {
            if let Some(slot) = s
                && !handles.contains(&slot.handle)
            {
                *s = None;
                bindings_changed = true;
            }
        }

        let mut i = 1;
        for handle in handles {
            if !self.handle_map.contains_key(&handle) {
                debug!("upload texture {:?}", handle.id());
                while self.slots[i].is_some() {
                    i += 1;
                }
                let mip_levels = handle.width.max(handle.height).ilog2().max(1);
                let texture = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: handle.width,
                            height: handle.height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: mip_levels,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Bc7RgbaUnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::default(),
                    &handle.data,
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.slots[i] = Some(TextureSlot {
                    handle: handle.clone(),
                    view,
                });
                self.handle_map.insert(handle, i as _);
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

    pub fn get_slot(&self, handle: &Handle<Texture>) -> Option<TextureSlotIndex> {
        self.handle_map.get(handle).copied()
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

pub struct TextureSlot {
    pub handle: Handle<Texture>,
    pub view: wgpu::TextureView,
}

pub struct Texture {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

pub fn process_texture(data: &[u8]) -> Texture {
    let image = image::load_from_memory(data).unwrap().to_rgba8();
    let mut texture = Texture {
        width: image.width(),
        height: image.height(),
        data: vec![],
    };
    let start = std::time::Instant::now();

    let mut mip_levels = vec![image];
    loop {
        let current = mip_levels.last().unwrap();
        if current.width() == 1 && current.height() == 1 {
            break;
        }

        // todo consider srgb/normals
        mip_levels.push(imageops::resize(
            current,
            (current.width() / 2).max(1),
            (current.height() / 2).max(1),
            imageops::FilterType::Triangle,
        ));
    }
    tracing::info!("DOWNSAMPLE TIME {:?}", start.elapsed());

    let start = std::time::Instant::now();
    for mip in mip_levels {
        let compressed = bc7::compress_blocks(
            &bc7::alpha_basic_settings(),
            &RgbaSurface {
                data: &mip,
                width: mip.width(),
                height: mip.height(),
                stride: mip.width() * 4,
            },
        );
        texture.data.extend(&compressed);
    }
    tracing::info!("COMPRESS TIME {:?}", start.elapsed());
    texture
}

pub fn save_texture(tex: &Texture) -> Vec<u8> {
    let mut out = vec![];
    out.extend(tex.width.to_le_bytes());
    out.extend(tex.height.to_le_bytes());
    out.extend(&tex.data);
    out
}

pub fn load_texture(data: &[u8]) -> Texture {
    Texture {
        width: u32::from_le_bytes(data[0..4].try_into().unwrap()),
        height: u32::from_le_bytes(data[4..8].try_into().unwrap()),
        data: data[8..].to_vec(),
    }
}
