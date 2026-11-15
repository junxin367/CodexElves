// 输入框装配：Ctrl+Z 编辑历史，以及把输入框 value 的读写接到富文本编辑内容；只由页面骨架创建输入框时调用。

import { session } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { draftKey } from "./drafts.js";
import {
  promptPlainText, promptSelection, replacePromptRange, setPromptSelection, skillIdentity,
  writePromptContent
} from "./prompt-editor.js";
import { selectedSkills, setSelectedSkills } from "./skill-catalog.js";
import { handlePromptInput } from "./skills.js";
import { selectedGoal, setSelectedGoal } from "../goal/state.js";
import { editingQueuedMessage } from "../queue/state.js";
import { privateInputScope } from "../session/capabilities.js";

function createPromptEditHistory(prompt, getScope, now = () => Date.now()) {
  // 用户要求 Ctrl+Z 撤销编辑内容：DOM 重绘不能依赖浏览器原生历史；只记录文字、Skill、目标与光标，不回滚附件或已发送消息。
  // 每个草稿独立保留最多 100 次编辑、约 4MiB 快照；连续输入在 1 秒内合并，粘贴、换行、选区替换与输入法提交各自独立。
  const entries = [], limit = 100, memoryLimit = 4 * 1024 * 1024;
  let owner = getScope(), index = 0, current = null, pending = null, scheduled = false;
  let composing = false, applying = false, generation = 0, bytes = 0, canGroup = false;
  const capture = () => {
    const selection = promptSelection(prompt);
    const skills = selectedSkills.map(skill => ({ ...skill }));
    return {
      text: promptPlainText(prompt), skills, goalMode: !!selectedGoal,
      signature: JSON.stringify([skills.map(skill => [skillIdentity(skill), skill._offset ?? 0]), !!selectedGoal]),
      selection: { ...selection, startTokens: new Set(selection.startTokens || []), endTokens: new Set(selection.endTokens || []) },
      scrollTop: prompt.scrollTop || 0, scrollLeft: prompt.scrollLeft || 0
    };
  };
  const same = (left, right) => left.text === right.text && left.signature === right.signature;
  const caret = selection => JSON.stringify([selection.start, selection.end,
    [...selection.startTokens].sort(), [...selection.endTokens].sort()]);
  const size = snapshot => snapshot.text.length * 2 + JSON.stringify(snapshot.skills).length * 2 + 128;
  function reset() {
    entries.length = 0; index = 0; current = pending = null; bytes = 0;
    composing = applying = scheduled = canGroup = false; ++generation; owner = getScope();
  }
  function ensureScope() {
    if (owner !== getScope()) reset();
  }
  function commit() {
    ensureScope(); scheduled = false;
    if (applying || composing) return;
    const after = capture(), before = pending?.before || current, type = pending?.type || "replace";
    pending = null;
    if (!before || same(before, after)) { current = after; return; }
    const previous = entries[index - 1], time = now(), hasRedo = index < entries.length;
    const grouped = canGroup && !hasRedo && previous && previous.type === type
      && ["insertText", "deleteContentBackward", "deleteContentForward"].includes(type)
      && time - previous.time <= 1000 && before.selection.start === before.selection.end
      && same(previous.after, before) && caret(previous.after.selection) === caret(before.selection);
    if (hasRedo) {
      for (const entry of entries.splice(index)) bytes -= entry.bytes;
    }
    if (grouped) {
      bytes -= previous.bytes; previous.after = after; previous.time = time;
      previous.bytes = size(previous.before) + size(after); bytes += previous.bytes;
    } else {
      const entry = { before, after, type, time, bytes: size(before) + size(after) };
      entries.push(entry); bytes += entry.bytes; ++index;
    }
    while (entries.length > 1 && (entries.length > limit || bytes > memoryLimit)) {
      bytes -= entries.shift().bytes; --index;
    }
    current = after; canGroup = true;
  }
  function begin(type = "replace", scheduleCommit = true) {
    ensureScope();
    if (applying) return;
    if (!pending) {
      const actual = capture();
      pending = { before: current && !same(current, actual) ? current : actual, type: composing ? "composition" : type };
    }
    if (type === "token") pending.type = type;
    // beforeinput 与浏览器实际改写 DOM 之间可能执行微任务，原生输入必须等 input 后再提交。
    if (!scheduleCommit || scheduled) return;
    scheduled = true;
    const version = generation;
    queueMicrotask(() => { if (version === generation) commit(); });
  }
  function step(direction) {
    ensureScope();
    if (applying || composing || prompt.disabled) return false;
    commit();
    const entry = direction < 0 ? entries[index - 1] : entries[index];
    if (!entry) return false;
    const snapshot = direction < 0 ? entry.before : entry.after;
    index += direction;
    applying = true; pending = null; scheduled = false; canGroup = false; ++generation;
    try {
      setSelectedSkills(snapshot.skills.map(skill => ({ ...skill })));
      // 恢复目标标签时使用当前/新草稿标识，不复活旧请求的目标标识而让迟到回执清空新输入。
      setSelectedGoal(snapshot.goalMode ? selectedGoal || randomId() : null);
      writePromptContent(prompt, snapshot.text);
      prompt.focus({ preventScroll: true });
      setPromptSelection(prompt, snapshot.selection.start, snapshot.selection.end, snapshot.selection);
      prompt.scrollTop = snapshot.scrollTop; prompt.scrollLeft = snapshot.scrollLeft;
      handlePromptInput({ inputType: direction < 0 ? "historyUndo" : "historyRedo" }, { preserveCanonical: true });
      current = capture();
    } finally { applying = false; }
    return true;
  }
  return {
    begin, commit, reset, undo: () => step(-1), redo: () => step(1),
    startComposition() { commit(); begin("composition", false); composing = true; },
    endComposition() { composing = false; },
    get composing() { return composing; }
  };
}
export function configurePromptEditor(prompt) {
  Object.defineProperties(prompt, {
    value: {
      configurable: true,
      get: () => promptPlainText(prompt),
      set: value => {
        setSelectedGoal(null);
        writePromptContent(prompt, String(value ?? ""), [], false);
        // 载入其他草稿或发送后清空均建立新基线，不能用撤销找回上一会话或已发送的内容。
        prompt.editHistory?.reset();
      }
    },
    selectionStart: { configurable: true, get: () => promptSelection(prompt).start },
    selectionEnd: { configurable: true, get: () => promptSelection(prompt).end },
    placeholder: {
      configurable: true,
      get: () => prompt.dataset.placeholder || "",
      set: value => {
        prompt.dataset.placeholder = String(value ?? "");
        prompt.setAttribute("aria-placeholder", prompt.dataset.placeholder);
      }
    },
    disabled: {
      configurable: true,
      get: () => prompt.getAttribute("contenteditable") !== "true",
      set: value => {
        const disabled = !!value;
        prompt.setAttribute("contenteditable", String(!disabled));
        prompt.setAttribute("aria-disabled", String(disabled));
        prompt.tabIndex = disabled ? -1 : 0;
        if (disabled && document.activeElement === prompt) prompt.blur();
      }
    }
  });
  prompt.setRangeText = (replacement, start = prompt.selectionStart, end = prompt.selectionEnd, mode = "preserve") =>
    replacePromptRange(prompt, replacement, start, end, mode);
  prompt.editHistory = createPromptEditHistory(prompt, () => JSON.stringify([
    privateInputScope(session), draftKey(), editingQueuedMessage?.editId || null
  ]));
  prompt.placeholder = prompt.dataset.placeholder;
  prompt.disabled = false;
}
