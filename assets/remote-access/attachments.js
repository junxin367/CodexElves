import { remoteMediaCandidates, remoteMediaUrl } from "./channels.js";
import { sha256Hex } from "./sha256.js";

const MAX_IMAGE_COUNT = 4;
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const MAX_TOTAL_BYTES = 12 * 1024 * 1024;
const MAX_IMAGE_DIMENSION = 16_384;
const MAX_IMAGE_PIXELS = 100_000_000;
const remoteImagePattern = /^\/api\/images\/[A-Za-z0-9_-]{20,128}$/u;
const supportedMimes = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
const remoteVideoPattern = /^\/api\/videos\/[a-f0-9]{64}$/u;
const MEDIA_RECOVERY_TIMEOUT_MS = 15_000;

function loadVideoPoster(image, path, loaded = () => {}) {
  if (image.posterRecovery?.path === path) return;
  image.posterRecovery?.dispose();
  const candidates = [...new Set(remoteMediaCandidates(path))];
  let next = 0, disposed = false, settled = false;
  const currentSource = () => !image.currentSrc
    || image.currentSrc === new URL(image.getAttribute("src"), location.href).href;
  const advance = () => {
    if (disposed || settled) return;
    const source = candidates[next++];
    if (source) image.src = source;
    else image.hidden = true;
  };
  image.posterRecovery = {
    path,
    dispose() { disposed = true; image.onload = null; image.onerror = null; }
  };
  image.crossOrigin = "anonymous";
  image.onload = () => {
    if (disposed || settled || !currentSource()) return;
    settled = true;
    image.hidden = false;
    loaded(image.getAttribute("src"));
  };
  image.onerror = () => { if (currentSource()) advance(); };
  advance();
}

export function videoPreviewMarkup(asset, label = "") {
  if (!remoteVideoPattern.test(asset?.url || "") || asset.poster !== `${asset.url}/poster`) return "";
  const escape = value => String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
  const name = escape(label || asset.name || "视频");
  // 正文只包含静态封面；这里不能创建 video、source 或指向视频的链接。
  return `<button type="button" class="message-video-preview" title="${name}">`
    + `<span class="message-video-cover"><img class="message-video-poster" crossorigin="anonymous" data-remote-media="${escape(asset.poster)}" alt="${name}" loading="lazy" decoding="async">`
    + `<span class="message-video-badge">▶</span></span><span class="message-video-name">${name}</span></button>`;
}

