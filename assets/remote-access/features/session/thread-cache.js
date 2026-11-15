// 会话展示缓存：会话元数据、阅读位置、工作区视图与关联菜单缓存，以及切换授权时的只读视图清理/恢复。

import { $, element } from "../../ui/dom.js";
import {
  selectedId, selectionVersion, session, setSelectionVersion, setSidebar, setThread, sidebar,
  thread, turns
} from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import { contextUsage, normalizeComposerMetrics } from "../composer/metrics.js";
import {
  modelCatalogCacheVersion, setModelDefaults, setModels, setModelsLoadedAt, setModelsPromise,
  setModelsRetryAt
} from "../composer/models.js";
import { effortLabels } from "../composer/prompt-editor.js";
import {
  catalogUsable, setAppliedGlobalSkills, setAppliedSkillCatalog, setGlobalSkillChoices,
  setGlobalSkillEpoch, setGlobalSkillsInvalidated, setGlobalSkillsLoadedAt,
  setSkillCatalogRevision, skillCatalogRequests, skillCatalogRetryAt, skillCatalogRevision,
  skillCatalogs, skillScopeAliases, stopCatalogRefresh, validCachedSkill
} from "../composer/skills.js";
import {
  followMessages, recentHistoryPage, renderMessages, resetMessages, setRecentHistoryPage,
  visibleMessageAnchor
} from "../conversation/history-view.js";
import {
  setHistoryCursor, setHistoryError, setHistoryInitialized, setTranscriptChecking
} from "../conversation/history.js";
import { transcriptEvents } from "../conversation/transcript-events.js";
import { transcriptModelName } from "../messages/items.js";
import { queueSnapshotIsVersioned } from "../queue/queue.js";
import { reconcileSentMessageDisplays } from "../queue/sent-display.js";
import { claudeThreadId, normalizeRelatedView, privateInputScope, validRelatedThreadId } from "./capabilities.js";
import { relatedHistoryKey, relatedSessionView, relatedThreadViews } from "./related.js";
import { threadDeleted } from "../sidebar/actions.js";

