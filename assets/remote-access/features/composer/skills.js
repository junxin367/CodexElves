// 斜杠命令与 Skill：命令菜单渲染与更新、Skill 目录的读取与刷新调度、选择压缩命令及输入事件处理；目录状态与缓存判断在 skill-catalog.js，选择 Skill 在 selected-skills.js。

import { $, button, element } from "../../ui/dom.js";
import { closePopup } from "../../ui/popup.js";
import { closeComposerMenus, loadModels, scheduleMessages, updateComposer } from "../../core/actions.js";
import {
  busy, connected, creating, pageLeaving, selectedId, selectionVersion, session, stopping, thread
} from "../../core/state.js";
import { readCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { closeSkillMenu, fitComposerMenu } from "./composer-menus.js";
import { rememberDraft, rememberQueuedMessageEdit } from "./drafts.js";
import { closeContextUsageTip, compactUsageDescription, contextUsage } from "./metrics.js";
import { modelContextKey } from "./model-catalog.js";
import {
  modelDefaults, modelsLoadedAt, modelsPromise, modelsRetryAt, modelsScope, setModelDefaults
} from "./options-state.js";
import {
  forgetPromptCanonicalText, promptDomText, promptSelection, restorePromptCanonicalContent,
  setPromptSelection, skillIdentity, skillVisibleName, syncPromptTrailingBreak,
  syncSelectedSkillsFromPrompt, writePromptContent
} from "./prompt-editor.js";
import { activatePromptOptimization } from "./prompt-optimization.js";
import { selectSkill } from "./selected-skills.js";
import {
  appliedGlobalSkills, appliedSkillCatalog, catalogCacheMs, catalogFallbackMs, catalogFresh,
  catalogNeedsRefresh, catalogRefreshAheadMs, catalogRetryMs, catalogUsable, compactCommandIcon,
  compactCommandMatches, globalSkillChoices, globalSkillEpoch, globalSkillsLoadedAt,
  goalComposerIcon, normalizeSlashIndex, promptOptimizationCommandIcon, promptOptimizationCommandMatches,
  selectedSkills, setAppliedGlobalSkills,
  setAppliedSkillCatalog, setCatalogRefreshTimer, setGlobalSkillChoices, setGlobalSkillEpoch,
  setGlobalSkillsInvalidated, setGlobalSkillsLoadedAt, setMessageSkillLoadScope,
  setSkillCatalogRevision, setSkillChoices, setSkillIndex, setSkillLoadVersion, setSkillLoading,
  setSkillRenderVersion, setSkillScope, skillCatalogFresh, skillCatalogNeedsRefresh,
  skillCatalogRequests, skillCatalogRetryAt, skillCatalogRevision, skillCatalogUsable,
  skillCatalogs, skillChoices, skillCwdKey, skillIndex, skillLoadVersion, skillLoading,
  skillRenderVersion, skillScope, skillScopeAliases, skillScopeKey, slashQuery, stopCatalogRefresh,
  validCachedSkill
} from "./skill-catalog.js";
import { compactContext, compactionLocked } from "../conversation/compaction.js";
import { pending } from "../conversation/pending-state.js";
import { goalComposerReason, selectGoalCommand } from "../goal/composer.js";
import { goalCommandMatches, selectedGoal } from "../goal/state.js";
import { editingMessage } from "../messages/message-edit.js";
import { editingQueuedMessage } from "../queue/state.js";
import { claudeCommandSupported } from "../session/action-capabilities.js";
import { backendCapability, isClaude, privateInputScope, threadReadOnly } from "../session/capabilities.js";
import { activeInstanceId } from "../session/instances.js";
import { cacheDisplayData, currentThreadDisplay, optionalCachedRead, readScope } from "../session/thread-cache.js";

export function compactCommandAvailable() {
  if (isClaude() && !claudeCommandSupported("compact")) return false;
  return !!session && !creating && !!selectedId && thread?.id === selectedId && !threadReadOnly() && connected
    && !busy && !stopping && !pending && !editingMessage && !editingQueuedMessage && !compactionLocked(selectedId);
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
  // 用户要求在 / 菜单提供提示词优化；独立优化服务不依赖 Claude 原生压缩或目标能力。
  if (promptOptimizationCommandMatches(query.query)) {
    const control = $("#prompt-optimize");
    entries.push({ type: "promptOptimize", enabled: !!control && !control.disabled && !threadReadOnly() });
  }
  return entries;
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
    if (entry.type !== "skill") {
      const goal = entry.type === "goal", optimize = entry.type === "promptOptimize";
      const usage = entry.type === "compact" ? contextUsage(currentThreadDisplay()?.tokenUsage) : null;
      const label = optimize ? "提示词优化" : goal ? "目标" : "压缩";
      const reason = goal ? goalComposerReason() : "";
      const action = optimize ? selectPromptOptimizationCommand : goal ? () => selectGoalCommand() : selectCompactCommand;
      const node = button("", reason || label, action,
        `skill-option slash-command-option${index === active ? " active" : ""}${goal ? " slash-goal-option" : ""}`);
      node.id = `skill-option-${index}`; node.disabled = !entry.enabled;
      node.setAttribute("role", "option"); node.setAttribute("aria-selected", String(index === active));
      const copy = element("span", "slash-command-copy");
      copy.append(element("strong", "", label));
      const description = optimize ? "使用已配置的模型改写当前输入"
        : goal ? reason || "设置要持续追求的目标" : compactUsageDescription(usage);
      if (description) copy.append(element("small", "muted slash-command-usage", description));
      const commandIcon = optimize ? promptOptimizationCommandIcon() : goal ? goalComposerIcon() : compactCommandIcon();
      node.append(commandIcon, copy);
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
  if (deadlines.length) setCatalogRefreshTimer(setTimeout(refreshCachedCatalogs, Math.max(0, Math.min(...deadlines) - time)));
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
function applySkillCatalog(record, scope) {
  if (skillScope === scope && appliedSkillCatalog === record && appliedGlobalSkills === globalSkillChoices) return;
  const previous = skillScope === scope ? slashMenuEntries()[skillIndex] : null;
  const allowedGlobals = new Set(record.globalPaths);
  setSkillChoices(record.backend === "claude" ? [...record.data]
    : [...globalSkillChoices.filter(skill => allowedGlobals.has(skill.path)), ...record.data]);
  if (record.backend === "claude" && isClaude() && modelsScope === modelContextKey() && record.nativeCapabilities) {
    // 同次原生探测也确认压缩/目标能力；历史会话没有完整模型目录时也能打开菜单底部入口。
    setModelDefaults({ ...modelDefaults, nativeCapabilities: record.nativeCapabilities });
  }
  setSkillScope(scope); setSkillIndex(0);
  if (previous) {
    const index = slashMenuEntries().findIndex(entry => entry.type === previous.type
      && (entry.type !== "skill" || skillIdentity(entry.skill) === skillIdentity(previous.skill)));
    if (index >= 0) setSkillIndex(index);
  }
  setAppliedSkillCatalog(record); setAppliedGlobalSkills(globalSkillChoices);
  setSkillRenderVersion(skillRenderVersion + 1); scheduleMessages();
}
async function readSkillCatalog(scope, payload, force) {
  const key = skillScopeKey(scope), owner = privateInputScope(session), revision = skillCatalogRevision;
  const claude = payload.backend === "claude";
  const current = () => privateInputScope(session) === owner && revision === skillCatalogRevision;
  if (!force && !skillCatalogUsable(scope) && readScope === owner) {
    if (!claude && !catalogUsable({ loadedAt: globalSkillsLoadedAt })) {
      const global = await optionalCachedRead(readCache.get(owner, "skills", "global"), 50);
      if (current() && catalogUsable(global) && Array.isArray(global.data) && global.data.every(validCachedSkill)) {
        setGlobalSkillChoices(global.data); setGlobalSkillsLoadedAt(global.loadedAt);
        setGlobalSkillEpoch(typeof global.epoch === "string" ? global.epoch : "");
        setGlobalSkillsInvalidated(global.invalidated === true);
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
      setGlobalSkillChoices(nextGlobals); setGlobalSkillsLoadedAt(loadedAt); setGlobalSkillsInvalidated(false);
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
  const version = setSkillLoadVersion(skillLoadVersion + 1), selection = selectionVersion, owner = privateInputScope(session);
  const loading = setSkillLoading({ scope });
  if (skillScope !== scope) {
    setSkillChoices(payload.backend === "claude" ? [] : [...globalSkillChoices]); setSkillScope(""); setSkillRenderVersion(skillRenderVersion + 1);
    setAppliedSkillCatalog(setAppliedGlobalSkills(null));
  }
  loading.promise = (async () => {
    try {
      const record = await readSkillCatalog(scope, payload, force);
      if (record && version === skillLoadVersion && selection === selectionVersion
          && privateInputScope(session) === owner) {
        applySkillCatalog(skillCatalogs.get(skillScopeKey(scope)) || record, scope);
      }
    } finally {
      if (skillLoading === loading) { setSkillLoading(null); scheduleCatalogRefresh(); }
    }
  })();
  return loading.promise;
}
function invalidateSkillCatalogs() {
  setSkillCatalogRevision(skillCatalogRevision + 1); skillCatalogRequests.clear(); skillCatalogRetryAt.clear();
  setGlobalSkillEpoch(randomId()); setGlobalSkillsInvalidated(true);
  setSkillLoading(null); setSkillLoadVersion(skillLoadVersion + 1); setMessageSkillLoadScope("");
  if (globalSkillsLoadedAt) cacheDisplayData("skills", "global", {
    data: globalSkillChoices, loadedAt: globalSkillsLoadedAt, epoch: globalSkillEpoch, invalidated: true
  }, { maxAge: catalogFallbackMs });
  scheduleCatalogRefresh();
}
export async function refreshSkillCatalog({ reload = true } = {}) {
  invalidateSkillCatalogs();
  if (!reload) return;
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
export function selectCompactCommand() {
  const query = slashQuery();
  if (!query || !compactCommandAvailable()) return;
  const prompt = $("#prompt");
  prompt.setRangeText("", query.start, query.end, "end");
  closeComposerMenus(); updateComposer(); prompt.focus();
  void compactContext();
}
export function selectPromptOptimizationCommand() {
  const query = slashQuery(), control = $("#prompt-optimize");
  if (!query || !control || control.disabled || threadReadOnly()) return;
  const prompt = $("#prompt");
  // 用户要求只移除 / 查询文本，保留其余提示词、Skill 和附件，再复用现有独立优化流程。
  prompt.setRangeText("", query.start, query.end, "end");
  rememberDraft(); closeComposerMenus(); updateComposer(); prompt.focus({ preventScroll: true });
  void activatePromptOptimization();
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
  setSkillIndex(0); void updateSkillMenu();
  $("#prompt")?.editHistory?.commit();
}
