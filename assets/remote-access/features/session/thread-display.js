// 会话显示快照：记录当前与后台会话的显示数据，恢复读取视图，同步关联会话的模型显示；缓存结构在 thread-cache.js。

import { $, element } from "../../ui/dom.js";
import { selectedId, session, setSidebar, sidebar, thread } from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import { contextUsage, normalizeComposerMetrics } from "../composer/metrics.js";
import { modelCatalogCacheVersion, setModelDefaults, setModels, setModelsLoadedAt } from "../composer/options-state.js";
import { effortLabels } from "../composer/prompt-editor.js";
import {
  catalogUsable, setGlobalSkillChoices, setGlobalSkillEpoch, setGlobalSkillsInvalidated,
  setGlobalSkillsLoadedAt, validCachedSkill
} from "../composer/skill-catalog.js";
import { transcriptModelName } from "../messages/item-presentation.js";
import { reconcileSentMessageDisplays } from "../queue/sent-display.js";
import { queueSnapshotIsVersioned } from "../queue/state.js";
import { claudeThreadId, normalizeRelatedView, privateInputScope, validRelatedThreadId } from "./capabilities.js";
import { relatedHistoryKey, relatedSessionView } from "./related-state.js";
import {
  cacheDisplayData, cachedReadingPosition, currentThreadDisplay, discardPersistentThreadDisplay,
  optionalCachedRead, persistentThreadDisplayAllowed, readGeneration, readScope, setReadGeneration,
  setReadScope, sidebarSnapshot, threadDisplayKey, threadDisplays, threadEphemeralState
} from "./thread-cache.js";
import { threadDeleted } from "../sidebar/deletion-state.js";

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
export async function restoreReadViews(value) {
  const scope = privateInputScope(value);
  if (readScope === scope && sidebar) return;
  setReadScope(scope);
  const revision = setReadGeneration(readGeneration + 1);
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
