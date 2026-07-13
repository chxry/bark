use crate::AssetProcessor;
use bark::gfx::texture::{CompressionFormat, Texture, TextureMode};
use image::imageops::{self, FilterType};
use intel_tex_2::{RSurface, RgSurface, RgbaSurface, bc4, bc5, bc7};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;

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
    fn process(&self, src_data: &[u8], _: PathBuf, out_path: PathBuf, opts: Self::Options) {
        let image = image::load_from_memory(src_data).unwrap().to_rgba8();

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
        bincode::serialize_into(File::create(out_path).unwrap(), &texture).unwrap();
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
