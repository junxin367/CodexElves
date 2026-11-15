// 已选 Skill：渲染输入框中已选的 Skill，选择 Skill 并移除对应的斜杠输入。

import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closeComposerMenus, updateComposer } from "../../core/actions.js";
import { closeSkillMenu } from "./composer-menus.js";
import { rememberDraft } from "./drafts.js";
import {
  promptBoundaryOffset, promptSelection, promptSkillPlacements, promptTokenPlacements,
  promptTokensBeforeBoundary, setPromptSelection, skillNodeKey, syncPromptTrailingBreak,
  syncSelectedSkillsFromPrompt, writePromptContent
} from "./prompt-editor.js";
import { selectedSkills, slashQuery } from "./skill-catalog.js";
import { selectedGoal } from "../goal/state.js";
import { isClaude } from "../session/capabilities.js";

export function renderSelectedSkills() {
  const prompt = $("#prompt");
  if (!prompt) return;
  const selection = promptSelection(prompt), offsets = new Map(promptSkillPlacements(prompt).map(item => [item.key, item.offset]));
  for (const skill of selectedSkills) if (offsets.has(skillNodeKey(skill))) skill._offset = offsets.get(skillNodeKey(skill));
  writePromptContent(prompt, prompt.value);
  if (document.activeElement === prompt) setPromptSelection(prompt, selection.start, selection.end, selection);
  updateComposer();
}
export function selectSkill(skill) {
  const query = slashQuery();
  if (isClaude()) {
    const prompt = $("#prompt"), command = skill?.command;
    if (!query || selectedGoal || (prompt.value || "").slice(0, query.start).trim()
        || skill.backend !== "claude" || typeof command !== "string"
        || !/^[\p{L}\p{N}_.:-]{1,256}$/u.test(command)) return;
    // Claude 使用原生 /skill-name 参数文本，不构造 Codex 的 $标记、文件路径或额外系统提示。
    prompt.setRangeText(`/${command} `, query.start, query.end, "end");
    rememberDraft(); closeComposerMenus(); updateComposer(); prompt.focus();
    return;
  }
  if (!query || selectedSkills.length >= 16) { if (selectedSkills.length >= 16) notice("最多选择 16 项 Skill", true); return; }
  const prompt = $("#prompt");
  prompt.setRangeText("", query.start, query.end, "end");
  selectedSkills.push({ ...skill, _offset: query.start });
  renderSelectedSkills(); rememberDraft(); closeComposerMenus(); prompt.focus(); setPromptSelection(prompt, query.start, query.start, true);
}
export function removeAdjacentPromptToken(prompt, direction) {
  const selection = window.getSelection();
  if (!selection?.rangeCount || !selection.isCollapsed) return false;
  const range = selection.getRangeAt(0);
  if (!prompt.contains(range.startContainer)) return false;
  const caret = promptBoundaryOffset(prompt, range.startContainer, range.startOffset);
  const before = promptTokensBeforeBoundary(prompt, range.startContainer, range.startOffset);
  const adjacent = promptTokenPlacements(prompt).filter(placement => placement.offset === caret
    && before.has(placement.key) === (direction < 0));
  const token = direction < 0 ? adjacent.at(-1) : adjacent[0];
  if (!token) return false;
  prompt.editHistory?.begin("token");
  const nextRange = range.cloneRange();
  token.node.remove();
  syncSelectedSkillsFromPrompt(prompt); syncPromptTrailingBreak(prompt);
  selection.removeAllRanges(); selection.addRange(nextRange);
  rememberDraft(); updateComposer(); closeSkillMenu();
  return true;
}
