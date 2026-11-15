// Skill 目录：可选 Skill 与全局目录的状态、目录缓存与刷新判断、斜杠命令匹配。

import { $, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { sidebar, thread } from "../../core/state.js";

export const catalogCacheMs = 60 * 60 * 1000;
export const catalogRefreshAheadMs = 5 * 60 * 1000, catalogRetryMs = 60 * 1000;
export const catalogFallbackMs = 7 * 24 * 60 * 60 * 1000;
export const skillCatalogs = new Map(), skillScopeAliases = new Map(), skillCatalogRequests = new Map();
export const skillCatalogRetryAt = new Map();
export let globalSkillChoices = [], globalSkillsLoadedAt = 0;
export function setGlobalSkillChoices(value) { return globalSkillChoices = value; }
export function setGlobalSkillsLoadedAt(value) { return globalSkillsLoadedAt = value; }
export let skillCatalogRevision = 0, globalSkillEpoch = "", globalSkillsInvalidated = false;
export function setSkillCatalogRevision(value) { return skillCatalogRevision = value; }
export function setGlobalSkillEpoch(value) { return globalSkillEpoch = value; }
export function setGlobalSkillsInvalidated(value) { return globalSkillsInvalidated = value; }
export let appliedSkillCatalog = null, appliedGlobalSkills = null;
export function setAppliedSkillCatalog(value) { return appliedSkillCatalog = value; }
export function setAppliedGlobalSkills(value) { return appliedGlobalSkills = value; }
let catalogRefreshTimer = null;
export function setCatalogRefreshTimer(value) { return catalogRefreshTimer = value; }
export let selectedSkills = [];
export function setSelectedSkills(value) { return selectedSkills = value; }
export let skillChoices = [], skillIndex = 0, skillLoadVersion = 0, skillScope = "";
export function setSkillIndex(value) { return skillIndex = value; }
export function setSkillChoices(value) { return skillChoices = value; }
export function setSkillLoadVersion(value) { return skillLoadVersion = value; }
export function setSkillScope(value) { return skillScope = value; }
export let skillLoading = null;
export function setSkillLoading(value) { return skillLoading = value; }
export let skillRenderVersion = 0, messageSkillLoadScope = "";
export function setSkillRenderVersion(value) { return skillRenderVersion = value; }
export function setMessageSkillLoadScope(value) { return messageSkillLoadScope = value; }
export function slashQuery() {
  const prompt = $("#prompt");
  if (!prompt || prompt.selectionStart !== prompt.selectionEnd) return null;
  const text = prompt.value.slice(0, prompt.selectionStart);
  const match = /(?:^|\s)\/([^\s/]*)$/.exec(text);
  return match ? { query: match[1].toLowerCase(), start: prompt.selectionStart - match[1].length - 1, end: prompt.selectionStart } : null;
}
export function compactCommandMatches(query) {
  return ["压缩", "compact", "context", "上下文"].some(value => value.includes(query));
}
export function compactCommandIcon() {
  const node = icon("compactContext");
  node.classList.add("slash-command-icon");
  return node;
}
export function goalComposerIcon() {
  const node = element("span", "ui-icon slash-command-icon");
  node.setAttribute("aria-hidden", "true");
  node.innerHTML = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.15"><circle cx="8" cy="8" r="6.25"/><circle cx="8" cy="8" r="3.25"/><circle cx="8" cy="8" r=".7" fill="currentColor" stroke="none"/></svg>';
  return node;
}
export function normalizeSlashIndex(entries) {
  if (entries[skillIndex]?.enabled) return;
  const first = entries.findIndex(entry => entry.enabled);
  skillIndex = first < 0 ? 0 : first;
}
export function moveSlashIndex(entries, offset) {
  const enabled = entries.map((entry, index) => entry.enabled ? index : -1).filter(index => index >= 0);
  if (!enabled.length) return;
  const current = enabled.indexOf(skillIndex);
  skillIndex = enabled[(current < 0 ? 0 : current + offset + enabled.length) % enabled.length];
}
export function catalogFresh(value) {
  const age = Date.now() - value?.loadedAt;
  return Number.isFinite(age) && age >= 0 && age < catalogCacheMs;
}
export function catalogUsable(value) {
  const age = Date.now() - value?.loadedAt;
  return Number.isFinite(value?.loadedAt) && value.loadedAt > 0 && age >= 0 && age < catalogFallbackMs;
}
export function catalogNeedsRefresh(value) {
  return !catalogFresh(value) || Date.now() - value.loadedAt >= catalogCacheMs - catalogRefreshAheadMs;
}
export function stopCatalogRefresh() {
  clearTimeout(catalogRefreshTimer); catalogRefreshTimer = null;
}
export function validCachedSkill(skill) {
  return typeof skill?.name === "string" && typeof skill?.path === "string"
    && (skill.backend !== "claude" || skill.command === skill.name
      && /^[\p{L}\p{N}_.:-]{1,256}$/u.test(skill.command) && skill.path === `claude-skill:${skill.command}`);
}
export function skillCwdKey(cwd) {
  if (typeof cwd !== "string" || !cwd) return null;
  const path = cwd.replaceAll("\\", "/").replace(/\/+$/, "") || "/";
  return `cwd:${/^[a-z]:\//i.test(path) || path.startsWith("//") ? path.toLowerCase() : path}`;
}
export function skillScopeKey(scope) {
  if (skillScopeAliases.has(scope)) return skillScopeAliases.get(scope);
  if (scope.startsWith("claude:")) return `context:${scope}`;
  let cwd;
  if (scope.startsWith("thread:")) {
    const id = scope.slice(7);
    cwd = thread?.id === id ? thread.cwd
      : [...(sidebar?.recent || []), ...(sidebar?.pinnedThreads || []),
        ...(sidebar?.projects || []).flatMap(project => project.threads || [])].find(item => item.id === id)?.cwd;
  } else if (scope.startsWith("project:"))
    cwd = sidebar?.projects?.find(project => project.id === scope.slice(8))?.roots?.[0];
  return skillCwdKey(cwd) || `context:${scope}`;
}
export function skillCatalogFresh(scope) {
  const record = skillCatalogs.get(skillScopeKey(scope));
  if (scope.startsWith("claude:"))
    return record?.backend === "claude" && record.epoch === globalSkillEpoch && catalogFresh(record);
  return !globalSkillsInvalidated && (record?.epoch || "") === globalSkillEpoch
    && catalogFresh(record) && catalogFresh({ loadedAt: globalSkillsLoadedAt });
}
export function skillCatalogUsable(scope) {
  if (scope.startsWith("claude:")) {
    const record = skillCatalogs.get(skillScopeKey(scope));
    return record?.backend === "claude" && catalogUsable(record);
  }
  return catalogUsable(skillCatalogs.get(skillScopeKey(scope))) && catalogUsable({ loadedAt: globalSkillsLoadedAt });
}
export function skillCatalogNeedsRefresh(scope) {
  return !skillCatalogFresh(scope) || catalogNeedsRefresh(skillCatalogs.get(skillScopeKey(scope)))
    || !scope.startsWith("claude:") && catalogNeedsRefresh({ loadedAt: globalSkillsLoadedAt });
}
