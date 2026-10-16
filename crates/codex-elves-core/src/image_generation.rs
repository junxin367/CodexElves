use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) struct ImageGenerationBridge {
    directory: PathBuf,
    response_id: String,
    items: HashMap<String, Value>,
    next_sequence: Option<u64>,
}

impl ImageGenerationBridge {
    pub(crate) fn new(home: &Path, thread_id: Option<&str>) -> Self {
        let thread = thread_id
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 96
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
            .map(str::to_string)
            .unwrap_or_else(|| {
                thread_id.map_or_else(
                    || uuid::Uuid::new_v4().to_string(),
                    |id| format!("{:x}", Sha256::digest(id.as_bytes())),
                )
            });
        Self {
            directory: home.join("generated_images").join(thread),
            response_id: String::new(),
            items: HashMap::new(),
            next_sequence: None,
        }
    }

    pub(crate) fn for_request(
        context: Option<&crate::request_headers::RequestContext>,
        request: Option<&Value>,
    ) -> Self {
        let thread = context
            .and_then(|context| context.log_thread_id())
            .or_else(|| {
                request
                    .and_then(|request| {
                        request
                            .pointer("/metadata/thread_id")
                            .or_else(|| request.pointer("/client_metadata/thread_id"))
                    })
                    .and_then(Value::as_str)
            });
        Self::new(&crate::codex_home::default_codex_home_dir(), thread)
    }

    pub(crate) fn rewrite_text(&mut self, text: &str) -> Option<Vec<String>> {
        if self.next_sequence.is_none()
            && !text.contains("image_generation_call")
            && !text.contains("\"response.created\"")
        {
            return None;
        }
        let event = serde_json::from_str::<Value>(text).ok()?;
        self.rewrite_event(event)
            .map(|events| events.into_iter().map(|event| event.to_string()).collect())
    }

    fn rewrite_event(&mut self, mut event: Value) -> Option<Vec<Value>> {
        let event_type = event.get("type")?.as_str()?.to_string();
        if event_type == "response.created" {
            if let Some(id) = event.pointer("/response/id").and_then(Value::as_str)
                && id != self.response_id
            {
                self.response_id = id.to_string();
                self.items.clear();
                self.next_sequence = None;
            }
            return None;
        }
        let original_sequence = event.get("sequence_number").and_then(Value::as_u64);
        let mut events = Vec::new();
        let mut changed = false;
        if event_type.starts_with("response.image_generation_call.") {
            changed = true;
        } else if matches!(
            event_type.as_str(),
            "response.output_item.added" | "response.output_item.done"
        ) && event.pointer("/item/type").and_then(Value::as_str)
            == Some("image_generation_call")
        {
            if event_type == "response.output_item.done" {
                let index = event
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if event
                    .pointer("/item/result")
                    .and_then(Value::as_str)
                    .is_some_and(|data| !data.is_empty())
                    || matches!(
                        event.pointer("/item/status").and_then(Value::as_str),
                        Some("failed" | "incomplete" | "cancelled")
                    )
                {
                    let (item, fresh) = self.image_message(&event["item"], index);
                    if fresh {
                        events.extend(image_message_events(&item, index));
                    }
                }
            }
            changed = true;
        } else if matches!(
            event_type.as_str(),
            "response.completed" | "response.incomplete" | "response.failed"
        ) && let Some(output) = event
            .pointer_mut("/response/output")
            .and_then(Value::as_array_mut)
        {
            for (index, item) in output.iter_mut().enumerate() {
                if item.get("type").and_then(Value::as_str) != Some("image_generation_call") {
                    continue;
                }
                let (message, fresh) = self.image_message(item, index as u64);
                if fresh {
                    events.extend(image_message_events(&message, index as u64));
                }
                *item = message;
                changed = true;
            }
            events.push(event);
        } else {
            events.push(event);
        }
        if !changed && self.next_sequence.is_none() {
            return None;
        }
        if self.next_sequence.is_none() {
            self.next_sequence = original_sequence;
        }
        for event in &mut events {
            if let Some(sequence) = self.next_sequence.as_mut() {
                event["sequence_number"] = json!(*sequence);
                *sequence += 1;
            }
        }
        Some(events)
    }

