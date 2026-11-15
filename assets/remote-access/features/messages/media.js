// 消息媒体与 Markdown：消息图片绑定与刷新、Markdown 渲染与刷新、图片视图；图片读取记录、预览缩放与 Mermaid 调度在 media-state.js。

import {
  bindMessageVideos, loadAttachmentThumbnail, retryOriginalImage, safeInlineImageSource
} from "../../attachments.js";
import { prepareRemoteTunnelAccess } from "../../channels.js";
import {
  bindLocalFileReferences, markdown, prepareMessageImage, settleMessageImage,
  withStableMessageLayout
} from "../../markdown.js";
import { copyText } from "../../ui/clipboard.js";
import { $, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { renderMessages } from "../../core/actions.js";
import { turns } from "../../core/state.js";
import { api } from "../api.js";
import { skillToken } from "../composer/prompt-editor.js";
import {
  followMessages, renderedItems, setHistoryScrollTop, updateScrollControl
} from "../conversation/history-state.js";
import { scheduleHistoryLoad } from "../conversation/history.js";
import { mergeRemoteImageProjection } from "../conversation/transcript-events.js";
import { rememberTranscript } from "../conversation/transcript.js";
import { openLocalFileInBrowser } from "../local-files.js";
import { reconcileChildren } from "./item-presentation.js";
import {
  currentMessageImageOwner, markdownRenderVersion, messageImageOwner, messageImageReadSequence,
  messageImageRefreshes, openImagePreview, scheduleMermaidBlocks, setMarkdownRenderVersion,
  setMessageImageReadSequence
} from "./media-state.js";
import { composerScroll, sessionTools } from "../shell-state.js";

async function refreshMessageImageAsset(owner) {
  if (!currentMessageImageOwner(owner)) return null;
  const key = JSON.stringify([owner.scope, owner.selection, owner.threadId, owner.turnId]);
  let request = messageImageRefreshes.get(key);
  if (!request) {
    request = { controller: new AbortController(), targets: new Map(), promise: null,
      mediaReadVersion: setMessageImageReadSequence(messageImageReadSequence + 1) };
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
function refreshMarkdown() {
  setMarkdownRenderVersion(markdownRenderVersion + 1);
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