function videoTime(value) {
  const seconds = Number.isFinite(value) && value > 0 ? Math.floor(value) : 0;
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function videoPreviewIcon(name) {
  const paths = {
    close: '<path d="m6 6 12 12M18 6 6 18"/>',
    play: '<circle cx="12" cy="12" r="9"/><path d="m10 8 6 4-6 4Z" fill="currentColor" stroke="none"/>',
    pause: '<circle cx="12" cy="12" r="9"/><path d="M9 8h2v8H9ZM13 8h2v8h-2Z" fill="currentColor" stroke="none"/>'
  };
  const icon = document.createElement("span");
  icon.className = "ui-icon";
  icon.setAttribute("aria-hidden", "true");
  icon.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">${paths[name]}</svg>`;
  return icon;
}

export function fitMediaPreview(dialog, panel, media, dimensions, { fillAvailable = false } = {}) {
  const pixels = (style, property) => Number.parseFloat(style.getPropertyValue(property)) || 0;
  const resize = () => {
    if (!dialog.open) return;
    const { width, height } = dimensions?.() || {};
    if (!fillAvailable && (!width || !height)) return;
    const dialogStyle = getComputedStyle(dialog), panelStyle = getComputedStyle(panel);
    const margin = pixels(dialogStyle, "--preview-margin");
    const maxWidth = Math.max(1, pixels(dialogStyle, "max-width"));
    const maxHeight = Math.min(window.innerHeight - margin * 2, pixels(dialogStyle, "max-height") || Infinity);
    const frameWidth = pixels(dialogStyle, "border-left-width") + pixels(dialogStyle, "border-right-width")
      + pixels(panelStyle, "padding-left") + pixels(panelStyle, "padding-right");
    const framePadding = pixels(dialogStyle, "border-top-width") + pixels(dialogStyle, "border-bottom-width")
      + pixels(panelStyle, "padding-top") + pixels(panelStyle, "padding-bottom");
    const frameHeight = () => {
      let value = framePadding;
      for (const child of panel.children) {
        if (child === media) continue;
        const style = getComputedStyle(child);
        if (style.position === "absolute" || style.position === "fixed" || style.display === "none") continue;
        value += child.getBoundingClientRect().height + pixels(style, "margin-top") + pixels(style, "margin-bottom");
      }
      return value;
    };
    // 先按可用宽度测量文案，再确定窗口宽度，避免沿用上一帧的换行结果反复缩放。
    // 测量时移除媒体的旧尺寸，避免临时滚动条挤窄文案、触发下一帧再次调整。
    media.style.width = "0px";
    media.style.height = "0px";
    dialog.style.width = `${maxWidth}px`;
    if (fillAvailable) {
      media.style.width = `${Math.max(1, maxWidth - frameWidth)}px`;
      media.style.height = `${Math.max(1, maxHeight - frameHeight())}px`;
      return;
    }
    let scale = Math.min(1, Math.max(1, maxWidth - frameWidth) / width,
      Math.max(1, maxHeight - frameHeight()) / height);
    dialog.style.width = `${Math.min(maxWidth, Math.max(pixels(dialogStyle, "--preview-min-width"), width * scale + frameWidth))}px`;
    // 最终宽度下文案可能换行；只收缩媒体区域，窗口宽度不再随换行来回变化。
    scale = Math.min(scale, Math.max(1, maxHeight - frameHeight()) / height);
    media.style.width = `${width * scale}px`;
    media.style.height = `${height * scale}px`;
  };
  const observer = new ResizeObserver(resize);
  observer.observe(panel);
  window.addEventListener("resize", resize);
  return {
    resize,
    dispose() { observer.disconnect(); window.removeEventListener("resize", resize); }
  };
}

export function openVideoPreview(trigger, source, poster, name, prepareAccess = () => {}) {
  if (!remoteVideoPattern.test(source) || poster !== `${source}/poster`) return;
  prepareAccess();
  document.querySelector("#video-preview-dialog")?.close();
  const dialog = document.createElement("dialog");
  dialog.id = "video-preview-dialog";
  dialog.className = "image-preview-dialog video-preview-dialog";
  dialog.setAttribute("aria-label", `视频预览：${name}`);
  const panel = document.createElement("div");
  panel.className = "video-preview-panel";
  const close = document.createElement("button");
  close.type = "button";
  close.className = "image-preview-close";
  close.append(videoPreviewIcon("close"));
  close.setAttribute("aria-label", "关闭视频预览");
  close.onclick = () => dialog.close();
  const video = document.createElement("video");
  video.className = "video-preview-media";
  video.preload = "none";
  video.playsInline = true;
  video.crossOrigin = "anonymous";
  const initialPoster = remoteMediaCandidates(poster)[0];
  if (initialPoster) video.poster = initialPoster;
  // 弹窗打开时仍然没有 src。只有下方播放按钮的点击事件可以设置 src。
  const controls = document.createElement("div");
  controls.className = "video-preview-controls";
  const play = document.createElement("button");
  play.type = "button";
  play.className = "icon-button video-preview-play";
  play.append(videoPreviewIcon("play"));
  play.setAttribute("aria-label", "播放视频");
  play.title = "播放视频";
  const progress = document.createElement("input");
  progress.type = "range";
  progress.className = "video-preview-progress";
  progress.min = "0";
  progress.max = "1";
  progress.step = "0.1";
  progress.value = "0";
  progress.disabled = true;
  progress.setAttribute("aria-label", "视频播放进度");
  const time = document.createElement("span");
  time.className = "video-preview-time";
  time.textContent = "0:00 / 0:00";
  const status = document.createElement("p");
  status.className = "video-preview-status";
  status.setAttribute("role", "status");
  const title = document.createElement("p");
  title.className = "image-preview-caption";
  title.textContent = name;
  const cover = new Image();
  const thumbnail = trigger?.querySelector(".message-video-poster");
  // 预留完整播放区域，封面和不同宽高比的视频都在其中适配，不改变窗口尺寸。
  const layout = fitMediaPreview(dialog, panel, video, null, { fillAvailable: true });
  loadVideoPoster(cover, poster, source => {
    video.poster = source;
    if (thumbnail?.hidden) thumbnail.src = source;
  });
  let closed = false, seeking = false, failed = false, pendingSeek = null;
  let wantsPlayback = false, recoveryTimer = null, attempt = 0, playRequest = 0;
  let candidates = [], lastMediaTime = 0;
  const attempted = new Set();
  const clearRecovery = () => { clearTimeout(recoveryTimer); recoveryTimer = null; };
  const currentSource = () => {
    const source = video.getAttribute("src");
    return source && (!video.currentSrc || video.currentSrc === new URL(source, location.href).href);
  };
  const sync = () => {
    const duration = Number.isFinite(video.duration) && video.duration > 0 ? video.duration : 0;
    if (duration && video.readyState >= 1 && currentSource() && pendingSeek !== null) {
      const target = pendingSeek;
      pendingSeek = null;
      video.currentTime = Math.min(target, duration);
    }
    progress.max = String(duration || 1);
    progress.disabled = !duration || !video.getAttribute("src");
    if (!seeking) progress.value = String(video.currentTime || 0);
    progress.setAttribute("aria-valuetext", `${videoTime(video.currentTime)} / ${videoTime(duration)}`);
    time.textContent = `${videoTime(video.currentTime)} / ${videoTime(duration)}`;
    play.replaceChildren(videoPreviewIcon(wantsPlayback ? "pause" : "play"));
    const playLabel = wantsPlayback ? "暂停视频" : video.ended ? "重播视频" : failed ? "重试播放视频" : "播放视频";
    play.setAttribute("aria-label", playLabel);
    play.title = playLabel;
  };
  const fail = message => {
    clearRecovery();
    wantsPlayback = false;
    failed = true;
    ++playRequest;
    video.pause();
    status.textContent = message;
    sync();
  };
  const armRecovery = () => {
    if (recoveryTimer !== null || !wantsPlayback || closed || !dialog.open) return;
    const observed = attempt;
    recoveryTimer = setTimeout(() => {
      recoveryTimer = null;
      if (closed || !dialog.open || !wantsPlayback || observed !== attempt) return;
      if (video.error && video.error.code !== 2) {
        fail("视频无法播放，格式可能不受支持或文件已不可用");
        return;
      }
      recover();
    }, MEDIA_RECOVERY_TIMEOUT_MS);
  };
  const requestPlayback = () => {
    const observed = attempt, request = ++playRequest;
    armRecovery();
    void video.play().catch(error => {
      if (closed || !dialog.open || observed !== attempt || request !== playRequest || !wantsPlayback) return;
      if (error?.name === "AbortError") { fail("播放已取消，请重试"); return; }
      if (video.error?.code === 2) { recover(); return; }
      fail("无法播放视频，请重试或在电脑端查看");
    });
  };
  const selectSource = (source, target) => {
    clearRecovery();
    ++attempt;
    ++playRequest;
    attempted.add(source);
    pendingSeek = target;
    lastMediaTime = target;
    if (video.getAttribute("src") !== source || video.error) {
      video.src = source;
      video.load();
    }
    status.textContent = "正在加载视频…";
    sync();
    requestPlayback();
  };
  const recover = () => {
    if (closed || !dialog.open || !wantsPlayback) return;
    const source = candidates.find(value => !attempted.has(value));
    if (!source) { fail("播放线路暂不可用，请重试或在电脑端查看"); return; }
    selectSource(source, pendingSeek ?? video.currentTime ?? 0);
    status.textContent = "正在切换播放线路…";
  };
  play.onclick = () => {
    if (closed) return;
    if (wantsPlayback) {
      wantsPlayback = false;
      ++playRequest;
      clearRecovery();
      video.pause();
      status.textContent = "已暂停";
      sync();
      return;
    }
    prepareAccess();
    failed = false;
    candidates = [...new Set(remoteMediaCandidates(source))];
    attempted.clear();
    const selectedSource = candidates[0];
    if (!selectedSource) {
      fail("当前没有可用的播放线路，请稍后重试");
      return;
    }
    wantsPlayback = true;
    selectSource(selectedSource, video.ended ? 0 : pendingSeek ?? video.currentTime ?? 0);
  };
  progress.oninput = () => { seeking = true; time.textContent = `${videoTime(Number(progress.value))} / ${videoTime(video.duration)}`; };
  progress.onchange = () => {
    seeking = false;
    if (!progress.disabled && Number.isFinite(Number(progress.value))) {
      const target = Number(progress.value), selectedSource = remoteMediaCandidates(source)[0];
      if (selectedSource && video.getAttribute("src") !== selectedSource) {
        candidates = [...new Set(remoteMediaCandidates(source))];
        attempted.clear();
        pendingSeek = target;
        if (wantsPlayback) selectSource(selectedSource, target);
        else {
          ++attempt;
          ++playRequest;
          attempted.add(selectedSource);
          video.src = selectedSource;
          video.load();
        }
      } else if (selectedSource) video.currentTime = target;
      lastMediaTime = target;
      clearRecovery();
      armRecovery();
    }
    sync();
  };
  for (const event of ["loadedmetadata", "durationchange", "timeupdate", "pause", "ended"]) video.addEventListener(event, sync);
  video.addEventListener("timeupdate", () => {
    if (closed || !dialog.open || !wantsPlayback || !currentSource()) return;
    if (video.currentTime > lastMediaTime + 0.01) {
      lastMediaTime = video.currentTime;
      clearRecovery();
      armRecovery();
    }
  });
  video.addEventListener("ended", () => {
    if (closed || !dialog.open || !currentSource() || !video.ended) return;
    wantsPlayback = false; clearRecovery(); sync();
  });
  video.onplaying = () => {
    if (closed || !dialog.open || !wantsPlayback || !currentSource()) return;
    status.textContent = ""; sync();
  };
  video.onwaiting = video.onstalled = () => {
    if (closed || !dialog.open || !wantsPlayback || !currentSource()) return;
    status.textContent = "正在缓冲…"; armRecovery();
  };
  video.onerror = () => {
    if (closed || !dialog.open || !video.getAttribute("src") || !video.error || !currentSource()) return;
    if (video.error.code === 2) {
      if (wantsPlayback) recover();
      else fail("播放线路暂不可用，请重试或在电脑端查看");
    } else fail(video.error.code === 1 ? "播放已取消，请重试" : "视频无法播放，格式可能不受支持或文件已不可用");
  };
  // 用 click 关闭遮罩，避免关闭后的合成点击落到会话页面。
  let backdropPressed = false;
  const onBackdrop = event => {
    const rect = dialog.getBoundingClientRect();
    return event.target === dialog && (event.clientX < rect.left || event.clientX > rect.right
      || event.clientY < rect.top || event.clientY > rect.bottom);
  };
  dialog.onpointerdown = event => { backdropPressed = onBackdrop(event); };
  dialog.onpointermove = event => { if (!onBackdrop(event)) backdropPressed = false; };
  dialog.onpointercancel = () => { backdropPressed = false; };
  dialog.onclick = event => {
    if (backdropPressed && onBackdrop(event)) {
      event.preventDefault(); event.stopPropagation(); dialog.close();
    }
    backdropPressed = false;
  };
  dialog.oncancel = event => { event.preventDefault(); dialog.close(); };
  const stop = () => {
    if (closed) return;
    closed = true;
    wantsPlayback = false;
    ++attempt;
    ++playRequest;
    clearRecovery();
    video.pause();
    video.removeAttribute("src");
    video.load();
  };
  const hide = () => {
    if (document.visibilityState !== "hidden") return;
    wantsPlayback = false;
    ++playRequest;
    clearRecovery();
    video.pause();
    status.textContent = "已暂停";
    sync();
  };
  const leave = () => dialog.close();
  window.addEventListener("pagehide", leave);
  document.addEventListener("visibilitychange", hide);
  dialog.onclose = () => {
    layout.dispose();
    cover.posterRecovery?.dispose();
    cover.removeAttribute("src");
    stop();
    window.removeEventListener("pagehide", leave);
    document.removeEventListener("visibilitychange", hide);
    dialog.remove();
    document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
    if (trigger?.isConnected) trigger.focus({ preventScroll: true });
  };
  controls.append(play, progress, time);
  panel.append(close, video, controls, status, title);
  dialog.append(panel);
  document.body.append(dialog);
  document.body.classList.add("modal-open");
  dialog.showModal();
  layout.resize();
  play.focus();
}

export function bindMessageVideos(root, prepareAccess = () => {}) {
  for (const button of root.querySelectorAll(".message-video-preview")) {
    const poster = button.querySelector(".message-video-poster");
    const posterUrl = poster?.getAttribute("data-remote-media") || poster?.getAttribute("src") || "";
    const source = posterUrl.replace(/\/poster$/u, "");
    if (!remoteVideoPattern.test(source) || posterUrl !== `${source}/poster`) continue;
    const name = button.querySelector(".message-video-name")?.textContent || "视频";
    button.setAttribute("aria-label", `预览视频：${name}`);
    button.setAttribute("aria-haspopup", "dialog");
    loadVideoPoster(poster, posterUrl);
    button.onclick = event => {
      event.preventDefault(); event.stopPropagation();
      openVideoPreview(button, source, posterUrl, name, prepareAccess);
    };
  }
}

export function retryOriginalImage(image) {
  const source = image.originalImageSource;
  if (!source) return false;
  const recovery = image.attachmentRecovery;
  if (!recovery || recovery.source !== source) return false;
  const selectedSource = recovery.candidates.find(value => !recovery.attempted.has(value));
  if (!selectedSource) return false;
  recovery.attempted.add(selectedSource);
  image.src = selectedSource;
  return true;
}

export function loadAttachmentThumbnail(image, source, mime = "") {
  image.originalImageSource = source;
  const thumbnail = remoteImagePattern.test(source) && mime !== "image/gif" ? `${source}/thumbnail` : source;
  if (remoteImagePattern.test(source)) {
    // 第二线路仍由服务端校验 Origin 和媒体凭证；无 CORS 的图片会被 CORP 拦截。
    image.crossOrigin = "anonymous";
    image.setAttribute("data-remote-media", source);
  }
  const candidates = remoteImagePattern.test(source)
    ? [...remoteMediaCandidates(thumbnail), ...remoteMediaCandidates(source)]
    : [remoteMediaUrl(source)];
  image.attachmentRecovery = { source, candidates: [...new Set(candidates)], attempted: new Set() };
  // 每条缩略图线路只尝试一次，再回退原图；显式重试时重新创建尝试列表。
  image.onerror = () => { retryOriginalImage(image); };
  if (!retryOriginalImage(image)) {
    const recovery = image.attachmentRecovery;
    queueMicrotask(() => {
      if (image.attachmentRecovery === recovery) image.dispatchEvent(new Event("error"));
    });
  }
}

let attachments = [];
let attachmentRoot = null;
let attachmentInput = null;
let composerPrompt = null;
let changeListener = () => {};
let showNotice = () => {};
let previewImage = () => {};
let disabled = false;
let locked = false;
let remoteRemovalAllowed = false;
let readingFiles = false;
let attachmentVersion = 0;

function attachmentName(value, fallback = "图片") {
  const name = String(value || "").replace(/[\u0000-\u001f\u007f]/gu, "").trim();
  return name ? name.slice(0, 255) : fallback;
}

function copyAttachment(entry) {
  return entry.kind === "inline"
    ? {
        kind: "inline",
        name: entry.name,
        mime: entry.mime,
        dataUrl: entry.dataUrl,
        size: entry.size,
        width: entry.width,
        height: entry.height
      }
    : {
        kind: "remote",
        name: entry.name,
        source: entry.source,
        url: entry.url,
        mime: entry.mime || ""
      };
}

function safeRemoteImages(value) {
  if (!Array.isArray(value)) return [];
  const seen = new Set();
  return value.flatMap(image => {
    if (!image || typeof image.source !== "string" || !remoteImagePattern.test(image.url)
        || seen.has(image.url)) return [];
    seen.add(image.url);
    return [{
      kind: "remote",
      source: image.source,
      url: image.url,
      mime: typeof image.mime === "string" ? image.mime : "",
      name: attachmentName(image.name || image.source.split(/[\\/]/u).at(-1), "图片附件")
    }];
  });
}

function detectImageMime(bytes) {
  if (bytes.length >= 8 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e
      && bytes[3] === 0x47 && bytes[4] === 0x0d && bytes[5] === 0x0a
      && bytes[6] === 0x1a && bytes[7] === 0x0a) return "image/png";
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff)
    return "image/jpeg";
  const text = length => String.fromCharCode(...bytes.slice(0, length));
  if (bytes.length >= 6 && ["GIF87a", "GIF89a"].includes(text(6))) return "image/gif";
  if (bytes.length >= 12 && text(4) === "RIFF"
      && String.fromCharCode(...bytes.slice(8, 12)) === "WEBP") return "image/webp";
  return "";
}

