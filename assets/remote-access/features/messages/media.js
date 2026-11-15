// 消息媒体与 Markdown：图片预览缩放、消息图片刷新、Markdown/Mermaid 渲染与图片视图。

import {
  bindMediaPreviewDismiss, bindMessageVideos, fitMediaPreview, loadAttachmentThumbnail,
  mediaPreviewContainsPoint, requestWorkspaceMediaPreview, retryOriginalImage,
  safeInlineImageSource
} from "../../attachments.js";
import { prepareRemoteTunnelAccess, remoteMediaUrl } from "../../channels.js";
import {
  bindLocalFileReferences, markdown, prepareMessageImage, renderMermaidBlocks, settleMessageImage,
  withStableMessageLayout
} from "../../markdown.js";
import { copyText } from "../../ui/clipboard.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { pageLeaving, selectedId, selectionVersion, session, turns } from "../../core/state.js";
import { api } from "../api.js";
import { skillToken } from "../composer/prompt-editor.js";
import {
  followMessages, rememberTranscript, renderedItems, renderMessages, scheduleHistoryLoad,
  setHistoryScrollTop, updateScrollControl
} from "../conversation/history-view.js";
import { mergeRemoteImageProjection } from "../conversation/history.js";
import { openLocalFileInBrowser } from "../local-files.js";
import { reconcileChildren } from "./items.js";
import { privateInputScope } from "../session/capabilities.js";
import { rememberTransientHistory } from "../session/related.js";
import { composerScroll, sessionTools } from "../shell.js";

