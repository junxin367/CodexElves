// 会话展示缓存：会话元数据、阅读位置、工作区视图与关联菜单缓存，以及切换授权时清理读取视图；显示快照的记录与恢复在 thread-display.js。

import { $ } from "../../ui/dom.js";
import { selectedId, session, thread } from "../../core/state.js";
import { readCache } from "../../core/stores.js";
import {
  setModelDefaults, setModels, setModelsLoadedAt, setModelsPromise, setModelsRetryAt
} from "../composer/options-state.js";
import {
  setAppliedGlobalSkills, setAppliedSkillCatalog, setGlobalSkillChoices, setGlobalSkillEpoch,
  setGlobalSkillsInvalidated, setGlobalSkillsLoadedAt, setSkillCatalogRevision,
  skillCatalogRequests, skillCatalogRetryAt, skillCatalogRevision, skillCatalogs, skillScopeAliases,
  stopCatalogRefresh
} from "../composer/skill-catalog.js";
import { followMessages, recentHistoryPage, visibleMessageAnchor } from "../conversation/history-state.js";
import { normalizeRelatedView, privateInputScope } from "./capabilities.js";
import { relatedSessionView, relatedThreadViews } from "./related-state.js";

export const threadDisplays = new Map();
export let readScope = null, readGeneration = 0, readingPositionTimer = null;
export function setReadScope(value) { return readScope = value; }
export function setReadGeneration(value) { return readGeneration = value; }
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
export function cachedReadingPosition(snapshot, position) {
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