export function safeInlineImageSource(source) {
  if (typeof source !== "string" || source.length > Math.ceil(25 * 1024 * 1024 / 3) * 4 + 64) return null;
  const header = /^data:(image\/(?:png|jpeg|webp|gif));base64,/iu.exec(source);
  if (!header) return null;
  const encoded = source.slice(header[0].length);
  if (!encoded.length || encoded.length % 4 || !/^[A-Za-z0-9+/]+={0,2}$/u.test(encoded)) return null;
  const padding = encoded.endsWith("==") ? 2 : encoded.endsWith("=") ? 1 : 0;
  if (encoded.length / 4 * 3 - padding > 25 * 1024 * 1024) return null;
  try {
    const bytes = Uint8Array.from(atob(encoded.slice(0, 16)), character => character.charCodeAt(0));
    return detectImageMime(bytes) === header[1].toLowerCase() ? source : null;
  } catch {
    return null;
  }
}

function dataUrl(blob, mime) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("无法读取图片"));
    reader.onload = () => {
      const encoded = String(reader.result || "").split(",", 2)[1];
      if (!encoded) reject(new Error("无法读取图片"));
      else resolve(`data:${mime};base64,${encoded}`);
    };
    reader.readAsDataURL(blob);
  });
}

async function imageDimensions(blob) {
  if (typeof createImageBitmap === "function") {
    const bitmap = await createImageBitmap(blob);
    try { return { width: bitmap.width, height: bitmap.height }; }
    finally { bitmap.close(); }
  }
  return await new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      resolve({ width: image.naturalWidth, height: image.naturalHeight });
    };
    image.onerror = () => reject(new Error("图片内容损坏或无法解码"));
    void dataUrl(blob, blob.type || "image/png").then(source => { image.src = source; }, reject);
  });
}