export const messageImageMediaReads = new WeakMap(), messageImageRefreshes = new Map();
export let messageImageReadSequence = 0;
export function setMessageImageReadSequence(value) { return messageImageReadSequence = value; }
export let markdownRenderVersion = 0;
let mermaidScanPending = false;
function enableImagePreviewZoom(viewport, image) {
  const pointers = new Map();
  const fittedScale = () => Math.min(1, viewport.clientWidth / image.naturalWidth, viewport.clientHeight / image.naturalHeight);
  let minScale = fittedScale(), scale = minScale, x = 0, y = 0;
  const render = () => {
    const limitX = Math.max(0, (image.naturalWidth * scale - viewport.clientWidth) / 2);
    const limitY = Math.max(0, (image.naturalHeight * scale - viewport.clientHeight) / 2);
    x = Math.max(-limitX, Math.min(limitX, x));
    y = Math.max(-limitY, Math.min(limitY, y));
    image.style.transform = `translate(-50%, -50%) translate(${x}px, ${y}px) scale(${scale})`;
    viewport.classList.toggle("image-preview-zoomed", scale > minScale);
  };
  const point = (clientX, clientY) => {
    const box = viewport.getBoundingClientRect();
    return { x: clientX - box.left - box.width / 2, y: clientY - box.top - box.height / 2 };
  };
  const zoomAt = (nextScale, center = { x: 0, y: 0 }) => {
    nextScale = Math.max(minScale, nextScale);
    const ratio = nextScale / scale;
    x = center.x - (center.x - x) * ratio;
    y = center.y - (center.y - y) * ratio;
    scale = nextScale;
    render();
  };
  const reset = () => { scale = minScale; x = 0; y = 0; render(); };
  const gesture = () => {
    const [first, second] = pointers.values();
    return {
      ...point(second ? (first.x + second.x) / 2 : first.x, second ? (first.y + second.y) / 2 : first.y),
      distance: second ? Math.hypot(first.x - second.x, first.y - second.y) : 0
    };
  };
  viewport.onpointerdown = event => {
    if (event.pointerType !== "touch" && event.button !== 0) return;
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    // 用户要求图片周围空白可关闭；单指按在空白时不捕获单击，仍记录触点以保留跨空白的双指缩放。
    if (pointers.size === 1 && !mediaPreviewContainsPoint(image, viewport, event.clientX, event.clientY)) return;
    event.preventDefault();
    viewport.focus({ preventScroll: true });
    viewport.setPointerCapture(event.pointerId);
  };
  viewport.onpointermove = event => {
    if (!pointers.has(event.pointerId)) return;
    event.preventDefault();
    const before = gesture();
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const after = gesture();
    // 默认完整显示图片，双指缩小后也不能小于适应当前视口的尺寸。
    const nextScale = Math.max(minScale, before.distance ? scale * after.distance / before.distance : scale);
    const ratio = nextScale / scale;
    x = after.x - (before.x - x) * ratio;
    y = after.y - (before.y - y) * ratio;
    scale = nextScale;
    render();
  };
  const release = event => { pointers.delete(event.pointerId); };
  viewport.onpointerup = release;
  viewport.onpointercancel = release;
  viewport.onlostpointercapture = release;
  const wheel = event => {
    if (!event.deltaY) return;
    event.preventDefault();
    const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? viewport.clientHeight : 1);
    zoomAt(scale * Math.exp(-Math.max(-400, Math.min(400, delta)) * .002), point(event.clientX, event.clientY));
  };
  viewport.addEventListener("wheel", wheel, { passive: false });
  viewport.ondblclick = event => {
    event.preventDefault();
    if (scale > minScale) reset();
    else zoomAt(scale * 2, point(event.clientX, event.clientY));
  };
  viewport.onkeydown = event => {
    if (event.ctrlKey || event.metaKey || event.altKey || !["+", "=", "-", "0"].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "0") reset();
    else zoomAt(scale * (event.key === "-" ? 1 / 1.25 : 1.25));
  };
  const resizeObserver = new ResizeObserver(() => {
    pointers.clear();
    const nextMinScale = fittedScale();
    if (scale === minScale) {
      scale = nextMinScale;
      x = 0; y = 0;
    } else scale = Math.max(nextMinScale, scale);
    minScale = nextMinScale;
    render();
  });
  resizeObserver.observe(viewport);
  render();
  return () => { resizeObserver.disconnect(); viewport.removeEventListener("wheel", wheel); pointers.clear(); };
}
export function openImagePreview(link, image) {
  const source = link.getAttribute("href") || image.originalImageSource || image.getAttribute("src") || "";
  const remoteSource = /^\/api\/images\/[A-Za-z0-9_-]{20,128}$/.test(source);
  const inlineSource = safeInlineImageSource(source, { allowSvg: true });
  if (!remoteSource && !inlineSource) return;
  if (requestWorkspaceMediaPreview({ kind: "image", source: remoteSource ? source : inlineSource, name: image.alt || "图片" })) return;
  $("#image-preview-dialog")?.close();
  rememberTransientHistory();
  const trigger = link;
  const dialog = element("dialog", "image-preview-dialog media-preview-loading");
  dialog.id = "image-preview-dialog";
  dialog.setAttribute("aria-label", image.alt || "图片预览");
  dialog.setAttribute("aria-busy", "true");
  const panel = element("figure", "image-preview-panel");
  const close = button("", "关闭图片预览", () => dialog.close(), "image-preview-close");
  close.append(icon("close"));
  const loading = element("div", "media-preview-loading-indicator");
  loading.hidden = true;
  const loadingStatus = element("span", "media-preview-loading-label", "正在加载图片…");
  loadingStatus.setAttribute("role", "status");
  loadingStatus.prepend(element("span", "spinner"));
  const cancel = button("取消", "取消加载图片", () => dialog.close(), "media-preview-loading-cancel");
  loading.append(loadingStatus, cancel);
  const preview = element("img", "image-preview-image");
  const viewport = element("div", "image-preview-viewport");
  viewport.tabIndex = 0;
  viewport.setAttribute("role", "group");
  viewport.setAttribute("aria-label", "图片预览，可双指或滚轮缩放、拖动查看，按 0 恢复完整显示");
  viewport.title = "双指或滚轮缩放，拖动查看；双击缩放或恢复完整显示";
  const caption = element("figcaption", "image-preview-caption", image.alt || "图片");
  const layout = fitMediaPreview(dialog, panel, viewport, null, { fillAvailable: true });
  let disposeZoom = null, presenting = false, loadingTimer = null;
  preview.draggable = false;
  preview.decoding = "async";
  const initializeZoom = () => {
    if (!dialog.open || disposeZoom || !preview.complete
        || !preview.naturalWidth || !preview.naturalHeight) return;
    disposeZoom = enableImagePreviewZoom(viewport, preview);
    preview.classList.add("image-preview-ready");
  };
  const focusControl = control => {
    const focusedDialog = document.activeElement?.closest("dialog");
    if (!focusedDialog || focusedDialog === dialog) control.focus({ preventScroll: true });
  };
  const finishLoading = () => {
    clearTimeout(loadingTimer);
    loadingTimer = null;
    dialog.classList.remove("media-preview-loading");
    dialog.classList.add("image-preview-presented");
    dialog.removeAttribute("aria-busy");
    focusControl(close);
    loading.remove();
  };
  const present = async () => {
    if (presenting || !dialog.open || !preview.complete || !preview.naturalWidth || !preview.naturalHeight) return;
    presenting = true;
    try { await preview.decode(); } catch {}
    if (!dialog.open) return;
    layout.resize();
    initializeZoom();
    finishLoading();
  };
  preview.onload = present;
  if (remoteSource) preview.crossOrigin = "anonymous";
  preview.alt = image.alt || "图片预览"; preview.src = remoteMediaUrl(remoteSource ? source : inlineSource);
  preview.onerror = () => {
    disposeZoom?.();
    viewport.textContent = "图片不可用，请关闭后重试";
    viewport.classList.add("image-preview-unavailable");
    caption.textContent = `图片不可用：${image.alt || "附件"}`;
    finishLoading();
  };
  viewport.append(preview);
  panel.append(close, viewport, caption); dialog.append(panel, loading); document.body.append(dialog);
  const disposeDismiss = bindMediaPreviewDismiss(dialog, preview, viewport);
  dialog.oncancel = event => { event.preventDefault(); dialog.close(); };
  dialog.onclose = () => {
    clearTimeout(loadingTimer);
    disposeDismiss();
    layout.dispose();
    disposeZoom?.();
    dialog.remove();
    document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
    if (trigger.isConnected && document.activeElement === document.body) trigger.focus({ preventScroll: true });
  };
  document.body.classList.add("modal-open");
  dialog.showModal();
  layout.resize();
  // 缓存命中时跳过短暂的 loading，慢速加载仍可取消。
  loadingTimer = setTimeout(() => {
    loadingTimer = null;
    if (!dialog.open || !dialog.classList.contains("media-preview-loading")) return;
    loading.hidden = false;
    focusControl(cancel);
  }, 150);
  void present();
  return { close: () => { if (dialog.open) dialog.close(); } };
}
function messageImageOwner(node, url) {
  const message = node.closest("[data-key]"), turnId = message?.closest(".turn[data-turn-id]")?.dataset.turnId;
  const rendered = renderedItems.get(message?.dataset.key);
  const item = turns.get(turnId)?.items?.find(item => item.id === rendered?.item?.id
    && message.dataset.key === `${turnId}:${item.id}`);
  const asset = item?.remoteImages?.find(asset => asset.url === url && typeof asset.source === "string");
  if (!selectedId || !turnId || !item || !asset) return null;
  return { threadId: selectedId, turnId, itemId: item.id, source: asset.source,
    scope: privateInputScope(session), selection: selectionVersion };
}
function currentMessageImageOwner(owner) {
  return owner && owner.threadId === selectedId && owner.selection === selectionVersion
    && owner.scope === privateInputScope(session) && !pageLeaving;
}
async function refreshMessageImageAsset(owner) {
  if (!currentMessageImageOwner(owner)) return null;
  const key = JSON.stringify([owner.scope, owner.selection, owner.threadId, owner.turnId]);
  let request = messageImageRefreshes.get(key);
  if (!request) {
    request = { controller: new AbortController(), targets: new Map(), promise: null,
      mediaReadVersion: ++messageImageReadSequence };
    messageImageRefreshes.set(key, request);
    request.promise = Promise.resolve().then(async () => {
      const snapshots = new Map(), cursors = new Set(), itemIds = new Set();
      let cursor = null;
      // 用户要求重试旧图片时重读所属真实轮次；最多 12 页，避免服务忽略游标或异常分页拖住页面。
      // 这里只更新同消息、同 source 的媒体映射，绝不合并正文、状态、发送回执或主历史游标。
      for (let page = 0; page < 12 && currentMessageImageOwner(owner); page++) {
        const turn = turns.get(owner.turnId);
        const result = await api("/api/read/history", {
          threadId: owner.threadId, turnId: owner.turnId, itemCursor: cursor,
          turnStatus: turn?.status, turnStartedAt: turn?.startedAt, turnCompletedAt: turn?.completedAt
        }, undefined, request.controller.signal);
        if (!currentMessageImageOwner(owner) || request.controller.signal.aborted) return null;
        if (result?.scope !== "items" || result.threadId !== owner.threadId
            || result.turnId !== owner.turnId || !Array.isArray(result.data))
          throw new Error("图片历史返回内容与当前轮次不一致");
        let added = false;
        for (const item of result.data) {
          if (typeof item?.id !== "string") continue;
          if (!itemIds.has(item.id)) { itemIds.add(item.id); added = true; }
          snapshots.set(item.id, item);
        }
        if ([...request.targets].every(([id, sources]) => [...sources].every(source =>
          snapshots.get(id)?.remoteImages?.some(asset => asset.source === source)))) break;
        const next = result.itemsNextCursor;
        if (!added || typeof next !== "string" || !next || next === cursor || cursors.has(next)) break;
        cursors.add(next); cursor = next;
      }
      if (!currentMessageImageOwner(owner) || request.controller.signal.aborted) return null;
      const turn = turns.get(owner.turnId);
      if (!turn) return null;
      const items = (turn.items || []).map(item =>
        mergeRemoteImageProjection(item, snapshots.get(item.id), item, request.mediaReadVersion));
      if (items.some((item, index) => item !== turn.items[index])) {
        turns.set(owner.turnId, { ...turn, items });
        renderMessages();
        rememberTranscript();
      }
      return snapshots;
    }).finally(() => { if (messageImageRefreshes.get(key) === request) messageImageRefreshes.delete(key); });
  }
  if (!request.targets.has(owner.itemId)) request.targets.set(owner.itemId, new Set());
  request.targets.get(owner.itemId).add(owner.source);
  const snapshots = await request.promise;
  if (!snapshots || !currentMessageImageOwner(owner)
      || !snapshots.get(owner.itemId)?.remoteImages?.some(asset => asset.source === owner.source)) return null;
  return turns.get(owner.turnId)?.items?.find(item => item.id === owner.itemId)
    ?.remoteImages?.find(asset => asset.source === owner.source) || null;
}
function bindMessageImages(node) {
  for (const image of node.querySelectorAll(".message-image")) {
    const link = image.closest(".message-image-link");
    const source = image.getAttribute("data-remote-media");
    if (source && !image.getAttribute("src")) loadAttachmentThumbnail(image, source);
    prepareMessageImage(image);
    const updateLayout = change => {
      const root = $("#messages");
      if (root?.isConnected && image.isConnected && root.contains(image))
        withStableMessageLayout(root, change, { follow: followMessages && !composerScroll?.collapsed });
      else change();
    };
    image.onload = () => updateLayout(() => settleMessageImage(image));
    image.onerror = () => {
      if (retryOriginalImage(image)) return;
      updateLayout(() => {
        settleMessageImage(image);
        const source = image.originalImageSource || link?.getAttribute("href") || image.getAttribute("src");
        const placeholder = element("button", "attachment-placeholder image-load-retry", "重试图片");
        placeholder.type = "button";
        placeholder.title = `图片不可用：${image.alt || "附件"}，点击重新加载`;
        placeholder.setAttribute("aria-label", `重新加载图片：${image.alt || "附件"}`);
        placeholder.messageImageSource = image.getAttribute("data-remote-media") || image.getAttribute("src");
        placeholder.messageImageLabel = image.alt;
        placeholder.messageImageKey = JSON.stringify([placeholder.messageImageSource, image.alt, link?.getAttribute("href") || ""]);
        placeholder.onclick = async () => {
          if (!placeholder.isConnected || placeholder.disabled) return;
          const owner = messageImageOwner(placeholder, source);
          placeholder.disabled = true;
          placeholder.setAttribute("aria-busy", "true");
          prepareRemoteTunnelAccess();
          try {
            const asset = owner ? await refreshMessageImageAsset(owner) : { url: source };
            if (!placeholder.isConnected || owner && !currentMessageImageOwner(owner) || !asset) return;
            const retry = messageImage(asset, image.alt, source);
            if (retry) placeholder.replaceWith(retry);
          } catch (error) {
            if (placeholder.isConnected && (!owner || currentMessageImageOwner(owner)))
              placeholder.title = `图片暂未恢复：${error.message}，点击重试`;
          } finally {
            placeholder.disabled = false;
            placeholder.setAttribute("aria-busy", "false");
          }
        };
        image.closest(".message-image-link")?.replaceWith(placeholder);
      });
    };
    if (link && (/^\/api\/images\/[A-Za-z0-9_-]{20,128}$/.test(link.getAttribute("href") || "")
        || safeInlineImageSource(image.getAttribute("src")))) {
      link.title = image.alt ? `预览图片：${image.alt}` : "预览图片";
      link.onclick = event => {
        if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
        event.preventDefault(); openImagePreview(link, image);
      };
    }
  }
}
export function messageImage(asset, label, inlineSource, previous) {
  const remoteUrl = /^\/api\/images\/[A-Za-z0-9_-]{20,128}$/.test(asset?.url || "") ? asset.url : null;
  const source = remoteUrl || safeInlineImageSource(inlineSource);
  if (!source) return null;
  const caption = label || "图片附件";
  if (previous?.messageImageSource === source && previous.messageImageLabel === caption) return previous;
  const link = element(remoteUrl ? "a" : "button", "message-image-link"), image = element("img", "message-image");
  link.messageImageSource = source;
  link.messageImageLabel = caption;
  if (remoteUrl) {
    link.href = remoteUrl; link.target = "_blank"; link.rel = "noopener noreferrer";
  } else link.type = "button";
  image.alt = caption; image.loading = "lazy"; image.decoding = "async";
  loadAttachmentThumbnail(image, source);
  link.append(image); bindMessageImages(link); return link;
}
export function setMarkdown(node, text, remoteImages = [], options) {
  node.markdownSource = { text, remoteImages, options };
  scheduleMermaidBlocks();
  const html = markdown(text, remoteImages, options).trim();
  if (node.markdownHtml === html) return;
  if (node.querySelector(".message-image,.attachment-placeholder,.mermaid-block,.message-video-preview")) {
    const template = document.createElement("template");
    template.innerHTML = html;
    const imageKeys = parent => {
      if (parent.nodeType !== Node.ELEMENT_NODE) return "";
      const elements = parent.matches(".message-image,.attachment-placeholder,.mermaid-block,.message-video-preview") ? [parent]
        : [...parent.querySelectorAll(".message-image,.attachment-placeholder,.mermaid-block,.message-video-preview")];
      const keys = elements.flatMap(image => image.messageImageKey ? [image.messageImageKey]
        : image.matches(".message-video-preview") ? [JSON.stringify(["video", image.querySelector("img")?.getAttribute("data-remote-media") || image.querySelector("img")?.getAttribute("src"), image.textContent])]
        : image.matches(".mermaid-block") ? [JSON.stringify(["mermaid", image.querySelector("pre code")?.textContent])]
        : image.matches(".message-image") ? [JSON.stringify([image.getAttribute("data-remote-media") || image.getAttribute("src"), image.alt,
          image.closest(".message-image-link")?.getAttribute("href") || ""])] : []);
      return keys.length ? JSON.stringify(keys) : "";
    };
    const reconcile = (target, source) => {
      const previous = [...target.childNodes], children = [];
      for (const child of [...source.childNodes]) {
        const key = imageKeys(child);
        const index = key ? previous.findIndex(node => (node.nodeName === child.nodeName || node.messageImageKey)
          && imageKeys(node) === key) : -1;
        const current = index < 0 ? null : previous.splice(index, 1)[0];
        if (!current) { children.push(child); continue; }
        if (!current.messageImageKey && !current.matches(".message-image,.mermaid-block,.message-video-preview")) {
          for (const attribute of [...current.attributes])
            if (!child.hasAttribute(attribute.name)) current.removeAttribute(attribute.name);
          for (const attribute of child.attributes)
            if (current.getAttribute(attribute.name) !== attribute.value) current.setAttribute(attribute.name, attribute.value);
          reconcile(current, child);
        }
        children.push(current);
      }
      const retained = new Set(children);
      // 移动图片的祖先也可能重新加载图片，先移除不再使用的兄弟节点。
      for (const child of [...target.childNodes]) if (!retained.has(child)) child.remove();
      reconcileChildren(target, children);
    };
    reconcile(node, template.content);
  } else node.innerHTML = html;
  for (const reference of node.querySelectorAll(".message-skill-reference")) {
    const skill = skillToken({ name: reference.textContent, path: reference.title, displayName: reference.textContent });
    skill.classList.add("message-skill");
    reference.replaceWith(skill);
  }
  node.markdownHtml = html;
  for (const copy of node.querySelectorAll(".copy-code")) copy.onclick = () => copyText(copy.closest(".code-block").querySelector("code").textContent);
  bindLocalFileReferences(node, {
    openFile: (path, line) => sessionTools?.openFile(path, line),
    openBrowser: (path, line) => openLocalFileInBrowser(path, line)
  });
  bindMessageImages(node);
  bindMessageVideos(node, prepareRemoteTunnelAccess);
}
export function scheduleMermaidBlocks() {
  if (mermaidScanPending) return;
  mermaidScanPending = true;
  queueMicrotask(() => {
    mermaidScanPending = false;
    const root = $("#messages"), version = selectionVersion;
    if (!root?.isConnected) return;
    renderMermaidBlocks(root, () => {
      if (version === selectionVersion && root === $("#messages") && root.isConnected
          && followMessages && !composerScroll?.collapsed) root.scrollTop = root.scrollHeight;
    }, change => {
      if (version === selectionVersion && root === $("#messages") && root.isConnected)
        withStableMessageLayout(root, change, { follow: followMessages && !composerScroll?.collapsed });
    });
  });
}
function refreshMarkdown() {
  ++markdownRenderVersion;
  const root = $("#messages");
  const refresh = () => {
    for (const node of document.querySelectorAll(".markdown,.markdown-inline")) {
      const source = node.markdownSource;
      if (source) setMarkdown(node, source.text, source.remoteImages, source.options);
    }
    for (const cached of renderedItems.values())
      if (cached.node.isConnected) cached.markdownVersion = markdownRenderVersion;
  };
  if (root?.isConnected)
    withStableMessageLayout(root, refresh, { follow: followMessages && !composerScroll?.collapsed });
  else refresh();
  if (root) {
    setHistoryScrollTop(root.scrollTop);
    updateScrollControl(); scheduleHistoryLoad();
    scheduleMermaidBlocks();
  }
}
// Markdown 渲染库就绪后重新渲染已显示的消息。
export function installMarkdownRefresh() {
  window.addEventListener("codex-elves-markdown-ready", refreshMarkdown);
}
export function renderImageViewGroup(records, previous) {
  const groupRecords = Array.isArray(records) ? records : [records];
  const occurrences = new Map();
  const entries = groupRecords.flatMap(record =>
    (Array.isArray(record?.imagePaths) ? record.imagePaths : [record?.path])
      .filter(path => typeof path === "string" && path.trim())
      .map(path => {
        const sourceKey = JSON.stringify([record.id, path]), occurrence = occurrences.get(sourceKey) || 0;
        occurrences.set(sourceKey, occurrence + 1);
        return { key: JSON.stringify([sourceKey, occurrence]), path, asset: record.remoteImages?.find(image => image.source === path) };
      })
  );
  const details = previous || element("details", "tool-item image-view-item");
  let state = details.imageViewState;
  if (!state) {
    const summary = element("summary", "image-view-summary");
    const visual = element("span"), label = element("strong");
    const body = element("div", "image-view-body");
    summary.append(visual, label, icon("chevron"));
    details.replaceChildren(summary, body);
    state = { visual, label, body, rows: new Map(), running: null };
    details.imageViewState = state;
    if (!previous) details.open = true;
  }
  const running = groupRecords.some(record => ["inProgress", "running"].includes(record?.status));
  const failed = groupRecords.some(record => record?.status === "failed");
  details.className = `tool-item image-view-item${running ? " running" : ""}${failed ? " failed" : ""}`;
  if (state.running !== running) {
    state.visual.className = running ? "spinner" : "image-view-icon";
    state.visual.setAttribute("aria-hidden", "true");
    state.visual.innerHTML = running ? "" : '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="2"/><path d="m21 15-5-5L5 20"/></svg>';
    state.running = running;
  }
  const title = running ? "正在查看图像" : failed ? "图像查看失败" : entries.length > 1 ? `已查看 ${entries.length} 张图像` : "已查看图像";
  if (state.label.textContent !== title) state.label.textContent = title;
  const rows = new Map();
  for (const { key, path, asset } of entries) {
    const row = state.rows.get(key) || element("div", "image-view-entry");
    const preview = messageImage(asset, "已检查的图片", undefined, row.firstElementChild);
    row.title = path;
    reconcileChildren(row, [preview || $(".image-view-placeholder", row) || element("span", "image-view-placeholder", "图片预览不可用")]);
    rows.set(key, row);
  }
  reconcileChildren(state.body, [...rows.values()]);
  state.rows = rows;
  return details;
}
export function renderImageViewItem(item, previous) {
  return renderImageViewGroup([item], previous);
}