    fn image_message(&mut self, item: &Value, index: u64) -> (Value, bool) {
        let key = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}:{index}", self.response_id));
        if let Some(message) = self.items.get(&key) {
            return (message.clone(), false);
        }
        let text = match persist_image(&self.directory, item) {
            Ok(path) => {
                let path = path.to_string_lossy().replace('\\', "/");
                let path = path
                    .replace('%', "%25")
                    .replace('<', "%3C")
                    .replace('>', "%3E");
                format!("![生成的图片](<{path}>)")
            }
            Err(error) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "protocol_proxy.image_generation_result_failed",
                    json!({"itemId": item.get("id"), "error": error.to_string()}),
                );
                format!("图片结果无法显示：{error}。未自动重新请求生图。")
            }
        };
        let id = format!(
            "msg_ce_image_{:x}",
            Sha256::digest(format!("{}:{key}", self.response_id).as_bytes())
        );
        let message = json!({
            "type": "message",
            "id": id,
            "role": "assistant",
            "status": "completed",
            "phase": "final_answer",
            "content": [{"type": "output_text", "text": text, "annotations": []}]
        });
        self.items.insert(key, message.clone());
        (message, true)
    }

    pub(crate) fn rewrite_response(&mut self, response: &mut Value) -> bool {
        let Some(output) = response.get_mut("output").and_then(Value::as_array_mut) else {
            return false;
        };
        let mut changed = false;
        for (index, item) in output.iter_mut().enumerate() {
            if item.get("type").and_then(Value::as_str) == Some("image_generation_call") {
                *item = self.image_message(item, index as u64).0;
                changed = true;
            }
        }
        changed
    }
}

fn persist_image(directory: &Path, item: &Value) -> anyhow::Result<PathBuf> {
    let encoded = item
        .get("result")
        .and_then(Value::as_str)
        .filter(|result| !result.is_empty())
        .context("上游未返回完整图片数据")?;
    if matches!(
        item.get("status").and_then(Value::as_str),
        Some("failed" | "incomplete" | "cancelled")
    ) {
        bail!("上游图片生成未完成");
    }
    let encoded = encoded
        .strip_prefix("data:image/")
        .and_then(|value| value.split_once(";base64,").map(|(_, data)| data))
        .unwrap_or(encoded);
    if encoded.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
        bail!("图片数据超过 64 MiB 保存限制");
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("图片 Base64 数据无效")?;
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!("图片数据超过 64 MiB 保存限制");
    }
    let format = image::guess_format(&bytes).context("图片格式无法识别")?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        _ => bail!("上游返回了不支持的图片格式"),
    };
    image::ImageReader::with_format(Cursor::new(&bytes), format)
        .decode()
        .context("图片数据不完整或已损坏")?;
    std::fs::create_dir_all(directory).context("无法创建图片保存目录")?;
    let path = directory.join(format!("ce_{:x}.{extension}", Sha256::digest(&bytes)));
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(&bytes) {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(error).context("无法保存图片文件");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::read(&path).context("无法读取已有图片文件")? != bytes {
                bail!("图片保存路径已存在其他内容");
            }
        }
        Err(error) => return Err(error).context("无法保存图片文件"),
    }
    Ok(path)
}

fn image_message_events(item: &Value, index: u64) -> Vec<Value> {
    let mut started = item.clone();
    started["status"] = json!("in_progress");
    started["content"] = json!([]);
    let part = item["content"][0].clone();
    vec![
        json!({"type":"response.output_item.added","output_index":index,"item":started}),
        json!({"type":"response.content_part.added","output_index":index,"item_id":item["id"],
            "content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","output_index":index,"item_id":item["id"],
            "content_index":0,"delta":part["text"]}),
        json!({"type":"response.output_text.done","output_index":index,"item_id":item["id"],
            "content_index":0,"text":part["text"]}),
        json!({"type":"response.content_part.done","output_index":index,"item_id":item["id"],
            "content_index":0,"part":part}),
        json!({"type":"response.output_item.done","output_index":index,"item":item}),
    ]
}

pub(crate) struct ImageGenerationSseBridge {
    images: ImageGenerationBridge,
    buffer: Vec<u8>,
    cursor: usize,
    line_start: usize,
}

impl ImageGenerationSseBridge {
    pub(crate) fn new(images: ImageGenerationBridge) -> Self {
        Self {
            images,
            buffer: Vec::new(),
            cursor: 0,
            line_start: 0,
        }
    }

