use std::collections::{HashMap, VecDeque};
use std::io::Cursor;

use image::codecs::png::PngDecoder;
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::{
    ColorType, DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits,
    imageops::FilterType,
};

use super::LoadedImage;

const PREVIEW_EDGE: u32 = 256;
const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREVIEW_PIXELS: u64 = 16_000_000;
const MAX_CACHE_ENTRIES: usize = 128;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;

// 按源文件内容摘要缓存派生图。文件变化后生成新条目，不以可复用的文件路径作为版本。
#[derive(Default)]
pub(super) struct PreviewCache {
    entries: HashMap<[u8; 32], Option<LoadedImage>>,
    order: VecDeque<[u8; 32]>,
    bytes: usize,
}

impl PreviewCache {
    pub(super) fn get(&mut self, key: &[u8; 32]) -> Option<Option<LoadedImage>> {
        let value = self.entries.get(key)?.clone();
        self.order.retain(|entry| entry != key);
        self.order.push_back(*key);
        Some(value)
    }

    pub(super) fn insert(&mut self, key: [u8; 32], value: Option<LoadedImage>) {
        if let Some(previous) = self.entries.remove(&key) {
            self.bytes -= previous.as_ref().map_or(0, |image| image.bytes.len());
        }
        self.order.retain(|entry| entry != &key);
        let size = value.as_ref().map_or(0, |image| image.bytes.len());
        if size > MAX_CACHE_BYTES {
            return;
        }
        while self.entries.len() >= MAX_CACHE_ENTRIES || self.bytes + size > MAX_CACHE_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(previous) = self.entries.remove(&oldest) {
                self.bytes -= previous.as_ref().map_or(0, |image| image.bytes.len());
            }
        }
        self.bytes += size;
        self.entries.insert(key, value);
        self.order.push_back(key);
    }
}

pub(super) fn thumbnail(source: &LoadedImage) -> Option<LoadedImage> {
    let format = image::guess_format(&source.bytes).ok()?;
    // 动图继续使用原始内容，避免把 GIF、APNG 或动画 WebP 静默变成首帧。
    if format == ImageFormat::Gif
        || format == ImageFormat::Png
            && PngDecoder::new(Cursor::new(&source.bytes))
                .ok()?
                .is_apng()
                .ok()?
        || format == ImageFormat::WebP
            && WebPDecoder::new(Cursor::new(&source.bytes))
                .ok()?
                .has_animation()
    {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    limits.max_image_width = Some(super::MAX_REMOTE_IMAGE_DIMENSION);
    limits.max_image_height = Some(super::MAX_REMOTE_IMAGE_DIMENSION);
    let mut reader = ImageReader::with_format(Cursor::new(&source.bytes), format);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if width <= PREVIEW_EDGE && height <= PREVIEW_EDGE
        || u64::from(width) * u64::from(height) > MAX_PREVIEW_PIXELS
    {
        return None;
    }
    if !matches!(decoder.color_type(), ColorType::Rgb8 | ColorType::Rgba8) {
        return None;
    }
    let profile = decoder.icc_profile().ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut decoded = DynamicImage::from_decoder(decoder).ok()?;
    decoded.apply_orientation(orientation);
    let preview = decoded
        .resize(PREVIEW_EDGE, PREVIEW_EDGE, FilterType::Triangle)
        .to_rgba8();
    let mut bytes = Vec::new();
    let mut encoder = WebPEncoder::new_lossless(&mut bytes);
    if let Some(profile) = profile {
        encoder.set_icc_profile(profile).ok()?;
    }
    encoder
        .write_image(
            preview.as_raw(),
            preview.width(),
            preview.height(),
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    // 小文件或转换后没有节省流量时保留原图。
    (bytes.len() < source.bytes.len()).then_some(LoadedImage {
        bytes,
        mime: "image/webp",
    })
}
