// 消息输入编辑器：Skill/目标标记、选区与纯文本换算、输入框尺寸；撤销历史与输入框装配在 prompt-edit-history.js，输入区滚动挂载在 composer.js。

import { $, element } from "../../ui/dom.js";
import { goalComposerIcon, selectedSkills, setSelectedSkills } from "./skill-catalog.js";
import { selectedGoal, setSelectedGoal } from "../goal/state.js";

export const effortLabels = { none: "无", minimal: "最低", low: "轻度", medium: "中", high: "高", xhigh: "极高", max: "Max", ultra: "Ultra" };
export const skillIdentity = skill => `${skill.name}\0${skill.path}`;
export const sameSkills = (left, right) => JSON.stringify(left.map(skillIdentity)) === JSON.stringify(right.map(skillIdentity));
export const skillNodeKey = skill => encodeURIComponent(skillIdentity(skill));
export function skillVisibleName(skill) {
  const name = String(skill?.name || "").trim();
  const displayName = typeof skill?.displayName === "string" ? skill.displayName.trim() : "";
  if (displayName && displayName !== name) return displayName;
  return name.split(":").map(part => part.split("-").filter(Boolean)
    .map(word => word[0].toLocaleUpperCase() + word.slice(1)).join(" ")).join(": ");
}
const goalPromptTokenKey = "composer-goal";
const promptCanonicalText = new WeakMap();
function promptTokenKey(node) {
  if (node.nodeType !== Node.ELEMENT_NODE) return null;
  if (node.classList.contains("inline-goal")) return goalPromptTokenKey;
  return node.classList.contains("inline-skill") ? node.dataset.skillKey : null;
}
function promptNodeLength(node) {
  if (node.nodeType === Node.TEXT_NODE) return node.data.length;
  if (node.nodeType !== Node.ELEMENT_NODE || promptTokenKey(node) || node.hasAttribute("data-prompt-tail")) return 0;
  if (node.tagName === "BR") return 1;
  return [...node.childNodes].reduce((total, child) => total + promptNodeLength(child), 0);
}
const promptLineBlocks = new Set(["DIV", "P"]);
export function promptDomText(prompt) {
  if (!prompt) return "";
  let text = "", pendingBlockBreak = false;
  const appendBlockBreak = () => {
    if (text && !text.endsWith("\n")) text += "\n";
    pendingBlockBreak = false;
  };
  const visit = node => {
    if (node.nodeType === Node.TEXT_NODE) {
      if (pendingBlockBreak) appendBlockBreak();
      text += node.data;
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE || promptTokenKey(node) || node.hasAttribute("data-prompt-tail")) return;
    if (node.tagName === "BR") { pendingBlockBreak = false; text += "\n"; return; }
    const block = promptLineBlocks.has(node.tagName);
    if (block && text) appendBlockBreak();
    for (const child of node.childNodes) visit(child);
    if (block) pendingBlockBreak = true;
  };
  for (const child of prompt.childNodes) visit(child);
  return text;
}
export function promptPlainText(prompt = $("#prompt")) {
  if (!prompt) return "";
  return promptCanonicalText.has(prompt) ? promptCanonicalText.get(prompt) : promptDomText(prompt);
}
export function forgetPromptCanonicalText(prompt = $("#prompt")) {
  if (prompt) promptCanonicalText.delete(prompt);
}
export function restorePromptCanonicalContent(prompt = $("#prompt")) {
  const canonicalText = promptCanonicalText.get(prompt);
  if (!prompt || canonicalText == null || promptDomText(prompt) === canonicalText) return;
  const selection = promptSelection(prompt);
  writePromptContent(prompt, canonicalText);
  setPromptSelection(prompt, selection.start, selection.end, selection);
}
export function syncPromptTrailingBreak(prompt) {
  if (prompt.childNodes.length === 1 && prompt.firstChild.nodeName === "BR"
      && !prompt.firstChild.hasAttribute("data-prompt-line-break")) prompt.replaceChildren();
  const tail = prompt.querySelector("br[data-prompt-tail]");
  if (!promptPlainText(prompt).endsWith("\n")) tail?.remove();
  else if (tail !== prompt.lastChild) {
    const next = tail || document.createElement("br");
    next.dataset.promptTail = "";
    prompt.append(next);
  }
  prompt.classList.toggle("goal-only", !!prompt.querySelector(".inline-goal")
    && !promptPlainText(prompt) && !prompt.querySelector(".inline-skill"));
}
export function promptBoundaryOffset(prompt, container, boundary) {
  const range = document.createRange();
  range.selectNodeContents(prompt);
  range.setEnd(container, boundary);
  return promptDomText(range.cloneContents()).length;
}
export function promptSelection(prompt = $("#prompt")) {
  const length = promptPlainText(prompt).length, selection = window.getSelection();
  if (!prompt || !selection?.rangeCount) return { start: length, end: length };
  const range = selection.getRangeAt(0);
  if (!prompt.contains(range.startContainer) || !prompt.contains(range.endContainer))
    return { start: length, end: length };
  return {
    start: promptBoundaryOffset(prompt, range.startContainer, range.startOffset),
    end: promptBoundaryOffset(prompt, range.endContainer, range.endOffset),
    startTokens: promptTokensBeforeBoundary(prompt, range.startContainer, range.startOffset),
    endTokens: promptTokensBeforeBoundary(prompt, range.endContainer, range.endOffset)
  };
}
function promptPoint(prompt, target, afterSkills) {
  let remaining = Math.min(Math.max(0, target), promptPlainText(prompt).length);
  const find = parent => {
    for (let index = 0; index < parent.childNodes.length; index++) {
      const child = parent.childNodes[index];
      // 目标始终位于正文之前，数值正文位置 0 也落在目标之后。
      if (child.nodeType === Node.ELEMENT_NODE && child.classList.contains("inline-goal")) continue;
      if (child.nodeType === Node.ELEMENT_NODE && child.classList.contains("inline-skill")) {
        if (remaining === 0 && (afterSkills instanceof Set ? !afterSkills.has(child.dataset.skillKey) : !afterSkills))
          return { node: parent, offset: index };
        continue;
      }
      const length = promptNodeLength(child);
      if (child.nodeType === Node.TEXT_NODE && (remaining < length || remaining === length && !afterSkills))
        return { node: child, offset: remaining };
      if (child.nodeType === Node.ELEMENT_NODE && child.tagName !== "BR" && remaining <= length) {
        const point = find(child);
        if (point) return point;
      }
      if (remaining < length || child.nodeType === Node.ELEMENT_NODE && child.tagName === "BR" && remaining === 0)
        return { node: parent, offset: index };
      remaining -= length;
    }
    return { node: parent, offset: parent.childNodes.length };
  };
  return find(prompt);
}
export function promptTokensBeforeBoundary(prompt, container, boundary) {
  const keys = new Set();
  let found = false;
  const collect = node => {
    const key = promptTokenKey(node);
    if (key) {
      keys.add(key);
      return;
    }
    if (node.nodeType === Node.ELEMENT_NODE) for (const child of node.childNodes) collect(child);
  };
  const visit = node => {
    if (found) return;
    if (node === container) {
      if (node.nodeType === Node.ELEMENT_NODE)
        for (let index = 0; index < Math.min(Math.max(0, boundary), node.childNodes.length); index++)
          collect(node.childNodes[index]);
      found = true;
      return;
    }
    const key = promptTokenKey(node);
    if (key) {
      keys.add(key);
      return;
    }
    if (node.nodeType === Node.ELEMENT_NODE) for (const child of node.childNodes) visit(child);
  };
  visit(prompt);
  return keys;
}
export function setPromptSelection(prompt, start, end = start, afterSkills = true) {
  if (!prompt?.isConnected) return;
  const selection = window.getSelection(), range = document.createRange();
  const first = promptPoint(prompt, start, afterSkills?.startTokens ?? afterSkills);
  const last = promptPoint(prompt, end, afterSkills?.endTokens ?? afterSkills);
  range.setStart(first.node, first.offset); range.setEnd(last.node, last.offset);
  selection.removeAllRanges(); selection.addRange(range);
}
function skillTokenIcon() {
  const node = element("span", "inline-skill-icon");
  node.setAttribute("aria-hidden", "true");
  node.innerHTML = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"><path d="m8 1.75 5.5 3.18v6.14L8 14.25l-5.5-3.18V4.93Z"/><path d="m2.5 4.93 5.5 3.2 5.5-3.2M8 8.13v6.12"/></svg>';
  return node;
}
export function skillToken(skill) {
  const node = element("span", "inline-skill");
  const label = skillVisibleName(skill);
  node.contentEditable = "false";
  node.dataset.skillKey = skillNodeKey(skill);
  node.setAttribute("aria-label", `Skill：${label}`);
  node.title = label;
  node.append(skillTokenIcon(), element("span", "inline-skill-name", label));
  return node;
}
function goalToken() {
  const node = element("span", "inline-goal"), glyph = goalComposerIcon();
  node.contentEditable = "false";
  node.setAttribute("aria-label", "目标模式");
  node.title = "目标模式：删除此标签可取消，保留正文、Skill 和图片";
  glyph.className = "inline-skill-icon";
  node.append(glyph, element("span", "inline-skill-name", "目标"));
  return node;
}
export function promptTokenPlacements(prompt = $("#prompt")) {
  const placements = [];
  let offset = 0;
  const visit = node => {
    const key = promptTokenKey(node);
    if (key) {
      placements.push({ key, node, offset });
      return;
    }
    if (node.nodeType === Node.TEXT_NODE || node.nodeType !== Node.ELEMENT_NODE || node.tagName === "BR") {
      offset += promptNodeLength(node);
      return;
    }
    for (const child of node.childNodes) visit(child);
  };
  if (prompt) for (const child of prompt.childNodes) visit(child);
  return placements;
}
export function promptSkillPlacements(prompt = $("#prompt")) {
  return promptTokenPlacements(prompt).filter(placement => placement.key !== goalPromptTokenKey);
}
function appendPromptText(nodes, text) {
  const parts = String(text ?? "").split("\n");
  for (let index = 0; index < parts.length; index++) {
    if (parts[index]) nodes.push(document.createTextNode(parts[index]));
    if (index < parts.length - 1) {
      const lineBreak = document.createElement("br");
      lineBreak.dataset.promptLineBreak = "";
      nodes.push(lineBreak);
    }
  }
}
export function writePromptContent(prompt, text, skills = selectedSkills, goalMode = !!selectedGoal) {
  prompt.editHistory?.begin("replace");
  const canonicalText = String(text ?? "");
  promptCanonicalText.set(prompt, canonicalText);
  const ordered = skills.map((skill, index) => ({
    skill, index, offset: Math.min(canonicalText.length, Math.max(0, Number.isFinite(skill._offset) ? skill._offset : 0))
  })).sort((left, right) => left.offset - right.offset || left.index - right.index);
  const nodes = goalMode ? [goalToken()] : [];
  let cursor = 0;
  for (const entry of ordered) {
    if (entry.offset > cursor) appendPromptText(nodes, canonicalText.slice(cursor, entry.offset));
    entry.skill._offset = entry.offset;
    nodes.push(skillToken(entry.skill));
    cursor = entry.offset;
  }
  if (cursor < canonicalText.length) appendPromptText(nodes, canonicalText.slice(cursor));
  prompt.replaceChildren(...nodes);
  syncPromptTrailingBreak(prompt);
}
export function syncSelectedSkillsFromPrompt(prompt = $("#prompt")) {
  if (!prompt) return;
  // 原生删除、选区替换和输入法结束后，以实际节点为准；不要重绘已被删除的目标。
  const goals = [...prompt.querySelectorAll(".inline-goal")];
  if (!goals.length) setSelectedGoal(null);
  const goal = selectedGoal ? goals.shift() : null;
  for (const duplicate of goals) duplicate.remove();
  if (goal && goal !== prompt.firstChild) prompt.prepend(goal);
  const available = new Map(selectedSkills.map(skill => [skillNodeKey(skill), skill]));
  const next = [];
  for (const placement of promptSkillPlacements(prompt)) {
    const skill = available.get(placement.key);
    if (!skill || next.some(item => skillIdentity(item) === skillIdentity(skill))) continue;
    skill._offset = placement.offset;
    next.push(skill);
  }
  setSelectedSkills(next);
}
export function replacePromptRange(prompt, replacement, start, end, selectionMode = "preserve") {
  prompt.editHistory?.begin("replace");
  const text = promptPlainText(prompt), selection = promptSelection(prompt);
  const first = Math.min(Math.max(0, start), text.length), last = Math.min(Math.max(first, end), text.length);
  const insert = String(replacement), delta = insert.length - (last - first);
  const currentOffsets = new Map(promptSkillPlacements(prompt).map(item => [item.key, item.offset]));
  const range = window.getSelection()?.rangeCount ? window.getSelection().getRangeAt(0) : null;
  const exactSelection = range && selection.start === first && selection.end === last
    && prompt.contains(range.startContainer) && prompt.contains(range.endContainer);
  const beforeStart = exactSelection ? promptTokensBeforeBoundary(prompt, range.startContainer, range.startOffset) : null;
  const beforeEnd = exactSelection ? promptTokensBeforeBoundary(prompt, range.endContainer, range.endOffset) : null;
  if (exactSelection && !beforeStart.has(goalPromptTokenKey) && beforeEnd.has(goalPromptTokenKey))
    setSelectedGoal(null);
  setSelectedSkills(selectedSkills.filter(skill => {
    const key = skillNodeKey(skill);
    return !exactSelection || beforeStart.has(key) || !beforeEnd.has(key);
  }));
  for (const skill of selectedSkills) {
    const key = skillNodeKey(skill);
    const offset = currentOffsets.get(skillNodeKey(skill)) ?? skill._offset ?? 0;
    if (exactSelection && beforeStart.has(key)) skill._offset = offset;
    else skill._offset = offset >= last ? offset + delta : offset > first ? first + insert.length : offset;
  }
  writePromptContent(prompt, text.slice(0, first) + insert + text.slice(last));
  const next = selectionMode === "select" ? { start: first, end: first + insert.length }
    : selectionMode === "start" ? { start: first, end: first }
      : selectionMode === "end" ? { start: first + insert.length, end: first + insert.length }
        : {
            start: selection.start > last ? selection.start + delta : selection.start > first ? first + insert.length : selection.start,
            end: selection.end > last ? selection.end + delta : selection.end > first ? first + insert.length : selection.end
          };
  setPromptSelection(prompt, next.start, next.end);
}
export function composerPrompt(text = $("#prompt")?.value || "", skills = selectedSkills, goalMode = false) {
  const body = String(text ?? "");
  // 每个内联 Skill 对应一个标记，正文中手写的同名标记仍保留原位。
  const ordered = (skills || []).map((skill, index) => ({
    skill, index, offset: Math.min(body.length, Math.max(0, Number.isFinite(skill._offset) ? skill._offset : 0))
  })).sort((left, right) => left.offset - right.offset || left.index - right.index);
  let result = "", cursor = 0;
  for (const { skill, offset } of ordered) {
    result += body.slice(cursor, offset);
    if (result && !/\s$/.test(result)) result += " ";
    result += goalMode ? `[$${skill.name}](${skill.path})` : `$${skill.name}`;
    if (offset < body.length && !/\s/.test(body[offset])) result += " ";
    cursor = offset;
  }
  return (result + body.slice(cursor)).trim();
}
export function resizeEditorInput(input) {
  if (!input?.isConnected) return;
  const followEnd = document.activeElement === input && input.selectionStart === input.selectionEnd
    && input.selectionEnd === String(input.value ?? "").length;
  input.style.height = "auto";
  const style = getComputedStyle(input);
  const contentHeight = Math.max(parseFloat(style.minHeight), input.scrollHeight);
  const maxHeight = parseFloat(style.maxHeight);
  input.style.height = `${Math.min(maxHeight, contentHeight)}px`;
  input.style.overflowY = contentHeight > maxHeight ? "auto" : "hidden";
  if (contentHeight <= maxHeight) input.scrollTop = 0;
  // 用户要求回车换行后光标保持可见；仅无选区且光标位于末尾时跟随底部，中间编辑不跳滚动位置。
  else if (followEnd) input.scrollTop = input.scrollHeight;
}
export function resizeComposerInput() {
  resizeEditorInput($("#prompt"));
}
