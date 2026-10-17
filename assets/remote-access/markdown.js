import { marked, Renderer } from "./vendor/marked.esm.js";
import createDOMPurify from "./vendor/dompurify.esm.js";

const purifier = createDOMPurify(globalThis.window);

const escapeHtml = value => String(value ?? "")
  .replaceAll("&", "&amp;")
  .replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;")
  .replaceAll("'", "&#39;");

const safeHref = href => {
  const value = String(href ?? "").trim();
  if (!value || /[\u0000-\u001f\u007f\s<>"'`]/u.test(value)) return null;
  if (value.startsWith("#")) return value;
  if (!/^https?:\/\//iu.test(value)) return null;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
};

const imagePathPattern = /^\/api\/images\/[A-Za-z0-9_-]{20,128}$/u;
const safeImagePath = url => typeof url === "string" && imagePathPattern.test(url) ? url : null;

const isLocalReference = href => {
  const value = String(href ?? "").trim();
  if (/^file:/iu.test(value) || /^[a-z]:[\\/]/iu.test(value) || /^\\\\/u.test(value)) return true;
  return !/^[a-z][a-z\d+.-]*:/iu.test(value) && !value.startsWith("//") && !value.startsWith("#");
};

const inlineContent = (renderer, token) => token.tokens
  ? renderer.parser.parseInline(token.tokens)
  : escapeHtml(token.text);

const createRenderer = (remoteImages, { userMessage = false, skills = [], skillLink } = {}) => {
  const registeredImages = new Map();
  if (Array.isArray(remoteImages)) {
    for (const image of remoteImages) {
      if (typeof image?.source !== "string" || registeredImages.has(image.source)) continue;
      const url = safeImagePath(image.url);
      if (url) registeredImages.set(image.source, url);
    }
  }

  const renderer = new Renderer();
  const skillNames = new Map();
  for (const skill of skills) {
    if (typeof skill?.name === "string" && /^[\p{L}_][\p{L}\p{N}_.:/-]*$/u.test(skill.name))
      skillNames.set(skill.name, skill);
  }
  const skillPattern = userMessage && skillNames.size
    ? new RegExp(`(^|[^\\p{L}\\p{N}_$])\\$(${[...skillNames.keys()].sort((left, right) => right.length - left.length)
      .map(name => name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})(?![\\p{L}\\p{N}_:./-])`, "gu")
    : null;
  const skillPlaceholder = skill => `<span class="message-skill-reference" title="${escapeHtml(skill.path)}">${escapeHtml(skill.displayName || skill.name)}</span>`;
  let linkDepth = 0;
  const renderText = renderer.text;
  renderer.text = function (token) {
    const html = renderText.call(this, token);
    if (!skillPattern || linkDepth || token.type !== "text" || token.tokens) return html;
    return html.replace(skillPattern, (_match, prefix, name) => `${prefix}${skillPlaceholder(skillNames.get(name))}`);
  };

  renderer.code = ({ text, lang, raw }) => {
    const language = String(lang ?? "").trim().split(/\s+/u, 1)[0] || "代码";
    const fence = raw?.match(/^ {0,3}(`{3,}|~{3,})/u)?.[1];
    const closed = fence && new RegExp(`\\n {0,3}${fence[0]}{${fence.length},}[ \\t]*(?:\\n)?$`, "u").test(raw);
    const diagram = language.toLowerCase() === "mermaid" && closed;
    return `<div class="code-block${diagram ? " mermaid-block" : ""}"><div class="code-header"><span>${escapeHtml(language)}</span><button class="copy-code" type="button">复制</button></div><pre><code>${escapeHtml(text)}</code></pre></div>\n`;
  };

  renderer.html = ({ text }) => escapeHtml(text);

  renderer.link = function (token) {
    let label;
    linkDepth++;
    try { label = inlineContent(this, token); }
    finally { linkDepth--; }
    if (userMessage && isLocalReference(token.href)) {
      const text = token.tokens ? this.parser.parseInline(token.tokens, this.parser.textRenderer) : token.text;
      const skill = skillLink?.(token.href, text);
      if (skill) return skillPlaceholder(skill);
    }
    const href = safeHref(token.href);
    if (href?.startsWith("#")) return `<a href="${escapeHtml(href)}">${label}</a>`;
    if (href) {
      const title = token.title ? ` title="${escapeHtml(token.title)}"` : "";
      return `<a href="${escapeHtml(href)}"${title} target="_blank" rel="noopener noreferrer">${label}</a>`;
    }
    if (isLocalReference(token.href)) {
      return `<span class="local-file-reference" title="${escapeHtml(token.href)}">${label}</span>`;
    }
    return label;
  };

  renderer.image = token => {
    const alt = String(token.text ?? "");
    const source = String(token.href ?? "");
    const url = registeredImages.get(source);
    if (url) {
      const safeUrl = escapeHtml(url);
      return `<a class="message-image-link" href="${safeUrl}" target="_blank" rel="noopener noreferrer"><img class="message-image" src="${safeUrl}" alt="${escapeHtml(alt)}" loading="lazy" decoding="async"></a>`;
    }
    const label = alt.trim() || "图片";
    const title = source ? ` title="${escapeHtml(source)}"` : "";
    return `<span class="attachment-placeholder"${title}>[附件：${escapeHtml(label)}]</span>`;
  };

  const renderTable = renderer.table;
  renderer.table = function (token) {
    return `<div class="table-scroll">${renderTable.call(this, token)}</div>\n`;
  };

  return renderer;
};

const sanitizeOptions = {
  ALLOWED_TAGS: [
    "a", "blockquote", "br", "button", "code", "del", "div", "em", "h1", "h2", "h3",
    "h4", "h5", "h6", "hr", "img", "input", "li", "ol", "p", "pre", "span", "strong", "table",
    "tbody", "td", "th", "thead", "tr", "ul"
  ],
  ALLOWED_ATTR: [
    "align", "alt", "checked", "class", "decoding", "disabled", "href", "loading", "rel", "src",
    "start", "target", "title", "type"
  ],
  ALLOW_ARIA_ATTR: false,
  ALLOW_DATA_ATTR: false,
  ALLOW_UNKNOWN_PROTOCOLS: false,
  FORBID_ATTR: ["style"],
  FORBID_TAGS: [
    "audio", "embed", "form", "iframe", "math", "object", "script", "source", "style", "svg", "video"
  ],
  RETURN_TRUSTED_TYPE: false
};

purifier.addHook("uponSanitizeAttribute", (_node, data) => {
  if (data.attrName === "href" && safeHref(data.attrValue) === null && safeImagePath(data.attrValue) === null) {
    data.keepAttr = false;
  }
  if (data.attrName === "src" && safeImagePath(data.attrValue) === null) data.keepAttr = false;
  if (data.attrName === "srcset" || data.attrName === "xlink:href") {
    data.keepAttr = false;
  }
});

export function markdown(text, remoteImages = [], { userMessage = false, skills = [], skillLink } = {}) {
  const source = String(text ?? "");
  const html = marked.parse(userMessage ? source : source.replace(/\uE200[^\uE201]*\uE201/g, ""), {
    async: false,
    breaks: userMessage,
    gfm: true,
    pedantic: false,
    renderer: createRenderer(remoteImages, { userMessage, skills, skillLink })
  });
  return purifier.sanitize(html, sanitizeOptions);
}

const diagramPurifier = createDOMPurify(globalThis.window);
const diagrams = new Map();
let mermaidModule, diagramSequence = 0, diagramQueue = Promise.resolve();

async function renderDiagram(source, theme) {
  mermaidModule ||= import("./vendor/mermaid.esm.js").catch(error => {
    mermaidModule = null;
    throw error;
  });
  const { default: mermaid } = await mermaidModule;
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    suppressErrorRendering: true,
    logLevel: 5,
    theme: theme === "dark" ? "dark" : "default",
    fontFamily: 'Inter, "Segoe UI", "Microsoft YaHei", sans-serif',
    htmlLabels: false,
    flowchart: { htmlLabels: false },
    secure: [...Object.keys(mermaid.mermaidAPI.defaultConfig), "htmlLabels"]
  });
  const stage = document.createElement("div");
  stage.className = "mermaid-render-stage";
  stage.setAttribute("aria-hidden", "true");
  document.body.append(stage);
  try {
    const { svg } = await mermaid.mermaidAPI.render(`remote-mermaid-${++diagramSequence}`, source, stage);
    const clean = diagramPurifier.sanitize(svg, {
      USE_PROFILES: { svg: true, svgFilters: true },
      ADD_TAGS: ["style"],
      FORBID_TAGS: ["a", "foreignObject", "image", "script"],
      FORBID_ATTR: ["href", "xlink:href", "src"],
      ALLOW_DATA_ATTR: false
    });
    const document = new DOMParser().parseFromString(clean, "image/svg+xml");
    const image = document.documentElement;
    if (image.localName !== "svg") throw new Error("图表格式无效");
    const [, , width, height] = (image.getAttribute("viewBox") || "").split(/[\s,]+/u).map(Number);
    if (!(width > 0 && height > 0)) throw new Error("图表尺寸无效");
    image.setAttribute("width", String(width));
    image.setAttribute("height", String(height));
    image.removeAttribute("style");
    return { src: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(image))}`, width, height };
  } finally { stage.remove(); }
}

export function renderMermaidBlocks(node, onRender) {
  const theme = document.documentElement.dataset.theme || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  for (const block of node.querySelectorAll(".mermaid-block")) {
    const source = block.querySelector("pre code")?.textContent || "";
    const key = JSON.stringify([theme, source]);
    if (block.mermaidKey === key) continue;
    block.mermaidKey = key;
    const pre = block.querySelector("pre"), header = block.querySelector(".code-header");
    let image = block.querySelector(".mermaid-diagram");
    if (!image) {
      image = document.createElement("img");
      image.className = "mermaid-diagram";
      image.alt = "Mermaid 图表";
      image.hidden = true;
      const toggle = document.createElement("button");
      toggle.className = "mermaid-source-toggle";
      toggle.type = "button";
      toggle.textContent = "源码";
      toggle.setAttribute("aria-label", "查看 Mermaid 源码");
      toggle.hidden = true;
      toggle.onclick = () => {
        const showSource = pre.hidden;
        pre.hidden = !showSource;
        image.hidden = showSource;
        toggle.textContent = showSource ? "图表" : "源码";
        toggle.setAttribute("aria-label", showSource ? "查看 Mermaid 图表" : "查看 Mermaid 源码");
      };
      header.insertBefore(toggle, header.querySelector(".copy-code"));
      block.insertBefore(image, pre);
    }
    let status = block.querySelector(".mermaid-status");
    if (!status) {
      status = document.createElement("span");
      status.className = "mermaid-status";
      status.setAttribute("role", "status");
      header.firstElementChild.after(status);
    }
    status.textContent = "正在生成图表…";
    block.classList.add("mermaid-loading");
    if (!block.classList.contains("mermaid-ready")) pre.hidden = true;
    if (!diagrams.has(key)) {
      const result = source.length > 50_000 ? Promise.reject(new Error("图表内容过长"))
        : diagramQueue.then(() => renderDiagram(source, theme));
      diagramQueue = result.catch(() => {});
      diagrams.set(key, result);
      if (diagrams.size > 32) diagrams.delete(diagrams.keys().next().value);
    }
    diagrams.get(key).then(async result => {
      if (block.mermaidKey !== key) return;
      image.width = Math.ceil(result.width);
      image.height = Math.ceil(result.height);
      image.src = result.src;
      await image.decode();
      if (block.mermaidKey !== key) return;
      const toggle = block.querySelector(".mermaid-source-toggle");
      toggle.hidden = false;
      pre.hidden = toggle.textContent !== "图表";
      image.hidden = !pre.hidden;
      block.classList.add("mermaid-ready");
      block.classList.remove("mermaid-loading");
      status.remove();
      onRender?.();
    }).catch(() => {
      if (block.mermaidKey !== key) return;
      diagrams.delete(key);
      status.textContent = "图表暂时无法渲染，可查看或复制源码";
      pre.hidden = false;
      image.hidden = true;
      block.querySelector(".mermaid-source-toggle").hidden = true;
      block.classList.remove("mermaid-ready");
      block.classList.remove("mermaid-loading");
    });
  }
}