export const threadDisplays = new Map();
export let readScope = null, readGeneration = 0, readingPositionTimer = null;
let readingPositionRevision = 0;
export function threadEphemeralState(id, value = null) {
  const flags = [value?.ephemeral, value?.relatedView?.ephemeral,
    thread?.id === id ? thread?.ephemeral : undefined,
    relatedSessionView?.threadId === id ? relatedSessionView?.ephemeral : undefined,
    normalizeRelatedView(relatedThreadViews.get(id), id)?.ephemeral,
    selectedId === id ? recentHistoryPage?.ephemeral : undefined];
  return flags.includes(true) ? true : flags.includes(false) ? false : undefined;
}
export function discardPersistentThreadDisplay(id) {
  const scope = readScope, generation = readGeneration, positionRevision = readingPositionRevision;
  if (!scope || scope !== privateInputScope(session)) return;
  void (async () => {
    await readCache.remove(scope, "transcript", id);
    await readCache.remove(scope, "thread", id);
    const position = await readCache.get(scope, "view", "last");
    if (generation === readGeneration && scope === readScope && scope === privateInputScope(session)
        && positionRevision === readingPositionRevision
        && position?.threadId === id) await readCache.remove(scope, "view", "last");
  })().catch(() => {});
}
export function retireMissingEphemeralTranscript(id, error) {
  if (id !== selectedId || threadEphemeralState(id) !== true
      || relatedSessionView?.kind === "side" || relatedThreadViews.get(id)?.kind === "side"
      || thread?.id === id && thread.viewKind === "side"
      || /thread not loaded:/i.test(error?.message || "")) return;
  // 原生已确认不存在：使同轮回包失效，并允许重新选择同一 ID 核对原生状态。
  setSelectionVersion(selectionVersion + 1);
  resetMessages();
  setThread(null);
  threadDisplays.delete(threadDisplayKey(id));
  transcriptCache.remove(session?.device.id, id);
  discardPersistentThreadDisplay(id);
  turns.clear(); transcriptEvents.clear(); setRecentHistoryPage(null); setHistoryCursor(null);
  setHistoryInitialized(false); setTranscriptChecking(false);
  setHistoryError("此原生临时会话已关闭或不存在");
  renderMessages();
}
export function persistentThreadDisplayAllowed(id, value) {
  const ephemeral = threadEphemeralState(id, value);
  if (ephemeral === true) return false;
  const view = normalizeRelatedView(value?.relatedView, id)
    || normalizeRelatedView(relatedSessionView, id)
    || normalizeRelatedView(relatedThreadViews.get(id), id);
  const subagent = view?.kind === "subagent" || value?.viewKind === "subagent"
    || thread?.id === id && thread.viewKind === "subagent";
  // 旧子代理快照没有原生生命周期标识，重新读取原生数据后再决定能否缓存。
  return view?.kind !== "side" && (!subagent || ephemeral === false);
}
export function cacheDisplayData(kind, id, value, options) {
  if (!readScope || readScope !== privateInputScope(session)) return;
  if (["thread", "transcript", "view"].includes(kind)) {
    const threadId = kind === "view" ? value?.threadId : id;
    if (!persistentThreadDisplayAllowed(threadId, value) || value?.viewKind === "side"
        || relatedThreadViews.get(threadId)?.kind === "side") return;
  }
  if (kind === "sidebar") value = { ...value, sideThreadIds: [], unreadSideThreads: [] };
  if (kind === "view") readingPositionRevision++;
  void readCache.put(readScope, kind, id, value, options).catch(() => {});
}
export function threadDisplayKey(id) {
  return JSON.stringify([privateInputScope(session), id]);
}
export function currentThreadDisplay() {
  if (!selectedId) return null;
  const cached = threadDisplays.get(threadDisplayKey(selectedId));
  return thread?.id === selectedId ? { ...cached, ...thread } : cached || null;
}
export function syncRelatedModelDisplay(value = currentThreadDisplay()) {
  const node = $("#related-session-model");
  if (!node) return;
  const view = normalizeRelatedView(relatedSessionView, selectedId);
  const subagent = view?.kind === "subagent" || value?.viewKind === "subagent";
  const model = subagent && typeof value?.model === "string" ? value.model.trim() : "";
  const effort = subagent && typeof value?.reasoningEffort === "string" ? value.reasoningEffort.trim() : "";
  node.hidden = !model;
  if (!model) {
    node.replaceChildren(); node.removeAttribute("title"); node.removeAttribute("aria-label");
    return;
  }
  const name = transcriptModelName(model), effortLabel = effortLabels[effort] || effort;
  node.replaceChildren(element("span", "related-session-model-name", name));
  if (effortLabel) node.append(element("span", "related-session-model-effort", effortLabel));
  const description = `模型：${model}${name === model ? "" : `（${name}）`}`
    + `${effortLabel ? ` · 推理等级：${effortLabel}` : ""}`;
  node.title = description; node.setAttribute("aria-label", description);
}
export function rememberThreadDisplay(value = thread, { persist = true } = {}) {
  if (!session || !validRelatedThreadId(value?.id) || threadDeleted(value.id) || value.sideThreadClosed
      || value.model != null && typeof value.model !== "string"
      || value.reasoningEffort != null && typeof value.reasoningEffort !== "string") return null;
  const key = threadDisplayKey(value.id);
  value = { ...threadDisplays.get(key), ...value };
  const usage = contextUsage(value.tokenUsage), ephemeral = threadEphemeralState(value.id, value);
  // 缓存目标和队列的展示数据；审批、权限及写操作仍使用重新读取的会话。
  const display = {
    id: value.id, backend: value.backend || (claudeThreadId(value.id) ? "claude" : "codex"),
    ...(value.instanceId ? { instanceId: value.instanceId, instanceLabel: value.instanceLabel,
      wslDistribution: value.wslDistribution, wslUser: value.wslUser } : {}),
    model: value.model || "", reasoningEffort: value.reasoningEffort || "",
    ...(typeof value.fastMode === "boolean" ? { fastMode: value.fastMode } : {}),
    ...(normalizeComposerMetrics(value.sessionMetrics) ? { sessionMetrics: normalizeComposerMetrics(value.sessionMetrics) } : {}),
    // 用户要求缺容量时仍保留真实最后请求用量；缓存不能把已知 Token 一并丢掉，也不能伪造百分比。
    tokenUsage: usage || typeof value.tokenUsage?.last?.totalTokens === "number"
      && Number.isFinite(value.tokenUsage.last.totalTokens) && value.tokenUsage.last.totalTokens >= 0
      ? { modelContextWindow: usage?.total ?? null,
        // 用户要求模型容量缺失时使用配置压缩窗口；缓存所用分母和用量，百分比统一按 Codex 原有规则重算。
        ...(typeof value.tokenUsage.model === "string" ? { model: value.tokenUsage.model } : {}),
        ...(value.tokenUsage.modelContextWindowSource ? { modelContextWindowSource: value.tokenUsage.modelContextWindowSource } : {}),
        ...(value.tokenUsage.compaction && typeof value.tokenUsage.compaction === "object"
          ? { compaction: { windowTokens: value.tokenUsage.compaction.windowTokens,
            triggerPercent: value.tokenUsage.compaction.triggerPercent } } : {}),
        last: { totalTokens: value.tokenUsage.last.totalTokens } } : null,
    ...(Object.hasOwn(value, "goal") ? { goal: value.goal } : {}),
    ...(Array.isArray(value.queuedMessages) ? { queuedMessages: value.queuedMessages } : {}),
    ...(Object.hasOwn(value, "queueSnapshotVersion") ? { queueSnapshotVersion: value.queueSnapshotVersion } : {}),
    ...(typeof ephemeral === "boolean" ? { ephemeral } : {}),
    ...(["side", "subagent"].includes(value.viewKind) ? { viewKind: value.viewKind } : {})
  };
  threadDisplays.delete(key); threadDisplays.set(key, display);
  while (threadDisplays.size > 32) threadDisplays.delete(threadDisplays.keys().next().value);
  if (value.id === selectedId) syncRelatedModelDisplay(display);
  if (persist) cacheDisplayData("thread", value.id, display);
  return display;
}
export function rememberBackgroundThreadDisplay(event, params) {
  const queueChanged = event.method === "thread/queue/changed";
  const goalChanged = ["thread/goal/updated", "thread/goal/cleared"].includes(event.method);
  const id = params.threadId;
  if ((!queueChanged && !goalChanged) || id === selectedId || !validRelatedThreadId(id)
      || !threadDisplays.has(threadDisplayKey(id))) return;
  if (queueChanged && Array.isArray(params.queuedMessages)) {
    const previous = threadDisplays.get(threadDisplayKey(id)).queueSnapshotVersion;
    const versioned = queueSnapshotIsVersioned(params);
    if (previous && (!versioned || previous.queueEpoch === params.queueEpoch
        && params.queueRevision <= previous.queueRevision)) return;
    rememberThreadDisplay({ id, queuedMessages: params.queuedMessages,
      queueSnapshotVersion: versioned ? { queueEpoch: params.queueEpoch, queueRevision: params.queueRevision } : null });
    reconcileSentMessageDisplays(id, params.queuedMessages);
  }
  if (goalChanged)
    rememberThreadDisplay({ id, goal: event.method === "thread/goal/cleared" ? null : params.goal });
}
function ownsWorkspaceCache(scope, kind) {
  return !!scope && scope === readScope && scope === privateInputScope(session)
    && ["workspace-list", "workspace-detail"].includes(kind);
}
export const workspaceViewCache = {
  async get(scope, kind, id) {
    if (!ownsWorkspaceCache(scope, kind)) return null;
    const generation = readGeneration;
    const value = await readCache.get(scope, kind, id);
    return generation === readGeneration && ownsWorkspaceCache(scope, kind) ? value : null;
  },
  put(scope, kind, id, value) {
    return ownsWorkspaceCache(scope, kind)
      ? readCache.put(scope, kind, id, value) : Promise.resolve(false);
  },
  remove(scope, kind, id) {
    return ownsWorkspaceCache(scope, kind)
      ? readCache.remove(scope, kind, id) : Promise.resolve();
  }
};
export const relatedMenuCache = {
  async get(scope, id) {
    if (!scope || scope !== readScope || scope !== privateInputScope(session)) return null;
    const generation = readGeneration;
    const value = await readCache.get(scope, "session-menu", id);
    return generation === readGeneration && scope === readScope && scope === privateInputScope(session) ? value : null;
  },
  put(scope, id, value) {
    return scope && scope === readScope && scope === privateInputScope(session)
      ? readCache.put(scope, "session-menu", id, value) : Promise.resolve(false);
  },
  remove(scope, id) {
    return scope && scope === readScope && scope === privateInputScope(session)
      ? readCache.remove(scope, "session-menu", id) : Promise.resolve();
  }
};
export async function optionalCachedRead(task, timeout = 250) {
  let timer;
  try {
    return await Promise.race([task.catch(() => null),
      new Promise(resolve => { timer = setTimeout(() => resolve(null), timeout); })]);
  } finally { clearTimeout(timer); }
}
export function readingPosition() {
  const root = $("#messages"), anchor = root && visibleMessageAnchor(root);
  const ephemeral = threadEphemeralState(selectedId);
  return { threadId: selectedId, scrollTop: root?.scrollTop || 0, followMessages,
    ...(typeof ephemeral === "boolean" ? { ephemeral } : {}),
    ...(relatedSessionView ? { relatedView: normalizeRelatedView(relatedSessionView, selectedId) } : {}),
    anchor: anchor?.key ? { key: anchor.key, offset: anchor.offset } : null };
}
export function rememberReadingPosition() {
  clearTimeout(readingPositionTimer); readingPositionTimer = null;
  cacheDisplayData("view", "last", readingPosition());
}
export function scheduleReadingPosition() {
  clearTimeout(readingPositionTimer);
  readingPositionTimer = setTimeout(rememberReadingPosition, 500);
}
function cachedReadingPosition(snapshot, position) {
  const anchor = position?.anchor;
  const containsAnchor = anchor && snapshot.turns.some(turn =>
    Array.isArray(turn?.items) && turn.items.some(item => `${turn.id}:${item?.id}` === anchor.key));
  if (!position || position.followMessages || anchor && !containsAnchor
      || snapshot.cacheWindowOnly && !containsAnchor)
    return { followMessages: true, scrollTop: 0, anchor: null };
  return { followMessages: false, scrollTop: position.scrollTop || 0, anchor: containsAnchor ? anchor : null };
}
export function sidebarSnapshot(value) {
  const { unreadSideThreads, ...snapshot } = value;
  // 用户要求项目对齐桌面已保存项目；升级后的离线缓存也只保留已保存项目，旧合成项目的会话回到最近列表。
  const projects = value.projects.filter(project => !project.synthetic);
  const projectIds = new Set(projects.map(project => project.id));
  const displayThread = item => {
    const { status, priority, hasUnreadTurn, sideThreadAttention, requests, queuedMessages, queuedEdits, ...display } = item;
    if (value.nativeBridge === "harness" && display.projectId && !projectIds.has(display.projectId))
      display.projectId = null;
    return display;
  };
  return { ...snapshot,
    recent: value.recent.map(displayThread),
    pinnedThreads: value.pinnedThreads.map(displayThread),
    projects: projects.map(project => ({ ...project, threads: project.threads.map(displayThread) })) };
}
export function clearReadViews() {
  const previousScope = readScope;
  readScope = null; readGeneration++;
  threadDisplays.clear();
  clearTimeout(readingPositionTimer); readingPositionTimer = null;
  skillCatalogs.clear(); skillScopeAliases.clear();
  skillCatalogRequests.clear(); setSkillCatalogRevision(skillCatalogRevision + 1);
  skillCatalogRetryAt.clear(); stopCatalogRefresh();
  setGlobalSkillChoices([]); setGlobalSkillsLoadedAt(0);
  setGlobalSkillEpoch(""); setGlobalSkillsInvalidated(false);
  setAppliedSkillCatalog(setAppliedGlobalSkills(null));
  setModelsRetryAt(0);
  setModels([]); setModelsPromise(null); setModelsLoadedAt(0);
  setModelDefaults({ model: "", reasoningEffort: "" });
  return readCache.clear(previousScope).catch(() => {});
}
export async function restoreReadViews(value) {
  const scope = privateInputScope(value);
  if (readScope === scope && sidebar) return;
  readScope = scope;
  const revision = ++readGeneration;
  const load = (async () => {
    await readCache.activate(scope);
    const [savedSidebar, view, modelCatalog, globalSkills] = await Promise.all([
      readCache.get(scope, "sidebar", "main"), readCache.get(scope, "view", "last"),
      readCache.get(scope, "models", "global"), readCache.get(scope, "skills", "global")
    ]);
    const requested = new URLSearchParams(location.hash.slice(1)).get("thread");
    const id = requested || view?.threadId;
    const [snapshot, display] = typeof id === "string" && /^[A-Za-z0-9_-]{1,120}$/.test(id)
      ? await Promise.all([readCache.get(scope, "transcript", id), readCache.get(scope, "thread", id)]) : [];
    return { savedSidebar, view, modelCatalog, globalSkills, id, snapshot, display, requested };
  })().catch(() => null);
  // 缓存损坏、空间不足或旧标签页锁住数据库时，首屏直接走网络，不等待缓存修复。
  const saved = await optionalCachedRead(load);
  if (!saved || revision !== readGeneration || scope !== readScope || scope !== privateInputScope(session)) return;
  const validSummary = item => item && typeof item.id === "string";
  if (saved.savedSidebar?.preferences && Array.isArray(saved.savedSidebar.recent)
      && Array.isArray(saved.savedSidebar.pinnedThreads) && Array.isArray(saved.savedSidebar.projects)
      && Array.isArray(saved.savedSidebar.sideThreadIds)
      && saved.savedSidebar.recent.every(validSummary) && saved.savedSidebar.pinnedThreads.every(validSummary)
      && saved.savedSidebar.projects.every(project => validSummary(project) && Array.isArray(project.threads)
        && project.threads.every(validSummary)))
    setSidebar(sidebarSnapshot(saved.savedSidebar));
  // 用户要求模型能力共用目录；缺失 Fast 元数据或未过滤 Ultra 的旧缓存必须重新读取。
  if (saved.modelCatalog?.version === modelCatalogCacheVersion && Array.isArray(saved.modelCatalog.models)
      && saved.modelCatalog.models.every(model => model && typeof model.model === "string"
        && (model.supportedReasoningEfforts == null || Array.isArray(model.supportedReasoningEfforts)))
      && catalogUsable(saved.modelCatalog)) {
    setModels(saved.modelCatalog.models);
    setModelDefaults({ model: saved.modelCatalog.defaults?.model || "",
      reasoningEffort: saved.modelCatalog.defaults?.reasoningEffort || "",
      nativeCapabilities: saved.modelCatalog.defaults?.nativeCapabilities,
      fastModeAvailability: saved.modelCatalog.defaults?.fastModeAvailability,
      fastModeOwner: saved.modelCatalog.defaults?.fastModeOwner,
      fastModelAliases: saved.modelCatalog.defaults?.fastModelAliases });
    setModelsLoadedAt(saved.modelCatalog.loadedAt);
  }
  if (saved.globalSkills && Array.isArray(saved.globalSkills.data)
      && saved.globalSkills.data.every(validCachedSkill)
      && catalogUsable(saved.globalSkills)) {
    setGlobalSkillChoices(saved.globalSkills.data); setGlobalSkillsLoadedAt(saved.globalSkills.loadedAt);
    setGlobalSkillEpoch(typeof saved.globalSkills.epoch === "string" ? saved.globalSkills.epoch : "");
    setGlobalSkillsInvalidated(saved.globalSkills.invalidated === true);
  }
  if (typeof saved.id === "string" && /^[A-Za-z0-9_-]{1,120}$/.test(saved.id) && !threadDeleted(saved.id)) {
    if (!persistentThreadDisplayAllowed(saved.id, saved.snapshot)
        || !persistentThreadDisplayAllowed(saved.id, saved.display)
        || !persistentThreadDisplayAllowed(saved.id, saved.view?.threadId === saved.id
          ? { ...saved.snapshot, ...saved.view } : null)) {
      discardPersistentThreadDisplay(saved.id);
      return;
    }
    if (saved.display?.id === saved.id) rememberThreadDisplay(saved.display, { persist: false });
    if (Array.isArray(saved.snapshot?.turns) && saved.snapshot.turns.length) {
      const position = saved.view?.threadId === saved.id ? saved.view : saved.snapshot;
      transcriptCache.put(value.device.id, saved.id,
        { ...saved.snapshot, ...cachedReadingPosition(saved.snapshot, position) });
    }
    if (!saved.requested) {
      const state = { ...(history.state || {}) };
      const view = normalizeRelatedView(saved.view?.relatedView || saved.snapshot?.relatedView, saved.id);
      if (view) state[relatedHistoryKey] = view;
      else delete state[relatedHistoryKey];
      history.replaceState(state, "", `${location.pathname}${location.search}#thread=${encodeURIComponent(saved.id)}`);
    }
  }
}
