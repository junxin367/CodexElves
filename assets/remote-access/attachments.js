const MAX_IMAGE_COUNT = 4;
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const MAX_TOTAL_BYTES = 12 * 1024 * 1024;
const MAX_IMAGE_DIMENSION = 16_384;
const MAX_IMAGE_PIXELS = 100_000_000;
const remoteImagePattern = /^\/api\/images\/[A-Za-z0-9_-]{20,128}$/u;
const supportedMimes = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);

let attachments = [];
let attachmentRoot = null;
let attachmentInput = null;
let attachmentButton = null;
let composerPrompt = null;
let changeListener = () => {};
let showNotice = () => {};
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
    image.src = entry.kind === "inline" ? entry.dataUrl : entry.url;
    image.alt = entry.name;
    image.title = entry.name;
    image.decoding = "async";
    const caption = document.createElement("figcaption");
    caption.textContent = entry.name;
    item.append(image, caption);
    const canRemove = !locked || remoteRemovalAllowed && entry.kind === "remote";
    if (canRemove) {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "composer-attachment-remove";
      remove.setAttribute("aria-label", `移除图片 ${entry.name}`);
      remove.title = "移除图片";
      remove.textContent = "×";
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
  if (attachmentButton) {
    attachmentButton.disabled = disabled || locked || readingFiles;
    attachmentButton.hidden = locked;
  }
  if (attachmentInput) attachmentInput.disabled = disabled || locked || readingFiles;
}

async function addFiles(files) {
  const candidates = [...files].filter(file => supportedMimes.has(String(file?.type || "").toLowerCase())
    || /\.(?:png|jpe?g|webp|gif)$/iu.test(String(file?.name || "")));
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
    const next = [];
    for (const file of candidates) next.push(await readImageFile(file));
    if (version !== attachmentVersion) return true;
    attachments.push(...next);
    renderComposerAttachments();
    changeListener();
  } catch (error) {
    showNotice(error?.message || "无法添加图片", true);
  } finally {
    readingFiles = false;
    if (attachmentInput) attachmentInput.value = "";
    renderComposerAttachments();
    changeListener();
  }
  return true;
}

export function mountComposerAttachments({ form, prompt, onChange, notice } = {}) {
  if (!(form instanceof HTMLElement) || !(prompt instanceof HTMLElement))
    throw new Error("图片附件组件缺少编辑框");
  composerPrompt = prompt;
  changeListener = typeof onChange === "function" ? onChange : () => {};
  showNotice = typeof notice === "function" ? notice : () => {};
  attachmentRoot = document.createElement("div");
  attachmentRoot.id = "composer-attachments";
  attachmentRoot.className = "composer-attachments";
  attachmentRoot.setAttribute("aria-label", "待发送图片");
  attachmentRoot.hidden = true;
  prompt.before(attachmentRoot);

  attachmentInput = document.createElement("input");
  attachmentInput.type = "file";
  attachmentInput.accept = "image/png,image/jpeg,image/webp,image/gif";
  attachmentInput.multiple = true;
  attachmentInput.hidden = true;
  attachmentInput.setAttribute("aria-hidden", "true");
  attachmentInput.onchange = () => void addFiles(attachmentInput.files || []);
  form.append(attachmentInput);

  attachmentButton = document.createElement("button");
  attachmentButton.id = "attach-image";
  attachmentButton.type = "button";
  attachmentButton.className = "icon-button attach-image-button";
  attachmentButton.setAttribute("aria-label", "选择图片");
  attachmentButton.title = "选择图片";
  attachmentButton.textContent = "＋";
  attachmentButton.onclick = () => attachmentInput.click();
  form.querySelector(".composer-controls")?.prepend(attachmentButton);

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
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
    const reference = `inline:${[...digest].map(byte => byte.toString(16).padStart(2, "0")).join("")}`;
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
  thumbnail.src = image.url;
  thumbnail.alt = image.name;
  thumbnail.title = image.name;
  thumbnail.loading = "lazy";
  thumbnail.decoding = "async";
  thumbnail.draggable = false;
  root.append(thumbnail);
  return root;
}
