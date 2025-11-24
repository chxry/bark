use crate::assets::Handle;
use image::DynamicImage;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::num::NonZero;
use tracing::{debug, error};
use wgpu::util::DeviceExt;

type TextureSlotIndex = u32;

pub const MAX_BOUND_TEXTURES: TextureSlotIndex = 2048;

// todo unload assets from the cpu after uploaded
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TextureSource {
    asset: Handle<DynamicImage>,
    srgb: bool,
}

impl TextureSource {
    pub fn new(asset: Handle<DynamicImage>, srgb: bool) -> Self {
        Self { asset, srgb }
    }

    pub fn ready(&self) -> bool {
        self.asset.loaded()
    }
}

pub struct TextureManager {
    pub undefined_texture_view: wgpu::TextureView,
    pub layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
    pub slots: [Option<TextureSlot>; MAX_BOUND_TEXTURES as _],
    pub source_map: HashMap<TextureSource, TextureSlotIndex>,
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
            source_map: HashMap::new(),
        }
    }

    pub fn process_sources(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sources: HashSet<TextureSource>,
    ) {
        if sources.len() > MAX_BOUND_TEXTURES as _ {
            error!(
                "amount of bound textures exceeds limit ({})",
                MAX_BOUND_TEXTURES
            );
            return;
        }
        let mut bindings_changed = false;

        for s in &mut self.slots {
            if let Some(slot) = s
                && !sources.contains(&slot.source)
            {
                *s = None;
                bindings_changed = true;
            }
        }

        let mut i = 1;
        for source in sources {
            if !self.source_map.contains_key(&source) {
                debug!("upload texture {:?}", source.asset);
                let image = source.asset.get();
                while self.slots[i].is_some() {
                    i += 1;
                }
                let texture = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: image.width(),
                            height: image.height(),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: if source.srgb {
                            wgpu::TextureFormat::Rgba8UnormSrgb
                        } else {
                            wgpu::TextureFormat::Rgba8Unorm
                        },
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::default(),
                    &image.to_rgba8(),
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.slots[i] = Some(TextureSlot {
                    source: source.clone(),
                    view,
                });
                self.source_map.insert(source, i as _);
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

    pub fn get_slot(&self, source: &TextureSource) -> Option<TextureSlotIndex> {
        self.source_map.get(source).copied()
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
    pub source: TextureSource,
    pub view: wgpu::TextureView,
}

pub fn load_image(mut reader: impl Read) -> DynamicImage {
    let mut buf = vec![];
    reader.read_to_end(&mut buf).unwrap();
    image::load_from_memory(&buf).unwrap()
}
