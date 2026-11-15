// 斜杠命令与 Skill：命令菜单、Skill 目录缓存与刷新、选择 Skill/压缩命令及输入事件处理。

import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import {
  busy, connected, creating, pageLeaving, selectedId, selectionVersion, session, sidebar, stopping,
  thread
} from "../../core/state.js";
import { readCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { closeComposerMenus, closeSkillMenu, fitComposerMenu } from "./composer-menus.js";
import { updateComposer } from "./composer.js";
import { rememberDraft, rememberQueuedMessageEdit } from "./drafts.js";
import { closeContextUsageTip, compactUsageDescription, contextUsage } from "./metrics.js";
import {
  loadModels, modelContextKey, modelDefaults, modelsLoadedAt, modelsPromise, modelsRetryAt,
  modelsScope, setModelDefaults
} from "./models.js";
import {
  forgetPromptCanonicalText, promptBoundaryOffset, promptDomText, promptSelection,
  promptSkillPlacements, promptTokenPlacements, promptTokensBeforeBoundary,
  restorePromptCanonicalContent, setPromptSelection, skillIdentity, skillNodeKey, skillVisibleName,
  syncPromptTrailingBreak, syncSelectedSkillsFromPrompt, writePromptContent
} from "./prompt-editor.js";
import { compactContext, compactionLocked } from "../conversation/compaction.js";
import { scheduleMessages } from "../conversation/history-view.js";
import { pending } from "../conversation/pending.js";
import { goalCommandMatches, goalComposerReason, selectedGoal, selectGoalCommand } from "../goal/composer.js";
import { editingMessage } from "../messages/message-edit.js";
import { editingQueuedMessage } from "../queue/queued-edit.js";
import {
  backendCapability, claudeCommandSupported, isClaude, privateInputScope, threadReadOnly
} from "../session/capabilities.js";
import { activeInstanceId } from "../session/instances.js";
import { cacheDisplayData, currentThreadDisplay, optionalCachedRead, readScope } from "../session/thread-cache.js";

const catalogCacheMs = 60 * 60 * 1000;
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
export let selectedSkills = [];
export function setSelectedSkills(value) { return selectedSkills = value; }
export let skillChoices = [], skillIndex = 0, skillLoadVersion = 0, skillScope = "";
export function setSkillChoices(value) { return skillChoices = value; }
export function setSkillLoadVersion(value) { return skillLoadVersion = value; }
export function setSkillScope(value) { return skillScope = value; }
export let skillLoading = null;
export function setSkillLoading(value) { return skillLoading = value; }
export let skillRenderVersion = 0, messageSkillLoadScope = "";
export function setSkillRenderVersion(value) { return skillRenderVersion = value; }
export function setMessageSkillLoadScope(value) { return messageSkillLoadScope = value; }
export function renderSelectedSkills() {
  const prompt = $("#prompt");
  if (!prompt) return;
  const selection = promptSelection(prompt), offsets = new Map(promptSkillPlacements(prompt).map(item => [item.key, item.offset]));
  for (const skill of selectedSkills) if (offsets.has(skillNodeKey(skill))) skill._offset = offsets.get(skillNodeKey(skill));
  writePromptContent(prompt, prompt.value);
  if (document.activeElement === prompt) setPromptSelection(prompt, selection.start, selection.end, selection);
  updateComposer();
}
export function slashQuery() {
  const prompt = $("#prompt");
  if (!prompt || prompt.selectionStart !== prompt.selectionEnd) return null;
  const text = prompt.value.slice(0, prompt.selectionStart);
  const match = /(?:^|\s)\/([^\s/]*)$/.exec(text);
  return match ? { query: match[1].toLowerCase(), start: prompt.selectionStart - match[1].length - 1, end: prompt.selectionStart } : null;
}
export function compactCommandAvailable() {
  if (isClaude() && !claudeCommandSupported("compact")) return false;
  return !!session && !creating && !!selectedId && thread?.id === selectedId && !threadReadOnly() && connected
    && !busy && !stopping && !pending && !editingMessage && !editingQueuedMessage && !compactionLocked(selectedId);
}
function compactCommandMatches(query) {
  return ["压缩", "compact", "context", "上下文"].some(value => value.includes(query));
}
export function slashMenuEntries(query = slashQuery()) {
  if (!query) return [];
  const entries = [];
  // Claude 沿用原生斜杠调用语义，Skill 位于消息开头；Codex 保留原有内联 Skill 和目标组合。
  const nativeSkillPosition = !selectedGoal && !($("#prompt")?.value || "").slice(0, query.start || 0).trim();
  if (!isClaude() || nativeSkillPosition) entries.push(...skillChoices.filter(skill => !selectedSkills.some(item => skillIdentity(item) === skillIdentity(skill))
    && [skill.name, skill.displayName, skill.description].some(value => String(value).toLowerCase().includes(query.query)))
    .map(skill => ({ type: "skill", enabled: true, skill })));
  if (compactCommandMatches(query.query) && (!isClaude() || claudeCommandSupported("compact")))
    entries.push({ type: "compact", enabled: compactCommandAvailable() });
  if (goalCommandMatches(query.query) && (!isClaude() || claudeCommandSupported("goal")))
    entries.push({ type: "goal", enabled: !goalComposerReason() });
  return entries;
}
function compactCommandIcon() {
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
function normalizeSlashIndex(entries) {
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
export function renderSkillMenu(message) {
  const menu = $("#skill-menu"), query = slashQuery();
  if (!menu || !query || threadReadOnly()) { closeSkillMenu(); return; }
  closePopup(); closeContextUsageTip();
  menu.hidden = false; $("#model-settings").hidden = true; $("#model-button").setAttribute("aria-expanded", "false");
  $("#prompt").setAttribute("aria-expanded", "true");
  const queryChanged = menu.dataset.query !== query.query;
  const scrollTop = queryChanged ? 0 : $(".skill-options", menu)?.scrollTop || 0;
  const previousActive = $("#prompt").getAttribute("aria-activedescendant");
  menu.dataset.query = query.query;
  menu.replaceChildren();
  const list = element("div", "skill-options");
  menu.append(list);
  const entries = slashMenuEntries(query);
  normalizeSlashIndex(entries);
  const active = entries[skillIndex]?.enabled ? skillIndex : -1;
  const commands = entries.filter(entry => entry.type !== "skill");
  const skills = entries.filter(entry => entry.type === "skill");
  const appendOption = (entry, index, root) => {
    if (entry.type === "compact" || entry.type === "goal") {
      const goal = entry.type === "goal";
      const usage = contextUsage(currentThreadDisplay()?.tokenUsage);
      const label = goal ? "目标" : "压缩";
      const reason = goal ? goalComposerReason() : "";
      const node = button("", reason || label, goal ? () => selectGoalCommand() : selectCompactCommand,
        `skill-option slash-command-option${index === active ? " active" : ""}${goal ? " slash-goal-option" : ""}`);
      node.id = `skill-option-${index}`; node.disabled = !entry.enabled;
      node.setAttribute("role", "option"); node.setAttribute("aria-selected", String(index === active));
      const copy = element("span", "slash-command-copy");
      copy.append(element("strong", "", label));
      const description = goal ? reason || "设置要持续追求的目标" : compactUsageDescription(usage);
      if (description) copy.append(element("small", "muted slash-command-usage", description));
      node.append(goal ? goalComposerIcon() : compactCommandIcon(), copy);
      node.onmousedown = event => event.preventDefault();
      root.append(node);
      return;
    }
    const skill = entry.skill;
    const label = skillVisibleName(skill);
    const node = button("", skill.argumentHint ? `${label} ${skill.argumentHint}` : label,
      () => selectSkill(skill), `skill-option${index === active ? " active" : ""}`);
    node.id = `skill-option-${index}`; node.setAttribute("role", "option"); node.setAttribute("aria-selected", String(index === skillIndex));
    const description = element("small", "muted", skill.description);
    description.title = skill.description || "";
    node.append(element("strong", "", label), description);
    node.onmousedown = event => event.preventDefault();
    root.append(node);
  };
  if (message || !skills.length && !commands.length) list.append(element("p", "popover-empty muted", message || "没有匹配的可用 Skill"));
  for (const entry of skills) appendOption(entry, entries.indexOf(entry), list);
  if (commands.length) {
    const footer = element("div", "skill-menu-footer");
    for (const command of commands) appendOption(command, entries.indexOf(command), footer);
    menu.append(footer);
  }
  fitComposerMenu(menu);
  list.scrollTop = scrollTop;
  if (active >= 0) {
    $("#prompt").setAttribute("aria-activedescendant", `skill-option-${active}`);
    if (queryChanged || previousActive !== `skill-option-${active}`)
      $(`#skill-option-${active}`)?.scrollIntoView({ block: "nearest" });
  } else $("#prompt").removeAttribute("aria-activedescendant");
}
function catalogFresh(value) {
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
function activeSkillCatalogContext() {
  const projectId = creating ? $("#project-select")?.value : "";
  const scope = selectedId ? `thread:${selectedId}` : projectId ? `project:${projectId}`
    : creating && backendCapability("projectlessCreation") ? "projectless" : "";
  if (!scope) return null;
  const payload = selectedId ? { threadId: selectedId } : { projectId };
  if (!isClaude()) return { scope, payload };
  const instanceId = activeInstanceId();
  // 同一项目在不同 WSL 中可加载不同 Skill，不能复用 Codex 或其他实例的目录。
  return instanceId ? { scope: `claude:${instanceId}:${scope}`,
    payload: { ...payload, backend: "claude", instanceId } } : null;
}
export function stopCatalogRefresh() {
  clearTimeout(catalogRefreshTimer); catalogRefreshTimer = null;
}
export function scheduleCatalogRefresh() {
  stopCatalogRefresh();
  if (!session || pageLeaving || document.hidden || navigator.onLine === false) return;
  const deadlines = [], time = Date.now();
  const due = loadedAt => loadedAt + catalogCacheMs - catalogRefreshAheadMs;
  if (modelsLoadedAt && !modelsPromise)
    deadlines.push(Math.max(modelDefaults.fastModeAvailability?.available === null ? time
      : catalogFresh({ loadedAt: modelsLoadedAt }) ? due(modelsLoadedAt) : time, modelsRetryAt));
  const context = activeSkillCatalogContext(), key = context && skillScopeKey(context.scope);
  const record = key && skillCatalogs.get(key);
  const claude = context?.payload.backend === "claude";
  if (context && (record || !claude && globalSkillsLoadedAt) && !skillCatalogRequests.has(key) && skillLoading?.scope !== context.scope) {
    const stale = !skillCatalogFresh(context.scope);
    deadlines.push(Math.max(stale ? time : claude ? due(record.loadedAt)
      : Math.min(record ? due(record.loadedAt) : Infinity, due(globalSkillsLoadedAt)),
      skillCatalogRetryAt.get(key) || 0));
  }
  if (deadlines.length) catalogRefreshTimer = setTimeout(refreshCachedCatalogs, Math.max(0, Math.min(...deadlines) - time));
}
export function refreshCachedCatalogs() {
  stopCatalogRefresh();
  if (!session || pageLeaving || document.hidden || navigator.onLine === false) return;
  if (modelsLoadedAt) void loadModels().catch(() => {});
  const context = activeSkillCatalogContext();
  if (context && (context.payload.backend !== "claude" && globalSkillsLoadedAt
      || skillCatalogs.has(skillScopeKey(context.scope))))
    void loadSkillChoices(context.scope, context.payload).catch(() => {});
  scheduleCatalogRefresh();
}
export function validCachedSkill(skill) {
  return typeof skill?.name === "string" && typeof skill?.path === "string"
    && (skill.backend !== "claude" || skill.command === skill.name
      && /^[\p{L}\p{N}_.:-]{1,256}$/u.test(skill.command) && skill.path === `claude-skill:${skill.command}`);
}
function skillCwdKey(cwd) {
  if (typeof cwd !== "string" || !cwd) return null;
  const path = cwd.replaceAll("\\", "/").replace(/\/+$/, "") || "/";
  return `cwd:${/^[a-z]:\//i.test(path) || path.startsWith("//") ? path.toLowerCase() : path}`;
}
function skillScopeKey(scope) {
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
function skillCatalogFresh(scope) {
  const record = skillCatalogs.get(skillScopeKey(scope));
  if (scope.startsWith("claude:"))
    return record?.backend === "claude" && record.epoch === globalSkillEpoch && catalogFresh(record);
  return !globalSkillsInvalidated && (record?.epoch || "") === globalSkillEpoch
    && catalogFresh(record) && catalogFresh({ loadedAt: globalSkillsLoadedAt });
}
function skillCatalogUsable(scope) {
  if (scope.startsWith("claude:")) {
    const record = skillCatalogs.get(skillScopeKey(scope));
    return record?.backend === "claude" && catalogUsable(record);
  }
  return catalogUsable(skillCatalogs.get(skillScopeKey(scope))) && catalogUsable({ loadedAt: globalSkillsLoadedAt });
}
function skillCatalogNeedsRefresh(scope) {
  return !skillCatalogFresh(scope) || catalogNeedsRefresh(skillCatalogs.get(skillScopeKey(scope)))
    || !scope.startsWith("claude:") && catalogNeedsRefresh({ loadedAt: globalSkillsLoadedAt });
}
function applySkillCatalog(record, scope) {
  if (skillScope === scope && appliedSkillCatalog === record && appliedGlobalSkills === globalSkillChoices) return;
  const previous = skillScope === scope ? slashMenuEntries()[skillIndex] : null;
  const allowedGlobals = new Set(record.globalPaths);
  skillChoices = record.backend === "claude" ? [...record.data]
    : [...globalSkillChoices.filter(skill => allowedGlobals.has(skill.path)), ...record.data];
  if (record.backend === "claude" && isClaude() && modelsScope === modelContextKey() && record.nativeCapabilities) {
    // 同次原生探测也确认压缩/目标能力；历史会话没有完整模型目录时也能打开菜单底部入口。
    setModelDefaults({ ...modelDefaults, nativeCapabilities: record.nativeCapabilities });
  }
  skillScope = scope; skillIndex = 0;
  if (previous) {
    const index = slashMenuEntries().findIndex(entry => entry.type === previous.type
      && (entry.type !== "skill" || skillIdentity(entry.skill) === skillIdentity(previous.skill)));
    if (index >= 0) skillIndex = index;
  }
  appliedSkillCatalog = record; appliedGlobalSkills = globalSkillChoices;
  ++skillRenderVersion; scheduleMessages();
}
async function readSkillCatalog(scope, payload, force) {
  const key = skillScopeKey(scope), owner = privateInputScope(session), revision = skillCatalogRevision;
  const claude = payload.backend === "claude";
  const current = () => privateInputScope(session) === owner && revision === skillCatalogRevision;
  if (!force && !skillCatalogUsable(scope) && readScope === owner) {
    if (!claude && !catalogUsable({ loadedAt: globalSkillsLoadedAt })) {
      const global = await optionalCachedRead(readCache.get(owner, "skills", "global"), 50);
      if (current() && catalogUsable(global) && Array.isArray(global.data) && global.data.every(validCachedSkill)) {
        globalSkillChoices = global.data; globalSkillsLoadedAt = global.loadedAt;
        globalSkillEpoch = typeof global.epoch === "string" ? global.epoch : "";
        globalSkillsInvalidated = global.invalidated === true;
      }
    }
    const cached = await optionalCachedRead(readCache.get(owner, "skills", key), 50);
    if (current() && catalogUsable(cached) && Array.isArray(cached.data) && Array.isArray(cached.globalPaths)
        && cached.data.every(validCachedSkill) && cached.globalPaths.every(path => typeof path === "string")
        && (claude ? cached.backend === "claude" && cached.instanceId === payload.instanceId
            && cached.data.every(skill => skill.backend === "claude")
          : cached.backend !== "claude" && catalogUsable({ loadedAt: globalSkillsLoadedAt }))) {
      skillCatalogs.set(key, cached);
    }
  }
  if (!current()) return null;
  if (!force && skillCatalogUsable(scope)) {
    if (skillCatalogNeedsRefresh(scope) && Date.now() >= (skillCatalogRetryAt.get(key) || 0))
      void refreshSkillCatalogRecord(scope, payload, true).catch(() => {});
    scheduleCatalogRefresh();
    return skillCatalogs.get(key);
  }
  return refreshSkillCatalogRecord(scope, payload, force
    || !claude && !!globalSkillsLoadedAt && catalogNeedsRefresh({ loadedAt: globalSkillsLoadedAt }));
}
async function refreshSkillCatalogRecord(scope, payload, force) {
  const key = skillScopeKey(scope), owner = privateInputScope(session), revision = skillCatalogRevision;
  if (skillCatalogRequests.has(key)) return skillCatalogRequests.get(key);
  const current = () => privateInputScope(session) === owner && revision === skillCatalogRevision;
  const request = (async () => {
    const result = await api("/api/read/skills", { ...payload, forceReload: force });
    if (!current()) return null;
    if (!Array.isArray(result.data) || !result.data.every(validCachedSkill))
      throw new Error("Skill 列表返回格式无效");
    const claude = payload.backend === "claude";
    if (claude && (result.backend !== "claude" || result.instanceId !== payload.instanceId
        || result.data.some(skill => skill.backend !== "claude" || skill.command !== skill.name
          || skill.path !== `claude-skill:${skill.command}`)))
      throw new Error("Claude Skill 目录与当前实例不匹配");
    const data = result.data;
    const globals = claude ? [] : data.filter(skill => ["user", "system", "admin"].includes(skill.scope));
    const mergedGlobals = new Map((catalogUsable({ loadedAt: globalSkillsLoadedAt })
      ? globalSkillChoices : []).map(skill => [skill.path, skill]));
    for (const skill of globals) mergedGlobals.set(skill.path, skill);
    const nextGlobals = [...mergedGlobals.values()].sort((left, right) => left.name.localeCompare(right.name));
    const loadedAt = Date.now();
    const record = { cwd: result.cwd, loadedAt, epoch: globalSkillEpoch,
      ...(claude ? { backend: "claude", instanceId: result.instanceId, nativeCapabilities: result.nativeCapabilities } : {}),
      globalPaths: globals.map(skill => skill.path),
      data: claude ? data : data.filter(skill => !["user", "system", "admin"].includes(skill.scope)) };
    const resolvedKey = claude && skillCwdKey(result.cwd) ? JSON.stringify(["claude", result.instanceId, skillCwdKey(result.cwd)])
      : claude ? key
      : skillCwdKey(result.cwd) || key;
    // 完整新目录生成后再替换；刷新过程中保留原列表和已选项。
    if (!claude) {
      globalSkillChoices = nextGlobals; globalSkillsLoadedAt = loadedAt; globalSkillsInvalidated = false;
    }
    skillScopeAliases.set(scope, resolvedKey);
    skillCatalogs.delete(resolvedKey); skillCatalogs.set(resolvedKey, record);
    skillCatalogRetryAt.delete(key); skillCatalogRetryAt.delete(resolvedKey);
    while (skillCatalogs.size > 64) {
      const oldest = skillCatalogs.keys().next().value;
      skillCatalogs.delete(oldest); skillCatalogRetryAt.delete(oldest);
    }
    if (!claude) cacheDisplayData("skills", "global", { data: globalSkillChoices, loadedAt, epoch: globalSkillEpoch },
      { maxAge: catalogFallbackMs });
    cacheDisplayData("skills", resolvedKey, record, { maxAge: catalogFallbackMs });
    if (claude && key !== resolvedKey)
      cacheDisplayData("skills", key, record, { maxAge: catalogFallbackMs });
    const active = activeSkillCatalogContext();
    if (active && skillScopeKey(active.scope) === resolvedKey) {
      applySkillCatalog(record, active.scope);
      if ($("#skill-menu") && !$("#skill-menu").hidden && document.activeElement === $("#prompt"))
        renderSkillMenu();
    }
    return record;
  })();
  skillCatalogRequests.set(key, request);
  try { return await request; }
  catch (error) {
    if (current()) skillCatalogRetryAt.set(key, Date.now() + catalogRetryMs);
    throw error;
  }
  finally {
    if (skillCatalogRequests.get(key) === request) {
      skillCatalogRequests.delete(key);
      scheduleCatalogRefresh();
    }
  }
}
export async function loadSkillChoices(scope, payload, { force = false } = {}) {
  if (!force && skillLoading?.scope === scope) return skillLoading.promise;
  const version = ++skillLoadVersion, selection = selectionVersion, owner = privateInputScope(session);
  const loading = skillLoading = { scope };
  if (skillScope !== scope) {
    skillChoices = payload.backend === "claude" ? [] : [...globalSkillChoices]; skillScope = ""; ++skillRenderVersion;
    appliedSkillCatalog = appliedGlobalSkills = null;
  }
  loading.promise = (async () => {
    try {
      const record = await readSkillCatalog(scope, payload, force);
      if (record && version === skillLoadVersion && selection === selectionVersion
          && privateInputScope(session) === owner) {
        applySkillCatalog(skillCatalogs.get(skillScopeKey(scope)) || record, scope);
      }
    } finally {
      if (skillLoading === loading) { skillLoading = null; scheduleCatalogRefresh(); }
    }
  })();
  return loading.promise;
}
function invalidateSkillCatalogs() {
  skillCatalogRevision++; skillCatalogRequests.clear(); skillCatalogRetryAt.clear();
  globalSkillEpoch = randomId(); globalSkillsInvalidated = true;
  skillLoading = null; ++skillLoadVersion; messageSkillLoadScope = "";
  if (globalSkillsLoadedAt) cacheDisplayData("skills", "global", {
    data: globalSkillChoices, loadedAt: globalSkillsLoadedAt, epoch: globalSkillEpoch, invalidated: true
  }, { maxAge: catalogFallbackMs });
  scheduleCatalogRefresh();
}
export async function refreshSkillCatalog() {
  invalidateSkillCatalogs();
  const context = activeSkillCatalogContext();
  if (context) await loadSkillChoices(context.scope, context.payload, { force: true });
}
export async function updateSkillMenu() {
  if (editingMessage || editingQueuedMessage) { closeSkillMenu(); return; }
  const query = slashQuery(), prompt = $("#prompt");
  if (!query || !session || threadReadOnly() || document.activeElement !== prompt) { closeSkillMenu(); return; }
  const context = activeSkillCatalogContext();
  if (!context) { renderSkillMenu("请先选择会话，或为新会话选择项目"); return; }
  const { scope, payload } = context;
  if (skillScope === scope && skillCatalogUsable(scope)) {
    renderSkillMenu();
    void loadSkillChoices(scope, payload).catch(() => {});
    return;
  }
  const selection = selectionVersion;
  if (skillCatalogUsable(scope)) {
    applySkillCatalog(skillCatalogs.get(skillScopeKey(scope)), scope);
    renderSkillMenu();
  } else renderSkillMenu(isClaude() ? "正在读取所选 Claude 实例的 Skill…" : "正在读取本机可用 Skill…");
  const request = loadSkillChoices(scope, payload);
  const version = skillLoadVersion, deviceId = session.device.id;
  try {
    await request;
    if (skillScope === scope && selection === selectionVersion && $("#skill-menu") && !$("#skill-menu").hidden
        && document.activeElement === $("#prompt")) renderSkillMenu();
  } catch (error) {
    if (version === skillLoadVersion && selection === selectionVersion && session?.device.id === deviceId
        && $("#skill-menu") && !$("#skill-menu").hidden) renderSkillMenu(error.message);
  }
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
export function selectCompactCommand() {
  const query = slashQuery();
  if (!query || !compactCommandAvailable()) return;
  const prompt = $("#prompt");
  prompt.setRangeText("", query.start, query.end, "end");
  closeComposerMenus(); updateComposer(); prompt.focus();
  void compactContext();
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
export function handlePromptInput(event, { preserveCanonical = false } = {}) {
  if (!preserveCanonical) forgetPromptCanonicalText();
  if (!preserveCanonical) {
    const prompt = $("#prompt");
    if (prompt?.querySelector("div,p")) {
      const selection = promptSelection(prompt), text = promptDomText(prompt);
      writePromptContent(prompt, text);
      setPromptSelection(prompt, selection.start, selection.end, selection);
    }
  }
  if (event?.isComposing) { rememberQueuedMessageEdit(); updateComposer(); closeSkillMenu(); return; }
  if (preserveCanonical) restorePromptCanonicalContent();
  syncSelectedSkillsFromPrompt();
  syncPromptTrailingBreak($("#prompt"));
  rememberDraft();
  updateComposer();
  skillIndex = 0; void updateSkillMenu();
  $("#prompt")?.editHistory?.commit();
}
