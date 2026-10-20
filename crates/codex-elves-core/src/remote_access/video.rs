use std::collections::{HashMap, VecDeque};
use std::path::{Path as FsPath, PathBuf};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::{ApiError, LoadedImage, RemoteAccessController, WebState};

const MAX_VIDEOS: usize = 512;
const FALLBACK_POSTER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="144" viewBox="0 0 256 144"><rect width="256" height="144" rx="12" fill="#252830"/><circle cx="128" cy="72" r="27" fill="#444956"/><path d="m121 57 23 15-23 15z" fill="#fff"/></svg>"##;

#[derive(Clone)]
struct Video {
    path: PathBuf,
    mime: &'static str,
}

#[derive(Default)]
pub(super) struct Registry {
    entries: HashMap<String, Video>,
    paths: HashMap<PathBuf, String>,
    order: VecDeque<String>,
}

impl Registry {
    fn register(&mut self, video: Video) -> String {
        if let Some(id) = self.paths.get(&video.path).cloned() {
            self.order.retain(|entry| entry != &id);
            self.order.push_back(id.clone());
            return id;
        }
        while self.entries.len() >= MAX_VIDEOS {
            if let Some(id) = self.order.pop_front()
                && let Some(expired) = self.entries.remove(&id)
            {
                self.paths.remove(&expired.path);
            }
        }
        let id = super::random_token();
        self.paths.insert(video.path.clone(), id.clone());
        self.entries.insert(id.clone(), video);
        self.order.push_back(id.clone());
        id
    }
}

fn eligible_video(source: &str) -> Option<Video> {
    let source = source.trim();
    if source.is_empty()
        || source.len() > 32_767
        || source.contains('\0')
        || source.contains("://")
        || source.to_ascii_lowercase().starts_with("file:")
        || source.starts_with(r"\\")
        || source.starts_with("//")
    {
        return None;
    }
    #[cfg(windows)]
    let source = match source.as_bytes() {
        [b'/', drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic() => &source[1..],
        _ => source,
    };
    let path = PathBuf::from(source);
    if !path.is_absolute() || !super::input_path_has_local_prefix(&path) {
        return None;
    }
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "ogv" | "ogg" => "video/ogg",
        _ => return None,
    };
    Some(Video { path, mime })
}

pub(super) fn attach_item(controller: &RemoteAccessController, item: &mut Value) {
    if let Some(object) = item.as_object_mut() {
        object.remove("remoteVideos");
    }
    let mut sources = match item["type"].as_str() {
        Some("agentMessage" | "plan") => item["text"]
            .as_str()
            .map(|text| super::markdown_attachment_sources(text, true))
            .unwrap_or_default(),
        Some("userMessage") => item["content"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|part| match part["type"].as_str() {
                Some("text") => part["text"]
                    .as_str()
                    .map(|text| super::markdown_attachment_sources(text, true))
                    .unwrap_or_default(),
                Some("localVideo") => part["path"]
                    .as_str()
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                Some("video") => part["url"]
                    .as_str()
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    sources.retain(|source| seen.insert(source.clone()));
    let mut registry = controller
        .videos
        .write()
        .unwrap_or_else(|error| error.into_inner());
    let videos: Vec<_> = sources
        .into_iter()
        .filter_map(|source| {
            let video = eligible_video(&source)?;
            let name = video.path.file_name()?.to_string_lossy().into_owned();
            let mime = video.mime;
            let id = registry.register(video);
            Some(json!({
                "source": source, "url": format!("/api/videos/{id}"),
                "poster": format!("/api/videos/{id}/poster"), "mime": mime, "name": name,
            }))
        })
        .collect();
    if !videos.is_empty() {
        item["remoteVideos"] = Value::Array(videos);
    }
}

fn registered(state: &WebState, id: &str) -> Result<Video, ApiError> {
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApiError::new(404, "视频不存在或已过期"));
    }
    state
        .controller
        .videos
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .entries
        .get(id)
        .cloned()
        .ok_or_else(|| ApiError::new(404, "视频不存在或已过期"))
}

async fn local_file(video: &Video) -> Result<(PathBuf, std::fs::Metadata), ApiError> {
    let path = tokio::fs::canonicalize(&video.path)
        .await
        .map_err(|_| ApiError::new(404, "视频文件不存在或无法读取"))?;
    if !super::canonical_path_is_local(&path) {
        return Err(ApiError::new(403, "视频不在本机固定磁盘"));
    }
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|_| ApiError::new(404, "视频文件不存在或无法读取"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(ApiError::new(404, "视频文件为空或无法读取"));
    }
    Ok((path, metadata))
}

// 只读取所请求的片段，浏览器关闭弹窗或取消请求后不再继续读取文件。
pub(super) async fn content(
    State(state): State<WebState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let video = registered(&state, &id)?;
    let (path, metadata) = local_file(&video).await?;
    let size = metadata.len();
    let etag = metadata_etag(&id, &metadata);
    let range = headers.get(header::RANGE).filter(|_| {
        headers
            .get(header::IF_RANGE)
            .is_none_or(|value| value == etag.as_str())
    });
    let parsed = range.map(|value| {
        value
            .to_str()
            .ok()
            .and_then(|value| byte_range(value, size))
    });
    if parsed == Some(None) {
        return Ok((
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, format!("bytes */{size}"))],
        )
            .into_response());
    }
    let (start, end) = parsed.flatten().unwrap_or((0, size - 1));
    let length = end - start + 1;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| ApiError::new(404, "视频文件不存在或无法读取"))?;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|_| ApiError::new(503, "无法读取视频片段"))?;
    state
        .controller
        .authorization(device, None)
        .check()
        .await
        .map_err(super::authorization_error)?;
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(futures_util::stream::try_unfold(
            (file, length),
            |(mut file, remaining)| async move {
                if remaining == 0 {
                    return Ok::<_, std::io::Error>(None);
                }
                let mut chunk = vec![0; remaining.min(64 * 1024) as usize];
                let read = file.read(&mut chunk).await?;
                if read == 0 {
                    return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
                }
                chunk.truncate(read);
                Ok(Some((chunk, (file, remaining - read as u64))))
            },
        ))
    };
    let mut response = Response::new(body);
    *response.status_mut() = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let output = response.headers_mut();
    output.insert(header::CONTENT_TYPE, HeaderValue::from_static(video.mime));
    output.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).unwrap(),
    );
    output.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    output.insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
    output.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    output.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    if range.is_some() {
        output.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}")).unwrap(),
        );
    }
    Ok(response)
}