    pub(crate) fn push_bytes(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.buffer.extend_from_slice(bytes);
        let mut output = Vec::new();
        while self.cursor < self.buffer.len() {
            let byte = self.buffer[self.cursor];
            let newline = match byte {
                b'\r' if self.cursor + 1 == self.buffer.len() => break,
                b'\r' if self.buffer.get(self.cursor + 1) == Some(&b'\n') => 2,
                b'\r' | b'\n' => 1,
                _ => {
                    self.cursor += 1;
                    continue;
                }
            };
            let end = self.cursor + newline;
            if self.cursor == self.line_start {
                let frame: Vec<_> = self.buffer.drain(..end).collect();
                output.extend(self.rewrite_frame(&frame));
                self.cursor = 0;
                self.line_start = 0;
            } else {
                self.cursor = end;
                self.line_start = end;
            }
        }
        output
    }

    pub(crate) fn finish(&mut self) -> Vec<u8> {
        let frame = std::mem::take(&mut self.buffer);
        self.cursor = 0;
        self.line_start = 0;
        self.rewrite_frame(&frame)
    }

    fn rewrite_frame(&mut self, frame: &[u8]) -> Vec<u8> {
        let Ok(text) = std::str::from_utf8(frame) else {
            return frame.to_vec();
        };
        if self.images.next_sequence.is_none()
            && !text.contains("image_generation_call")
            && !text.contains("\"response.created\"")
        {
            return frame.to_vec();
        }
        let data = text
            .trim_start_matches('\u{feff}')
            .split(['\r', '\n'])
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|data| data.strip_prefix(' ').unwrap_or(data))
            .collect::<Vec<_>>()
            .join("\n");
        let Some(events) = self.images.rewrite_text(&data) else {
            return frame.to_vec();
        };
        let mut output = Vec::new();
        for data in events {
            let event: Value = serde_json::from_str(&data).expect("serialized Responses event");
            output.extend_from_slice(
                format!(
                    "event: {}\ndata: {data}\n\n",
                    event["type"].as_str().unwrap_or("message")
                )
                .as_bytes(),
            );
        }
        output
    }
}