async function readImageFile(file) {
  if (!(file instanceof Blob)) throw new Error("请选择图片文件");
  if (file.size <= 0) throw new Error("图片文件为空");
  if (file.size > MAX_IMAGE_BYTES) throw new Error("单张图片不能超过 5 MiB");
  const bytes = new Uint8Array(await file.arrayBuffer());
  const mime = detectImageMime(bytes);
  if (!supportedMimes.has(mime)) throw new Error("仅支持 PNG、JPEG、WebP 和 GIF 图片");
  if (file.type && supportedMimes.has(file.type.toLowerCase()) && file.type.toLowerCase() !== mime)
    throw new Error("图片扩展类型与实际内容不一致");
  const blob = new Blob([bytes], { type: mime });
  const dimensions = await imageDimensions(blob);
  if (!dimensions.width || !dimensions.height
      || dimensions.width > MAX_IMAGE_DIMENSION || dimensions.height > MAX_IMAGE_DIMENSION
      || dimensions.width * dimensions.height > MAX_IMAGE_PIXELS)
    throw new Error("图片尺寸过大，最大边不能超过 16384 像素");
  return {
    kind: "inline",
    name: attachmentName(file.name, `图片.${mime.split("/")[1]}`),
    mime,
    dataUrl: await dataUrl(blob, mime),
    size: file.size,
    ...dimensions
  };
}

