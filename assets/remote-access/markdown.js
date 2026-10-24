// 格式增强不属于登录和会话请求的启动依赖；未就绪时只显示安全的纯文本。
const stateEvent = "codex-elves-markdown-state";
const maximumRetries = 2;
let renderer = null;
let loading = null;
let status = "deferred";
let attempt = 0;
const modules = new Map();

const escapeHtml = value => String(value ?? "")
  .replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;").replaceAll("'", "&#39;");

function updateFallbackStatus() {
  for (const node of document.querySelectorAll(".markdown-deferred-status")) {
    node.textContent = status === "failed"
      ? "格式加载失败，正文仍可阅读" : "正在加载正文格式…";
  }
  for (const button of document.querySelectorAll(".markdown-load-retry")) {
    button.hidden = status !== "failed";
    button.disabled = status === "loading";
    button.textContent = attempt >= maximumRetries ? "刷新后重试格式加载" : "重试格式加载";
  }
  window.dispatchEvent(new CustomEvent(stateEvent, { detail: { status } }));
}

function loadModule(path) {
  if (!modules.has(path)) {
    const suffix = attempt ? `?retry=${attempt}` : "";
    const request = import(`./${path}${suffix}`);
    modules.set(path, request);
    void request.catch(() => {
      if (modules.get(path) === request) modules.delete(path);
    });
  }
  return modules.get(path);
}

function loadRenderer({ retry = false } = {}) {
  if (renderer) return Promise.resolve(renderer);
  if (loading) return loading;
  if (status === "failed" && !retry) return Promise.resolve(null);
  if (retry && attempt >= maximumRetries) return Promise.resolve(null);
  if (retry) attempt++;
  status = "loading";
  updateFallbackStatus();
  // 失败模块可能被当前文档记住；手动重试使用有限次数的新 URL。
  // 依赖分别导入，避免重试入口仍引用同一失败的静态依赖图。
  loading = Promise.all([
    loadModule("markdown-renderer.js"),
    loadModule("vendor/marked.esm.js"),
    loadModule("vendor/dompurify.esm.js")
  ]).then(([module, marked, purify]) => {
    renderer = module.createMarkdownRenderer({
      marked: marked.marked,
      Renderer: marked.Renderer,
      createDOMPurify: purify.default
    });
    status = "ready";
    updateFallbackStatus();
    window.dispatchEvent(new Event("codex-elves-markdown-ready"));
    return renderer;
  }).catch(() => {
    status = "failed";
    updateFallbackStatus();
    return null;
  }).finally(() => { loading = null; });
  return loading;
}

export function markdown(text, remoteImages = [], options = {}) {
  if (renderer) return renderer.markdown(text, remoteImages, options);
  const source = String(text ?? "");
  if (!source) return "";
  void loadRenderer();
  const plain = options.userMessage ? source : source.replace(/\uE200[^\uE201]*\uE201/g, "");
  if (options.inline) return escapeHtml(plain);
  const message = status === "failed" ? "格式加载失败，正文仍可阅读" : "正在加载正文格式…";
  const retryLabel = attempt >= maximumRetries ? "刷新后重试格式加载" : "重试格式加载";
  return `<div class="markdown-plain-text">${escapeHtml(plain)}</div>`
    + `<div class="markdown-deferred"><span class="markdown-deferred-status">${message}</span>`
    + `<button class="markdown-load-retry text-button" type="button"${status === "failed" ? "" : " hidden"}>${retryLabel}</button></div>`;
}

export function renderMermaidBlocks(node, onRender, withLayout) {
  renderer?.renderMermaidBlocks(node, onRender, withLayout);
}

const anchorIgnored = "[hidden],button,textarea,input,.code-header,.markdown-deferred,.transcript-checking-notice";
const anchorCharacters = value => [...String(value).matchAll(/[\p{L}\p{N}]/gu)];
const anchorText = value => String(value).replace(/[^\p{L}\p{N}]/gu, "");

