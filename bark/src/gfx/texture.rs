use crate::assets::AssetProcessor;
use image::imageops::{self, FilterType};
use intel_tex_2::{RSurface, RgSurface, RgbaSurface, bc4, bc5, bc7};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use strum::FromRepr;

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
