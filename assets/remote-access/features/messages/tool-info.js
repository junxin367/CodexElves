// 工具调用识别：命令文本、Skill 读取识别与条目渲染、工具结果分段；活动摘要与工具输出渲染共用。

import { element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { skillVisibleName } from "../composer/prompt-editor.js";
import { skillChoices } from "../composer/skill-catalog.js";

export function commandText(item) {
  return [...item.commandActions || []].reverse().find(action => typeof action.command === "string"
    && action.command.trim() && !/^(?:["']?(?:[a-z]:[\\/].*[\\/])?(?:bash|cmd|fish|powershell|pwsh|sh|zsh)(?:\.exe)?["']?\s)/i.test(action.command.trim()))?.command
    || item.command || "";
}
function skillPathInfo(value, displayName = "") {
  if (typeof value !== "string" || !value.trim()) return null;
  const path = value.trim(), normalized = path.replace(/\\/g, "/");
  const match = normalized.match(/(?:^|\/)skills\/(?:[^/]+\/)*([^/]+)\/SKILL\.md$/i);
  if (!match) return null;
  const catalog = skillChoices.find(skill => typeof skill?.path === "string"
    && skill.path.replace(/\\/g, "/").toLocaleLowerCase() === normalized.toLocaleLowerCase());
  const name = String(displayName || (catalog ? skillVisibleName(catalog) : "") || skillVisibleName({ name: match[1] })).trim()
    .replace(/\s+(?:skill|技能)$/i, "");
  return name ? { name, path } : null;
}
function nativeSkillReadInfo(item) {
  const surfaces = [
    item?.toolSurface,
    item?._meta?.["codex/toolSurface"],
    item?.result?._meta?.["codex/toolSurface"]
  ];
  const actions = Array.isArray(item?.commandActions) ? item.commandActions : [];
  for (const candidate of [...surfaces, ...actions]) {
    if (!candidate || typeof candidate !== "object") continue;
    const marker = [candidate.kind, candidate.type, candidate.action, candidate.title]
      .filter(value => typeof value === "string").join(" ");
    if (!/skill/i.test(marker) || !/(?:read|load|view|open)/i.test(marker)) continue;
    const displayName = candidate.displayName || candidate.skillDisplayName || candidate.skillName || candidate.name;
    const info = skillPathInfo(candidate.skillPath || candidate.path || candidate.literalPath, displayName);
    if (info) return info;
    const name = typeof displayName === "string"
      ? skillVisibleName({ name: displayName }).replace(/\s+(?:skill|技能)$/i, "") : "";
    if (name) return { name, path: "" };
  }
  return null;
}
function skillReadTokens(value) {
  const tokens = [];
  for (let index = 0; index < value.length;) {
    while (/\s/.test(value[index] || "")) index++;
    if (index >= value.length) break;
    const quote = value[index];
    if (quote === "'" || quote === '"') {
      let token = "", closed = false;
      for (index++; index < value.length; index++) {
        const character = value[index];
        if (character === quote) {
          if (quote === "'" && value[index + 1] === "'") { token += "'"; index++; continue; }
          closed = true; index++; break;
        }
        if (quote === '"' && (character === "$" || character === "`")) return null;
        token += character;
      }
      if (!closed) return null;
      tokens.push({ value: token, quoted: true });
      continue;
    }
    const start = index;
    while (index < value.length && !/\s/.test(value[index])) {
      if (value[index] === "'" || value[index] === '"') return null;
      index++;
    }
    tokens.push({ value: value.slice(start, index), quoted: false });
  }
  return tokens;
}
function skillReadCmdlet(segment, kind) {
  const tokens = skillReadTokens(segment);
  if (!tokens?.length || tokens[0].value.toLocaleLowerCase() !== kind.toLocaleLowerCase()) return null;
  let path = null;
  for (let index = 1; index < tokens.length; index++) {
    const option = tokens[index].value.toLocaleLowerCase();
    if (option === "-literalpath") {
      const value = tokens[++index];
      if (!value?.quoted || path) return null;
      path = skillPathInfo(value.value);
      if (!path) return null;
    } else if (kind === "Get-Content" && ["-raw", "-force"].includes(option)) {
      continue;
    } else if (kind === "Get-Content" && ["-totalcount", "-readcount", "-encoding"].includes(option)) {
      const value = tokens[++index]?.value || "";
      if (option === "-encoding" ? !/^[a-z0-9-]+$/i.test(value) : !/^\d+$/.test(value)) return null;
    } else if (kind === "Test-Path" && option === "-pathtype") {
      if (tokens[++index]?.value.toLocaleLowerCase() !== "leaf") return null;
    } else {
      return null;
    }
  }
  return path;
}
function skillReadPipeline(segment) {
  const tokens = skillReadTokens(segment);
  if (!tokens?.length || tokens[0].value.toLocaleLowerCase() !== "select-object") return false;
  for (let index = 1; index < tokens.length; index++) {
    if (!["-first", "-skip", "-last"].includes(tokens[index].value.toLocaleLowerCase())
        || !/^\d+$/.test(tokens[++index]?.value || "")) return false;
  }
  return tokens.length > 1;
}
function commandSkillReadInfo(command) {
  if (typeof command !== "string" || !command.trim()) return null;
  const guardedCommand = command.trim().replace(/^\$ErrorActionPreference\s*=\s*(['"])Stop\1\s*;\s*/i, "");
  const guarded = guardedCommand.match(/^if\s*\(\s*(Test-Path\b[^()]*)\s*\)\s*\{\s*([^{}]+)\s*\}$/i);
  if (guarded) {
    const checked = skillReadCmdlet(guarded[1], "Test-Path");
    const pipeline = guarded[2].split("|").map(part => part.trim());
    const read = pipeline[0] ? skillReadCmdlet(pipeline[0], "Get-Content") : null;
    if (!checked || !read || pipeline.slice(1).some(part => !skillReadPipeline(part))) return null;
    return checked.path.replace(/\\/g, "/").toLocaleLowerCase()
      === read.path.replace(/\\/g, "/").toLocaleLowerCase() ? read : null;
  }
  if (/(?:&&|\|\||[&<>`{}()]|\$\()/u.test(command)) return null;
  const paths = [];
  let reads = 0;
  for (const raw of command.replace(/\r?\n/g, " ").split(";")) {
    const statement = raw.trim();
    if (!statement || /^\$ErrorActionPreference\s*=\s*(['"])Stop\1$/i.test(statement)) continue;
    const pipeline = statement.split("|").map(part => part.trim());
    if (!pipeline[0] || pipeline.some(part => !part)) return null;
    let info = skillReadCmdlet(pipeline[0], "Get-Content");
    if (info) {
      reads++;
      if (pipeline.slice(1).some(part => !skillReadPipeline(part))) return null;
    } else {
      info = skillReadCmdlet(pipeline[0], "Test-Path");
      if (!info || pipeline.length !== 1) return null;
    }
    paths.push(info);
  }
  if (!reads || !paths.length) return null;
  const path = paths[0].path.replace(/\\/g, "/").toLocaleLowerCase();
  return paths.every(info => info.path.replace(/\\/g, "/").toLocaleLowerCase() === path) ? paths[0] : null;
}
export function skillReadInfo(item) {
  if (item?.type !== "commandExecution") return null;
  const native = nativeSkillReadInfo(item);
  const command = commandText(item);
  if (typeof command === "string" && command.trim()) {
    const detected = commandSkillReadInfo(command);
    if (!detected) return null;
    return { ...detected, name: native?.name || detected.name };
  }
  return native;
}
export function renderSkillReadItem(item, previous, info) {
  const node = previous?.classList?.contains("skill-read-item") ? previous : element("div", "skill-read-item");
  const running = ["inProgress", "running"].includes(item.status);
  const interrupted = ["interrupted", "cancelled", "canceled"].includes(item.status);
  const failed = !interrupted && (item.status === "failed" || item.exitCode != null && item.exitCode !== 0);
  node.className = `skill-read-item${running ? " running" : ""}${interrupted ? " interrupted" : ""}${failed ? " failed" : ""}`;
  const label = running ? `正在读取 ${info.name} 技能` : interrupted ? `已中断读取 ${info.name} 技能`
    : failed ? `读取 ${info.name} 技能失败` : `已读取 ${info.name} 技能`;
  const visual = running ? element("span", "spinner skill-read-icon") : icon("wrench");
  visual.classList.add("skill-read-icon");
  node.replaceChildren(visual, element("span", "skill-read-label", label));
  node.setAttribute("role", "status");
  node.setAttribute("aria-label", label);
  return node;
}
export function toolResultParts(item) {
  const parts = [];
  const append = (content, depth = 0) => {
    for (const entry of Array.isArray(content) ? content : []) {
      if (["text", "inputText"].includes(entry?.type) && typeof entry.text === "string") {
        let nested = null;
        if (depth < 4 && entry.text.trimStart().startsWith("{")) {
          try {
            const value = JSON.parse(entry.text);
            if (Array.isArray(value?.content)
                && Object.keys(value).every(key => ["content", "structuredContent", "_meta", "isError"].includes(key))
                && value.content.every(part => part && typeof part.type === "string"))
              nested = value;
          } catch {}
        }
        if (nested) {
          append(nested.content, depth + 1);
          if (!nested.content.length && nested.structuredContent != null)
            parts.push({ type: "text", text: JSON.stringify(nested.structuredContent, null, 2) });
        } else if (entry.text) parts.push({ type: "text", text: entry.text });
      } else if (entry?.type === "image" || entry?.type === "inputImage") {
        const source = entry.type === "inputImage" ? entry.imageUrl
          : typeof entry.data === "string" && typeof entry.mimeType === "string"
            ? `data:${entry.mimeType};base64,${entry.data}` : "";
        parts.push({ type: "image", source });
      } else if (["resource", "embedded_resource"].includes(entry?.type) && typeof entry.resource?.text === "string") {
        parts.push({ type: "text", text: entry.resource.text });
      } else if (entry?.type === "resource_link") {
        parts.push({ type: "text", text: [entry.title || entry.name || "资源", entry.uri, entry.description].filter(Boolean).join("\n") });
      } else {
        parts.push({ type: "text", text: "此类型工具结果请在 Codex 桌面查看。" });
      }
    }
  };
  // Claude 网页搜索与动态工具一样把原生结果放在 contentItems；MCP 结果沿用 result.content。
  append(["dynamicToolCall", "webSearch"].includes(item.type) ? item.contentItems : item.result?.content);
  if (!parts.length && item.result?.structuredContent != null)
    parts.push({ type: "text", text: JSON.stringify(item.result.structuredContent, null, 2) });
  if (typeof item.error?.message === "string" && !parts.some(part => part.text === item.error.message))
    parts.unshift({ type: "text", text: item.error.message });
  return parts;
}
