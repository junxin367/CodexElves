// 消息媒体的底层部分：图片读取记录与归属、图片预览，以及 Mermaid 渲染调度与 Markdown 渲染版本。

import {
  bindMediaPreviewDismiss, fitMediaPreview, mediaPreviewContainsPoint, requestWorkspaceMediaPreview,
  safeInlineImageSource
} from "../../attachments.js";
import { remoteMediaUrl } from "../../channels.js";
import { renderMermaidBlocks, withStableMessageLayout } from "../../markdown.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { pageLeaving, selectedId, selectionVersion, session, turns } from "../../core/state.js";
import { followMessages, renderedItems } from "../conversation/history-state.js";
import { privateInputScope } from "../session/capabilities.js";
import { rememberTransientHistory } from "../session/related.js";
import { composerScroll } from "../shell-state.js";

export const messageImageMediaReads = new WeakMap(), messageImageRefreshes = new Map();
export let messageImageReadSequence = 0;
export function setMessageImageReadSequence(value) { return messageImageReadSequence = value; }
export let markdownRenderVersion = 0;
export function setMarkdownRenderVersion(value) { return markdownRenderVersion = value; }
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
export function messageImageOwner(node, url) {
  const message = node.closest("[data-key]"), turnId = message?.closest(".turn[data-turn-id]")?.dataset.turnId;
  const rendered = renderedItems.get(message?.dataset.key);
  const item = turns.get(turnId)?.items?.find(item => item.id === rendered?.item?.id
    && message.dataset.key === `${turnId}:${item.id}`);
  const asset = item?.remoteImages?.find(asset => asset.url === url && typeof asset.source === "string");
  if (!selectedId || !turnId || !item || !asset) return null;
  return { threadId: selectedId, turnId, itemId: item.id, source: asset.source,
    scope: privateInputScope(session), selection: selectionVersion };
}
export function currentMessageImageOwner(owner) {
  return owner && owner.threadId === selectedId && owner.selection === selectionVersion
    && owner.scope === privateInputScope(session) && !pageLeaving;
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
