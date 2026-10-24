import { videoPreviewMarkup } from "./attachments.js";

function localFileReference(value, explicit = false) {
  let path = String(value ?? "").trim(), line = null;
  if (!path || path.length > 16384 || /[\u0000-\u001f\u007f<>"`]/u.test(path)) return null;
  const position = /(?::([1-9]\d*)(?::[1-9]\d*)?|#L([1-9]\d*)(?:C\d+)?(?:-L?\d+(?:C\d+)?)?)$/u.exec(path);
  if (position) { line = Number(position[1] || position[2]); path = path.slice(0, position.index); }
  if (/^file:/iu.test(path)) {
    try {
      const url = new URL(path);
      if (url.hostname && url.hostname !== "localhost") return null;
      path = decodeURIComponent(url.pathname);
    } catch { return null; }
  } else {
    if (/^[a-z][a-z\d+.-]*:/iu.test(path) && !/^[a-z]:[\\/]/iu.test(path)) return null;
    try { path = decodeURIComponent(path); } catch { return null; }
  }
  path = path.replaceAll("\\", "/");
  if (/^\/[a-z]:\//iu.test(path)) path = path.slice(1);
  if (path.startsWith("//") || path.startsWith("#") || /[\u0000-\u001f\u007f?#]/u.test(path)) return null;
  const name = path.split("/").at(-1);
  if (!name || name === "." || name === "..") return null;
  if (!explicit && !/\.(?:md|mdx|txt|rst|log|json|jsonl|ya?ml|toml|xml|ini|conf|config|csv|tsv|js|mjs|cjs|jsx|ts|tsx|vue|svelte|css|scss|less|html?|rs|py|java|kt|kts|go|c|cc|cpp|h|hpp|cs|swift|sh|ps1|bat|sql|proto|gradle|properties|png|jpe?g|gif|webp|svg|pdf)$/iu.test(name)
      && !/^(?:README|LICENSE|Dockerfile|Makefile|Cargo\.lock|\.gitignore|\.env)$/u.test(name)) return null;
  return { path, name, line: Number.isSafeInteger(line) && line <= 1000000 ? line : null };
}

export function createMarkdownRenderer({ marked, Renderer, createDOMPurify }) {

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
  const safeVideoPoster = url => typeof url === "string" && /^\/api\/videos\/[a-f0-9]{64}\/poster$/u.test(url) ? url : null;

  const isLocalReference = href => {
    const value = String(href ?? "").trim();
    if (/^file:/iu.test(value) || /^[a-z]:[\\/]/iu.test(value) || /^\\\\/u.test(value)) return true;
    return !/^[a-z][a-z\d+.-]*:/iu.test(value) && !value.startsWith("//") && !value.startsWith("#");
  };

  const inlineContent = (renderer, token) => token.tokens
    ? renderer.parser.parseInline(token.tokens)
    : escapeHtml(token.text);

  const createRenderer = (remoteImages, { userMessage = false, skills = [], skillLink, remoteVideos = [] } = {}) => {
    const registeredVideos = new Map((Array.isArray(remoteVideos) ? remoteVideos : [])
      .filter(video => typeof video?.source === "string").map(video => [video.source, video]));
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
    const fileLink = reference => `<a class="local-file-reference" href="#project-file" data-local-file-path="${escapeHtml(reference.path)}"${reference.line ? ` data-local-file-line="${reference.line}"` : ""} title="${escapeHtml(reference.path)}">${escapeHtml(reference.name)}</a>`;
    const renderPaths = text => {
      const pattern = /(?:file:\/\/\/|\/?[a-z]:[\\/]|\.{1,2}[\\/]|\/)?[\p{L}\p{N}_.@+~-]+(?:[\\/][\p{L}\p{N}_.@+~-]+)*(?::[1-9]\d*(?::[1-9]\d*)?|#L[1-9]\d*(?:C\d+)?)?/giu;
      let result = "", start = 0;
      for (const match of text.matchAll(pattern)) {
        const candidate = match[0].replace(/[.,;!?]+$/u, "");
        const reference = localFileReference(candidate);
        if (!reference || match.index > 0 && /[\p{L}\p{N}_@:/\\.-]/u.test(text[match.index - 1])) continue;
        result += escapeHtml(text.slice(start, match.index)) + fileLink(reference);
        start = match.index + candidate.length;
      }
      return result + escapeHtml(text.slice(start));
    };
    const renderText = renderer.text;
    renderer.text = function (token) {
      const html = renderText.call(this, token);
      if (linkDepth || token.type !== "text" || token.tokens) return html;
      const linked = renderPaths(token.text);
      return skillPattern ? linked.replace(skillPattern, (_match, prefix, name) =>
        `${prefix}${skillPlaceholder(skillNames.get(name))}`) : linked;
    };
    const renderCodespan = renderer.codespan;
    renderer.codespan = function (token) {
      const reference = !linkDepth && localFileReference(token.text);
      return reference ? fileLink(reference) : renderCodespan.call(this, token);
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
      const video = registeredVideos.get(token.href);
      const preview = videoPreviewMarkup(video, token.text);
      if (preview && linkDepth === 0) return preview;
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
        const reference = localFileReference(token.href, true);
        if (reference) return fileLink(reference);
        return `<span class="local-file-reference" title="${escapeHtml(token.href)}">${label}</span>`;
      }
      return label;
    };

    renderer.image = token => {
      const alt = String(token.text ?? "");
      const source = String(token.href ?? "");
      const video = videoPreviewMarkup(registeredVideos.get(source), alt);
      if (video) return video;
      const url = registeredImages.get(source);
      if (url) {
        const safeUrl = escapeHtml(url);
        return `<a class="message-image-link" href="${safeUrl}" target="_blank" rel="noopener noreferrer"><img class="message-image" data-remote-media="${safeUrl}" alt="${escapeHtml(alt)}" loading="lazy" decoding="async"></a>`;
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
      "start", "target", "title", "type", "data-remote-media", "data-local-file-path", "data-local-file-line"
    ],
    ALLOW_ARIA_ATTR: false,
    ALLOW_DATA_ATTR: false,
    // This attribute is inert file metadata, never a navigable URI. Windows
    // drive letters must survive the sanitizer's generic URI check.
    ADD_URI_SAFE_ATTR: ["data-local-file-path"],
    ALLOW_UNKNOWN_PROTOCOLS: false,
    FORBID_ATTR: ["style"],
    FORBID_TAGS: [
      "audio", "embed", "form", "iframe", "math", "object", "script", "source", "style", "svg", "video"
    ],
    RETURN_TRUSTED_TYPE: false
  };

  purifier.addHook("uponSanitizeAttribute", (_node, data) => {
    if (data.attrName === "data-local-file-path" && !localFileReference(data.attrValue, true)) data.keepAttr = false;
    if (data.attrName === "href" && safeHref(data.attrValue) === null && safeImagePath(data.attrValue) === null) {
      data.keepAttr = false;
    }
    if (data.attrName === "src" && safeImagePath(data.attrValue) === null && safeVideoPoster(data.attrValue) === null) data.keepAttr = false;
    if (data.attrName === "data-remote-media" && safeImagePath(data.attrValue) === null && safeVideoPoster(data.attrValue) === null) data.keepAttr = false;
    if (data.attrName === "srcset" || data.attrName === "xlink:href") {
      data.keepAttr = false;
    }
  });

  function markdown(text, remoteImages = [], { userMessage = false, skills = [], skillLink, remoteVideos = [], inline = false } = {}) {
    const source = String(text ?? "");
    if (inline) {
      const renderer = new Renderer();
      renderer.html = ({ text }) => escapeHtml(text);
      renderer.link = function (token) { return inlineContent(this, token); };
      renderer.image = ({ text }) => escapeHtml(text);
      renderer.br = () => " ";
      return purifier.sanitize(marked.parseInline(source.replace(/\uE200[^\uE201]*\uE201/g, ""), {
        async: false, gfm: true, renderer
      }), { ...sanitizeOptions, ALLOWED_TAGS: ["code", "del", "em", "strong"], ALLOWED_ATTR: [] });
    }
    const html = marked.parse(userMessage ? source : source.replace(/\uE200[^\uE201]*\uE201/g, ""), {
      async: false,
      breaks: userMessage,
      gfm: true,
      pedantic: false,
      renderer: createRenderer(remoteImages, { userMessage, skills, skillLink, remoteVideos })
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

  const pendingDiagrams = new Map();
  const diagramTheme = () => document.documentElement.dataset.theme
    || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  let diagramObserver = null, diagramRoot = null;

  function observeDiagramRoot(node) {
    if (diagramRoot === node) return;
    diagramObserver?.disconnect();
    pendingDiagrams.clear();
    diagramRoot = node;
    const observer = typeof IntersectionObserver === "function" ? new IntersectionObserver(entries => {
      if (diagramObserver !== observer || diagramRoot !== node) return;
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        const pending = pendingDiagrams.get(entry.target);
        pendingDiagrams.delete(entry.target);
        observer.unobserve(entry.target);
        if (pending && entry.target.isConnected && node.contains(entry.target))
          renderMermaidBlock(entry.target, pending.onRender);
      }
    }, { root: node, rootMargin: "200px" }) : null;
    diagramObserver = observer;
  }

  function renderMermaidBlocks(node, onRender, withLayout = change => change()) {
    observeDiagramRoot(node);
    // 每次会话挂载或刷新时清理旧页面的观察项，不保留已切走会话的 DOM。
    for (const [block] of pendingDiagrams) {
      if (block.isConnected) continue;
      diagramObserver?.unobserve(block);
      pendingDiagrams.delete(block);
    }
    for (const block of node.querySelectorAll(".mermaid-block")) {
      if (!block.isConnected) continue;
      block.mermaidLayout = withLayout;
      block.mermaidRoot = node;
      const key = JSON.stringify([diagramTheme(), block.querySelector("pre code")?.textContent || ""]);
      if (block.mermaidKey === key) continue;
      if (diagramObserver) {
        pendingDiagrams.set(block, { onRender });
        diagramObserver.observe(block);
      } else renderMermaidBlock(block, onRender);
    }
  }

  function currentDiagramBlock(block, key) {
    if (block.mermaidKey !== key) return false;
    if (block.isConnected && block.mermaidRoot === diagramRoot && diagramRoot?.contains(block)) return true;
    // 生成或解码期间切走或更换消息根，重新挂载时仍能使用已完成缓存。
    block.mermaidKey = null;
    return false;
  }

  function renderMermaidBlock(block, onRender) {
    const withLayout = change => (block.mermaidLayout || (change => change()))(change);
    const theme = diagramTheme();
    const source = block.querySelector("pre code")?.textContent || "";
    const key = JSON.stringify([theme, source]);
    if (block.mermaidKey === key) return;
    block.mermaidKey = key;
    const pre = block.querySelector("pre"), header = block.querySelector(".code-header");
    let image = block.querySelector(".mermaid-diagram"), status;
    withLayout(() => {
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
        toggle.onclick = () => withLayout(() => {
          const showSource = pre.hidden;
          pre.hidden = !showSource;
          block.querySelector(".mermaid-diagram").hidden = showSource;
          toggle.textContent = showSource ? "图表" : "源码";
          toggle.setAttribute("aria-label", showSource ? "查看 Mermaid 图表" : "查看 Mermaid 源码");
        });
        header.insertBefore(toggle, header.querySelector(".copy-code"));
        block.insertBefore(image, pre);
      }
      status = block.querySelector(".mermaid-status");
      if (!status) {
        status = document.createElement("span");
        status.className = "mermaid-status";
        status.setAttribute("role", "status");
        header.firstElementChild.after(status);
      }
      status.textContent = "正在生成图表…";
      // 源码或上一次的图表保持可读，直到完整新图解码；不猜测图表高度。
      block.classList.add("mermaid-loading");
    });
    if (!diagrams.has(key)) {
      const result = source.length > 50_000 ? Promise.reject(new Error("图表内容过长"))
        : diagramQueue.then(() => renderDiagram(source, theme));
      diagramQueue = result.catch(() => {});
      diagrams.set(key, result);
      if (diagrams.size > 32) diagrams.delete(diagrams.keys().next().value);
    }
    diagrams.get(key).then(async result => {
      if (!currentDiagramBlock(block, key)) return;
      const next = document.createElement("img");
      next.className = image.className;
      next.alt = image.alt;
      next.width = Math.ceil(result.width);
      next.height = Math.ceil(result.height);
      next.src = result.src;
      await next.decode();
      if (!currentDiagramBlock(block, key)) return;
      withLayout(() => {
        image.replaceWith(next);
        image = next;
        const toggle = block.querySelector(".mermaid-source-toggle");
        toggle.hidden = false;
        pre.hidden = toggle.textContent !== "图表";
        image.hidden = !pre.hidden;
        block.classList.add("mermaid-ready");
        block.classList.remove("mermaid-loading");
        status.remove();
        onRender?.();
      });
    }).catch(() => {
      if (!currentDiagramBlock(block, key)) return;
      diagrams.delete(key);
      withLayout(() => {
        status.textContent = "图表暂时无法渲染，可查看或复制源码";
        if (!block.classList.contains("mermaid-ready")) {
          pre.hidden = false;
          image.hidden = true;
          block.querySelector(".mermaid-source-toggle").hidden = true;
        }
        block.classList.remove("mermaid-loading");
      });
    });
  }

  return { markdown, renderMermaidBlocks };
}