function characterRect(node, offset) {
  if (!node?.isConnected || !node.length) return null;
  const range = document.createRange();
  range.setStart(node, Math.min(offset, node.length - 1));
  range.setEnd(node, Math.min(offset + 1, node.length));
  const rect = range.getBoundingClientRect();
  return rect.width && rect.height ? rect : null;
}

function anchorContext(node, offset, scope) {
  const contextStart = Math.max(0, offset - 128);
  const characters = anchorCharacters(node.textContent.slice(contextStart, offset + 192));
  const index = characters.findIndex(character => character.index >= offset - contextStart);
  const position = index < 0 ? characters.length : index;
  const walker = document.createTreeWalker(scope, NodeFilter.SHOW_TEXT);
  let source = "", sourceOffset = 0;
  for (let current; (current = walker.nextNode());) {
    if (current.parentElement?.closest(anchorIgnored)) continue;
    if (current === node) sourceOffset = source.length + anchorText(current.textContent.slice(0, offset)).length;
    source += anchorText(current.textContent);
  }
  return {
    before: characters.slice(Math.max(0, position - 32), position).map(character => character[0]).join(""),
    after: characters.slice(position, position + 48).map(character => character[0]).join(""),
    source, sourceOffset
  };
}

function readingAnchor(root) {
  const box = root.getBoundingClientRect();
  const notice = root.querySelector(":scope > .transcript-checking-notice")?.getBoundingClientRect();
  const top = Math.max(box.top, notice?.bottom || box.top), bottom = box.bottom;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  // 常见浏览器先命中视口首行，避免每次图片完成都遍历整段已读历史。
  let pointNode = null;
  for (const x of [box.left + 24, box.left + box.width / 2]) {
    const y = Math.min(bottom - 1, top + 2);
    const point = document.caretPositionFromPoint?.(x, y);
    const range = point ? null : document.caretRangeFromPoint?.(x, y);
    const candidate = point?.offsetNode || range?.startContainer;
    const offset = point?.offset ?? range?.startOffset ?? 0;
    if (candidate?.nodeType !== Node.TEXT_NODE || !root.contains(candidate)
        || candidate.parentElement?.closest(anchorIgnored)) continue;
    const rect = characterRect(candidate, Math.min(offset, candidate.length - 1));
    if (rect && rect.bottom > top && rect.top < bottom) { pointNode = candidate; break; }
  }
  if (pointNode) walker.currentNode = pointNode;
  for (let node = pointNode || walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.textContent?.trim() || node.parentElement?.closest(anchorIgnored)) continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    if (![...range.getClientRects()].some(rect => rect.height && rect.bottom > top && rect.top < bottom)) continue;
    // 找到视口内第一行的字符；长消息不能只保存整条消息的顶部。
    let left = 0, right = node.length - 1;
    while (left < right) {
      const middle = Math.floor((left + right) / 2), rect = characterRect(node, middle);
      if (rect && rect.bottom > top) right = middle;
      else left = middle + 1;
    }
    let offset = left, rect = characterRect(node, offset);
    // 换行符自身没有矩形，最多跨过当前空白段。
    while (!rect && offset < Math.min(node.length - 1, left + 128)) rect = characterRect(node, ++offset);
    if (!rect || rect.top >= bottom || rect.bottom <= top) continue;
    const scope = node.parentElement.closest(".markdown") || node.parentElement.closest("[data-key],.turn") || node.parentElement;
    return {
      node, offset, scope, top: rect.top - box.top,
      ...anchorContext(node, offset, scope),
      scopeTop: scope.getBoundingClientRect().top - box.top
    };
  }
  const node = [...root.querySelectorAll("[data-key],.turn")].find(candidate => {
    const rect = candidate.getBoundingClientRect();
    return rect.height && rect.bottom > top && rect.top < bottom;
  });
  return node ? { scope: node, scopeTop: node.getBoundingClientRect().top - box.top } : null;
}