fn byte_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let decimal = |value: &str| -> Option<u64> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        value.parse().ok()
    };
    if size == 0 {
        return None;
    }
    if start.is_empty() {
        let suffix = decimal(end)?;
        return (suffix > 0).then_some((size.saturating_sub(suffix), size - 1));
    }
    let start = decimal(start)?;
    let end = if end.is_empty() {
        size - 1
    } else {
        decimal(end)?.min(size - 1)
    };
    (start < size && start <= end).then_some((start, end))
}

fn metadata_etag(id: &str, metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .unwrap_or_default();
    format!("\"{id}-{}-{}\"", metadata.len(), modified.as_nanos())
}

pub(super) async fn poster(
    State(state): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let video = registered(&state, &id)?;
    let (path, metadata) = local_file(&video).await?;
    // 按文件版本缓存封面，不读取整个视频计算摘要。
    let key: [u8; 32] = Sha256::digest(metadata_etag(&id, &metadata).as_bytes()).into();
    let permit = state
        .controller
        .image_preview_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ApiError::new(503, "视频预览暂不可用"))?;
    let controller = state.controller.clone();
    let image = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if let Some(Some(cached)) = controller
            .image_previews
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
        {
            return cached;
        }
        let preview = local_thumbnail(&path).unwrap_or_else(|| LoadedImage {
            bytes: FALLBACK_POSTER.to_vec(),
            mime: "image/svg+xml",
        });
        controller
            .image_previews
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(key, Some(preview.clone()));
        preview
    })
    .await
    .map_err(|_| ApiError::new(503, "无法生成视频预览"))?;
    state
        .controller
        .authorization(device, None)
        .check()
        .await
        .map_err(super::authorization_error)?;
    let etag = super::strong_etag(&image.bytes);
    if super::if_none_match_matches(&headers, &etag) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::VARY, HeaderValue::from_static("Cookie")),
            ],
        )
            .into_response());
    }
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(image.mime)),
            (
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&image.bytes.len().to_string()).unwrap(),
            ),
            (header::ETAG, etag),
            (header::VARY, HeaderValue::from_static("Cookie")),
            (
                header::HeaderName::from_static("cross-origin-resource-policy"),
                HeaderValue::from_static("same-origin"),
            ),
        ],
        image.bytes,
    )
        .into_response())
}

#[cfg(not(windows))]
fn local_thumbnail(_path: &FsPath) -> Option<LoadedImage> {
    None
}

#[cfg(windows)]
fn local_thumbnail(path: &FsPath) -> Option<LoadedImage> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY,
    };
    use windows::core::PCWSTR;

    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    struct BitmapGuard(HBITMAP);
    impl Drop for BitmapGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteObject(self.0);
            }
        }
    }
    struct DcGuard(HDC);
    impl Drop for DcGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok()?;
        let _com = ComGuard;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // 固定磁盘检查已在 canonicalize 后完成；Shell 不接受 Windows 的 \\?\ 路径前缀。
        let shell_path = if wide.starts_with(&[92, 92, 63, 92]) {
            &wide[4..]
        } else {
            &wide
        };
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(shell_path.as_ptr()), None).ok()?;
        let bitmap = BitmapGuard(
            factory
                .GetImage(SIZE { cx: 256, cy: 256 }, SIIGBF_THUMBNAILONLY)
                .ok()?,
        );
        let mut info = BITMAP::default();
        if GetObjectW(
            bitmap.0,
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut info as *mut BITMAP).cast()),
        ) == 0
        {
            return None;
        }
        let (width, height) = (
            u32::try_from(info.bmWidth).ok()?,
            u32::try_from(info.bmHeight).ok()?,
        );
        if width == 0 || height == 0 || width > 512 || height > 512 {
            return None;
        }
        let dc = DcGuard(CreateCompatibleDC(None));
        if dc.0.is_invalid() {
            return None;
        }
        let mut dib = BITMAPINFO::default();
        dib.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        if GetDIBits(
            dc.0,
            bitmap.0,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut dib,
            DIB_RGB_COLORS,
        ) != height as i32
        {
            return None;
        }
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        let image = image::RgbaImage::from_raw(width, height, pixels)?;
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, image::ImageFormat::WebP)
            .ok()?;
        Some(LoadedImage {
            bytes: encoded.into_inner(),
            mime: "image/webp",
        })
    }
}
