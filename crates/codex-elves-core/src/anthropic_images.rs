//! 在最终 Anthropic 消息上检查图片，包含历史和 tool_result 中的图片。

use std::io::Cursor;

use anyhow::Context;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, imageops::FilterType};
use serde_json::{Value, json};

const MANY_IMAGE_THRESHOLD: usize = 20;
const MANY_IMAGE_MAX_DIMENSION: u32 = 2000;
const SINGLE_IMAGE_MAX_DIMENSION: u32 = 8000;

pub(crate) fn enforce_image_limits(
    request: &mut Value,
    diagnostic_id: Option<&str>,
) -> anyhow::Result<()> {
    let Some(messages) = request.get_mut("messages").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let image_count: usize = messages
        .iter()
        .map(|message| count_images(&message["content"]))
        .sum();
    if image_count == 0 {
        return Ok(());
    }
    let max_dimension = if image_count > MANY_IMAGE_THRESHOLD {
        MANY_IMAGE_MAX_DIMENSION
    } else {
        SINGLE_IMAGE_MAX_DIMENSION
    };
    let mut resized = Vec::new();
    for (index, message) in messages.iter_mut().enumerate() {
        if let Some(content) = message.get_mut("content") {
            resize_content(
                content,
                max_dimension,
                &format!("messages.{index}.content"),
                &mut resized,
            )?;
        }
    }
    if !resized.is_empty() {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.anthropic_images_resized",
            json!({
                "diagnosticId": diagnostic_id,
                "imageCount": image_count,
                "maxDimension": max_dimension,
                "resized": resized
            }),
        );
    }
    Ok(())
}

fn count_images(content: &Value) -> usize {
    let Some(blocks) = content.as_array() else {
        return 0;
    };
    blocks
        .iter()
        .map(|block| match block["type"].as_str() {
            Some("image") => 1,
            Some("tool_result") => count_images(&block["content"]),
            _ => 0,
        })
        .sum()
}

fn resize_content(
    content: &mut Value,
    max_dimension: u32,
    path: &str,
    resized: &mut Vec<Value>,
) -> anyhow::Result<()> {
    let Some(blocks) = content.as_array_mut() else {
        return Ok(());
    };
    for (index, block) in blocks.iter_mut().enumerate() {
        let block_path = format!("{path}.{index}");
        match block["type"].as_str() {
            Some("image") => {
                if let Some(source) = block.get_mut("source") {
                    if let Some(change) = resize_source(source, max_dimension)
                        .with_context(|| format!("无法缩放 Anthropic 图片 {block_path}"))?
                    {
                        resized.push(json!({
                            "path": block_path,
                            "before": change.0,
                            "after": change.1
                        }));
                    }
                }
            }
            Some("tool_result") => {
                if let Some(nested) = block.get_mut("content") {
                    resize_content(
                        nested,
                        max_dimension,
                        &format!("{block_path}.content"),
                        resized,
                    )?;
                }
            }
            // 不进入 tool_use.input，避免改写用户传给工具的业务参数。
            _ => {}
        }
    }
    Ok(())
}

type Dimensions = (u32, u32);

fn resize_source(
    source: &mut Value,
    max_dimension: u32,
) -> anyhow::Result<Option<(Dimensions, Dimensions)>> {
    if source["type"] != "base64" {
        // URL / file_id 不在本地读取或下载；它们仍计入整个请求的图片数。
        return Ok(None);
    }
    let Some(data) = source["data"].as_str() else {
        return Ok(None);
    };
    let Ok(bytes) = STANDARD
        .decode(data)
        .or_else(|_| STANDARD_NO_PAD.decode(data))
    else {
        return Ok(None);
    };
    let Ok(format) = image::guess_format(&bytes) else {
        return Ok(None);
    };
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::WebP
    ) {
        return Ok(None);
    }
    let Ok((width, height)) =
        ImageReader::with_format(Cursor::new(&bytes), format).into_dimensions()
    else {
        // 保留原有上游对不完整或不支持图片的校验行为，不把坏图片转成文字。
        return Ok(None);
    };
    if width <= max_dimension && height <= max_dimension {
        // 合规图片不重编码，避免每轮历史回放都损失画质。
        return Ok(None);
    }

    // 使用 ImageReader 默认解码内存限制，避免为异常尺寸无限分配内存。
    let mut decoder = ImageReader::with_format(Cursor::new(&bytes), format).into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let image = image.resize(max_dimension, max_dimension, FilterType::Lanczos3);
    let mut encoded = Cursor::new(Vec::new());
    let media_type = if format == ImageFormat::Jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90)
            .encode_image(&image.to_rgb8())?;
        "image/jpeg"
    } else {
        // PNG 保留透明度；GIF / WebP 使用解码后的静态画面。
        image.write_to(&mut encoded, ImageFormat::Png)?;
        "image/png"
    };
    source["media_type"] = json!(media_type);
    source["data"] = json!(STANDARD.encode(encoded.into_inner()));
    Ok(Some(((width, height), (image.width(), image.height()))))
}