function replacementCharacter(anchor) {
  const scope = anchor.scope;
  if (!scope?.isConnected) return null;
  const walker = document.createTreeWalker(scope, NodeFilter.SHOW_TEXT), segments = [];
  let text = "";
  for (let node; (node = walker.nextNode());) {
    if (node.parentElement?.closest(anchorIgnored)) continue;
    const characters = anchorCharacters(node.textContent);
    if (!characters.length) continue;
    const positions = [];
    let normalized = "";
    for (const character of characters) { positions.push(normalized.length); normalized += character[0]; }
    segments.push({ node, start: text.length, characters, positions });
    text += normalized;
  }
  // 原始文本中的 Markdown 标点在格式化后消失；匹配正文字符及上下文。
  const candidates = [
    [anchor.before + anchor.after, anchor.before.length],
    [anchor.after, 0], [anchor.before, anchor.before.length],
    [anchor.after.slice(0, 20), 0], [anchor.before.slice(-20), Math.min(20, anchor.before.length)]
  ];
  for (const [query, delta] of candidates) {
    if (query.length < 6) continue;
    const previous = [], matches = [];
    for (let index = anchor.source?.indexOf(query); index >= 0; index = anchor.source.indexOf(query, index + 1))
      previous.push(index);
    for (let index = text.indexOf(query); index >= 0; index = text.indexOf(query, index + 1)) {
      matches.push(index);
    }
    // 链接目的地址等会从正文消失，不能用原始字符比例选择重复段落。
    // 同一上下文的数量一致时，按原出现序号恢复；数量变化则换上下文或退回容器。
    const ordinal = previous.indexOf(anchor.sourceOffset - delta);
    if (ordinal < 0 || matches.length !== previous.length) continue;
    const found = matches[ordinal] + delta;
    const segment = segments.findLast(value => value.start <= found);
    if (segment) {
      const index = segment.positions.findLastIndex(position => position <= found - segment.start);
      const character = segment.characters[Math.max(0, index)];
      return { node: segment.node, offset: character.index };
    }
  }
  return null;
}

export function withStableMessageLayout(root, change, { follow = false } = {}) {
  if (!root?.isConnected) return change();
  const anchor = follow ? null : readingAnchor(root);
  try { return change(); }
  finally {
    if (root.isConnected) {
      if (follow) root.scrollTop = root.scrollHeight;
      else if (anchor?.scope?.isConnected && root.contains(anchor.scope)) {
        const character = !anchor.node ? null : anchor.node.isConnected && root.contains(anchor.node)
          ? anchor : replacementCharacter(anchor);
        const rect = character ? characterRect(character.node, character.offset) : null;
        root.scrollTop += (rect ? rect.top : anchor.scope.getBoundingClientRect().top)
          - root.getBoundingClientRect().top - (rect ? anchor.top : anchor.scopeTop);
      }
    }
  }
}

export function prepareMessageImage(image) {
  if (image.closest(".user-attachments,.image-view-entry")) return;
  if (image.complete && image.naturalWidth) { settleMessageImage(image); return; }
  // 保留一个非零、同位置的 img 供浏览器触发 lazy 下载，解码前不让未知比例改动正文。
  image.classList.add("message-image-pending");
  image.closest(".message-image-link")?.classList.add("message-image-loading");
}

export function settleMessageImage(image) {
  if (image.naturalWidth && image.naturalHeight) {
    image.width = image.naturalWidth;
    image.height = image.naturalHeight;
  }
  image.classList.remove("message-image-pending");
  image.closest(".message-image-link")?.classList.remove("message-image-loading");
}

document.addEventListener("click", event => {
  const target = event.target instanceof Element ? event.target.closest(".markdown-load-retry") : null;
  if (!target || !target.closest(".markdown-deferred")) return;
  if (attempt >= maximumRetries) {
    // 不自行刷新页面，避免丢失尚未存好的输入或干扰待回执操作。
    target.textContent = "请先保留输入，再刷新页面";
    return;
  }
  void loadRenderer({ retry: true });
});