pub(crate) fn rewrite_body(
    body: &[u8],
    is_stream: bool,
    context: Option<&crate::request_headers::RequestContext>,
    request: Option<&Value>,
) -> Vec<u8> {
    let mut images = ImageGenerationBridge::for_request(context, request);
    if is_stream {
        let mut stream = ImageGenerationSseBridge::new(images);
        let mut output = stream.push_bytes(body);
        output.extend(stream.finish());
        output
    } else if let Ok(mut response) = serde_json::from_slice::<Value>(body)
        && images.rewrite_response(&mut response)
    {
        response.to_string().into_bytes()
    } else {
        body.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_image(format: image::ImageFormat) -> Vec<u8> {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            8,
            8,
            image::Rgb([30, 160, 90]),
        ));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, format).unwrap();
        encoded.into_inner()
    }

    fn image_item(id: &str, bytes: &[u8]) -> Value {
        json!({
            "id": id, "type": "image_generation_call", "status": "completed",
            "result": base64::engine::general_purpose::STANDARD.encode(bytes)
        })
    }

    fn wire(events: &[Value], newline: &str) -> Vec<u8> {
        events
            .iter()
            .map(|event| {
                format!(
                    "event: {}{newline}data: {event}{newline}{newline}",
                    event["type"].as_str().unwrap()
                )
            })
            .collect::<String>()
            .into_bytes()
    }

    fn events(body: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(body)
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|data| serde_json::from_str(data).ok())
            .collect()
    }

    fn path_from_message(message: &Value) -> PathBuf {
        let text = message["content"][0]["text"].as_str().unwrap();
        PathBuf::from(
            text.strip_prefix("![生成的图片](<")
                .unwrap()
                .strip_suffix(">)")
                .unwrap(),
        )
    }

    #[test]
    fn image_generation_sse_deduplicates_items_and_keeps_terminal_output_consistent() {
        let home = tempfile::tempdir().unwrap();
        let png = encoded_image(image::ImageFormat::Png);
        let item = image_item("ig_one", &png);
        let tool = json!({"type":"function_call","id":"fc_one","call_id":"call_one",
            "name":"exec_command","arguments":"{\"cmd\":\"echo 中文\"}"});
        let source = wire(
            &[
                json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_one","output":[]}}),
                json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,
                "item":{"id":"ig_one","type":"image_generation_call","status":"in_progress"}}),
                json!({"type":"response.image_generation_call.generating","sequence_number":2,"item_id":"ig_one","output_index":0}),
                json!({"type":"response.output_item.done","sequence_number":3,"output_index":0,"item":item}),
                json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":item}),
                json!({"type":"response.output_item.done","sequence_number":5,"output_index":1,"item":tool}),
                json!({"type":"response.completed","sequence_number":6,"response":{"id":"resp_one","status":"completed",
                "output":[item,tool],"usage":{"input_tokens":1,"output_tokens":2}}}),
            ],
            "\r\n",
        );
        let mut bridge = ImageGenerationSseBridge::new(ImageGenerationBridge::new(
            home.path(),
            Some("thread_one"),
        ));
        let mut output = Vec::new();
        for chunk in source.chunks(7) {
            output.extend(bridge.push_bytes(chunk));
        }
        output.extend(bridge.finish());
        let output = events(&output);
        let messages = output
            .iter()
            .filter(|event| {
                event["type"] == "response.output_item.done" && event["item"]["type"] == "message"
            })
            .collect::<Vec<_>>();
        assert_eq!(messages.len(), 1);
        let saved = path_from_message(&messages[0]["item"]);
        assert_eq!(std::fs::read(&saved).unwrap(), png);
        assert!(saved.starts_with(home.path().join("generated_images/thread_one")));
        let terminal = &output.last().unwrap()["response"];
        assert_eq!(terminal["output"][0], messages[0]["item"]);
        assert_eq!(terminal["output"][1], tool);
        assert_eq!(
            terminal["usage"],
            json!({"input_tokens":1,"output_tokens":2})
        );
        let sequences = output
            .iter()
            .map(|event| event["sequence_number"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sequences, (0..output.len() as u64).collect::<Vec<_>>());
        assert_eq!(
            std::fs::read_dir(saved.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn image_generation_terminal_only_multiformat_images_emit_once_each() {
        let home = tempfile::tempdir().unwrap();
        let mut bridge = ImageGenerationBridge::new(home.path(), Some("thread_formats"));
        let items = [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::WebP,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, format)| image_item(&format!("ig_{index}"), &encoded_image(format)))
        .collect::<Vec<_>>();
        let terminal =
            json!({"type":"response.completed","response":{"id":"resp_formats","output":items}});
        let result = bridge.rewrite_event(terminal.clone()).unwrap();
        assert_eq!(result.len(), 19);
        for item in result.last().unwrap()["response"]["output"]
            .as_array()
            .unwrap()
        {
            assert!(path_from_message(item).is_file());
        }
        let repeated = bridge.rewrite_event(terminal).unwrap();
        assert_eq!(repeated.len(), 1);
        assert_eq!(
            std::fs::read_dir(home.path().join("generated_images/thread_formats"))
                .unwrap()
                .count(),
            3
        );
    }

    #[test]
    fn image_generation_snapshot_supplies_data_missing_from_item_done() {
        let home = tempfile::tempdir().unwrap();
        let mut bridge = ImageGenerationBridge::new(home.path(), Some("thread_snapshot"));
        assert!(
            bridge
                .rewrite_event(json!({"type":"response.output_item.done","output_index":0,
            "item":{"id":"ig_snapshot","type":"image_generation_call","status":"completed"}}))
                .unwrap()
                .is_empty()
        );
        let result = bridge
            .rewrite_event(json!({"type":"response.completed","response":{"output":[
                image_item("ig_snapshot", &encoded_image(image::ImageFormat::Png))
            ]}}))
            .unwrap();
        assert_eq!(result.len(), 7);
        assert!(path_from_message(&result[5]["item"]).is_file());
    }

    #[test]
    fn image_generation_invalid_or_missing_data_returns_an_error_without_retry_or_file() {
        let home = tempfile::tempdir().unwrap();
        for item in [
            json!({"id":"ig_invalid","type":"image_generation_call","status":"completed","result":"not-base64!"}),
            json!({"id":"ig_missing","type":"image_generation_call","status":"completed"}),
            json!({"id":"ig_failed","type":"image_generation_call","status":"failed","result":"AAAA"}),
            image_item("ig_corrupt", b"\x89PNG\r\n\x1a\ntruncated"),
        ] {
            let mut bridge = ImageGenerationBridge::new(home.path(), Some("thread_error"));
            let mut response = json!({"output":[item],"status":"completed"});
            assert!(bridge.rewrite_response(&mut response));
            let text = response["output"][0]["content"][0]["text"]
                .as_str()
                .unwrap();
            assert!(text.starts_with("图片结果无法显示："));
            assert!(text.contains("未自动重新请求生图"));
            assert!(!text.contains("!["));
        }
        assert!(!home.path().join("generated_images").exists());
    }

    #[test]
    fn image_generation_never_overwrites_existing_files_or_uses_untrusted_thread_paths() {
        let home = tempfile::tempdir().unwrap();
        let mut bridge = ImageGenerationBridge::new(home.path(), Some("../../outside"));
        let png = encoded_image(image::ImageFormat::Png);
        let item = image_item("../../outside", &png);
        let mut response = json!({"output":[item]});
        bridge.rewrite_response(&mut response);
        let path = path_from_message(&response["output"][0]);
        assert!(path.starts_with(home.path().join("generated_images")));
        assert_eq!(
            path.components().count(),
            home.path().components().count() + 3
        );
        std::fs::write(&path, b"existing user content").unwrap();
        let mut bridge = ImageGenerationBridge::new(home.path(), Some("../../outside"));
        let mut response = json!({"output":[item]});
        bridge.rewrite_response(&mut response);
        assert_eq!(std::fs::read(&path).unwrap(), b"existing user content");
        assert!(
            response["output"][0]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("已存在其他内容")
        );
    }

    #[test]
    fn image_generation_storage_failure_does_not_return_a_broken_image_link() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("generated_images"), b"not a directory").unwrap();
        let mut bridge = ImageGenerationBridge::new(home.path(), Some("thread_storage"));
        let mut response =
            json!({"output":[image_item("ig_storage", &encoded_image(image::ImageFormat::Png))]});
        bridge.rewrite_response(&mut response);
        let text = response["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("无法创建图片保存目录"));
        assert!(!text.contains("!["));
    }

    #[test]
    fn image_generation_ordinary_sse_is_byte_identical_for_all_chunk_boundaries() {
        let home = tempfile::tempdir().unwrap();
        for newline in ["\n", "\r\n", "\r"] {
            let body = format!(
                "\u{feff}: comment{newline}{newline}id: 12{newline}data: {{\"type\":\"response.created\",\"response\":{{\"id\":\"resp_text\"}}}}{newline}{newline}data: {{\"type\":\"response.output_text.delta\",\"delta\":\"中文😊\"}}{newline}{newline}data: [DONE]{newline}{newline}data: {{\"type\":\"response.completed\",\"response\":{{\"output\":[]}}}}"
            );
            for size in [1, 2, 3, 7, 127, 4096] {
                let mut bridge =
                    ImageGenerationSseBridge::new(ImageGenerationBridge::new(home.path(), None));
                let mut result = Vec::new();
                for chunk in body.as_bytes().chunks(size) {
                    result.extend(bridge.push_bytes(chunk));
                }
                result.extend(bridge.finish());
                assert_eq!(result, body.as_bytes(), "newline={newline:?} chunk={size}");
            }
        }
        assert!(!home.path().join("generated_images").exists());
    }

    #[test]
    fn image_generation_new_response_resets_sequence_and_item_deduplication() {
        let home = tempfile::tempdir().unwrap();
        let mut bridge = ImageGenerationBridge::new(home.path(), Some("thread_turns"));
        let item = image_item("ig_reused", &encoded_image(image::ImageFormat::Png));
        for response_id in ["resp_first", "resp_second"] {
            assert!(
                bridge
                    .rewrite_event(json!({"type":"response.created","sequence_number":0,
                "response":{"id":response_id}}))
                    .is_none()
            );
            let output = bridge
                .rewrite_event(json!({"type":"response.output_item.done",
                "sequence_number":1,"output_index":0,"item":item}))
                .unwrap();
            assert_eq!(output.len(), 6);
            assert_eq!(output[0]["sequence_number"], 1);
        }
    }

    #[test]
    fn image_generation_reads_complete_payload_beyond_proxy_capture_limit() {
        let home = tempfile::tempdir().unwrap();
        let image = image::RgbImage::from_fn(1024, 1024, |x, y| {
            let value = (y * 1024 + x).wrapping_mul(2654435761);
            image::Rgb([(value >> 24) as u8, (value >> 16) as u8, (value >> 8) as u8])
        });
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        let body = wire(
            &[json!({"type":"response.completed","response":{
            "output":[image_item("ig_large",&bytes)]}})],
            "\n",
        );
        assert!(body.len() > 2 * 1024 * 1024);
        let mut bridge = ImageGenerationSseBridge::new(ImageGenerationBridge::new(
            home.path(),
            Some("thread_large"),
        ));
        let mut output = Vec::new();
        for chunk in body.chunks(16381) {
            output.extend(bridge.push_bytes(chunk));
        }
        output.extend(bridge.finish());
        let result = events(&output);
        let saved = path_from_message(&result.last().unwrap()["response"]["output"][0]);
        assert_eq!(std::fs::read(saved).unwrap(), bytes);
        assert!(output.len() < 10000);
    }
}