function renderComposerAttachments() {
  if (!attachmentRoot) return;
  attachmentRoot.replaceChildren();
  for (const [index, entry] of attachments.entries()) {
    const item = document.createElement("figure");
    item.className = "composer-attachment";
    const image = document.createElement("img");
    image.className = "composer-attachment-image";
    loadAttachmentThumbnail(image, entry.kind === "inline" ? entry.dataUrl : entry.url, entry.mime);
    image.alt = entry.name;
    image.title = entry.name;
    image.decoding = "async";
    const preview = document.createElement("button");
    preview.type = "button";
    preview.className = "composer-attachment-preview";
    preview.setAttribute("aria-label", `查看图片 ${entry.name}`);
    preview.setAttribute("aria-haspopup", "dialog");
    preview.title = `查看图片 ${entry.name}`;
    preview.onclick = () => previewImage(preview, image);
    preview.append(image);
    const caption = document.createElement("figcaption");
    caption.textContent = entry.name;
    item.append(preview, caption);
    const canRemove = !locked || remoteRemovalAllowed && entry.kind === "remote";
    if (canRemove) {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "composer-attachment-remove";
      remove.setAttribute("aria-label", `移除图片 ${entry.name}`);
      remove.title = "移除图片";
      remove.innerHTML = '<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="m6 6 12 12M18 6 6 18"/></svg>';
      remove.disabled = disabled || readingFiles;
      remove.onclick = () => {
        if (disabled || readingFiles || locked && !(remoteRemovalAllowed && entry.kind === "remote")) return;
        attachments.splice(index, 1);
        renderComposerAttachments();
        changeListener();
      };
      item.append(remove);
    }
    attachmentRoot.append(item);
  }
  attachmentRoot.hidden = attachments.length === 0;
  attachmentRoot.classList.toggle("attachments-locked", locked);
  if (attachmentInput) attachmentInput.disabled = disabled || locked || readingFiles;
}

