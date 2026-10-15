(() => {
  if (window.__codexElvesRemoteAccess?.version === 22) return;
  window.__codexElvesRemoteAccess?.dispose?.();
  let manager = null;
  let appScope = null;
  let unsubscribe = [];
  let eventSequence = 0;
  const epoch = crypto.randomUUID();
  const notifications = [];
  const queuedReorders = new Set();
  const queuedEditStore = (() => {
    const existing = window.__codexElvesRemoteQueuedEditStore;
    if (existing?.version === 1 && existing.sessions instanceof Map) return existing;
    const created = { version: 1, sessions: new Map() };
    window.__codexElvesRemoteQueuedEditStore = created;
    return created;
  })();
  const methods = [
    "thread/started", "thread/status/changed", "thread/name/updated", "thread/archived", "thread/unarchived", "thread/deleted",
    "thread/queue/changed",
    "turn/started", "turn/completed", "turn/diff/updated", "item/started", "item/completed", "item/agentMessage/delta",
    "item/reasoning/summaryTextDelta", "item/reasoning/summaryPartAdded", "item/reasoning/textDelta",
    "item/commandExecution/outputDelta", "item/fileChange/patchUpdated", "item/plan/delta", "turn/plan/updated",
    "thread/tokenUsage/updated", "thread/settings/updated", "thread/goal/updated", "thread/goal/cleared",
    "skills/changed", "serverRequest/resolved"
  ];
  const isManager = value => value?.hostId === "local" && typeof value.sendFollowUpMessage === "function"
    && typeof value.listThreadTurns === "function" && value.storage?.readGlobalState;
  const pushNotification = event => {
    if (["thread/archived", "thread/deleted"].includes(event.method)) {
      nativeCatalog.delete(event.params?.threadId);
      catalogLoaded = false;
    }
    if (transcriptEventTypes.has(event.params?.item?.type)) {
      const item = nativeTranscriptItem(event.params.item);
      if (!item) return;
      event = { ...event, params: { ...event.params, item } };
    } else if (["item/started", "item/completed"].includes(event.method)
        && ["userMessage", "agentMessage"].includes(event.params?.item?.type)) {
      const { threadId, turnId, item } = event.params;
      const enriched = enrichMessageMetadata(threadId, [{ id: turnId, status: "inProgress", items: [item] }])[0]?.items[0];
      if (enriched) event = { ...event, params: { ...event.params, item: enriched } };
    }
    if (Array.isArray(event.params?.turn?.items)) {
      event = {
        ...event,
        params: {
          ...event.params,
          turn: {
            ...event.params.turn,
            items: event.params.turn.items.map(item => transcriptEventTypes.has(item?.type)
              ? nativeTranscriptItem(item) : item).filter(Boolean)
          }
        }
      };
    }
    notifications.push({ sequence: ++eventSequence, event });
    if (notifications.length > 2048) notifications.splice(0, notifications.length - 2048);
  };
  const emit = event => {
    if (event.method === "thread/queue/changed") {
      void queueSnapshot(event.params?.threadId).then(snapshot => {
        pushNotification({
          ...event,
          params: {
            ...event.params,
            queuedMessages: snapshot.queuedMessages,
            queuedEdits: snapshot.queuedEdits
          }
        });
      }).catch(() => pushNotification(event));
      return;
    }
    pushNotification(event);
  };
  function discover() {
    const existing = window.__codexElvesConversationStateManager;
    if (isManager(existing) && !existing.disposed) manager = existing;
    const scopes = new Set(), fibers = new Set(), values = new WeakSet();
    const queue = [];
    for (const element of document.querySelectorAll("#root, #root *")) {
      const key = Object.keys(element).find(key => key.startsWith("__reactFiber$") || key.startsWith("__reactContainer$"));
      for (let fiber = element[key], depth = 0; fiber && depth < 160 && !fibers.has(fiber); fiber = fiber.return, depth++) {
        fibers.add(fiber);
        for (let hook = fiber.memoizedState, n = 0; hook && n < 128; hook = hook.next, n++) {
          const value = hook.memoizedState?.current;
          if (value?.node?.familyBindings instanceof Map && typeof value.get === "function") appScope = value;
        }
        queue.push([fiber.memoizedProps, 0], [fiber.memoizedState, 0]);
        const contexts = [fiber.memoizedProps?.value];
        for (let dep = fiber.dependencies?.firstContext, n = 0; dep && n < 32; dep = dep.next, n++) contexts.push(dep.memoizedValue);
        for (const context of contexts) {
          if (!(context instanceof Map)) continue;
          for (const initial of context.values()) {
            for (let scope = initial, n = 0; scope && n < 16 && !scopes.has(scope); scope = scope.parent, n++) {
              scopes.add(scope);
              if (!(scope.familyBindings instanceof Map)) continue;
              for (const bindings of scope.familyBindings.values()) {
                const signal = bindings.get?.("local")?.value;
                if (!signal?.atom || !signal.store?.get) continue;
                try {
                  const candidate = signal.store.get(signal.atom);
                  if (isManager(candidate) && !candidate.disposed) manager = candidate;
                } catch {}
              }
            }
          }
        }
      }
    }
    for (let n = 0; n < queue.length && n < 20_000; n++) {
      const [value, depth] = queue[n];
      if (!value || (typeof value !== "object" && typeof value !== "function") || values.has(value)) continue;
      values.add(value);
      if (value.node?.familyBindings instanceof Map && typeof value.get === "function") appScope = value;
      if (isManager(value) && !value.disposed) manager = value;
      if (depth > 7 || value instanceof Node || value === window || value === document) continue;
      if (value instanceof Map) {
        for (const entry of value.values()) queue.push([entry, depth + 1]);
      } else {
        for (const descriptor of Object.values(Object.getOwnPropertyDescriptors(value))) {
          if ("value" in descriptor) queue.push([descriptor.value, depth + 1]);
        }
      }
    }
    if (!manager) throw failure(503, "未发现本机原生会话管理器，请检查 CodexElves 注入连接");
    if (!unsubscribe.length) {
      unsubscribe.push(manager.addNotificationCallback(methods, emit));
      unsubscribe.push(manager.addApprovalRequestListener(() => emit({ method: "remote/requestsChanged" })));
      unsubscribe.push(manager.addApprovalResolvedListener(() => emit({ method: "remote/requestsChanged" })));
      unsubscribe.push(manager.addUserInputRequestListener(() => emit({ method: "remote/requestsChanged" })));
    }
    return manager;
  }
  function failure(code, message) { return Object.assign(new Error(message), { code, remoteValidationFailure: true }); }
  function requiredId(value) {
    if (typeof value !== "string" || !/^[a-zA-Z0-9_-]{1,120}$/.test(value)) throw failure(400, "会话 ID 无效");
    return value;
  }
  const state = async key => await manager.storage.readGlobalState(key);
  const array = value => Array.isArray(value) ? value : [];
  const HISTORY_TURN_PAGE_LIMIT = 3;
  const HISTORY_ITEM_PAGE_LIMIT = 40;
  const transcriptEventTypes = new Set(["modelChanged", "modelRerouted", "contextCompaction"]);
  const normalizePath = path => String(path || "").replaceAll("\\", "/").replace(/\/+$/, "").toLowerCase();
  async function nativeModelCatalog() {
    const data = [], cursors = new Set();
    let cursor = null;
    do {
      const page = await manager.requestClient.sendRequest("model/list", { limit: 100, cursor });
      data.push(...array(page.data));
      cursor = page.nextCursor ?? null;
      if (cursor && cursors.has(cursor)) throw failure(503, "原生模型列表分页无效");
      if (cursor) cursors.add(cursor);
    } while (cursor);
    return data.filter(model => !model.hidden);
  }
  async function validateReasoningEffort(model, effort) {
    if (!effort) return;
    if (typeof model !== "string" || !model.trim())
      throw failure(409, "请先打开会话，确认模型后再设置思考深度");
    const entry = (await nativeModelCatalog()).find(entry => entry.model === model);
    if (!entry) throw failure(409, "当前模型不在原生模型目录中，请重新打开会话");
    if (!array(entry.supportedReasoningEfforts).some(entry => entry.reasoningEffort === effort))
      throw failure(400, "思考深度超出此模型支持的范围");
  }
  async function defaultModelSettings() {
    try {
      const response = await manager.requestClient.sendRequest("config/read", { cwd: null, includeLayers: false });
      const config = response?.config;
      const model = typeof config?.model === "string" && config.model.trim() ? config.model : null;
      const effort = config?.model_reasoning_effort ?? config?.modelReasoningEffort ?? config?.reasoningEffort;
      return {
        defaultModel: model,
        defaultReasoningEffort: typeof effort === "string" && effort ? effort : null
      };
    } catch {
      return { defaultModel: null, defaultReasoningEffort: null };
    }
  }
  const timestamp = value => typeof value === "number" ? (value < 100_000_000_000 ? value * 1000 : value) : 0;
  let nativeCatalog = new Map();
  let catalogLoaded = false;
  let catalogLoading = null;
  function nativeUnreadState(thread, id) {
    const current = typeof manager.getThreadListSummary === "function" ? manager.getThreadListSummary(id) : null;
    return current?.hasUnreadTurn === true || current?.hasUnreadTurn !== false && thread.hasUnreadTurn === true;
  }
  function summary(thread) {
    const id = thread.conversationId || thread.id;
    return {
      id, title: thread.title || thread.name || thread.preview || "新会话", cwd: thread.cwd || "",
      createdAt: timestamp(thread.createdAt), updatedAt: timestamp(thread.updatedAt),
      recencyAt: timestamp(thread.recencyAt ?? thread.updatedAt), hasUnreadTurn: nativeUnreadState(thread, id),
      status: thread.threadRuntimeStatus || thread.status || { type: "idle" },
      threadSource: thread.threadSource, projectId: null, pinned: false
    };
  }
  function sortMode(value, preferences) {
    return value === "manual" && preferences.manualSortVersion === 1 ? "manual" : "updated_at";
  }
  async function sidebar(payload) {
    const keys = ["local-projects", "pinned-project-ids", "project-order", "pinned-thread-ids",
      "sidebar-project-thread-orders", "thread-project-assignments", "projectless-thread-ids"];
    const stored = Object.fromEntries(await Promise.all(keys.map(async key => [key, await state(key)])));
    const preferences = manager.storage.readValue("flat-project-sidebar-preferences-v1") || {};
    const sharedSort = manager.storage.readValue("codex-sidebar-sort-mode-v1");
    const pinnedSort = manager.storage.readValue("pinned-sidebar-sort-mode-v1") === "manual" ? "manual" : "updated_at";
    const chatSort = sharedSort == null ? sortMode(preferences.chatSortMode, preferences) : "updated_at";
    const projectSort = sharedSort == null ? sortMode(preferences.projectSortMode, preferences) : "updated_at";
    const observeThreads = threads => {
      for (const thread of array(threads)) {
        const id = thread.conversationId || thread.id;
        if (thread.threadSource === "subagent" || thread.source?.subAgent || thread.source?.subagent) {
          nativeCatalog.delete(id);
          continue;
        }
        nativeCatalog.set(id, summary(thread));
      }
    };
    if (!catalogLoaded) {
      if (!catalogLoading) catalogLoading = (async () => {
        let cursor = null;
        const cursors = new Set();
        do {
          const page = await manager.requestClient.sendRequest("thread/list", { cursor, limit: 100, sortKey: "updated_at", archived: false });
          observeThreads(page.data);
          cursor = page.nextCursor || null;
          if (cursor && cursors.has(cursor)) throw failure(503, "原生会话目录分页游标重复");
          if (cursor) cursors.add(cursor);
        } while (cursor);
        catalogLoaded = true;
      })().finally(() => { catalogLoading = null; });
      await catalogLoading;
    } else {
      const page = await manager.requestClient.sendRequest("thread/list", { cursor: null, limit: 100, sortKey: "updated_at", archived: false });
      observeThreads(page.data);
    }
    observeThreads(manager.getRecentThreadSummaries());
    const pins = await manager.runtime.pinnedThreads();
    if (!pins) throw failure(503, "当前 Codex 版本的原生置顶服务不可用");
    const pinnedState = await pins.list({ hostId: "local", useAppServerPins: true });
    const pinnedIds = array(pinnedState.threadIds);
    const projects = Object.entries(stored["local-projects"] || {}).map(([id, project]) => ({
      id, name: project.name || project.rootPaths?.[0]?.split(/[\\/]/).at(-1) || id,
      roots: array(project.rootPaths), pinned: array(stored["pinned-project-ids"]).includes(id),
      recencyAt: 0, threads: []
    }));
    const assignments = stored["thread-project-assignments"] || {};
    const projectless = new Set(array(stored["projectless-thread-ids"]));
    const threads = [...nativeCatalog.values()].map(thread => {
      const assigned = assignments[thread.id];
      let projectId = typeof assigned === "string" ? assigned : assigned?.projectKind === "local" ? assigned.projectId : null;
      if (assigned == null && !projectless.has(thread.id)) {
        const matches = projects.filter(project => project.roots.some(root => normalizePath(root) === normalizePath(thread.cwd)));
        if (matches.length === 1) projectId = matches[0].id;
      }
      return { ...thread, projectId, pinned: pinnedIds.includes(thread.id) };
    });
    const manualOrder = stored["sidebar-project-thread-orders"] || {};
    const sortThreads = (items, ids, mode) => items.sort((a, b) => mode === "manual"
      ? (ids.indexOf(a.id) < 0 ? ids.length : ids.indexOf(a.id)) - (ids.indexOf(b.id) < 0 ? ids.length : ids.indexOf(b.id)) || b.recencyAt - a.recencyAt
      : b.recencyAt - a.recencyAt);
    for (const project of projects) {
      const children = threads.filter(thread => thread.projectId === project.id);
      project.recencyAt = Math.max(0, ...children.map(thread => thread.recencyAt));
      project.threads = sortThreads(children.filter(thread => !thread.pinned), array(manualOrder[project.id]?.threadIds), projectSort);
    }
    const projectOrder = array(stored["project-order"]);
    const pinOrder = array(stored["pinned-project-ids"]);
    const pinPosition = id => pinOrder.indexOf(id) < 0 ? pinOrder.length : pinOrder.indexOf(id);
    projects.sort((a, b) => a.pinned !== b.pinned ? (a.pinned ? -1 : 1)
      : a.pinned ? pinPosition(a.id) - pinPosition(b.id)
      : (projectOrder.indexOf(a.id) < 0 ? projectOrder.length : projectOrder.indexOf(a.id))
        - (projectOrder.indexOf(b.id) < 0 ? projectOrder.length : projectOrder.indexOf(b.id)));
    const pinnedThreads = sortThreads(threads.filter(thread => thread.pinned), pinnedIds, pinnedSort);
    const recent = sortThreads(threads.filter(thread => !thread.pinned), array(manager.storage.readValue("codex-sidebar-chat-order-v1")?.threadIds), chatSort);
    return { ok: true, projects, pinnedThreads, recent, nextCursor: null,
      creationAvailable: !!appScope,
      preferences: { mode: preferences.mode === "list" ? "list" : "project", chats: chatSort, projects: projectSort, pinned: pinnedSort } };
  }
  async function readThread(id) {
    const response = await manager.readThread(requiredId(id), { includeTurns: false });
    const thread = response.thread || response;
    if (!thread?.id) throw failure(404, "会话不存在或已删除");
    return thread;
  }
  function optionalCursor(value, label) {
    if (value == null || value === "") return null;
    if (typeof value !== "string" || value.length > 4096) throw failure(400, `${label}无效`);
    return value;
  }
  async function nativeItemsPage(threadId, turnId, cursor) {
    let limit = HISTORY_ITEM_PAGE_LIMIT, page;
    for (;;) {
      try {
        page = await manager.requestClient.sendRequest("thread/items/list", {
          threadId, turnId, cursor, limit, sortDirection: "desc"
        });
        break;
      } catch (error) {
        if (limit <= 1 || !(error instanceof Error)
            || !error.message.includes("decoded message length too large")) throw error;
        limit = Math.floor(limit / 2);
      }
    }
    if (!Array.isArray(page?.data)) throw failure(503, "原生会话消息分页返回无效");
    const nextCursor = optionalCursor(page.nextCursor, "原生消息分页游标");
    if (nextCursor != null && nextCursor === cursor)
      throw failure(503, "原生会话消息分页游标未前进");
    const itemStartedAtMsById = {};
    const items = page.data.map(entry => {
      const item = entry && typeof entry === "object" && "item" in entry ? entry.item : entry;
      if (entry && typeof entry === "object" && "item" in entry && entry.turnId !== turnId)
        throw failure(503, "原生会话消息归属错误");
      if (!item || typeof item !== "object") throw failure(503, "原生会话消息内容无效");
      if (typeof item.id === "string") {
        if (Number.isFinite(entry?.startedAtMs)) itemStartedAtMsById[item.id] = entry.startedAtMs;
      }
      return item;
    }).reverse();
    return {
      items, nextCursor, itemStartedAtMsById,
      pageLimit: limit, newestSnapshotItemId: items.at(-1)?.id ?? null
    };
  }
  function enrichHistoryTurn(threadId, turn, itemPage) {
    const enriched = enrichMessageMetadata(threadId, [{
      ...turn,
      items: itemPage.items,
      itemsView: itemPage.nextCursor == null ? "full" : "summary"
    }], { itemStartedAtMsById: itemPage.itemStartedAtMsById })[0];
    return {
      ...enriched,
      itemsPageComplete: true,
      itemsComplete: itemPage.nextCursor == null,
      itemsNextCursor: itemPage.nextCursor,
      newestSnapshotItemId: itemPage.newestSnapshotItemId
    };
  }
  function nativeTranscriptItem(item) {
    if (!transcriptEventTypes.has(item?.type) || typeof item.id !== "string" || !item.id) return null;
    if (item.type === "contextCompaction") {
      return {
        id: item.id,
        type: item.type,
        ...(typeof item.completed === "boolean" ? { completed: item.completed } : {}),
        ...(Number.isFinite(item.startedAtMs) ? { startedAtMs: item.startedAtMs } : {}),
        ...(["manual", "automatic"].includes(item.source) ? { source: item.source } : {})
      };
    }
    const fromModel = typeof item.fromModel === "string" ? item.fromModel : null;
    const toModel = typeof item.toModel === "string" ? item.toModel : null;
    if (!fromModel && !toModel) return null;
    return {
      id: item.id,
      type: item.type,
      ...(fromModel ? { fromModel } : {}),
      ...(toModel ? { toModel } : {}),
      ...(item.type === "modelRerouted" && typeof item.reason === "string"
        ? { reason: item.reason.slice(0, 1000) } : {})
    };
  }
  function nativeTranscriptEvents(conversation, turnIds = null) {
    const allowedTurns = turnIds ? new Set(turnIds) : null;
    const events = [];
    for (const turn of nativeConversationTurns(conversation)) {
      const turnId = nativeTurnId(turn);
      if (!turnId || allowedTurns && !allowedTurns.has(turnId)) continue;
      const items = array(turn?.items);
      for (let index = 0; index < items.length; index++) {
        const item = nativeTranscriptItem(items[index]);
        if (!item) continue;
        const beforeItemId = items.slice(index + 1)
          .map(candidate => typeof candidate?.id === "string" ? candidate.id : null).find(Boolean) || null;
        events.push({ turnId, item, ...(beforeItemId ? { beforeItemId } : {}) });
      }
    }
    return events;
  }
  async function historyPage(payload = {}) {
    const threadId = requiredId(payload.threadId);
    const turnId = payload.turnId == null ? null : requiredId(payload.turnId);
    if (turnId != null) {
      const itemCursor = optionalCursor(payload.itemCursor, "消息分页游标");
      const itemPage = await nativeItemsPage(threadId, turnId, itemCursor);
      const turn = enrichHistoryTurn(threadId, {
        id: turnId,
        status: typeof payload.turnStatus === "string" ? payload.turnStatus : "completed",
        startedAt: Number.isFinite(payload.turnStartedAt) ? payload.turnStartedAt : null,
        completedAt: Number.isFinite(payload.turnCompletedAt) ? payload.turnCompletedAt : null
      }, itemPage);
      return {
        ok: true,
        scope: "items",
        threadId,
        turnId,
        data: turn.items,
        itemOrder: "asc",
        itemDirection: "older",
        itemsPageComplete: true,
        itemsComplete: itemPage.nextCursor == null,
        itemsNextCursor: itemPage.nextCursor,
        newestSnapshotItemId: itemPage.newestSnapshotItemId,
        pageLimit: itemPage.pageLimit,
        transcriptEvents: nativeTranscriptEvents(manager.getConversation(threadId), [turnId])
      };
    }
    const cursor = optionalCursor(payload.cursor, "会话分页游标");
    const page = await manager.requestClient.sendRequest("thread/turns/list", {
      threadId,
      cursor,
      limit: HISTORY_TURN_PAGE_LIMIT,
      itemsView: "notLoaded",
      sortDirection: "desc"
    });
    if (!Array.isArray(page?.data)) throw failure(503, "原生会话历史分页返回无效");
    const nextCursor = optionalCursor(page.nextCursor, "原生会话分页游标");
    if (nextCursor != null && nextCursor === cursor)
      throw failure(503, "原生会话历史分页游标未前进");
    const data = [], itemsPaginationByTurnId = {};
    for (const turn of page.data) {
      const pageTurnId = requiredId(turn?.id);
      const itemPage = await nativeItemsPage(threadId, pageTurnId, null);
      data.push(enrichHistoryTurn(threadId, turn, itemPage));
      itemsPaginationByTurnId[pageTurnId] = {
        nextCursor: itemPage.nextCursor,
        hasMore: itemPage.nextCursor != null,
        complete: itemPage.nextCursor == null,
        pageComplete: true,
        direction: "older",
        order: "asc",
        newestSnapshotItemId: itemPage.newestSnapshotItemId,
        pageLimit: itemPage.pageLimit
      };
    }
    return {
      ok: true,
      scope: "turns",
      threadId,
      data,
      nextCursor,
      backwardsCursor: page.backwardsCursor ?? null,
      turnOrder: "desc",
      itemOrder: "asc",
      turnsPageComplete: true,
      turnsComplete: nextCursor == null,
      itemsPaginationByTurnId,
      transcriptEvents: nativeTranscriptEvents(manager.getConversation(threadId), data.map(turn => turn.id))
    };
  }
  async function markThreadRead(id) {
    id = requiredId(id);
    await readThread(id);
    if (typeof manager.markConversationAsRead !== "function") throw failure(503, "当前 Codex 版本的原生已读服务不可用");
    await manager.markConversationAsRead(id);
    catalogLoaded = false;
    emit({ method: "remote/sidebarChanged" });
    return { ok: true, threadId: id };
  }
  function runtimeGoal(goal) {
    if (!goal || typeof goal !== "object" || Array.isArray(goal)) return null;
    const objective = typeof goal.objective === "string" ? goal.objective.trim() : "";
    const status = typeof goal.status === "string" ? goal.status : "";
    if (!objective || !status) return null;
    const count = value => Number.isFinite(value) && value >= 0 ? Math.floor(value) : null;
    return {
      objective, status,
      tokenBudget: count(goal.tokenBudget),
      tokensUsed: count(goal.tokensUsed),
      timeUsedSeconds: count(goal.timeUsedSeconds),
      createdAt: count(goal.createdAt),
      updatedAt: count(goal.updatedAt)
    };
  }
  async function updateThreadGoal(payload = {}) {
    const id = requiredId(payload.threadId);
    const thread = await readThread(id);
    if (!manager.getConversation(id) || manager.getConversation(id).resumeState === "needs_resume") {
      await manager.resumeConversation({
        conversationId: id, model: null, reasoningEffort: null,
        workspaceRoots: [thread.cwd], collaborationMode: null
      });
    }
    const objective = typeof payload.objective === "string" ? payload.objective.trim() : "";
    const objectiveBytes = new TextEncoder().encode(objective).byteLength;
    if (!objective || objectiveBytes > 16 * 1024) throw failure(400, "目标内容不能为空或超过 16 KiB");
    if (!Number.isSafeInteger(payload.expectedCreatedAt) || payload.expectedCreatedAt < 0
        || typeof payload.expectedObjective !== "string"
        || !["active", "paused", "blocked", "budgetLimited", "usageLimited"].includes(payload.expectedStatus))
      throw failure(400, "目标编辑基线无效");
    const { goal: nativeGoal } = await manager.requestClient.sendRequest("thread/goal/get", { threadId: id });
    const current = runtimeGoal(nativeGoal);
    if (!current || current.status === "complete") throw failure(409, "目标已结束或不存在，请重新读取会话");
    if (current.createdAt !== payload.expectedCreatedAt
        || current.objective !== payload.expectedObjective.trim()
        || current.status !== payload.expectedStatus)
      throw failure(409, "目标已发生变化，请重新读取后再编辑");
    const { goal } = await manager.requestClient.sendRequest("thread/goal/set", {
      threadId: id, objective, status: current.status
    });
    const updated = runtimeGoal(goal);
    if (!updated || updated.createdAt !== current.createdAt
        || updated.objective !== objective || updated.status !== current.status)
      throw new Error("原生目标更新结果无效");
    return { ok: true, threadId: id, goal: updated };
  }
  function runtimeCompaction(id, conversation = manager.getConversation(id)) {
    const turns = conversation?.turnHistory?.kind === "canonical"
      ? manager.threadStore?.getLoadedConversationHistoryTurns?.(conversation.turnHistory.history)
      : conversation?.turns;
    let latest = null;
    for (const turn of array(turns)) {
      const turnId = turn?.turnId ?? turn?.id ?? null;
      const turnStatus = typeof turn?.status === "string" ? turn.status : turn?.status?.type;
      const turnEnded = ["completed", "failed", "interrupted", "cancelled", "canceled"].includes(turnStatus);
      for (const item of array(turn?.items)) {
        if (item?.type !== "contextCompaction") continue;
        const itemId = typeof item.id === "string" ? item.id : null;
        const completed = item.completed === true
          || ["completed", "failed", "interrupted", "cancelled", "canceled"].includes(item.status);
        latest = {
          state: completed || turnEnded ? "idle"
            : itemId === "pending-manual-context-compaction" ? "pending" : "running",
          turnId, itemId
        };
      }
    }
    if (latest?.state === "running") return latest;
    const pending = manager.manualContextCompactions?.pendingCounts?.get?.(id);
    if ((Number.isFinite(pending) && pending > 0) || latest?.state === "pending") {
      return {
        state: "pending",
        turnId: latest?.state === "pending" ? latest.turnId : null,
        itemId: latest?.state === "pending" ? latest.itemId : "pending-manual-context-compaction"
      };
    }
    return { state: "idle", turnId: latest?.turnId ?? null, itemId: latest?.itemId ?? null };
  }
  function nativeConversationTurns(conversation) {
    const turns = conversation?.turnHistory?.kind === "canonical"
      ? manager.threadStore?.getLoadedConversationHistoryTurns?.(conversation.turnHistory.history)
      : conversation?.turns;
    return array(turns);
  }
  function messageActionManager() {
    if (manager?.disposed) {
      for (const dispose of unsubscribe) { try { dispose?.(); } catch {} }
      unsubscribe = [];
      manager = null;
      appScope = null;
    }
    if (!manager) discover();
    return manager;
  }
  function nativeTurnId(turn) {
    return typeof turn?.turnId === "string" ? turn.turnId : typeof turn?.id === "string" ? turn.id : null;
  }
  function nativeTurnStatus(turn) {
    return typeof turn?.status === "string" ? turn.status : turn?.status?.type;
  }
  function initialUserMessage(turn) {
    const messages = array(turn?.items).filter(item => item?.type === "userMessage");
    const clientId = turn?.params?.clientUserMessageId;
    return messages.find(item => clientId && item.clientId === clientId) || messages[0] || null;
  }
  function pendingNativeMessage(id, conversation, turns) {
    if (array(conversation?.unconfirmedTurnSubmissions).length) return true;
    const queued = manager.getTurnCoordinator?.()?.readMessages?.(id)
      ?? manager.storage.readQueuedFollowUps?.()?.value?.[id];
    if (array(queued?.messages || queued).length) return true;
    return turns.some(turn => array(turn?.items).some(item =>
      item?.type === "steeringUserMessage" && item.status === "pending"));
  }
  function editableMessageState(id) {
    const conversation = manager.getConversation(id);
    if (!conversation) return { reason: "会话尚未加载", metadata: null };
    if (typeof manager.editLastUserTurn !== "function")
      return { reason: "当前 Codex 版本不支持编辑消息", metadata: null };
    const turns = nativeConversationTurns(conversation);
    const status = manager.getThreadRuntimeStatusEvidence(id)
      || conversation.threadRuntimeStatus || conversation.status;
    const statusType = typeof status === "string" ? status : status?.type;
    if (["active", "inProgress", "running", "pending"].includes(statusType)
        || turns.some(turn => nativeTurnStatus(turn) === "inProgress"))
      return { reason: "会话正在执行，不能编辑消息", metadata: null };
    if (runtimeCompaction(id, conversation).state !== "idle")
      return { reason: "会话正在压缩上下文，不能编辑消息", metadata: null };
    if (pendingNativeMessage(id, conversation, turns))
      return { reason: "会话仍有待确认或排队消息，不能编辑消息", metadata: null };
    let target = null, targetIndex = -1;
    for (let index = turns.length - 1; index >= 0; index--) {
      const candidate = turns[index];
      if (!array(candidate?.params?.input).length) continue;
      target = candidate;
      targetIndex = index;
      break;
    }
    if (targetIndex >= 0 && turns.slice(targetIndex).some(turn =>
      array(turn?.items).some(item => item?.type === "steeringUserMessage")))
      return { reason: "最后一条用户消息是补充消息，不能作为初始请求编辑", metadata: null };
    const turnId = nativeTurnId(target), message = initialUserMessage(target);
    if (!turnId || typeof message?.id !== "string")
      return { reason: "最后一条用户消息缺少原生编辑标识", metadata: null };
    return {
      reason: null,
      metadata: { turnId, messageId: message.id, editable: true }
    };
  }
  function nativeAssistantSentAtMs(turn, item, assistantMessageStartedAtMsById, itemStartedAtMsById) {
    if (item.delivery === "async") return null;
    const agents = array(turn?.items).filter(candidate => candidate?.type === "agentMessage");
    const final = agents.at(-1) === item ? turn?.finalAssistantStartedAtMs : null;
    return timestamp(turn?.aeonAssistantMessageStartedAtMsById?.[item.id])
      || timestamp(itemStartedAtMsById?.[item.id])
      || timestamp(assistantMessageStartedAtMsById?.[item.id])
      || timestamp(final)
      || null;
  }
  function nativeUserSentAtMs(turn, sourceTurn, item, source, initial, fallbackInitial, itemStartedAtMsById) {
    const projected = timestamp(item?.sentAtMs);
    const itemStarted = timestamp(itemStartedAtMsById?.[item.id]);
    if (source?.type === "steeringUserMessage")
      return timestamp(source.restoreMessage?.createdAt) || timestamp(source.sentAtMs)
        || projected || itemStarted || null;
    if (source?.type === "userMessage")
      return timestamp(source.sentAtMs)
        || (source === initial ? timestamp(sourceTurn?.turnStartedAtMs) : 0)
        || projected || itemStarted || null;
    return projected || itemStarted
      || (item === fallbackInitial
        ? timestamp(sourceTurn?.turnStartedAtMs) || timestamp(turn?.startedAt)
        : 0)
      || null;
  }
  function enrichMessageMetadata(id, historyTurns, {
    assistantMessageStartedAtMsById = null,
    itemStartedAtMsById = null
  } = {}) {
    messageActionManager();
    const conversation = manager.getConversation(id);
    const nativeTurns = nativeConversationTurns(conversation);
    const nativeById = new Map(nativeTurns.flatMap(turn => {
      const turnId = nativeTurnId(turn);
      return turnId ? [[turnId, turn]] : [];
    }));
    const editable = editableMessageState(id).metadata;
    return array(historyTurns).map(turn => {
      const turnId = typeof turn?.id === "string" ? turn.id : null;
      const sourceTurn = nativeById.get(turnId) || null;
      const sourceItems = new Map();
      for (const item of array(sourceTurn?.items)) {
        if (typeof item?.id === "string") sourceItems.set(item.id, item);
        if (item?.type === "steeringUserMessage" && typeof item.serverUserMessageId === "string")
          sourceItems.set(item.serverUserMessageId, item);
      }
      const initial = initialUserMessage(sourceTurn);
      const fallbackInitial = array(turn?.items).find(item => item?.type === "userMessage");
      const assistantItems = array(turn?.items).filter(item => item?.type === "agentMessage");
      return {
        ...turn,
        items: array(turn?.items).map(item => {
          if (!["userMessage", "agentMessage"].includes(item?.type)) return item;
          const source = sourceItems.get(item.id);
          let sentAtMs = null;
          if (item.type === "userMessage") {
            sentAtMs = nativeUserSentAtMs(
              turn, sourceTurn, item, source, initial, fallbackInitial, itemStartedAtMsById
            );
          } else {
            sentAtMs = source?.type === "agentMessage"
              ? nativeAssistantSentAtMs(sourceTurn, source, assistantMessageStartedAtMsById, itemStartedAtMsById)
              : item.delivery !== "async"
                ? timestamp(itemStartedAtMsById?.[item.id])
                  || (item === assistantItems.at(-1) ? timestamp(turn?.completedAt) : 0) || null
                : null;
          }
          const messageMetadata = {
            messageId: typeof item.id === "string" ? item.id : null,
            turnId,
            sentAtMs,
            editable: editable?.turnId === turnId && editable.messageId === item.id
          };
          const completed = item.type === "agentMessage"
            ? nativeTurnStatus(sourceTurn || turn) !== "inProgress" || item !== assistantItems.at(-1)
            : item.completed;
          return {
            ...item,
            ...(sentAtMs == null ? {} : { sentAtMs }),
            ...(item.type === "agentMessage" ? { completed } : {}),
            messageMetadata
          };
        })
      };
    });
  }
  async function editLastMessage(payload = {}) {
    messageActionManager();
    const id = requiredId(payload.threadId);
    await readThread(id);
    const state = editableMessageState(id);
    if (!state.metadata) throw failure(409, state.reason || "最后一条用户消息当前不可编辑");
    if (payload.turnId !== state.metadata.turnId || payload.messageId !== state.metadata.messageId)
      throw failure(409, "消息已变化，请刷新会话后重试");
    if (typeof payload.prompt !== "string" || !payload.prompt.trim())
      throw failure(400, "消息不能为空");
    const conversation = manager.getConversation(id);
    const options = {
      turnId: state.metadata.turnId,
      message: payload.prompt,
      agentMode: "default",
      shouldSendPermissionOverrides: false,
      serviceTier: conversation?.latestServiceTier ?? null
    };
    await manager.editLastUserTurn(id, options);
    emit({ method: "remote/threadReverted", params: { threadId: id } });
    return { ok: true, threadId: id, ...state.metadata };
  }
  function runtimeThread(id, nativeThread = null) {
    const conversation = manager.getConversation(id);
    const queued = manager.getTurnCoordinator?.()?.readMessages?.(id)
      ?? manager.storage.readQueuedFollowUps?.()?.value?.[id];
    const collaborationSettings = conversation?.latestCollaborationMode?.settings
      ?? conversation?.latestThreadSettings?.collaborationMode?.settings;
    return {
      id,
      model: nativeThread?.model ?? collaborationSettings?.model
        ?? conversation?.latestThreadSettings?.model ?? conversation?.latestModel ?? null,
      reasoningEffort: nativeThread?.reasoningEffort ?? nativeThread?.reasoning_effort
        ?? collaborationSettings?.reasoning_effort ?? conversation?.latestThreadSettings?.effort
        ?? conversation?.latestReasoningEffort ?? null,
      serviceTier: conversation?.latestServiceTier ?? null,
      tokenUsage: conversation?.latestTokenUsageInfo ?? null,
      goal: runtimeGoal(conversation?.threadGoal ?? conversation?.completedThreadGoal),
      compaction: runtimeCompaction(id, conversation),
      transcriptEvents: nativeTranscriptEvents(conversation),
      editableMessage: editableMessageState(id).metadata,
      status: manager.getThreadRuntimeStatusEvidence(id) || { type: "idle" },
      requests: array(conversation?.requests).map(request => ({ id: request.id, method: request.method, params: request.params })),
      queuedMessages: array(queued?.messages || queued),
      queuedEdits: queuedEditSummaries(id)
    };
  }
  function queuedEditSummaries(threadId) {
    return [...queuedEditStore.sessions.values()].flatMap(session =>
      session.threadId === threadId && !["committed", "canceled"].includes(session.state)
        ? [{
            editId: session.editId,
            messageId: session.messageId,
            originalText: session.originalText,
            state: session.state
          }]
        : []).slice(0, 1);
  }
  async function queueSnapshot(id) {
    id = requiredId(id);
    if (typeof manager.getTurnCoordinator !== "function")
      throw failure(503, "当前 Codex 版本的原生队列服务不可用");
    const coordinator = await manager.getTurnCoordinator();
    if (typeof coordinator?.serverQueue?.load !== "function"
        || typeof coordinator?.readMessages !== "function")
      throw failure(503, "当前 Codex 版本不支持原生队列读取");
    await coordinator.serverQueue.load(id);
    const queued = coordinator.readMessages(id);
    return {
      ok: true,
      threadId: id,
      queuedMessages: array(queued?.messages || queued),
      queuedEdits: queuedEditSummaries(id)
    };
  }
  async function removeQueued(payload = {}) {
    try {
      if (manager?.disposed) {
        for (const dispose of unsubscribe) { try { dispose?.(); } catch {} }
        unsubscribe = [];
        manager = null;
        appScope = null;
      }
      if (!manager) discover();
      const threadId = requiredId(payload.threadId);
      const messageId = requiredId(payload.messageId);
      await readThread(threadId);
      if (typeof manager.getTurnCoordinator !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      const coordinator = await manager.getTurnCoordinator();
      if (typeof coordinator?.removeQueuedMessage !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      const message = array(coordinator.readMessages?.(threadId)).find(message => message?.id === messageId);
      if (!message) throw failure(409, "此队列消息已不存在，请刷新会话");
      if (message.submission?.status && message.submission.status !== "queued")
        throw failure(409, "此消息已提交或送达尚未确认，不能作为待执行消息取消");
      const removed = await coordinator.removeQueuedMessage(threadId, messageId);
      if (removed == null) throw failure(409, "此队列消息已不存在，请刷新会话");
      const queued = coordinator.readMessages?.(threadId)
        ?? manager.storage.readQueuedFollowUps?.()?.value?.[threadId];
      return { ok: true, threadId, messageId, queuedMessages: array(queued?.messages || queued) };
    } catch (error) {
      return { ok: false, code: error.code || 503, message: error.message || "移除原生队列消息失败" };
    }
  }
  function queuedOrderIds(value) {
    if (!Array.isArray(value) || !value.length) throw failure(400, "队列排序参数无效");
    const ids = value.map(requiredId);
    if (new Set(ids).size !== ids.length) throw failure(400, "队列排序消息 ID 不能重复");
    return ids;
  }
  async function reorderQueued(payload = {}) {
    let threadId = null;
    let ownsReorder = false;
    let reorderStarted = false;
    try {
      threadId = requiredId(payload.threadId);
      const messageIds = queuedOrderIds(payload.messageIds);
      const expectedMessageIds = queuedOrderIds(payload.expectedMessageIds);
      if (messageIds.length !== expectedMessageIds.length
          || messageIds.some(messageId => !expectedMessageIds.includes(messageId)))
        throw failure(400, "队列排序必须包含相同的消息");
      if (queuedReorders.has(threadId)) throw failure(409, "队列正在排序，请刷新会话后重试");
      queuedReorders.add(threadId);
      ownsReorder = true;
      await readThread(threadId);
      if (typeof manager.getTurnCoordinator !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      const coordinator = await manager.getTurnCoordinator();
      if (typeof coordinator?.serverQueue?.load !== "function"
          || typeof coordinator?.readMessages !== "function"
          || typeof coordinator?.reorderQueuedMessages !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列排序");
      await coordinator.serverQueue.load(threadId);
      const currentMessageIds = array(coordinator.readMessages(threadId)).map(message => message?.id);
      if (currentMessageIds.length !== expectedMessageIds.length
          || currentMessageIds.some((messageId, index) => messageId !== expectedMessageIds[index]))
        throw failure(409, "队列已发生变化，请刷新会话后重试");
      if (!messageIds.some((messageId, index) => messageId !== expectedMessageIds[index])) {
        return {
          ok: true,
          threadId,
          queuedMessages: array(coordinator.readMessages(threadId)),
          queuedEdits: queuedEditSummaries(threadId)
        };
      }
      reorderStarted = true;
      await coordinator.reorderQueuedMessages(threadId, messageIds);
      const queuedMessages = array(coordinator.readMessages(threadId));
      if (queuedMessages.length !== messageIds.length
          || queuedMessages.some((message, index) => message?.id !== messageIds[index]))
        throw new Error("原生队列排序结果未确认");
      return {
        ok: true,
        threadId,
        queuedMessages,
        queuedEdits: queuedEditSummaries(threadId)
      };
    } catch (error) {
      return {
        ok: false,
        ...(reorderStarted && !error.remoteValidationFailure ? { outcomeUnknown: true } : {}),
        code: error.code || 503,
        ...(threadId ? { threadId } : {}),
        message: reorderStarted && !error.remoteValidationFailure
          ? "原生队列排序结果未确认，请刷新会话检查，不要重复排序"
          : error.message || "原生队列排序失败"
      };
    } finally {
      if (ownsReorder) queuedReorders.delete(threadId);
    }
  }
  function queuedMessageText(message) {
    if (typeof message?.text === "string") return message.text;
    if (typeof message?.context?.prompt === "string") return message.context.prompt;
    if (typeof message?.prompt === "string") return message.prompt;
    if (typeof message?.message?.prompt === "string") return message.message.prompt;
    return "";
  }
  function queuedMessageAccepted(conversation, message) {
    const clientId = message?.clientUserMessageId;
    if (typeof clientId !== "string" || !clientId) return false;
    return nativeConversationTurns(conversation).some(turn => nativeTurnId(turn)
      && array(turn?.items).some(item => item?.clientUserMessageId === clientId || item?.clientId === clientId));
  }
  function queuedMessageUnconfirmed(conversation, message) {
    return array(conversation?.unconfirmedTurnSubmissions).some(submission => submission?.terminal === true
      && (submission.serverQueuedMessageId === message?.id
        || submission.clientUserMessageId === message?.clientUserMessageId));
  }
  function queuedMessageWithText(message, prompt) {
    const context = {
      ...message.context,
      prompt,
      ...(message.context?.untrustedAppMessage && typeof message.context.untrustedAppMessage === "object"
        ? { untrustedAppMessage: { ...message.context.untrustedAppMessage, text: prompt } }
        : {})
    };
    return { ...message, text: prompt, context };
  }
  const untrustedAppInputPrefix = "codex-untrusted-app-input:";
  function utf8ByteLength(value) {
    return new TextEncoder().encode(value).length;
  }
  function queuedTextMatches(value, expected) {
    return value === expected || value.trim() === expected.trim();
  }
  function xmlDecode(value) {
    return value.replaceAll("&lt;", "<").replaceAll("&gt;", ">").replaceAll("&quot;", "\"")
      .replaceAll("&apos;", "'").replaceAll("&amp;", "&");
  }
  function xmlEncode(value) {
    return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
      .replaceAll("\"", "&quot;").replaceAll("'", "&apos;");
  }
  function replaceQueuedPromptText(text, originalText, prompt, allowSenderWrapper = true) {
    if (allowSenderWrapper) {
      const from = /^\[from: [^\]\r\n]+\](?:\r?\n|$)/.exec(text);
      if (from) {
        const body = replaceQueuedPromptText(text.slice(from[0].length), originalText, prompt, false);
        if (body != null) return `${from[0]}${body}`;
      }
      const opening = "<codex_multi_user_message>";
      const closing = "</codex_multi_user_message>";
      const openingIndex = text.lastIndexOf(opening);
      const closingIndex = openingIndex < 0 ? -1 : text.indexOf(closing, openingIndex + opening.length);
      if (openingIndex >= 0 && closingIndex >= 0) {
        const block = text.slice(openingIndex, closingIndex + closing.length);
        const bodyMatch = /<body>([\s\S]*?)<\/body>/i.exec(block);
        if (bodyMatch) {
          const body = xmlDecode(bodyMatch[1]);
          const replaced = replaceQueuedPromptText(body, originalText, prompt, false);
          if (replaced != null) {
            const bodyStart = openingIndex + bodyMatch.index + bodyMatch[0].indexOf(bodyMatch[1]);
            return `${text.slice(0, bodyStart)}${xmlEncode(replaced)}${text.slice(bodyStart + bodyMatch[1].length)}`;
          }
        }
      }
    }
    const matches = [...text.matchAll(/## My request(?: for Codex)?:/g)];
    const contentStart = matches.length ? matches.at(-1).index + matches.at(-1)[0].length : 0;
    const content = text.slice(contentStart);
    if (!queuedTextMatches(content, originalText)) return null;
    const leading = content.match(/^\s*/)?.[0] || "";
    const trailing = content.match(/\s*$/)?.[0] || "";
    return `${text.slice(0, contentStart)}${leading}${prompt}${trailing}`;
  }
  function rewriteQueuedTextItem(item, originalText, prompt) {
    if (item?.type !== "text" || typeof item.text !== "string" || !Array.isArray(item.text_elements)) return null;
    if (item.text_elements.length === 1) {
      const element = item.text_elements[0];
      const placeholder = element?.placeholder;
      if (element?.byteRange?.start === 0
          && element.byteRange.end === utf8ByteLength(item.text)
          && typeof placeholder === "string"
          && placeholder.startsWith(untrustedAppInputPrefix)) {
        let appInput;
        try { appInput = JSON.parse(placeholder.slice(untrustedAppInputPrefix.length)); } catch { return null; }
        if (typeof appInput?.message?.text === "string") {
          if (!queuedTextMatches(appInput.message.text, originalText)) return null;
          return {
            ...item,
            text_elements: [{
              ...element,
              placeholder: untrustedAppInputPrefix + JSON.stringify({
                ...appInput,
                message: { ...appInput.message, text: prompt }
              })
            }]
          };
        }
        const text = replaceQueuedPromptText(item.text, originalText, prompt);
        if (text == null) return null;
        return {
          ...item,
          text,
          text_elements: [{
            ...element,
            byteRange: { ...element.byteRange, end: utf8ByteLength(text) }
          }]
        };
      }
    }
    if (item.text_elements.length !== 0) return null;
    const text = replaceQueuedPromptText(item.text, originalText, prompt);
    return text == null ? null : { ...item, text };
  }
  function rewriteQueuedSubmissionInput(input, originalText, prompt) {
    if (!Array.isArray(input)) throw failure(409, "此队列消息缺少可安全编辑的原生输入");
    if (prompt === originalText) return input;
    const rewritten = input.map(item => rewriteQueuedTextItem(item, originalText, prompt));
    const candidates = rewritten.flatMap((item, index) => item == null ? [] : [index]);
    if (candidates.length === 0 && originalText === "" && prompt
        && !input.some(item => item?.type === "text"))
      return [{ type: "text", text: prompt, text_elements: [] }, ...input];
    if (candidates.length !== 1)
      throw failure(409, "无法唯一定位此队列消息的原生文本，请在 Codex 桌面编辑");
    return input.map((item, index) => index === candidates[0] ? rewritten[index] : item);
  }
  function validateQueuedImageSources(value, field) {
    if (!Array.isArray(value) || value.length > 4
        || value.some(source => typeof source !== "string" || !source || source.length > 32767
          || /[\u0000-\u001f\u007f]/u.test(source))
        || new Set(value).size !== value.length)
      throw failure(400, `${field}无效`);
    return [...value];
  }
  function hexBytes(bytes) {
    return [...bytes].map(byte => byte.toString(16).padStart(2, "0")).join("");
  }
  const queuedInlineImageMaxBytes = 25 * 1024 * 1024;
  const queuedInlineImageMaxSourceLength = Math.ceil(queuedInlineImageMaxBytes / 3) * 4 + 64;
  async function queuedImageSourceReference(source) {
    if (typeof source !== "string" || !source)
      throw failure(409, "队列图片 source 无效，请刷新后重试");
    if (!source.toLowerCase().startsWith("data:image/")) {
      if (source.length > 32767 || /[\u0000-\u001f\u007f]/u.test(source))
        throw failure(409, "队列图片 source 无效，请刷新后重试");
      return source;
    }
    if (source.length > queuedInlineImageMaxSourceLength)
      throw failure(409, "队列图片内容过大，请刷新后重试");
    const match = /^data:image\/(?:png|jpeg|webp|gif);base64,([A-Za-z0-9+/]+={0,2})$/iu.exec(source);
    if (!match) throw failure(409, "队列图片内容无效，请刷新后重试");
    let binary;
    try { binary = atob(match[1]); } catch { throw failure(409, "队列图片内容无效，请刷新后重试"); }
    if (!binary.length || binary.length > queuedInlineImageMaxBytes)
      throw failure(409, "队列图片内容过大，请刷新后重试");
    const bytes = Uint8Array.from(binary, character => character.charCodeAt(0));
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    return `inline:${hexBytes(new Uint8Array(digest))}`;
  }
  async function queuedImageAttachmentRecords(message) {
    const attachments = message?.context?.imageAttachments;
    if (attachments == null) return [];
    if (!Array.isArray(attachments)) throw failure(409, "队列图片上下文格式已变化，请刷新后重试");
    const records = [];
    const references = new Set();
    for (const [index, attachment] of attachments.entries()) {
      if (!attachment || typeof attachment !== "object")
        throw failure(409, "队列图片上下文格式已变化，请刷新后重试");
      const reference = await queuedImageSourceReference(attachment.src);
      if (references.has(reference))
        throw failure(409, "队列消息包含重复图片 source，无法安全编辑");
      references.add(reference);
      records.push({ index, reference, attachment });
    }
    return records;
  }
  function queuedInputMatchesAttachment(item, attachment) {
    if (!item || typeof item !== "object") return false;
    if (typeof attachment.localPath === "string" && attachment.localPath)
      return item.type === "localImage" && item.path === attachment.localPath;
    return item.type === "image" && item.url === attachment.src
      || item.type === "localImage" && item.path === attachment.src;
  }
  async function buildQueuedImagePlan(message, input, expectedSources) {
    if (!Array.isArray(input)) throw failure(409, "此队列消息缺少可安全编辑的原生输入");
    const records = await queuedImageAttachmentRecords(message);
    const references = records.map(record => record.reference);
    if (!sameJsonValue(references, expectedSources))
      throw failure(409, "队列图片已变化，请刷新后重试");
    const usedInputIndexes = new Set();
    for (const record of records) {
      const candidates = input.flatMap((item, index) =>
        queuedInputMatchesAttachment(item, record.attachment) ? [index] : []);
      if (candidates.length !== 1 || usedInputIndexes.has(candidates[0]))
        throw failure(409, "无法唯一定位队列图片的原生输入，请在 Codex 桌面编辑");
      record.inputIndex = candidates[0];
      usedInputIndexes.add(candidates[0]);
    }
    return records;
  }
  function validateRetainedQueuedImages(originalSources, retainedSources) {
    let retainedIndex = 0;
    for (const source of originalSources)
      if (source === retainedSources[retainedIndex]) retainedIndex += 1;
    if (retainedIndex !== retainedSources.length)
      throw failure(409, "保留图片不属于本次队列编辑基线，请取消后重新编辑");
    return new Set(retainedSources);
  }
  function rewriteQueuedMessageImages(message, plan, retainedSources) {
    const retained = new Set(retainedSources);
    const removedIndexes = new Set(plan
      .filter(record => !retained.has(record.reference))
      .map(record => record.index));
    const imageAttachments = array(message.context?.imageAttachments)
      .filter((_, index) => !removedIndexes.has(index));
    return {
      ...message,
      context: { ...message.context, imageAttachments }
    };
  }
  function appendQueuedMessageImages(message, images) {
    if (!images.length) return message;
    return {
      ...message,
      context: {
        ...message.context,
        imageAttachments: [
          ...array(message.context?.imageAttachments),
          ...images
        ]
      }
    };
  }
  function rewriteQueuedInputImages(input, plan, retainedSources, addedInputs = []) {
    const retained = new Set(retainedSources);
    const removableInputs = new Map(plan.map(record => [record.inputIndex, record.reference]));
    const rewritten = input.filter((_, index) => {
      const reference = removableInputs.get(index);
      return reference == null || retained.has(reference);
    });
    if (!addedInputs.length) return rewritten;
    const retainedInputIndexes = plan
      .filter(record => retained.has(record.reference))
      .map(record => record.inputIndex);
    const insertionAfterInputIndex = retainedInputIndexes.length
      ? Math.max(...retainedInputIndexes)
      : plan.length ? Math.min(...plan.map(record => record.inputIndex)) - 1 : input.length - 1;
    const insertionIndex = input.slice(0, insertionAfterInputIndex + 1)
      .filter((_, index) => {
        const reference = removableInputs.get(index);
        return reference == null || retained.has(reference);
      }).length;
    return [
      ...rewritten.slice(0, insertionIndex),
      ...addedInputs,
      ...rewritten.slice(insertionIndex)
    ];
  }
  async function queuedAddedImages(attachments, existingSources) {
    const references = [];
    const seen = new Set(existingSources);
    for (const attachment of attachments) {
      const reference = await queuedImageSourceReference(attachment.src);
      if (seen.has(reference))
        throw failure(409, "新增图片与原有图片或其他新增图片重复");
      seen.add(reference);
      references.push(reference);
    }
    return { attachments, inputs: remoteImageInputs(attachments), references };
  }
  function queuedAddedImageFingerprint(additions) {
    return additions.references.map((reference, index) => {
      const attachment = additions.attachments[index];
      return {
        name: attachment.filename,
        mediaType: attachment.src.slice(0, attachment.src.indexOf(",")).toLowerCase(),
        reference
      };
    });
  }
  function sameJsonValue(left, right) {
    if (left === right) return true;
    if (left == null || right == null || typeof left !== "object" || typeof right !== "object") return false;
    if (Array.isArray(left) || Array.isArray(right))
      return Array.isArray(left) && Array.isArray(right) && left.length === right.length
        && left.every((value, index) => sameJsonValue(value, right[index]));
    const leftKeys = Object.keys(left).sort();
    const rightKeys = Object.keys(right).sort();
    return leftKeys.length === rightKeys.length && leftKeys.every((key, index) =>
      key === rightKeys[index] && sameJsonValue(left[key], right[key]));
  }
  async function readServerQueuedSubmissions(threadId) {
    const submissions = [];
    const cursors = new Set();
    let cursor = null;
    do {
      const page = await manager.requestClient.sendRequest("thread/queue/list", { threadId, cursor });
      if (!Array.isArray(page?.data)) throw failure(503, "原生队列列表格式不受支持");
      submissions.push(...page.data);
      cursor = page.nextCursor ?? null;
      if (cursor != null) {
        const key = JSON.stringify(cursor);
        if (cursors.has(key)) throw failure(503, "原生队列分页游标重复");
        cursors.add(key);
      }
    } while (cursor != null);
    return submissions;
  }
  function queuedEditFailure(error, fallback) {
    return { ok: false, code: error.code || 503, message: error.message || fallback };
  }
  function compactQueuedEditResult(result) {
    const { queuedMessages, ...compact } = result;
    return compact;
  }
  async function replayQueuedEditResult(entry, result) {
    if (!result?.ok) return result;
    try {
      messageActionManager();
      const coordinator = await manager.getTurnCoordinator?.();
      if (typeof coordinator?.readMessages !== "function")
        throw new Error("原生队列快照不可用");
      return {
        ...result,
        queuedMessages: array(coordinator.readMessages(entry.threadId))
      };
    } catch {
      return {
        ...result,
        queueSyncPending: true,
        message: "编辑操作已完成，但本机队列快照暂不可用，请刷新会话"
      };
    }
  }
  function queuedEditOutcomeUnknown(entry, message) {
    const result = {
      ok: false,
      outcomeUnknown: true,
      code: 503,
      threadId: entry.threadId,
      editId: entry.editId,
      messageId: entry.messageId,
      editState: "outcomeUnknown",
      message
    };
    entry.state = "outcomeUnknown";
    entry.result = result;
    entry.token = null;
    entry.beginResult = null;
    return result;
  }
  function queuedEditIdentityMatches(entry, identity) {
    return entry.threadId === identity.threadId
      && entry.messageId === identity.messageId
      && entry.originalText === identity.originalText
      && entry.clientUserMessageId === identity.clientUserMessageId
      && entry.createdAt === identity.createdAt
      && sameJsonValue(entry.expectedImageSources ?? null, identity.expectedImageSources);
  }
  function queuedEditConflict(threadId, editId) {
    return [...queuedEditStore.sessions.values()].find(session =>
      session.editId !== editId
      && session.threadId === threadId
      && !["committed", "canceled"].includes(session.state));
  }
  async function beginQueuedEdit(payload = {}) {
    let identity;
    try {
      identity = {
        threadId: requiredId(payload.threadId),
        messageId: requiredId(payload.messageId),
        editId: requiredId(payload.editId),
        originalText: typeof payload.originalText === "string" ? payload.originalText : null,
        clientUserMessageId: requiredId(payload.clientUserMessageId),
        createdAt: payload.createdAt,
        expectedImageSources: payload.expectedImageSources == null
          ? null : validateQueuedImageSources(payload.expectedImageSources, "队列图片编辑基线")
      };
      if (identity.originalText == null || !Number.isFinite(identity.createdAt))
        throw failure(400, "队列消息身份无效，请刷新会话");
    } catch (error) {
      return queuedEditFailure(error, "开始编辑原生队列消息失败");
    }
    const existing = queuedEditStore.sessions.get(identity.editId);
    if (existing) {
      if (!queuedEditIdentityMatches(existing, identity))
        return { ok: false, code: 409, message: "编辑标识已用于另一条队列消息" };
      if (existing.beginPromise) return await existing.beginPromise;
      if (["committed", "canceled", "outcomeUnknown"].includes(existing.state) && existing.result)
        return await replayQueuedEditResult(existing, existing.result);
      if (existing.beginResult) return await replayQueuedEditResult(existing, existing.beginResult);
      return existing.result || { ok: false, code: 409, message: "队列消息编辑状态不可恢复" };
    }
    if (queuedEditConflict(identity.threadId, identity.editId))
      return { ok: false, code: 409, message: "此会话已有一条队列消息正在编辑" };
    const entry = {
      ...identity,
      state: "validating",
      token: null,
      beginPromise: null,
      beginResult: null,
      terminalAction: null,
      terminalPrompt: null,
      terminalAddedImageFingerprint: null,
      terminalPromise: null,
      result: null
    };
    queuedEditStore.sessions.set(entry.editId, entry);
    entry.beginPromise = (async () => {
      let removeStarted = false;
      try {
        messageActionManager();
        const thread = await readThread(entry.threadId);
        if (runtimeCompaction(entry.threadId).state !== "idle")
          throw failure(409, "上下文压缩完成前不能编辑队列消息");
        if (typeof manager.getTurnCoordinator !== "function")
          throw failure(503, "当前 Codex 版本的原生队列服务不可用");
        const coordinator = await manager.getTurnCoordinator();
        const serverQueue = coordinator?.serverQueue;
        if (typeof coordinator?.readMessages !== "function"
            || typeof coordinator?.removeQueuedMessage !== "function"
            || typeof coordinator?.restoreQueuedMessage !== "function"
            || typeof serverQueue?.canEnqueue !== "function"
            || typeof manager.requestClient?.sendRequest !== "function")
          throw failure(503, "当前 Codex 版本不支持原生队列编辑");
        if (queuedEditConflict(entry.threadId, entry.editId))
          throw failure(409, "此会话已有一条队列消息正在编辑");
        const current = array(coordinator.readMessages(entry.threadId))
          .find(message => message?.id === entry.messageId);
        if (!current) throw failure(409, "此队列消息已执行或不存在，请刷新会话");
        if (current.clientUserMessageId !== entry.clientUserMessageId
            || current.createdAt !== entry.createdAt
            || queuedMessageText(current) !== entry.originalText)
          throw failure(409, "队列消息已变化，请刷新后重试");
        const conversation = manager.getConversation(entry.threadId);
        if (queuedMessageUnconfirmed(conversation, current))
          throw failure(409, "此队列消息的提交状态尚未确认，不能编辑");
        if (queuedMessageAccepted(conversation, current))
          throw failure(409, "此队列消息已引导到会话，不能编辑");
        if (current.submission?.status && current.submission.status !== "queued")
          throw failure(409, "此队列消息正在发送或结果尚未确认，不能编辑");
        if (!await serverQueue.canEnqueue(entry.threadId, current))
          throw failure(409, "此队列消息当前无法安全恢复，请稍后重试");
        const matches = (await readServerQueuedSubmissions(entry.threadId))
          .filter(submission => submission?.id === entry.messageId);
        if (matches.length !== 1) throw failure(409, "此队列消息已执行或不存在，请刷新会话");
        const serverSubmission = matches[0];
        if (serverSubmission.clientUserMessageId !== entry.clientUserMessageId)
          throw failure(409, "队列消息身份已变化，请刷新后重试");
        if (entry.expectedImageSources != null)
          await buildQueuedImagePlan(current, serverSubmission.input, entry.expectedImageSources);
        entry.state = "removing";
        removeStarted = true;
        const removed = await coordinator.removeQueuedMessage(entry.threadId, entry.messageId);
        if (removed == null) {
          removeStarted = false;
          throw failure(409, "此队列消息已执行或不存在，请刷新会话");
        }
        entry.token = {
          ...removed,
          serverSubmission: removed.serverSubmission ?? serverSubmission
        };
        if (entry.token.canRestore === false)
          return queuedEditOutcomeUnknown(entry, "原生队列已移除消息，但未提供可恢复状态；请刷新会话检查");
        entry.state = "editing";
        const result = {
          ok: true,
          threadId: entry.threadId,
          editId: entry.editId,
          messageId: entry.messageId,
          editState: "editing",
          queuedMessages: array(coordinator.readMessages(entry.threadId))
        };
        entry.beginResult = compactQueuedEditResult(result);
        return result;
      } catch (error) {
        if (removeStarted)
          return queuedEditOutcomeUnknown(entry, "原生队列移除结果未确认，请刷新会话检查，不要重复开始编辑");
        if (queuedEditStore.sessions.get(entry.editId) === entry)
          queuedEditStore.sessions.delete(entry.editId);
        return queuedEditFailure(error, "开始编辑原生队列消息失败");
      } finally {
        entry.beginPromise = null;
      }
    })();
    return await entry.beginPromise;
  }
  async function finishQueuedEdit(payload = {}, action) {
    let threadId, editId, prompt = null, retainedImageSources = null, addedImages = null;
    try {
      threadId = requiredId(payload.threadId);
      editId = requiredId(payload.editId);
      if (action === "commit") {
        prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
        retainedImageSources = payload.retainedImageSources == null
          ? null : validateQueuedImageSources(payload.retainedImageSources, "队列保留图片");
        addedImages = payload.images == null ? null : remoteImageAttachments(payload);
      }
    } catch (error) {
      return queuedEditFailure(error, action === "commit" ? "保存队列消息失败" : "取消队列消息编辑失败");
    }
    let entry = queuedEditStore.sessions.get(editId);
    if (!entry)
      return { ok: false, code: 409, message: "队列消息编辑恢复令牌已丢失，请刷新队列确认原消息状态" };
    if (entry.threadId !== threadId)
      return { ok: false, code: 409, message: "编辑标识不属于此会话" };
    if (entry.beginPromise) {
      await entry.beginPromise;
      entry = queuedEditStore.sessions.get(editId);
      if (!entry)
        return { ok: false, code: 409, message: "队列消息未进入可编辑状态" };
    }
    if (action === "commit"
        && (retainedImageSources != null || addedImages != null)
        && entry.expectedImageSources == null)
      return { ok: false, code: 409, message: "本次队列编辑未建立完整图片基线，请取消后重新编辑" };
    if (retainedImageSources != null)
      validateRetainedQueuedImages(entry.expectedImageSources, retainedImageSources);
    const effectiveRetainedSources = retainedImageSources ?? entry.expectedImageSources ?? [];
    if (action === "commit" && effectiveRetainedSources.length + (addedImages?.length || 0) > 4)
      return { ok: false, code: 400, message: "每条消息最多保留或新增 4 张图片" };
    if (action === "commit" && !prompt && !effectiveRetainedSources.length && !(addedImages?.length))
      return { ok: false, code: 400, message: "队列消息不能为空" };
    let addedImagePlan = null;
    let addedImageFingerprint = null;
    if (action === "commit" && (retainedImageSources != null || addedImages != null)) {
      try {
        addedImagePlan = await queuedAddedImages(
          addedImages ?? [],
          effectiveRetainedSources
        );
        addedImageFingerprint = addedImages == null ? null : queuedAddedImageFingerprint(addedImagePlan);
      } catch (error) {
        return queuedEditFailure(error, "保存队列消息失败");
      }
    }
    if (entry.terminalAction) {
      if (entry.terminalAction !== action
          || action === "commit" && (entry.terminalPrompt !== prompt
            || !sameJsonValue(entry.terminalRetainedImageSources, retainedImageSources)
            || !sameJsonValue(entry.terminalAddedImageFingerprint ?? null, addedImageFingerprint)))
        return { ok: false, code: 409, message: "此编辑标识已用于另一项完成操作" };
      if (entry.terminalPromise) return await entry.terminalPromise;
      return entry.result
        ? await replayQueuedEditResult(entry, entry.result)
        : { ok: false, code: 409, message: "队列消息编辑完成状态不可恢复" };
    }
    if (entry.state === "outcomeUnknown")
      return entry.result || { ok: false, outcomeUnknown: true, code: 503, message: "队列消息编辑结果未知，请刷新确认" };
    if (entry.state !== "editing" || !entry.token)
      return { ok: false, code: 409, message: "队列消息当前不在可保存或取消的编辑状态" };
    entry.terminalAction = action;
    entry.terminalPrompt = prompt;
    entry.terminalRetainedImageSources = retainedImageSources;
    entry.terminalAddedImageFingerprint = addedImageFingerprint;
    entry.state = action === "commit" ? "committing" : "canceling";
    entry.terminalPromise = (async () => {
      let restoreStarted = false;
      let expectedInput = null;
      let expectedImageSources = null;
      try {
        messageActionManager();
        if (typeof manager.getTurnCoordinator !== "function")
          throw failure(503, "当前 Codex 版本的原生队列服务不可用");
        const coordinator = await manager.getTurnCoordinator();
        if (typeof coordinator?.restoreQueuedMessage !== "function"
            || typeof coordinator?.readMessages !== "function")
          throw failure(503, "当前 Codex 版本不支持恢复原生队列消息");
        let restoreToken = entry.token;
        if (action === "commit") {
          let message = queuedMessageWithText(entry.token.message, prompt);
          let serverSubmission = null;
          expectedImageSources = entry.expectedImageSources;
          if (entry.token.serverSubmission != null) {
            let input = entry.token.serverSubmission.input;
            if (retainedImageSources != null || addedImages != null) {
              const plan = await buildQueuedImagePlan(
                entry.token.message,
                entry.token.serverSubmission.input,
                entry.expectedImageSources
              );
              const additions = addedImagePlan;
              input = rewriteQueuedInputImages(
                input,
                plan,
                effectiveRetainedSources,
                additions.inputs
              );
              message = rewriteQueuedMessageImages(message, plan, effectiveRetainedSources);
              message = appendQueuedMessageImages(message, additions.attachments);
              expectedImageSources = [...effectiveRetainedSources, ...additions.references];
            }
            input = rewriteQueuedSubmissionInput(input, entry.originalText, prompt);
            expectedInput = input;
            serverSubmission = { ...entry.token.serverSubmission, input };
          } else if (retainedImageSources != null || addedImages != null) {
            throw failure(409, "此队列消息缺少可安全编辑的原生图片输入");
          }
          restoreToken = {
            ...entry.token,
            message,
            ...(serverSubmission == null ? {} : { serverSubmission })
          };
        }
        restoreStarted = true;
        const messageId = await coordinator.restoreQueuedMessage(entry.threadId, restoreToken);
        if (messageId !== entry.messageId)
          throw new Error("原生队列未确认恢复后的消息身份");
        const restoredMessage = array(coordinator.readMessages(entry.threadId))
          .find(message => message?.id === entry.messageId);
        if (!restoredMessage
            || restoredMessage.clientUserMessageId !== entry.clientUserMessageId
            || restoredMessage.createdAt !== entry.createdAt
            || queuedMessageText(restoredMessage) !== (action === "commit" ? prompt : entry.originalText))
          throw new Error("原生队列恢复后的消息身份或内容未确认");
        if (action === "commit" && expectedImageSources != null) {
          const restoredSources = (await queuedImageAttachmentRecords(restoredMessage))
            .map(record => record.reference);
          if (!sameJsonValue(restoredSources, expectedImageSources))
            throw new Error("原生队列恢复后的图片未确认");
          const submissions = (await readServerQueuedSubmissions(entry.threadId))
            .filter(submission => submission?.id === entry.messageId);
          if (submissions.length !== 1
              || submissions[0].clientUserMessageId !== entry.clientUserMessageId
              || !sameJsonValue(submissions[0].input, expectedInput))
            throw new Error("原生队列恢复后的真实输入未确认");
          await buildQueuedImagePlan(restoredMessage, submissions[0].input, expectedImageSources);
        }
        entry.state = action === "commit" ? "committed" : "canceled";
        entry.result = {
          ok: true,
          threadId: entry.threadId,
          editId: entry.editId,
          messageId,
          editState: entry.state
        };
        entry.token = null;
        entry.beginResult = null;
        return {
          ...entry.result,
          queuedMessages: array(coordinator.readMessages(entry.threadId))
        };
      } catch (error) {
        if (restoreStarted)
          return queuedEditOutcomeUnknown(entry, action === "commit"
            ? "原生队列保存结果未确认，请刷新队列检查，不要重复保存"
            : "原生队列恢复结果未确认，请刷新队列检查，不要重复取消");
        entry.terminalAction = null;
        entry.terminalPrompt = null;
        entry.terminalRetainedImageSources = null;
        entry.terminalAddedImageFingerprint = null;
        entry.state = "editing";
        entry.result = null;
        return queuedEditFailure(error, action === "commit" ? "保存队列消息失败" : "取消队列消息编辑失败");
      } finally {
        entry.terminalPromise = null;
      }
    })();
    return await entry.terminalPromise;
  }
  async function cancelQueuedEdit(payload = {}) {
    return await finishQueuedEdit(payload, "cancel");
  }
  async function editQueuedLegacy(payload = {}) {
    let coordinator = null;
    let deferral = null;
    let updateStarted = false;
    try {
      messageActionManager();
      const threadId = requiredId(payload.threadId);
      const messageId = requiredId(payload.messageId);
      if (payload.images != null)
        throw failure(409, "旧版队列编辑不支持新增图片，请刷新后重试");
      const prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
      if (!prompt) throw failure(400, "队列消息不能为空");
      if (payload.retainedImageSources != null)
        throw failure(409, "旧版队列编辑不支持删除图片，请刷新后重试");
      if (typeof payload.originalText !== "string"
          || typeof payload.clientUserMessageId !== "string"
          || !Number.isFinite(payload.createdAt))
        throw failure(400, "队列消息身份无效，请刷新会话");
      await readThread(threadId);
      if (runtimeCompaction(threadId).state !== "idle")
        throw failure(409, "上下文压缩完成前不能编辑队列消息");
      if (typeof manager.getTurnCoordinator !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      coordinator = await manager.getTurnCoordinator();
      if (typeof coordinator?.readMessages !== "function"
          || typeof manager.requestClient?.sendRequest !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列编辑");
      deferral = coordinator.deferAutomaticTurns?.(threadId) ?? null;
      const current = array(coordinator.readMessages(threadId)).find(message => message?.id === messageId);
      if (!current) throw failure(409, "此队列消息已执行或不存在，请刷新会话");
      if (current.clientUserMessageId !== payload.clientUserMessageId
          || current.createdAt !== payload.createdAt
          || queuedMessageText(current) !== payload.originalText)
        throw failure(409, "队列消息已变化，请刷新后重试");
      const conversation = manager.getConversation(threadId);
      if (queuedMessageUnconfirmed(conversation, current))
        throw failure(409, "此队列消息的提交状态尚未确认，不能编辑");
      if (queuedMessageAccepted(conversation, current))
        throw failure(409, "此队列消息已引导到会话，不能编辑");
      if (current.submission?.status && current.submission.status !== "queued")
        throw failure(409, "此队列消息正在发送或结果尚未确认，不能编辑");
      const matches = (await readServerQueuedSubmissions(threadId)).filter(submission => submission?.id === messageId);
      if (matches.length !== 1) throw failure(409, "此队列消息已执行或不存在，请刷新会话");
      const serverSubmission = matches[0];
      if (serverSubmission.clientUserMessageId !== payload.clientUserMessageId)
        throw failure(409, "队列消息身份已变化，请刷新后重试");
      const input = rewriteQueuedSubmissionInput(serverSubmission.input, payload.originalText, prompt);
      updateStarted = true;
      const result = await manager.requestClient.sendRequest("thread/queue/update", {
        threadId,
        queuedSubmissionId: messageId,
        input
      });
      if (result?.queuedSubmission?.id !== messageId
          || !sameJsonValue(result.queuedSubmission.input, input))
        throw new Error("原生队列未确认原地编辑结果");
      const queued = array(coordinator.readMessages(threadId)).map(message =>
        message?.id === messageId ? queuedMessageWithText(message, prompt) : message);
      return {
        ok: true,
        threadId,
        messageId,
        replacedMessageId: messageId,
        queuedMessages: queued
      };
    } catch (error) {
      if (updateStarted && !error.remoteValidationFailure)
        return { ok: false, outcomeUnknown: true, code: 503, message: "原生队列编辑结果未确认，请刷新会话后检查，不要重复提交" };
      return { ok: false, code: error.code || 503, message: error.message || "编辑原生队列消息失败" };
    } finally {
      try { deferral?.dispose?.(); } catch {}
    }
  }
  async function editQueued(payload = {}) {
    if (payload.editId != null) return await finishQueuedEdit(payload, "commit");
    return await editQueuedLegacy(payload);
  }
  async function steerQueued(payload = {}) {
    let submitted = false;
    try {
      messageActionManager();
      const threadId = requiredId(payload.threadId);
      const messageId = requiredId(payload.messageId);
      await readThread(threadId);
      if (runtimeCompaction(threadId).state !== "idle")
        throw failure(409, "上下文压缩完成前不能引导队列消息");
      const coordinator = await manager.getTurnCoordinator?.();
      if (typeof coordinator?.sendQueuedMessageNow !== "function"
          || typeof coordinator.execution?.prepare !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列引导");
      const prepare = async (request, phase) => {
        if (request?.conversationId !== threadId || request?.message?.id !== messageId)
          throw failure(409, "队列消息身份已变化，请刷新会话");
        const message = request.message;
        const serverQueuedMessage = message.serverQueuedMessageId != null;
        const clientUserMessageId = request.options?.clientUserMessageId;
        if (serverQueuedMessage && (message.serverQueuedMessageId !== messageId
            || typeof clientUserMessageId !== "string" || !clientUserMessageId))
          throw failure(409, "服务端队列消息身份无效，请刷新会话");
        if (!serverQueuedMessage && (message.submissionOptions?.clientUserMessageId != null
            && message.submissionOptions.clientUserMessageId !== message.id
            || clientUserMessageId != null && clientUserMessageId !== message.id))
          throw failure(409, "队列消息身份与原生准备参数不一致，请刷新会话");
        const prepared = await coordinator.execution.prepare(request.conversationId, request.message, phase);
        if (prepared?.status === "paused") throw new Error(prepared.reason || "原生队列消息暂时不可用");
        if (prepared?.status !== "ready" || !prepared.submission)
          throw failure(503, "原生队列消息准备失败");
        if (!serverQueuedMessage) return prepared.submission;
        const submission = prepared.submission;
        const start = submission.start;
        const steer = submission.steer;
        const admissionStart = steer?.userMessageAdmissionStart;
        if (start?.request?.clientUserMessageId !== message.id
            || steer?.clientUserMessageId !== message.id
            || start.context?.restoreMessage !== message
            || steer.restoreMessage !== message
            || start.request.input !== steer.input
            || admissionStart != null && admissionStart.request?.clientUserMessageId !== message.id)
          throw failure(503, "原生准备结果不符合服务端队列消息身份契约");
        if (clientUserMessageId === message.id) return submission;
        const updatedStart = {
          ...start,
          request: { ...start.request, clientUserMessageId }
        };
        const updatedAdmissionStart = admissionStart == null
          ? null
          : admissionStart === start
            ? updatedStart
            : {
              ...admissionStart,
              request: { ...admissionStart.request, clientUserMessageId }
            };
        return {
          ...submission,
          start: updatedStart,
          steer: {
            ...steer,
            clientUserMessageId,
            ...(updatedAdmissionStart == null ? {} : { userMessageAdmissionStart: updatedAdmissionStart })
          }
        };
      };
      submitted = true;
      const result = await coordinator.sendQueuedMessageNow(threadId, messageId, undefined, prepare);
      if (result?.status !== "sent") throw failure(409, "此队列消息当前无法引导，请刷新会话");
      return { ok: true, threadId, messageId, turnId: result.turnId };
    } catch (error) {
      return { ok: false, code: error.code || 503,
        ...(submitted && !error.remoteValidationFailure ? { outcomeUnknown: true } : {}),
        message: error.message || "引导结果未确认，请查看原生队列，不要重复操作" };
    }
  }
  async function skillCatalog(payload, trustedCwd = null) {
    let cwd = trustedCwd;
    if (!cwd) {
      if (payload.threadId) cwd = (await readThread(payload.threadId)).cwd;
      else {
        const projectId = requestedProjectId(payload.projectId);
        if (projectId == null) cwd = (await nativeProjectlessWorkspace(null)).cwd;
        else cwd = (await localProjectWorkspace(projectId)).cwd;
      }
    }
    if (!cwd) throw failure(400, "未能确定 Skill 所属的本机工作目录");
    const result = await manager.requestClient.sendRequest("skills/list", { cwds: [cwd], forceReload: true });
    const skills = new Map();
    for (const entry of array(result.data)) {
      if (normalizePath(entry.cwd) !== normalizePath(cwd)) continue;
      for (const skill of array(entry.skills)) {
        if (!skill.enabled || typeof skill.name !== "string" || typeof skill.path !== "string") continue;
        skills.set(skill.path, {
          name: skill.name, path: skill.path, displayName: skill.interface?.displayName || skill.name,
          description: skill.interface?.shortDescription || skill.shortDescription || skill.description || ""
        });
      }
    }
    return { ok: true, data: [...skills.values()] };
  }
  async function skillInputs(payload, cwd = null) {
    if (payload.skills == null) return [];
    if (!Array.isArray(payload.skills) || payload.skills.length > 16
        || payload.skills.some(skill => !skill || typeof skill.name !== "string" || typeof skill.path !== "string"))
      throw failure(400, "Skill 选择无效，最多选择 16 项");
    if (!payload.skills.length) return [];
    const available = (await skillCatalog(payload, cwd)).data;
    const selected = new Map();
    for (const skill of payload.skills) {
      const entry = available.find(item => item.name === skill.name && item.path === skill.path);
      if (!entry) throw failure(409, `Skill「${skill.name}」已停用或不属于当前项目，请重新选择`);
      selected.set(entry.path, { type: "skill", name: entry.name, path: entry.path });
    }
    return [...selected.values()];
  }
  const remoteInputImageMaxBytes = 5 * 1024 * 1024;
  const remoteInputImageMaxSourceLength = Math.ceil(remoteInputImageMaxBytes / 3) * 4 + 64;
  const remoteImageDataPattern = /^data:image\/(?:png|jpeg|webp|gif);base64,(?=.+)(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
  function remoteImageAttachments(payload = {}) {
    if (payload.images == null) return [];
    if (!Array.isArray(payload.images) || payload.images.length > 4)
      throw failure(400, "每条消息最多添加 4 张图片");
    let total = 0;
    return payload.images.map(image => {
      if (!image || typeof image.name !== "string" || !image.name.trim() || image.name.length > 255
          || !["image/png", "image/jpeg", "image/webp", "image/gif"].includes(image.mime)
          || !Number.isSafeInteger(image.size) || image.size <= 0 || image.size > remoteInputImageMaxBytes
          || typeof image.dataUrl !== "string" || image.dataUrl.length > remoteInputImageMaxSourceLength
          || !remoteImageDataPattern.test(image.dataUrl)
          || !image.dataUrl.toLowerCase().startsWith(`data:${image.mime};base64,`))
        throw failure(400, "图片附件参数无效");
      total += image.size;
      if (total > 12 * 1024 * 1024) throw failure(413, "图片总大小不能超过 12 MiB");
      return {
        src: image.dataUrl,
        filename: image.name.trim(),
        localPath: null
      };
    });
  }
  function remoteAttachmentContext(prompt, cwd, images) {
    return {
      prompt,
      addedFiles: [],
      fileAttachments: [],
      pastedTextAttachments: [],
      uploadedFileAttachments: [],
      ideContext: null,
      imageAttachments: images,
      workspaceRoots: [cwd],
      commentAttachments: [],
      mcpAppModelContextAttachments: [],
      appshotContexts: []
    };
  }
  const remoteImageInputs = images => images.map(image => ({ type: "image", url: image.src }));
  async function queueMessage(payload = {}) {
    let threadId = null;
    let clientUserMessageId = null;
    let queueAddStarted = false;
    let queuedSubmission = null;
    try {
      messageActionManager();
      threadId = requiredId(payload.threadId);
      const prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
      const imageAttachments = remoteImageAttachments(payload);
      if (!prompt && !imageAttachments.length) throw failure(400, "消息不能为空");
      const thread = await readThread(threadId);
      if (runtimeCompaction(threadId).state !== "idle")
        throw failure(409, "上下文压缩完成前不能加入队列");
      if (typeof manager.getTurnCoordinator !== "function"
          || typeof manager.requestClient?.sendRequest !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      const coordinator = await manager.getTurnCoordinator();
      const serverQueue = coordinator?.serverQueue;
      if (typeof coordinator?.execution?.prepare !== "function"
          || typeof serverQueue?.isEnabled !== "function"
          || typeof serverQueue?.canEnqueue !== "function"
          || typeof serverQueue?.load !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列发送");
      const cwd = manager.getConversationCwd(threadId) || thread.cwd || "/";
      const message = {
        id: crypto.randomUUID(),
        text: prompt,
        context: remoteAttachmentContext(prompt, cwd, imageAttachments),
        cwd,
        createdAt: Date.now()
      };
      clientUserMessageId = message.id;
      if (!await serverQueue.isEnabled(threadId))
        throw failure(409, "此会话当前不支持原生队列发送");
      const inputItems = await skillInputs({ ...payload, threadId });
      const conversation = manager.getConversation(threadId);
      const model = payload.model == null ? undefined : String(payload.model);
      const reasoningEffort = payload.reasoningEffort == null ? undefined : String(payload.reasoningEffort);
      const selectedModel = model === undefined
        ? thread.model ?? conversation?.latestCollaborationMode?.settings?.model ?? conversation?.latestModel
        : model;
      if (model !== undefined && !model.trim())
        throw failure(400, "请求模型不能为空");
      await validateReasoningEffort(selectedModel, reasoningEffort);
      const resumed = await manager.resumeConversation({
        conversationId: threadId,
        model: model ?? null,
        reasoningEffort: reasoningEffort ?? null,
        workspaceRoots: [cwd],
        collaborationMode: conversation?.latestCollaborationMode ?? null
      });
      if (resumed?.status === "not-ready") throw failure(409, "会话暂不可用，请稍后重试");
      if (!await serverQueue.canEnqueue(threadId, message))
        throw failure(409, "此会话当前不支持原生队列发送");
      if (model !== undefined || reasoningEffort !== undefined) {
        const current = manager.getConversation(threadId);
        const selected = model ?? current?.latestModel;
        if (typeof selected !== "string" || !selected.trim())
          throw failure(409, "无法为此会话设置思考深度");
        if (typeof manager.updateThreadSettingsForNextTurn !== "function")
          throw failure(503, "当前 Codex 版本不支持设置下一轮模型");
        await manager.updateThreadSettingsForNextTurn(threadId, {
          model: selected,
          effort: reasoningEffort === undefined ? current?.latestReasoningEffort ?? null : reasoningEffort
        });
      }
      const prepared = await coordinator.execution.prepare(threadId, message, "start");
      if (prepared?.status === "paused")
        throw failure(409, prepared.reason || "原生队列消息暂时不可用");
      const start = prepared?.submission?.start;
      const preparedInput = start?.request?.input;
      if (prepared?.status !== "ready"
          || start?.request?.threadId !== threadId
          || start.request.clientUserMessageId !== clientUserMessageId
          || start.context?.restoreMessage !== message
          || !Array.isArray(preparedInput))
        throw failure(503, "原生队列消息准备失败");
      const input = [...preparedInput, ...inputItems];
      queueAddStarted = true;
      const result = await manager.requestClient.sendRequest("thread/queue/add", {
        threadId,
        input,
        clientUserMessageId
      });
      const accepted = result?.queuedSubmission;
      if (typeof accepted?.id !== "string" || accepted.clientUserMessageId !== clientUserMessageId)
        throw new Error("原生队列未确认消息身份或输入");
      if (!sameJsonValue(accepted.input, input))
        throw new Error("原生队列未确认消息身份或输入");
      queuedSubmission = accepted;
      await serverQueue.load(threadId);
      const queued = coordinator.readMessages?.(threadId);
      return {
        ok: true,
        threadId,
        messageId: queuedSubmission.id,
        clientUserMessageId,
        queuedMessages: array(queued?.messages || queued)
      };
    } catch (error) {
      if (queuedSubmission) {
        return {
          ok: true,
          threadId,
          messageId: queuedSubmission.id,
          clientUserMessageId: queuedSubmission.clientUserMessageId,
          queueSyncPending: true,
          message: "消息已加入原生队列，但本机队列同步未确认，请刷新会话"
        };
      }
      if (queueAddStarted && !error.remoteValidationFailure)
        return {
          ok: false, outcomeUnknown: true, code: 503, threadId, clientUserMessageId,
          message: "原生队列加入结果未确认，请检查会话，不要重复操作"
        };
      return { ok: false, code: error.code || 503, message: error.message || "加入原生队列失败" };
    }
  }
  async function nativeCreationModule() {
    const resource = [...document.querySelectorAll('link[rel="modulepreload"]')].map(link => link.href)
      .concat(performance.getEntriesByType("resource").map(entry => entry.name))
      .find(name => /\/app-initial-[^/]+\.js$/.test(name));
    if (!resource || !resource.startsWith("app://-/")) throw failure(503, "未发现原生创建模块");
    return await import(resource);
  }
  function nativeCreationProvider(module) {
    if (!appScope) discover();
    if (!appScope) throw failure(503, "当前 Codex 版本的原生创建上下文不可用");
    const factory = Object.values(module).find(value => typeof value === "function"
      && /requestClient:\w+\.get\(/.test(Function.prototype.toString.call(value))
      && /dynamicTools:new/.test(Function.prototype.toString.call(value))
      && /window\.dispatchEvent/.test(Function.prototype.toString.call(value)));
    if (!factory) throw failure(503, "当前 Codex 版本不支持原生创建适配，请在桌面新建会话");
    const provider = factory(appScope, "local");
    if (typeof provider?.readCreationInputs !== "function") throw failure(503, "原生创建输入不可用");
    return provider;
  }
  async function nativeCreationInputs() {
    return nativeCreationProvider(await nativeCreationModule());
  }
  function requestedProjectId(value) {
    if (value == null || value === "") return null;
    if (typeof value !== "string" || !/^[a-zA-Z0-9_-]{1,120}$/.test(value))
      throw failure(400, "项目 ID 无效");
    return value;
  }
  async function localProjectWorkspace(projectId) {
    const project = (await state("local-projects") || {})[projectId];
    const workspaceRoots = project?.rootPaths;
    if (!Array.isArray(workspaceRoots) || !workspaceRoots.length
        || workspaceRoots.some(root => typeof root !== "string" || !root))
      throw failure(400, "请选择现有本机项目");
    return {
      cwd: workspaceRoots[0],
      workspaceRoots: [...workspaceRoots],
      projectAssignment: { projectKind: "local", projectId }
    };
  }
  async function nativeProjectlessWorkspace(prompt, module = null) {
    const creationModule = module || await nativeCreationModule();
    const resolver = Object.values(creationModule).find(value => typeof value === "function"
      && /projectless-thread-cwd/.test(Function.prototype.toString.call(value))
      && /projectlessOutputDirectory/.test(Function.prototype.toString.call(value))
      && /workspaceRoots/.test(Function.prototype.toString.call(value)));
    if (!resolver)
      throw failure(503, "当前 Codex 版本不支持原生无项目会话，请在桌面新建会话");
    let workspace;
    try {
      workspace = await resolver(["~"], { prompt: prompt ?? null });
    } catch (error) {
      throw failure(error?.code || 503, error?.message || "原生无项目工作目录准备失败");
    }
    if (!workspace || typeof workspace.cwd !== "string" || !workspace.cwd
        || typeof workspace.projectlessOutputDirectory !== "string" || !workspace.projectlessOutputDirectory
        || !Array.isArray(workspace.workspaceRoots) || !workspace.workspaceRoots.length
        || workspace.workspaceRoots.some(root => typeof root !== "string" || !root))
      throw failure(503, "原生无项目工作目录无效");
    return {
      cwd: workspace.cwd,
      workspaceRoots: [...workspace.workspaceRoots],
      projectlessOutputDirectory: workspace.projectlessOutputDirectory
    };
  }
  async function call(action, payload = {}) {
    try {
      if (action === "dispose") { window.__codexElvesRemoteAccess.dispose(); return { ok: true }; }
      if (manager?.disposed) {
        for (const dispose of unsubscribe) { try { dispose?.(); } catch {} }
        unsubscribe = [];
        manager = null;
        appScope = null;
        emit({ method: "remote/resync" });
      }
      if (!manager) discover();
      switch (action) {
        case "events": {
          const reset = payload.epoch !== epoch || payload.cursor < (notifications[0]?.sequence || 1) - 1;
          return { ok: true, epoch, cursor: eventSequence, reset,
            events: notifications.filter(event => event.sequence > (payload.cursor || 0)).map(event => event.event) };
        }
        case "sidebar": return await sidebar(payload);
        case "queue": return await queueSnapshot(payload.threadId);
        case "markRead": return await markThreadRead(payload.threadId);
        case "removeQueued": return await removeQueued(payload);
        case "reorderQueued": return await reorderQueued(payload);
        case "beginQueuedEdit": return await beginQueuedEdit(payload);
        case "editQueued": return await editQueued(payload);
        case "cancelQueuedEdit": return await cancelQueuedEdit(payload);
        case "steerQueued": return await steerQueued(payload);
        case "editMessage": return await editLastMessage(payload);
        case "updateGoal": return await updateThreadGoal(payload);
        case "history": return await historyPage(payload);
        case "thread": {
          const thread = await readThread(payload.threadId);
          if (!manager.getConversation(thread.id) || manager.getConversation(thread.id).resumeState === "needs_resume") {
            await manager.resumeConversation({
              conversationId: thread.id, model: null, reasoningEffort: null,
              workspaceRoots: [thread.cwd], collaborationMode: null
            });
          }
          const runtime = runtimeThread(thread.id, thread);
          const status = manager.getThreadRuntimeStatusEvidence(thread.id)
            || thread.threadRuntimeStatus || thread.status || runtime.status;
          return { ok: true, thread: { ...summary(thread), ...runtime, status } };
        }
        case "models": {
          const models = (await nativeModelCatalog()).map(model => ({
            id: model.id, model: model.model, displayName: model.displayName,
            supportedReasoningEfforts: array(model.supportedReasoningEfforts),
            defaultReasoningEffort: model.defaultReasoningEffort
          }));
          return { ok: true, data: models, ...await defaultModelSettings() };
        }
        case "skills": return await skillCatalog(payload);
        case "send": {
          const id = requiredId(payload.threadId);
          const loaded = await readThread(id);
          const prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
          const imageAttachments = remoteImageAttachments(payload);
          if (!prompt && !imageAttachments.length) throw failure(400, "消息不能为空");
          const conversation = manager.getConversation(id);
          const status = manager.getThreadRuntimeStatusEvidence?.(id)
            || conversation?.threadRuntimeStatus || loaded.status;
          const runtime = status?.status ?? status;
          const statusType = typeof runtime === "string" ? runtime : runtime?.type;
          const queued = manager.getTurnCoordinator?.()?.readMessages?.(id);
          if (["active", "inProgress", "running", "pending"].includes(statusType)
              || nativeConversationTurns(conversation).some(turn => nativeTurnStatus(turn) === "inProgress")
              || array(queued?.messages || queued).length)
            return await queueMessage({ ...payload, threadId: id });
          const cwd = manager.getConversationCwd(id) || loaded.cwd || "/";
          const options = {
            prompt,
            inputItems: await skillInputs(payload),
            ...(imageAttachments.length
              ? { attachmentContext: remoteAttachmentContext(payload.prompt, cwd, imageAttachments) }
              : {})
          };
          const selectedModel = payload.model || loaded.model
            || conversation?.latestCollaborationMode?.settings?.model || conversation?.latestModel;
          await validateReasoningEffort(selectedModel, payload.reasoningEffort);
          if (payload.model != null) options.model = String(payload.model);
          if (payload.reasoningEffort != null) options.reasoningEffort = String(payload.reasoningEffort);
          const result = await manager.sendFollowUpMessage(id, options);
          if (result?.status === "outcome-unknown") return { ok: false, outcomeUnknown: true, threadId: id, message: "原生客户端未确认送达，请查看会话，不要重复发送" };
          return { ok: true, threadId: id, result: result ?? null };
        }
        case "create": {
          let startConversationSubmitted = false;
          try {
            const projectId = requestedProjectId(payload.projectId);
            await validateReasoningEffort(payload.model, payload.reasoningEffort);
            const imageAttachments = remoteImageAttachments(payload);
            const prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
            if (!prompt && !imageAttachments.length) throw failure(400, "消息不能为空");
            let provider, workspace;
            if (projectId == null) {
              const module = await nativeCreationModule();
              provider = nativeCreationProvider(module);
              workspace = await nativeProjectlessWorkspace(prompt, module);
            } else {
              provider = await nativeCreationInputs();
              workspace = await localProjectWorkspace(projectId);
            }
            const skills = await skillInputs(payload, workspace.cwd);
            startConversationSubmitted = true;
            const result = await manager.startConversation({
              input: [
                ...(prompt ? [{ type: "text", text: prompt, text_elements: [] }] : []),
                ...remoteImageInputs(imageAttachments),
                ...skills
              ],
              cwd: workspace.cwd, workspaceRoots: workspace.workspaceRoots,
              ...(projectId == null ? {
                workspaceKind: "projectless",
                projectlessOutputDirectory: workspace.projectlessOutputDirectory
              } : {
                projectAssignment: workspace.projectAssignment
              }),
              useAppServerPermissionDefault: true, mode: "default", threadStartKind: "default",
              ...(payload.model ? {
                collaborationMode: {
                  mode: "default",
                  settings: { model: String(payload.model), reasoning_effort: payload.reasoningEffort || null, developer_instructions: null }
                }
              } : {})
            }, { readThreadCreationInputs: (params, mode) => provider.readCreationInputs(params, mode) });
            const accepted = result.status === "created" && ["accepted", "not-requested"].includes(result.firstTurn?.status);
            return {
              ...result, ok: accepted, threadId: result.conversationId,
              outcomeUnknown: result.status === "outcome-unknown" || result.firstTurn?.status === "outcome-unknown",
              ...(!accepted ? { code: 409, message: result.firstTurn?.message || `会话 ${result.conversationId || ""} 的首条消息未确认，请先查看会话，不要重复新建` } : {})
            };
          } catch (error) {
            if (!startConversationSubmitted && !error?.remoteValidationFailure)
              throw failure(error?.code || 503, error?.message || "原生创建输入准备失败");
            throw error;
          }
        }
        case "stop": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          await manager.stopThread(id, "user");
          await manager.cleanBackgroundTerminals(id);
          const terminals = await manager.requestClient.sendRequest("thread/backgroundTerminals/list", { threadId: id });
          return { ok: true, threadId: id, backgroundTerminals: terminals };
        }
        case "compact": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          if (runtimeCompaction(id).state !== "idle") throw failure(409, "此会话正在压缩上下文，请等待完成后再试");
          await manager.compactThread(id);
          return { ok: true, threadId: id, compaction: runtimeCompaction(id) };
        }
        case "pinProject": {
          const id = requiredId(payload.projectId);
          if (!(await state("local-projects"))?.[id]) throw failure(404, "项目不存在");
          await manager.storage.updateGlobalState("pinned-project-ids", current => {
            const pins = array(current).filter(value => value !== id);
            return payload.pinned === true ? [...pins, id] : pins;
          });
          emit({ method: "remote/sidebarChanged" });
          return { ok: true };
        }
        case "pinThread": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          const pins = await manager.runtime.pinnedThreads();
          if (!pins) throw failure(503, "原生置顶服务不可用");
          const result = await pins.set({ hostId: "local", useAppServerPins: true, threadId: id, pinned: payload.pinned === true });
          if (result?.success === false) throw failure(409, "Codex 未能保存置顶状态");
          const current = await pins.list({ hostId: "local", useAppServerPins: true });
          manager.setPinnedThreadIds(array(current.threadIds));
          emit({ method: "remote/sidebarChanged" });
          return { ok: true };
        }
        case "sort": {
          if (!["manual", "updated_at"].includes(payload.mode)) throw failure(400, "不支持的排序方式");
          if (payload.target === "pinned") manager.storage.writeValue("pinned-sidebar-sort-mode-v1", payload.mode);
          else {
            const current = manager.storage.readValue("flat-project-sidebar-preferences-v1") || {};
            if (!["chats", "projects", "grouping"].includes(payload.target)) throw failure(400, "排序对象无效");
            manager.storage.writeValue("flat-project-sidebar-preferences-v1", {
              ...current, initialized: true, manualSortVersion: 1,
              ...(payload.target === "grouping" ? { mode: payload.grouping === "list" ? "list" : "project" }
                : { [payload.target === "projects" ? "projectSortMode" : "chatSortMode"]: payload.mode })
            });
            manager.storage.writeValue("codex-sidebar-sort-mode-v1", undefined);
          }
          emit({ method: "remote/sidebarChanged" });
          return { ok: true };
        }
        case "rename":
          await readThread(payload.threadId);
          if (!payload.title?.trim() || payload.title.length > 200) throw failure(400, "标题需为 1–200 个字符");
          await manager.setThreadTitle(payload.threadId, payload.title.trim());
          return { ok: true };
        case "approve":
        case "answer": {
          const id = requiredId(payload.threadId);
          const request = array(manager.getConversation(id)?.requests).find(request =>
            request.id === payload.requestId && request.method === payload.method);
          if (!request) throw failure(409, "此审批或询问已结束，请刷新会话");
          const decisions = ["accept", "acceptForSession", "decline", "cancel"];
          if (action === "approve") {
            if (!decisions.includes(payload.decision)) throw failure(400, "审批决定无效");
            const available = request.params?.availableDecisions;
            if (Array.isArray(available) && !available.includes(payload.decision)) throw failure(400, "此请求不允许该审批决定，请使用原生允许的选项");
            if (request.method === "item/commandExecution/requestApproval")
              await manager.replyWithCommandExecutionApprovalDecision(id, request.id, payload.decision);
            else if (request.method === "item/fileChange/requestApproval")
              await manager.replyWithFileChangeApprovalDecision(id, request.id, payload.decision);
            else throw failure(400, "此请求需使用专用回答表单");
          } else {
            if (request.method === "item/tool/requestUserInput") {
              const answers = payload.response?.answers;
              if (!answers || typeof answers !== "object") throw failure(400, "询问答案无效");
              const questions = array(request.params?.questions);
              if (Object.keys(answers).some(id => !questions.some(question => question.id === id))) throw failure(400, "回答包含未请求的问题");
              for (const question of questions) {
                const values = answers[question.id]?.answers;
                if (!Array.isArray(values) || !values.every(value => typeof value === "string")) throw failure(400, "请使用文本回答所有问题");
              }
              await manager.replyWithUserInputResponse(id, request.id, { answers });
            } else if (request.method === "item/permissions/requestApproval") {
              if (!["turn", "session"].includes(payload.response?.scope)) throw failure(400, "权限有效期无效");
              const requested = request.params?.permissions || {};
              if (payload.response?.grant !== true && payload.response?.grant !== false) throw failure(400, "权限决定无效");
              await manager.replyWithPermissionsRequestApprovalResponse(id, request.id, {
                permissions: payload.response.grant ? requested : {}, scope: payload.response.scope
              });
            } else if (request.method === "mcpServer/elicitation/request") {
              if (!["form", "openai/form"].includes(request.params?.mode)) throw failure(400, "此类询问请在 Codex 桌面处理");
              if (!["accept", "decline", "cancel"].includes(payload.response?.action)) throw failure(400, "回答无效");
              await manager.replyWithMcpServerElicitationResponse(id, request.id, payload.response);
            } else throw failure(400, "此类询问请在 Codex 桌面处理");
          }
          return { ok: true };
        }
        default: throw failure(404, "不支持的原生会话操作");
      }
    } catch (error) {
      if (["send", "create", "editMessage", "updateGoal"].includes(action) && !error.remoteValidationFailure) {
        return { ok: false, outcomeUnknown: true, code: 503, message: action === "updateGoal"
          ? "原生目标更新未确认完成，请检查会话，不要重复保存"
          : "原生操作未确认完成，请检查会话，不要重复发送或新建" };
      }
      return { ok: false, code: error.code || 503, message: error.message || "原生会话操作失败" };
    }
  }
  window.__codexElvesRemoteAccess = {
    version: 22, call, removeQueued, reorderQueued, beginQueuedEdit, editQueued, cancelQueuedEdit, queueMessage,
    dispose() {
      for (const dispose of unsubscribe) { try { dispose?.(); } catch {} }
      unsubscribe = [];
      if (window.__codexElvesRemoteAccess?.epoch === epoch) delete window.__codexElvesRemoteAccess;
    },
    epoch
  };
})();
