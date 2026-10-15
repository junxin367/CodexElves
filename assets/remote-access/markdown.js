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

const createRenderer = remoteImages => {
  const registeredImages = new Map();
  if (Array.isArray(remoteImages)) {
    for (const image of remoteImages) {
      if (typeof image?.source !== "string" || registeredImages.has(image.source)) continue;
      const url = safeImagePath(image.url);
      if (url) registeredImages.set(image.source, url);
    }
  }

  const renderer = new Renderer();

  renderer.code = ({ text, lang }) => {
    const language = String(lang ?? "").trim().split(/\s+/u, 1)[0] || "代码";
    return `<div class="code-block"><div class="code-header"><span>${escapeHtml(language)}</span><button class="copy-code" type="button">复制</button></div><pre><code>${escapeHtml(text)}</code></pre></div>\n`;
  };

  renderer.html = ({ text }) => escapeHtml(text);

  renderer.link = function (token) {
    const label = inlineContent(this, token);
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

export function markdown(text, remoteImages = [], { userMessage = false } = {}) {
  const source = String(text ?? "");
  const html = marked.parse(userMessage ? source : source.replace(/\uE200[^\uE201]*\uE201/g, ""), {
    async: false,
    breaks: userMessage,
    gfm: true,
    pedantic: false,
    renderer: createRenderer(remoteImages)
  });
  return purifier.sanitize(html, sanitizeOptions);
}