async function addFiles(files) {
  const candidates = [...files];
  if (!candidates.length) return false;
  if (disabled || locked || readingFiles) {
    showNotice(locked
      ? remoteRemovalAllowed ? "编辑排队消息时只能删除原图片，不能添加图片" : "编辑时保留原附件，不能增删图片"
      : "当前不能添加图片", true);
    return true;
  }
  if (attachments.length + candidates.length > MAX_IMAGE_COUNT) {
    showNotice(`每条消息最多添加 ${MAX_IMAGE_COUNT} 张图片`, true);
    return true;
  }
  const currentBytes = attachments.reduce((total, entry) =>
    total + (entry.kind === "inline" ? entry.size : 0), 0);
  if (currentBytes + candidates.reduce((total, file) => total + file.size, 0) > MAX_TOTAL_BYTES) {
    showNotice("图片总大小不能超过 12 MiB", true);
    return true;
  }
  readingFiles = true;
  renderComposerAttachments();
  changeListener();
  const version = attachmentVersion;
  try {
    const next = [], errors = [];
    for (const file of candidates) {
      try { next.push(await readImageFile(file)); }
      catch (error) { errors.push(`${attachmentName(file?.name)}：${error?.message || "无法添加图片"}`); }
      if (version !== attachmentVersion) return true;
    }
    attachments.push(...next);
    renderComposerAttachments();
    changeListener();
    if (errors.length) showNotice(`已添加 ${next.length} 张图片；${errors.join("；")}`, true);
  } catch (error) {
    showNotice(error?.message || "无法添加图片", true);
  } finally {
    if (version === attachmentVersion) {
      readingFiles = false;
      if (attachmentInput) attachmentInput.value = "";
      renderComposerAttachments();
      changeListener();
    }
  }
  return true;
}

export function mountComposerAttachments({ form, prompt, onChange, notice, onPreview } = {}) {
  if (!(form instanceof HTMLElement) || !(prompt instanceof HTMLElement))
    throw new Error("图片附件组件缺少编辑框");
  composerPrompt = prompt;
  changeListener = typeof onChange === "function" ? onChange : () => {};
  showNotice = typeof notice === "function" ? notice : () => {};
  previewImage = typeof onPreview === "function" ? onPreview : () => {};
  attachmentRoot = document.createElement("div");
  attachmentRoot.id = "composer-attachments";
  attachmentRoot.className = "composer-attachments";
  attachmentRoot.setAttribute("aria-label", "待发送图片");
  attachmentRoot.hidden = true;
  prompt.before(attachmentRoot);

  attachmentInput = document.createElement("input");
  attachmentInput.id = "composer-image-input";
  attachmentInput.type = "file";
  attachmentInput.accept = "image/*";
  attachmentInput.multiple = true;
  attachmentInput.hidden = true;
  attachmentInput.setAttribute("aria-hidden", "true");
  attachmentInput.onchange = event => void addFiles([...(event.target.files || [])]);
  form.append(attachmentInput);

  form.addEventListener("dragover", event => {
    if (![...event.dataTransfer?.items || []].some(item => item.kind === "file"
        && supportedMimes.has(item.type.toLowerCase()))) return;
    event.preventDefault();
    form.classList.add("composer-image-dragover");
  });
  form.addEventListener("dragleave", event => {
    if (!form.contains(event.relatedTarget)) form.classList.remove("composer-image-dragover");
  });
  form.addEventListener("drop", event => {
    form.classList.remove("composer-image-dragover");
    const files = [...event.dataTransfer?.files || []];
    if (!files.some(file => supportedMimes.has(file.type.toLowerCase()))) return;
    event.preventDefault();
    void addFiles(files);
  });
  renderComposerAttachments();
}

export function handleComposerImagePaste(event) {
  const files = [...event?.clipboardData?.files || []];
  if (!files.some(file => supportedMimes.has(file.type.toLowerCase()))) return false;
  event.preventDefault();
  void addFiles(files);
  return true;
}

export function composerHasAttachments() {
  return attachments.length > 0;
}

export function composerAttachmentsLoading() {
  return readingFiles;
}

export function composerCanAddImages() {
  return !disabled && !locked && !readingFiles;
}

export function chooseComposerImages() {
  if (composerCanAddImages()) attachmentInput?.click();
}

export function composerAttachmentSnapshot() {
  return attachments.map(copyAttachment);
}

export function composerAttachmentsPayload() {
  return attachments.flatMap(entry => entry.kind === "inline"
    ? [{ name: entry.name, mime: entry.mime, dataUrl: entry.dataUrl, size: entry.size }]
    : []);
}

export function composerRemoteAttachmentSources() {
  return attachments.flatMap(entry =>
    entry.kind === "remote" && typeof entry.source === "string" ? [entry.source] : []);
}

export function setComposerAttachments(value = [], { readOnly = false, removableRemote = false } = {}) {
  attachmentVersion += 1;
  readingFiles = false;
  attachments = Array.isArray(value) ? value.flatMap(entry => {
    if (entry?.kind === "inline" && supportedMimes.has(entry.mime)
        && typeof entry.dataUrl === "string" && Number.isFinite(entry.size))
      return [copyAttachment(entry)];
    if (entry?.kind === "remote" && (remoteImagePattern.test(entry.url) || safeInlineImageSource(entry.url)))
      return [copyAttachment(entry)];
    return [];
  }).slice(0, MAX_IMAGE_COUNT) : [];
  locked = readOnly;
  remoteRemovalAllowed = readOnly && removableRemote;
  renderComposerAttachments();
}

export function setComposerAttachmentsDisabled(value) {
  disabled = value === true;
  renderComposerAttachments();
}

export function clearComposerAttachments() {
  setComposerAttachments([]);
}

export function attachmentsFromRemoteImages(value) {
  return safeRemoteImages(value);
}

export async function attachmentsFromQueuedMessage(message) {
  const registeredImages = safeRemoteImages(message?.remoteImages);
  const images = message?.context?.imageAttachments;
  if (!Array.isArray(images)) return registeredImages;
  const attachments = [];
  for (const image of images) {
    const registered = registeredImages.find(entry => entry.source === image?.src);
    if (registered) {
      attachments.push({ ...registered, name: attachmentName(image.filename || image.name, registered.name) });
      continue;
    }
    const source = safeInlineImageSource(image?.src);
    if (!source) continue;
    const binary = atob(source.slice(source.indexOf(",") + 1));
    const bytes = Uint8Array.from(binary, character => character.charCodeAt(0));
    const reference = `inline:${await sha256Hex(bytes)}`;
    attachments.push(registeredImages.find(entry => entry.source === reference) || {
      kind: "remote",
      name: attachmentName(image.filename || image.name, "图片附件"),
      source: reference,
      url: source,
      mime: source.slice(5, source.indexOf(";")).toLowerCase()
    });
  }
  return attachments;
}

export function renderQueuedMessageAttachments(message) {
  const registeredImages = safeRemoteImages(message?.remoteImages);
  const images = [];
  const inlineAttachments = message?.context?.imageAttachments;
  for (const attachment of Array.isArray(inlineAttachments) ? inlineAttachments : []) {
    if (images.some(image => image.source === attachment?.src)) continue;
    const registered = registeredImages.find(image => image.source === attachment?.src);
    if (registered) { images.push(registered); continue; }
    const url = safeInlineImageSource(attachment?.src);
    if (url) images.push({ source: attachment.src, url, name: attachmentName(attachment.name, "图片附件") });
  }
  for (const image of registeredImages)
    if (!images.some(entry => entry.url === image.url)) images.push(image);
  if (!images.length) return null;
  const root = document.createElement("div");
  root.className = "queued-message-attachments";
  root.setAttribute("aria-label", `${images.length} 张图片附件，仅显示第一张`);
  const image = images[0];
  const thumbnail = document.createElement("img");
  thumbnail.className = "queued-message-attachment";
  loadAttachmentThumbnail(thumbnail, image.url, image.mime);
  thumbnail.alt = image.name;
  thumbnail.title = image.name;
  thumbnail.loading = "lazy";
  thumbnail.decoding = "async";
  thumbnail.draggable = false;
  root.append(thumbnail);
  return root;
}
