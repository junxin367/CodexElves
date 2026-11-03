(() => {
  if (window.__codexElvesRemoteAccess?.version === 128) return;
  window.__codexElvesRemoteAccess?.dispose?.();
  let manager = null;
  let appScope = null;
  let nativeIntl = null;
  let nativeNavigate = null;
  let unsubscribe = [];
  let sidebarUnsubscribe = [];
  const sidebarListeners = new Set();
  let eventSequence = 0;
  let queueSequence = 0;
  let requestsSequence = 0;
  const epoch = crypto.randomUUID();
  const notifications = [];
  let notificationEpoch = epoch;
  let notificationManager = null;
  let notificationSequence = 0;
  const notificationEvents = [];
  const pendingNotifications = new Map();
  const completedNotificationTurns = new Set();
  const queuedReorders = new Set();
  const queuedSendSubmissions = new Map();
  const queuedEditAcceptanceProofs = new WeakMap();
  const queuedAcceptanceByManager = new WeakMap();
  const asyncQuestionSignatures = new Map();
  const blockingRequestSnapshots = new Map();
  const observedRequestThreads = new Set();
  const asyncQuestionStateModules = new WeakMap();
  const asyncQuestionRecoveries = new Map();
  let asyncQuestionNativeBindings = null;
  const sidebarSignatures = new Map();
  const deletedThreadIds = new Set();
  const pendingThreadMetadataReads = new Map();
  const nativeCallbackFailureCounts = new Map();
  const remoteImageDiagnosticCounts = new Map();
  const nativeCompatibilityDiagnosticCounts = new Map();
  const INITIAL_THREAD_METADATA_WINDOW_MS = 15_000;
  const INITIAL_THREAD_METADATA_RETRY_DELAYS = [100, 200, 400, 800, 1000];
  let goalMaterializer = null;
  const nativeModuleSnapshots = new Map();
  const nativeResourceSources = new Map();
  let relatedNativeState = null;
  let nativeSideBindings = null;
  let requestClientConversationReader = null;
  let requestClientGlobalStateReader = null;
  let requestClientQueuePrepare = null;
  const nativeQueuedEditInputContracts = new WeakMap();
  let requestClientEventSubscription = null;
  const requestClientQueues = new Map();
  const requestClientConversations = new Map();
  const requestClientSubagentStates = new Map();
  let sideThreadsManager = null;
  const nativeSideThreads = new Map();
  const sideThreadTitleWrites = new Map();
  const sideScopeWatchers = new Map();
  const requestClientSideThreads = new Map();
  const goalSubmissions = new Map();
  const isDeletedThread = id => typeof id === "string" && deletedThreadIds.has(id.toLowerCase());
  const questionReplyStore = (() => {
    const existing = window.__codexElvesRemoteQuestionReplies;
    if (existing instanceof Map) return existing;
    const created = new Map();
    window.__codexElvesRemoteQuestionReplies = created;
    return created;
  })();
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
  const isRequestClient = value => value?.hostId === "local"
    && typeof value.sendRequest === "function"
    && typeof value.sendAppServerExtensionRequest === "function"
    && typeof value.prewarmThreadStart === "function";
  const isManager = value => value?.hostId === "local"
    && (value.__codexElvesRequestClientManager === true
      || typeof value.sendFollowUpMessage === "function"
        && typeof value.listThreadTurns === "function" && value.storage?.readGlobalState);
  let requestClientSidebarSnapshot = null;
  const requestClientSnapshotArray = value => {
    if (Array.isArray(value)) return value;
    if (value instanceof Set) return [...value];
    if (value && typeof value !== "string" && typeof value[Symbol.iterator] === "function") {
      try { return [...value]; } catch {}
    }
    return [];
  };
  const mergeRequestClientSnapshotMap = (previous, next) => {
    if (!(next instanceof Map)) return previous;
    const merged = new Map(previous instanceof Map ? previous : []);
    for (const [key, value] of next) {
      const current = merged.get(key);
      if (typeof current === "function" && typeof value === "function") {
        try {
          if (array(current()).length >= array(value()).length) continue;
        } catch {}
      }
      merged.set(key, value);
    }
    return merged;
  };
  const requestClientSnapshotThreadId = (value, snapshot = requestClientSidebarSnapshot) => {
    if (value && typeof value === "object") {
      const id = value.conversationId ?? value.threadId ?? value.id;
      if (typeof id === "string" && id) return id;
    }
    if (typeof value !== "string" || !value) return null;
    if (value.startsWith("codex:thread:local:")) return value.slice("codex:thread:local:".length);
    if (value.startsWith("local:")) return value.slice("local:".length);
    const candidates = [];
    const addCandidate = candidate => {
      if (typeof candidate !== "string" || !candidate || candidates.includes(candidate)) return;
      candidates.push(candidate);
      if (candidate.startsWith("local-")) addCandidate(candidate.slice("local-".length));
    };
    addCandidate(value);
    if (value.includes(":")) addCandidate(value.slice(value.lastIndexOf(":") + 1));
    for (const candidate of candidates)
      if (snapshot?.conversationByKey instanceof Map && snapshot.conversationByKey.has(candidate))
        return candidate;
    return candidates.find(candidate => /^[a-zA-Z0-9_-]{1,120}$/.test(candidate)) || null;
  };
  const requestClientSnapshotConversations = (snapshot = requestClientSidebarSnapshot) => {
    if (!(snapshot?.conversationByKey instanceof Map)) return [];
    const rows = [];
    for (const [key, value] of snapshot.conversationByKey) {
      const id = requestClientSnapshotThreadId(value, snapshot) || requestClientSnapshotThreadId(key, snapshot);
      if (!id || !value || typeof value !== "object") continue;
      // 新版索引只保存来源和 threadKey；它不能作为摘要覆盖 thread/list 的真实状态。
      if (value.source === "codex" && typeof value.threadKey === "string"
          && Object.keys(value).every(name => ["source", "threadKey", "id", "conversationId", "threadId"].includes(name)))
        continue;
      rows.push({ ...value, id: value.id || id, conversationId: value.conversationId || id });
    }
    return rows;
  };
  const requestClientSnapshotVisibleIds = (name, snapshot = requestClientSidebarSnapshot) => {
    const lists = snapshot?.visibleThreadLists instanceof Map ? snapshot.visibleThreadLists : new Map();
    const read = source => {
      try { return typeof source === "function" ? source() : source; } catch { return []; }
    };
    let values = read(lists.get(name));
    if (!requestClientSnapshotArray(values).length && name === "pinned") {
      values = [...lists.entries()]
        .filter(([key]) => String(key).startsWith("codex:pinned:"))
        .flatMap(([, source]) => requestClientSnapshotArray(read(source)));
    }
    return requestClientSnapshotArray(values).filter(value => !(typeof value === "string"
      && (value.startsWith("codex:project:") || value.startsWith("codex:unified-project:"))))
      .map(value => requestClientSnapshotThreadId(value, snapshot)).filter(Boolean);
  };
  const requestClientSnapshotState = (key, snapshot = requestClientSidebarSnapshot) => {
    if (key === "local-projects") {
      const result = {};
      for (const [projectKey, value] of snapshot?.projectByKey instanceof Map ? snapshot.projectByKey : []) {
        const group = value?.group || value;
        const id = group?.projectId || String(projectKey).slice(String(projectKey).lastIndexOf(":") + 1);
        if (!id || group?.projectKind && group.projectKind !== "local") continue;
        result[id] = {
          name: group.label || group.name || group.path?.split(/[\\/]/).at(-1) || id,
          rootPaths: requestClientSnapshotArray(group.rootPaths || (group.path ? [group.path] : [])),
          threadKeys: requestClientSnapshotArray(group.threadKeys)
        };
      }
      return result;
    }
    if (key === "thread-project-assignments") {
      const assignments = {};
      for (const [projectKey, value] of snapshot?.projectByKey instanceof Map ? snapshot.projectByKey : []) {
        const group = value?.group || value;
        const projectId = group?.projectId || String(projectKey).slice(String(projectKey).lastIndexOf(":") + 1);
        if (!projectId || group?.projectKind && group.projectKind !== "local") continue;
        for (const thread of requestClientSnapshotArray(group.threadKeys)) {
          const id = requestClientSnapshotThreadId(thread, snapshot);
          if (id) assignments[id] = projectId;
        }
      }
      return assignments;
    }
    if (key === "pinned-thread-ids") return requestClientSnapshotVisibleIds("pinned", snapshot);
    if (key === "projectless-thread-ids") {
      const assigned = new Set(Object.keys(requestClientSnapshotState("thread-project-assignments", snapshot)));
      return requestClientSnapshotVisibleIds("chats", snapshot).filter(id => !assigned.has(id));
    }
    return {};
  };
  function minimalManagerStorage() {
    const existing = window.__codexElvesRemoteRequestClientStorage;
    const values = existing instanceof Map ? existing : new Map();
    window.__codexElvesRemoteRequestClientStorage = values;
    return {
      async readGlobalState(key) {
        try {
          if (key === "pinned-thread-ids") {
            const runtime = await requestClientRuntime();
            return (await runtime.pinnedThreads.list({ hostId: "local", useAppServerPins: true })).threadIds;
          }
          return await requestClientGlobalState(key);
        } catch (error) {
          reportNativeCallbackFailure("request_client_global_state", error);
          return requestClientSnapshotState(key);
        }
      },
      async updateGlobalState(key, updater) {
        const { module } = await nativeModuleSnapshot("initial");
        const update = nativeFunction(module, source => source.includes("Failed to persist global state")
          && source.includes(".pending??Promise.resolve()"), "应用状态保存");
        return await update(appScope, key, updater);
      },
      readValue(key) { return values.has(key) ? values.get(key) : null; },
      writeValue(key, value) {
        if (value === undefined) values.delete(key);
        else values.set(key, value);
        return true;
      },
      readQueuedFollowUps() { return null; }
    };
  }
  function requestClientManager(requestClient) {
    const storage = minimalManagerStorage();
    const adapter = {
      __codexElvesRequestClientManager: true,
      hostId: "local",
      requestClient,
      storage,
      runtime: {
        pinnedThreads: async () => (await requestClientRuntime()).pinnedThreads
      },
      disposed: false,
      getConversation(threadId) {
        const loaded = requestClientConversations.get(threadId);
        if (loaded?.manager === this && loaded.value) return loaded.value;
        return requestClientSnapshotConversations().find(thread =>
          (thread.conversationId || thread.id) === threadId) || null;
      },
      getCachedConversations() {
        const conversations = new Map(requestClientSnapshotConversations().map(thread =>
          [thread.conversationId || thread.id, thread]));
        for (const [id, record] of requestClientConversations)
          if (record.manager === this && !record.disposed && record.value && !isDeletedThread(id))
            conversations.set(id, record.value);
        return [...conversations.values()];
      },
      getRecentThreadSummaries() {
        const ids = requestClientSnapshotVisibleIds("chats");
        return requestClientSnapshotConversations().filter(thread =>
          ids.includes(thread.conversationId || thread.id));
      },
      getThreadListSummary(threadId) {
        return this.getConversation(threadId);
      },
      getThreadHasUnreadTurn(threadId) {
        const value = this.getConversation(threadId)?.hasUnreadTurn;
        return typeof value === "boolean" ? value : undefined;
      },
      getThreadRuntimeStatusEvidence(threadId) {
        const thread = this.getConversation(threadId);
        return thread?.threadRuntimeStatus || thread?.status || null;
      },
      getConversationCwd(threadId) {
        const thread = this.getConversation(threadId);
        return thread?.cwd || thread?.workingDirectory || null;
      },
      async getTurnCoordinator() { return requestClientTurnCoordinator(this); },
      async resumeConversation(params) {
        return await (await requestClientNativeRpc()).resumeConversation(params);
      },
      async updateThreadSettingsForNextTurn(id, settings) {
        return await (await requestClientNativeRpc()).updateThreadSettingsForNextTurn(id, settings);
      },
      async readThread(threadId, options = {}) {
        const includeTurns = options.includeTurns === true;
        try {
          const response = await requestClient.sendRequest("thread/read", { threadId, includeTurns });
          return response?.thread || response;
        } catch (error) {
          if (!includeTurns || !String(error?.message || "").includes("ephemeral threads do not support includeTurns"))
            throw error;
          const response = await requestClient.sendRequest("thread/read", { threadId, includeTurns: false });
          const thread = response?.thread || response;
          const history = await requestClientSideHistory(threadId);
          return { ...thread, turns: history.turns };
        }
      }
    };
    // 新版把执行器移到了桌面 RPC。只映射已核对的原生方法，不把缺失能力伪装成只读会话。
    for (const method of [
      "editLastUserTurn", "startConversation", "stopThread", "cleanBackgroundTerminals", "compactThread",
      "setThreadTitle", "setPinnedThreadIds", "markConversationAsRead", "sendFollowUpMessage",
      "setThreadGoal", "setThreadGoalStatus", "clearThreadGoal", "createThreadGoalAttachmentDirectory",
      "writeThreadGoalAttachment", "removeThreadGoalAttachmentDirectory",
      "replyWithCommandExecutionApprovalDecision", "replyWithFileChangeApprovalDecision",
      "replyWithPermissionsRequestApprovalResponse", "replyWithUserInputResponse",
      "replyWithMcpServerElicitationResponse"
    ]) adapter[method] = async (...args) => {
      let rpc;
      try { rpc = await requestClientNativeRpc(); }
      catch (error) { throw failure(error.code || 503, error.message || "原生操作尚未就绪"); }
      if (manager !== adapter || adapter.disposed)
        throw failure(409, "原生会话连接已变化，请刷新后再操作");
      if (typeof rpc[method] !== "function")
        throw failure(503, "当前 Codex 版本的原生操作不可用，请更新桌面端");
      return await rpc[method](...args);
    };
    return adapter;
  }
  function reportNativeCallbackFailure(callbackName, error) {
    try {
      const name = String(callbackName || "unknown").slice(0, 120);
      const count = (nativeCallbackFailureCounts.get(name) || 0) + 1;
      nativeCallbackFailureCounts.set(name, count);
      if (count > 20 && count % 20 !== 0) return;
      const payload = {
        event: "renderer.remote_native_callback_failed",
        detail: {
          callback: name,
          count,
          errorName: String(error?.name || "").slice(0, 120),
          errorMessage: String(error?.message || error || "").slice(0, 1000),
        },
        helperBase: window.__CODEX_SESSION_DELETE_HELPER__ || "",
        location: window.location?.href || "",
        userAgent: navigator.userAgent || "",
        timestamp: new Date().toISOString(),
      };
      const bridge = window.__codexSessionDeleteBridge;
      const fallback = () => {
        try {
          fetch(`${payload.helperBase || "http://127.0.0.1:45221"}/diagnostics/log`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
            keepalive: true,
          }).catch(() => {});
        } catch {}
      };
      if (typeof bridge === "function") {
        try {
          Promise.resolve(bridge("/diagnostics/log", payload)).catch(fallback);
          return;
        } catch {}
      }
      fallback();
    } catch {}
  }
  function safeNativeCallback(callbackName, callback) {
    return (...args) => {
      try {
        const result = callback(...args);
        if (result && typeof result.then === "function")
          return Promise.resolve(result).catch(error => {
            reportNativeCallbackFailure(callbackName, error);
            return undefined;
          });
        return result;
      } catch (error) {
        reportNativeCallbackFailure(callbackName, error);
        return undefined;
      }
    };
  }
  function reportRemoteImageDiagnostic(eventName, detail = {}) {
    try {
      const name = String(eventName || "remote_image_diagnostic").slice(0, 120);
      const count = (remoteImageDiagnosticCounts.get(name) || 0) + 1;
      remoteImageDiagnosticCounts.set(name, count);
      if (count > 20 && count % 20 !== 0) return;
      const payload = {
        event: name,
        detail: {
          ...detail,
          count,
        },
        helperBase: window.__CODEX_SESSION_DELETE_HELPER__ || "",
        location: window.location?.href || "",
        userAgent: navigator.userAgent || "",
        timestamp: new Date().toISOString(),
      };
      const bridge = window.__codexSessionDeleteBridge;
      const fallback = () => {
        try {
          fetch(`${payload.helperBase || "http://127.0.0.1:45221"}/diagnostics/log`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
            keepalive: true,
          }).catch(() => {});
        } catch {}
      };
      if (typeof bridge === "function") {
        try {
          Promise.resolve(bridge("/diagnostics/log", payload)).catch(fallback);
          return;
        } catch {}
      }
      fallback();
    } catch {}
  }
  function reportNativeCompatibilityDiagnostic(action, error, detail = {}) {
    try {
      const message = String(error?.message || error || "").slice(0, 1000);
      if (!/当前 Codex 版本的原生|原生关联会话/.test(message)) return;
      const name = String(action || "unknown").slice(0, 120);
      const key = `${name}:${message}`;
      const count = (nativeCompatibilityDiagnosticCounts.get(key) || 0) + 1;
      nativeCompatibilityDiagnosticCounts.set(key, count);
      if (count > 20 && count % 20 !== 0) return;
      const payload = {
        event: "renderer.remote_native_compatibility_failed",
        detail: {
          action: name,
          ...detail,
          count,
          errorName: String(error?.name || "").slice(0, 120),
          errorMessage: message,
        },
        helperBase: window.__CODEX_SESSION_DELETE_HELPER__ || "",
        location: window.location?.href || "",
        userAgent: navigator.userAgent || "",
        timestamp: new Date().toISOString(),
      };
      const bridge = window.__codexSessionDeleteBridge;
      const fallback = () => {
        try {
          fetch(`${payload.helperBase || "http://127.0.0.1:45221"}/diagnostics/log`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
            keepalive: true,
          }).catch(() => {});
        } catch {}
      };
      if (typeof bridge === "function") {
        try {
          Promise.resolve(bridge("/diagnostics/log", payload)).catch(fallback);
          return;
        } catch {}
      }
      fallback();
    } catch {}
  }
  function reportOptionalFeatureDegraded(eventName, action, error, detail = {}) {
    try {
      const name = String(action || "unknown").slice(0, 120);
      const message = String(error?.message || error || "").slice(0, 1000);
      const key = `${eventName}:${name}:${message}`;
      const count = (nativeCompatibilityDiagnosticCounts.get(key) || 0) + 1;
      nativeCompatibilityDiagnosticCounts.set(key, count);
      if (count > 20 && count % 20 !== 0) return;
      const payload = {
        event: eventName,
        detail: {
          action: name,
          ...detail,
          degraded: true,
          count,
          errorName: String(error?.name || "").slice(0, 120),
          errorMessage: message,
        },
        helperBase: window.__CODEX_SESSION_DELETE_HELPER__ || "",
        location: window.location?.href || "",
        userAgent: navigator.userAgent || "",
        timestamp: new Date().toISOString(),
      };
      const bridge = window.__codexSessionDeleteBridge;
      const fallback = () => {
        try {
          fetch(`${payload.helperBase || "http://127.0.0.1:45221"}/diagnostics/log`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
            keepalive: true,
          }).catch(() => {});
        } catch {}
      };
      if (typeof bridge === "function") {
        try {
          Promise.resolve(bridge("/diagnostics/log", payload)).catch(fallback);
          return;
        } catch {}
      }
      fallback();
    } catch {}
  }
  function reportSideFeatureDegraded(action, error, detail = {}) {
    reportOptionalFeatureDegraded("renderer.remote_side_feature_degraded", action, error, detail);
  }
  function reportSidebarFeatureDegraded(action, error, detail = {}) {
    reportOptionalFeatureDegraded("renderer.remote_sidebar_feature_degraded", action, error, detail);
  }
  function nativeImageAttachmentId() {
    return `remote-image:${crypto.randomUUID()}`;
  }
  function validateNativeImageAttachments(images, phase) {
    const invalid = array(images).flatMap((attachment, index) =>
      typeof attachment?.id === "string" && attachment.id
        ? []
        : [{ index, keys: attachment && typeof attachment === "object" ? Object.keys(attachment) : [] }]);
    if (!invalid.length) return;
    reportRemoteImageDiagnostic("remote_image_attachment_invalid", {
      phase,
      invalidCount: invalid.length,
      firstInvalidIndex: invalid[0].index,
      firstInvalidKeys: invalid[0].keys.slice(0, 16),
    });
    throw failure(500, "原生图片附件上下文缺少附件 ID");
  }
  function installRemoteImageErrorDiagnostics() {
    const previous = window.__codexElvesRemoteImageErrorDiagnostics;
    previous?.dispose?.();
    const onError = event => {
      const error = event?.error;
      const message = String(error?.message || event?.message || "");
      const stack = String(error?.stack || "");
      if (!/startsWith/.test(message) || !/(app-primary|imageAttachments)/i.test(`${message}\n${stack}`))
        return;
      reportRemoteImageDiagnostic("remote_image_attachment_render_failed", {
        errorType: String(error?.name || "Error").slice(0, 120),
        errorMessage: message.slice(0, 400),
        stackPreview: stack.slice(0, 800),
      });
    };
    window.addEventListener("error", onError);
    window.__codexElvesRemoteImageErrorDiagnostics = {
      dispose() {
        window.removeEventListener("error", onError);
        if (window.__codexElvesRemoteImageErrorDiagnostics?.dispose === this.dispose)
          delete window.__codexElvesRemoteImageErrorDiagnostics;
      },
    };
  }
  installRemoteImageErrorDiagnostics();
  function resetStaleManager() {
    const existing = window.__codexElvesConversationStateManager;
    if (!manager || !manager.disposed && !(isManager(existing) && !existing.disposed && existing !== manager)) return;
    for (const dispose of [...unsubscribe, ...sidebarUnsubscribe]) { try { dispose?.(); } catch {} }
    unsubscribe = []; sidebarUnsubscribe = [];
    for (const watcher of sideScopeWatchers.values()) watcher.dispose?.();
    sideScopeWatchers.clear(); nativeSideThreads.clear(); pendingThreadMetadataReads.clear();
    requestClientEventSubscription?.dispose();
    requestClientEventSubscription = null;
    clearRequestClientSubagentStates();
    for (const record of requestClientQueues.values()) disposeRequestClientQueue(record);
    requestClientQueues.clear();
    requestClientSideThreads.clear();
    for (const record of requestClientConversations.values()) disposeRequestClientConversation(record);
    requestClientConversations.clear();
    sideThreadTitleWrites.clear();
    manager = null; appScope = null; nativeNavigate = null; nativeIntl = null;
    nativeCatalog.clear(); sidebarSignatures.clear(); catalogLoaded = false;
    catalogLoading = null; catalogGeneration++;
    emit({ method: "remote/resync" });
  }
  const pushNotificationUnsafe = event => {
    if (event.method === "thread/deleted") forgetDeletedThread(event.params?.threadId);
    else if (event.method !== "remote/sidebarChanged" && isDeletedThread(event.params?.threadId)) return;
    if (["thread/archived", "thread/deleted"].includes(event.method)) {
      nativeCatalog.delete(event.params?.threadId);
      catalogLoaded = false;
    }
    const displayItem = nativeDisplayMessageItem(event.params?.item);
    const displayTurnItems = Array.isArray(event.params?.turn?.items)
      ? event.params.turn.items.map(nativeDisplayMessageItem) : null;
    const turnChanged = displayTurnItems
      && displayTurnItems.some((item, index) => item !== event.params.turn.items[index]);
    if (displayItem !== event.params?.item || turnChanged) {
      event = {
        ...event,
        params: {
          ...event.params,
          ...(displayItem !== event.params?.item ? { item: displayItem } : {}),
          ...(turnChanged ? { turn: { ...event.params.turn, items: displayTurnItems } } : {})
        }
      };
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
  const pushNotification = event => {
    try {
      return pushNotificationUnsafe(event);
    } catch (error) {
      reportNativeCallbackFailure("notification_delivery", error);
      try {
        notifications.push({ sequence: ++eventSequence, event });
        if (notifications.length > 2048) notifications.splice(0, notifications.length - 2048);
      } catch {}
      return false;
    }
  };
  const emitUnsafe = event => {
    if (["remote/sidebarChanged", "remote/requestsChanged", "remote/relatedThreadsChanged", "remote/resync",
      "remote/reconnected", "thread/started", "thread/status/changed", "thread/name/updated",
      "thread/archived", "thread/unarchived", "thread/deleted", "turn/started", "turn/completed",
      "remote/asyncQuestionsChanged"].includes(event.method)) {
      for (const listener of sidebarListeners) {
        try { listener({ method: event.method, threadId: event.params?.threadId, status: event.params?.status }); } catch {}
      }
    }
    observeNotificationEvent(event);
    if (event.method === "serverRequest/resolved")
      event = { ...event, params: { ...event.params, requestsEpoch: epoch } };
    if (event.method === "thread/queue/changed") {
      void queueSnapshot(event.params?.threadId).then(snapshot => {
        pushNotification({
          ...event,
          params: {
            ...event.params,
            queuedMessages: snapshot.queuedMessages,
            queuedEdits: snapshot.queuedEdits,
            queueEpoch: snapshot.queueEpoch,
            queueRevision: snapshot.queueRevision
          }
        });
      }).catch(() => pushNotification(event));
      return;
    }
    pushNotification(event);
  };
  const emit = event => {
    try {
      return emitUnsafe(event);
    } catch (error) {
      reportNativeCallbackFailure("notification_emit", error);
      return pushNotification(event);
    }
  };
  function discover(sidebarOnly = false, quick = false) {
    const existing = window.__codexElvesConversationStateManager;
    if (isManager(existing) && !existing.disposed) manager = existing;
    const cached = window.__codexElvesNativeConversationContext;
    if (cached?.manager === manager && !manager?.disposed && cached.appScope && cached.nativeNavigate
        && (sidebarOnly || typeof cached.nativeIntl?.formatMessage === "function")) {
      ({ appScope, nativeIntl, nativeNavigate } = cached);
    } else if (!(sidebarOnly && quick && isManager(manager) && !manager.disposed)) {
      const scopes = new Set(), fibers = new Set(), values = new WeakSet();
      const queue = [];
      let requestClient = manager?.requestClient && isRequestClient(manager.requestClient)
        ? manager.requestClient : null;
      const scan = elements => {
        for (const element of elements) {
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
              if (typeof context?.formatMessage === "function") nativeIntl = context;
              if (typeof context?.intl?.formatMessage === "function") nativeIntl = context.intl;
              if (typeof context?.router?.navigate === "function")
                nativeNavigate = path => context.router.navigate(path);
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
                      if (!requestClient && isRequestClient(candidate)) requestClient = candidate;
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
          if (!requestClient && isRequestClient(value)) requestClient = value;
          if (value.projectByKey instanceof Map
              || value.conversationByKey instanceof Map
              || value.visibleThreadLists instanceof Map) {
            requestClientSidebarSnapshot = {
              ...(requestClientSidebarSnapshot || {}),
              ...(value.projectByKey instanceof Map ? {
                projectByKey: mergeRequestClientSnapshotMap(
                  requestClientSidebarSnapshot?.projectByKey, value.projectByKey)
              } : {}),
              ...(value.conversationByKey instanceof Map ? {
                conversationByKey: mergeRequestClientSnapshotMap(
                  requestClientSidebarSnapshot?.conversationByKey, value.conversationByKey)
              } : {}),
              ...(value.visibleThreadLists instanceof Map ? {
                visibleThreadLists: mergeRequestClientSnapshotMap(
                  requestClientSidebarSnapshot?.visibleThreadLists, value.visibleThreadLists)
              } : {})
            };
          }
          if (depth > 7 || value instanceof Node || value === window || value === document) continue;
          if (value instanceof Map) {
            for (const entry of value.values()) queue.push([entry, depth + 1]);
          } else {
            for (const descriptor of Object.values(Object.getOwnPropertyDescriptors(value))) {
              if ("value" in descriptor) queue.push([descriptor.value, depth + 1]);
            }
          }
        }
      };
      if (sidebarOnly) scan(document.querySelectorAll(
        "#root, [data-above-composer-conversation-id], nav[role='navigation'], [data-app-shell-titlebar]"
      ));
      if (!sidebarOnly || !manager || !appScope || !nativeNavigate)
        scan(document.querySelectorAll("#root, #root *"));
      if (!manager && requestClient) manager = requestClientManager(requestClient);
      if (!manager) throw failure(503, "未发现本机原生会话管理器，请检查 CodexElves 注入连接",
        { reason: "native-manager-unavailable" });
      if (!manager.__codexElvesRequestClientManager)
        window.__codexElvesNativeConversationContext = { manager, appScope, nativeIntl, nativeNavigate };
    }
    if (sideThreadsManager !== manager) {
      clearRequestClientSubagentStates();
      for (const watcher of sideScopeWatchers.values()) watcher.dispose?.();
      sideScopeWatchers.clear();
      sideThreadsManager = manager;
      nativeSideThreads.clear();
      requestClientSideThreads.clear();
      sideThreadTitleWrites.clear();
    }
    if (notificationManager !== manager) {
      if (notificationManager) {
        notificationEpoch = crypto.randomUUID();
        notificationSequence = 0;
        notificationEvents.length = 0;
        pendingNotifications.clear();
        completedNotificationTurns.clear();
      }
      notificationManager = manager;
    }
    if (sidebarOnly && !manager.__codexElvesRequestClientManager
        && !unsubscribe.length && !sidebarUnsubscribe.length) {
      sidebarUnsubscribe.push(manager.addNotificationCallback([
        "thread/started", "thread/status/changed", "thread/name/updated",
        "thread/archived", "thread/unarchived", "thread/deleted", "turn/started", "turn/completed"
      ], safeNativeCallback("sidebar_notification", event => {
        if (["thread/archived", "thread/deleted"].includes(event.method))
          nativeCatalog.delete(event.params?.threadId);
        if (event.method === "thread/deleted") forgetDeletedThread(event.params?.threadId);
        if (event.method === "thread/started") catalogLoaded = false;
        emit(event);
      })));
      if (typeof manager.addConversationStateCallback === "function")
        sidebarUnsubscribe.push(manager.addConversationStateCallback(
          safeNativeCallback("sidebar_conversation_state", syncSidebarConversationState)
        ));
    }
    if (!sidebarOnly && !manager.__codexElvesRequestClientManager && !unsubscribe.length) {
      for (const dispose of sidebarUnsubscribe) { try { dispose?.(); } catch {} }
      sidebarUnsubscribe = [];
      unsubscribe.push(manager.addNotificationCallback(
        methods, safeNativeCallback("notification", emit)
      ));
      unsubscribe.push(manager.addApprovalRequestListener(
        safeNativeCallback("approval_request", syncRequestNotification)
      ));
      unsubscribe.push(manager.addApprovalResolvedListener(
        safeNativeCallback("approval_resolved", syncRequestNotification)
      ));
      unsubscribe.push(manager.addUserInputRequestListener(
        safeNativeCallback("user_input_request", syncRequestNotification)
      ));
      if (typeof manager.addConversationStateCallback === "function")
        unsubscribe.push(manager.addConversationStateCallback(safeNativeCallback(
          "conversation_state",
          (id, conversation) => {
            syncPendingNotifications(id, conversation);
            if (observedRequestThreads.has(id)) syncBlockingRequests(id, conversation);
            if (asyncQuestionSignatures.has(id)) syncAsyncQuestions(id, conversation);
            syncSidebarConversationState(id, conversation);
          }
        )));
      refreshPendingNotifications();
    }
    if (manager.__codexElvesRequestClientManager) ensureRequestClientEvents();
    return manager;
  }
  function blockingRequestsSnapshot(id, conversation = manager.getConversation(id)) {
    observedRequestThreads.add(id);
    if (!conversation || !Array.isArray(conversation.requests)) return null;
    const signature = JSON.stringify(conversation.requests
      .filter(request => request.method !== "remote/requestUserInputAsync")
      .map(request => ({ id: request.id, method: request.method, params: request.params })));
    let snapshot = blockingRequestSnapshots.get(id);
    if (!snapshot || snapshot.signature !== signature) {
      snapshot = { signature, threadId: id, requests: JSON.parse(signature),
        requestsEpoch: epoch, requestsRevision: ++requestsSequence };
      blockingRequestSnapshots.set(id, snapshot);
    }
    const { signature: _, ...fields } = snapshot;
    return fields;
  }
  function syncBlockingRequests(id, conversation, identity = null) {
    const previous = blockingRequestSnapshots.get(id);
    const snapshot = blockingRequestsSnapshot(id, conversation);
    if (snapshot && (previous !== blockingRequestSnapshots.get(id) || identity))
      emit({ method: "remote/requestsChanged", params: { ...identity, ...snapshot } });
    return snapshot;
  }
  function syncRequestNotification(payload) {
    const threadId = payload?.threadId ?? payload?.conversationId;
    const requestId = payload?.requestId;
    const sourceManager = manager;
    // Owner callbacks precede removeConversationRequest; inspect the settled native state.
    queueMicrotask(() => {
      try {
        if (manager !== sourceManager || sourceManager.disposed) return;
        if (typeof threadId !== "string" || !threadId) {
          emit({ method: "remote/requestsChanged" }); return;
        }
        const identity = { threadId,
          ...(["string", "number"].includes(typeof requestId) ? { requestId } : {}) };
        syncPendingNotifications(threadId, sourceManager.getConversation(threadId));
        if (!syncBlockingRequests(threadId, sourceManager.getConversation(threadId), identity))
          emit({ method: "remote/requestsChanged", params: identity });
      } catch (error) {
        reportNativeCallbackFailure("request_notification_microtask", error);
      }
    });
  }
  function failure(code, message, details = {}) {
    return Object.assign(new Error(message), { code, remoteValidationFailure: true, ...details });
  }
  const EXECUTION_CONFIG_RETRY_DELAYS_MS = [100, 250, 500, 1000, 2000, 3000];
  function isExecutionConfigLoading(value) {
    if (value?.status === "not-ready" && value.reason === "execution-config-loading") return true;
    if (value?.result?.status === "not-ready" && value.result.reason === "execution-config-loading") return true;
    return value instanceof Error && value.message === "App-server task is not ready: execution-config-loading";
  }
  function executionConfigLoadingFailure() {
    return failure(409, "桌面正在加载执行配置，请稍后重试");
  }
  async function retryExecutionConfigLoading(operation, assertCurrent = null) {
    for (let attempt = 0; ; attempt++) {
      assertCurrent?.();
      let result;
      let loading = false;
      try {
        result = await operation();
        loading = isExecutionConfigLoading(result);
      } catch (error) {
        if (!isExecutionConfigLoading(error)) throw error;
        loading = true;
      }
      if (!loading) return result;
      if (attempt >= EXECUTION_CONFIG_RETRY_DELAYS_MS.length)
        throw executionConfigLoadingFailure();
      await new Promise(resolve => setTimeout(resolve, EXECUTION_CONFIG_RETRY_DELAYS_MS[attempt]));
    }
  }
  function requiredId(value) {
    if (typeof value !== "string" || !/^[a-zA-Z0-9_-]{1,120}$/.test(value)) throw failure(400, "会话 ID 无效");
    return value;
  }
  const state = async key => await manager.storage.readGlobalState(key);
  const array = value => Array.isArray(value) ? value : [];
  const GENERIC_MAIN_THREAD_TITLES = new Set(["新会话", "new chat", "new conversation"]);
  function cleanThreadTitle(value) {
    const title = String(value || "").replace(/[\u0000-\u001f\u007f-\u009f]/g, "").trim();
    return Array.from(title).slice(0, 160).join("");
  }
  function isGenericMainThreadTitle(value) {
    return GENERIC_MAIN_THREAD_TITLES.has(String(value || "").trim().toLowerCase());
  }
  function mainThreadTitle(...sources) {
    const titles = sources.map(cleanThreadTitle).filter(Boolean);
    return titles.find(title => !isGenericMainThreadTitle(title)) || titles[0] || "新会话";
  }
  function notificationThread(id, conversation = manager.getConversation(id)) {
    if (typeof id !== "string" || !/^[a-zA-Z0-9_-]{1,120}$/.test(id) || isDeletedThread(id)) return null;
    const current = manager.getThreadListSummary?.(id);
    const cached = nativeCatalog.get(id);
    const sources = [conversation, current, nativeCatalog.get(id)].filter(Boolean);
    if (!sources.length || sources.some(thread => thread.threadSource === "subagent"
        || thread.source === "subagent" || thread.source?.subAgent || thread.source?.subagent)) return null;
    const title = conversation?.sideConversation === true
      ? sideThreadTitle(conversation, nativeSideThreads.get(id)?.title
        || current?.title || current?.name || cached?.title)
      : mainThreadTitle(current?.title, current?.name, cached?.title, cached?.name,
        conversation?.title, conversation?.name);
    return { threadId: id, title };
  }
  function appendNotification(action, kind, thread, notificationId = null) {
    const sequence = ++notificationSequence;
    const id = `${notificationEpoch}:${sequence}`;
    const event = { epoch: notificationEpoch, sequence, id, action,
      notificationId: notificationId ?? id, kind, ...thread, createdAtMs: Date.now() };
    notificationEvents.push(event);
    if (notificationEvents.length > 256) notificationEvents.shift();
    pushNotification({ method: "remote/notification", params: event });
    return event;
  }
  function notificationMcpApproval(params = {}) {
    const record = value => value && typeof value === "object" && !Array.isArray(value);
    const metadata = params._meta;
    if (params.mode !== "form" || !record(metadata)
        || ["browser_auth", "browser_email_otp", "tool_suggestion"].includes(metadata.codex_approval_kind)) return false;
    if (metadata.codex_approval_kind === "mcp_tool_call") return true;
    const persist = metadata.persist === undefined ? []
      : Array.isArray(metadata.persist) ? metadata.persist : [metadata.persist];
    const validPersist = (metadata.persist === undefined || !Array.isArray(metadata.persist)
      || metadata.persist.length >= 1 && metadata.persist.length <= 2)
      && persist.every(value => ["session", "always"].includes(value));
    return ["browser", "browser-use"].includes(params.serverName)
      && record(params.requestedSchema?.properties) && Object.keys(params.requestedSchema.properties).length === 0
      && metadata.persist !== undefined && validPersist
      && typeof metadata.origin === "string" && !!metadata.origin.trim();
  }
  function notificationRequestKind(request) {
    const method = request?.method;
    if (["item/commandExecution/requestApproval", "item/fileChange/requestApproval",
      "item/permissions/requestApproval", "execCommandApproval", "applyPatchApproval"].includes(method)) return "approval";
    if (method === "mcpServer/elicitation/request") return notificationMcpApproval(request.params) ? "approval" : "question";
    if (["item/tool/requestUserInput", "remote/requestUserInputAsync"].includes(method)) return "question";
    return null;
  }
  function syncPendingNotifications(id, conversation = manager.getConversation(id)) {
    const thread = notificationThread(id, conversation);
    const desired = new Map();
    if (thread && conversation) {
      const asyncRequests = asyncQuestionRequests(id, conversation);
      const requests = [...array(conversation.requests), ...asyncRequests];
      for (const request of requests) {
        const kind = notificationRequestKind(request);
        if (!kind || !(typeof request.id === "string"
            || typeof request.id === "number" && Number.isSafeInteger(request.id))) continue;
        const key = JSON.stringify([id, typeof request.id, request.id, request.method,
          request.params?.turnId ?? null, request.params?.itemId ?? null]);
        desired.set(key, kind);
      }
    }
    for (const [key, previous] of pendingNotifications) {
      if (previous.threadId !== id || desired.has(key)) continue;
      pendingNotifications.delete(key);
      appendNotification("dismiss", previous.kind,
        { threadId: previous.threadId, title: previous.title }, previous.notificationId);
    }
    for (const [key, kind] of desired)
      if (!pendingNotifications.has(key)) pendingNotifications.set(key, appendNotification("notify", kind, thread));
  }
  function refreshPendingNotifications() {
    if (typeof manager.getCachedConversations !== "function") return;
    const loaded = new Set();
    for (const entry of manager.getCachedConversations() || []) {
      const conversation = Array.isArray(entry) && entry.length === 2 ? entry[1] : entry;
      const id = conversation?.conversationId ?? conversation?.id;
      if (typeof id !== "string") continue;
      loaded.add(id);
      syncPendingNotifications(id, conversation);
    }
    for (const id of new Set([...pendingNotifications.values()].map(event => event.threadId)))
      if (!loaded.has(id)) syncPendingNotifications(id, null);
  }
  function observeNotificationEvent(event) {
    if (event.method === "thread/deleted") {
      syncPendingNotifications(event.params?.threadId, null);
      return;
    }
    if (event.method !== "turn/completed") return;
    const { threadId, turn, turnId = turn?.id ?? turn?.turnId } = event.params || {};
    const status = typeof turn?.status === "string" ? turn.status : turn?.status?.type;
    const kind = status === "completed" ? "completed" : null;
    const thread = notificationThread(threadId);
    if (!kind || !thread || typeof turnId !== "string" || !turnId) return;
    const key = JSON.stringify([threadId, turnId]);
    if (completedNotificationTurns.has(key)) return;
    completedNotificationTurns.add(key);
    if (completedNotificationTurns.size > 256)
      completedNotificationTurns.delete(completedNotificationTurns.values().next().value);
    appendNotification("notify", kind, thread);
  }
  function notificationSnapshot(payload) {
    if (!payload || typeof payload !== "object" || Array.isArray(payload)
        || Object.keys(payload).some(key => !["epoch", "cursor"].includes(key))
        || Object.keys(payload).length !== 0 && (typeof payload.epoch !== "string"
          || !/^[a-zA-Z0-9_-]{1,120}$/.test(payload.epoch)
          || !Number.isSafeInteger(payload.cursor) || payload.cursor < 0))
      throw failure(400, "通知游标无效");
    refreshPendingNotifications();
    const reset = payload.epoch !== notificationEpoch || payload.cursor > notificationSequence;
    const truncated = !reset && payload.cursor < (notificationEvents[0]?.sequence ?? 1) - 1;
    return { ok: true, schemaVersion: 1, notificationEpoch, notificationCursor: notificationSequence,
      reset, truncated, pending: [...pendingNotifications.values()],
      events: reset ? [] : notificationEvents.filter(event => event.sequence > payload.cursor) };
  }
  const HISTORY_TURN_PAGE_LIMIT = 3;
  const HISTORY_ITEM_PAGE_LIMIT = 40;
  const HISTORY_TURN_READ_CONCURRENCY = 2;
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
    return data.filter(model => !model.hidden).map(model => ({
      ...model,
      supportedReasoningEfforts: array(model.supportedReasoningEfforts)
    }));
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
  function threadModelSettings(conversation, nativeThread = null) {
    const collaboration = conversation?.latestCollaborationMode?.settings
      ?? conversation?.latestThreadSettings?.collaborationMode?.settings;
    const effort = Object.hasOwn(nativeThread ?? {}, "reasoningEffort") ? nativeThread.reasoningEffort
      : Object.hasOwn(nativeThread ?? {}, "reasoning_effort") ? nativeThread.reasoning_effort
      : Object.hasOwn(collaboration ?? {}, "reasoning_effort") ? collaboration.reasoning_effort
      : Object.hasOwn(conversation?.latestThreadSettings ?? {}, "effort") ? conversation.latestThreadSettings.effort
      : conversation?.latestReasoningEffort;
    return {
      model: nativeThread?.model ?? collaboration?.model
        ?? conversation?.latestThreadSettings?.model ?? conversation?.latestModel ?? null,
      effort: effort ?? null
    };
  }
  async function requestedThreadModelSettings(payload, nativeThread, conversation) {
    if (payload.model == null && payload.reasoningEffort == null) return undefined;
    const current = threadModelSettings(conversation, nativeThread);
    const model = payload.model == null ? current.model : String(payload.model);
    if (typeof model !== "string" || !model.trim()) throw failure(400, "请求模型不能为空");
    const effort = payload.reasoningEffort == null
      ? payload.model != null && model !== current.model ? null : current.effort
      : String(payload.reasoningEffort);
    await validateReasoningEffort(model, effort);
    return { model, effort };
  }
  async function applyThreadModelSettings(id, settings) {
    if (!settings) return;
    if (typeof manager.updateThreadSettingsForNextTurn !== "function")
      throw failure(503, "当前 Codex 版本不支持设置下一轮模型");
    try {
      const applied = await manager.updateThreadSettingsForNextTurn(id, settings);
      if (applied === false) throw failure(409, "模型或思考深度未能应用，请重新核对后发送");
    } catch (error) {
      throw failure(error.code || 503, error.message || "模型或思考深度切换失败，请稍后重试");
    }
  }
  const timestamp = value => typeof value === "number" ? (value < 100_000_000_000 ? value * 1000 : value) : 0;
  let nativeCatalog = new Map();
  let sidebarPriorityOptions = { enabled: true, showPinned: false, showScheduled: false, scheduledIds: new Set() };
  let catalogLoaded = false;
  let catalogLoading = null;
  let catalogGeneration = 0;
  function nativeListSummary(id) {
    const conversation = manager.getConversation(id);
    return manager.getThreadListSummary?.(id)
      || conversation && manager.threadStore?.stateReaders?.getListSummary?.(conversation)
      || manager.getThreadSummary?.(id) || null;
  }
  function nativeUnreadState(thread, id) {
    const unread = manager.getThreadHasUnreadTurn?.(id);
    if (typeof unread === "boolean") return unread;
    const current = nativeListSummary(id);
    return current?.hasUnreadTurn === true || current?.hasUnreadTurn !== false && thread.hasUnreadTurn === true;
  }
  function sidebarPriority(id, current, unread, activityTimes) {
    const conversation = manager.getConversation(id);
    const view = conversation && manager.threadStore?.stateReaders?.getListState?.(conversation);
    const status = manager.getThreadRuntimeStatusEvidence?.(id)
      || conversation?.threadRuntimeStatus || current.threadRuntimeStatus || current.status || { type: "idle" };
    const runtime = status?.status ?? status;
    const type = typeof runtime === "string" ? runtime : runtime?.type;
    const waiting = !!view?.pendingRequest
      || asyncQuestionRequests(id, conversation).length > 0
      || array(runtime?.activeFlags).some(flag => flag === "waitingOnApproval" || flag === "waitingOnUserInput");
    const active = type === "active" || type === "inProgress"
      || type !== "systemError" && view?.resumeState !== "needs_resume"
        && (view?.isResponseInProgress === true || view?.isConversationInProgress === true);
    const attentionState = waiting ? "waiting" : unread ? "unread" : active ? "active" : "idle";
    const activity = activityTimes || manager.storage.readValue("thread-user-activity-times-v1") || {};
    const lastActivity = activity[JSON.stringify(["local", id])];
    const silentHeartbeat = (attentionState === "idle" || attentionState === "active")
      && view?.latestTurnIsSilentHeartbeat === true;
    const unattendedHeartbeat = attentionState === "active" && view?.pendingHeartbeatStartedAtMs != null
      && (lastActivity == null || lastActivity < view.pendingHeartbeatStartedAtMs);
    return {
      status,
      attentionState,
      active,
      eligible: sidebarPriorityOptions.enabled && !silentHeartbeat && !unattendedHeartbeat
        && (sidebarPriorityOptions.showScheduled || !sidebarPriorityOptions.scheduledIds.has(id))
        && current.threadStartKind !== "aeon" && current.threadSource !== "aeon"
    };
  }
  const DEFAULT_SIDE_THREAD_TITLE = "新建侧边会话";
  function isDefaultSideThreadTitle(value) {
    const title = cleanThreadTitle(value).toLowerCase();
    return !title || /^(?:side chat|侧边聊天|新建侧边会话)(?:\s+\d+)?$/.test(title)
      || isGenericMainThreadTitle(title);
  }
  function readNativeSideTab(entry, id) {
    if (!entry?.scope || entry.scope.value?.conversationId !== entry.parentThreadId
        || entry.parentThreadId === id || !entry.tab?.tabId || entry.controller?.tabById$ == null) return null;
    let tab;
    try { tab = entry.scope.get(entry.controller.tabById$, entry.tab.tabId); }
    catch { return null; }
    const threadId = tab?.props?.content?.conversationId ?? tab?.props?.conversationId;
    return tab?.tabId === entry.tab.tabId && tab.tabType?.kind === "local-side-chat" && threadId === id ? tab : null;
  }
  function isEphemeralSideThread(conversation, id = conversation?.conversationId || conversation?.id) {
    if (conversation?.ephemeral === false) return false;
    const entry = nativeSideThreads.get(id);
    return (conversation?.ephemeral === true || entry?.ephemeral === true)
      && (conversation?.sideConversation === true || !!entry || requestClientSideThreads.has(id));
  }
  function sideThreadTitle(conversation, fallback = "", ...fallbacks) {
    const id = conversation?.conversationId || conversation?.id;
    const confirmed = manager?.__codexElvesRequestClientManager
      && (nativeSideThreads.has(id) || requestClientSideThreads.has(id));
    if (conversation?.sideConversation !== true && !confirmed || subagentIdentity(conversation)) return null;
    if (isEphemeralSideThread(conversation, id)) {
      const entry = nativeSideThreads.get(id);
      // 临时侧边拒绝元数据更新；其实际名称属于原生标签，不能采用失败 RPC 的乐观标题。
      return cleanThreadTitle(readNativeSideTab(entry, id)?.title) || cleanThreadTitle(entry?.title)
        || cleanThreadTitle(requestClientSideThreads.get(id)?.title) || DEFAULT_SIDE_THREAD_TITLE;
    }
    const candidates = [conversation?.title, conversation?.name, fallback, ...fallbacks]
      .map(cleanThreadTitle).filter(Boolean);
    return candidates.find(value => !isDefaultSideThreadTitle(value))
      || cleanThreadTitle(fallback) || candidates[0] || DEFAULT_SIDE_THREAD_TITLE;
  }
  async function setEphemeralSideThreadTitle(id, title, conversation, initialEntry, expectedTitle, current, {
    automatic = false, onDispatch = () => {}
  } = {}) {
    const sourceManager = manager;
    if (!isEphemeralSideThread(conversation, id) || subagentIdentity(conversation))
      throw failure(409, "原生临时侧边身份已变化，请刷新后核对");
    const parentThreadId = initialEntry?.parentThreadId || conversation.forkedFromId;
    if (typeof parentThreadId !== "string" || !parentThreadId || parentThreadId === id)
      throw failure(503, "原生临时侧边标签尚未就绪，请刷新后核对", { sideTitleNotReady: true });
    const initialNode = initialEntry?.scope?.node;
    const initialTabId = initialEntry?.tab?.tabId, initialInstance = initialEntry?.tab?.contentInstance;
    const native = await nativeSideState();
    if (!current() || manager !== sourceManager || sourceManager.disposed)
      throw failure(409, "原生会话连接已变化，请核对名称后再操作");
    const entries = syncNativeSideThreads(native);
    const entry = entries.find(entry => entry.threadId === id && entry.parentThreadId === parentThreadId);
    if (!entry && !initialEntry?.scope)
      throw failure(503, "原生临时侧边标签尚未就绪，请刷新后核对", { sideTitleNotReady: true });
    const original = initialEntry?.scope ? initialEntry : entry;
    // lookupScope 每次返回新包装对象；原生 node 才是页面身份。
    const node = initialEntry?.scope ? initialNode : entry?.scope?.node;
    const tabId = initialEntry?.scope ? initialTabId : entry?.tab?.tabId;
    const instance = initialEntry?.scope ? initialInstance : entry?.tab?.contentInstance;
    const tab = readNativeSideTab(entry, id);
    const latest = sourceManager.getConversation(id);
    if (!entry || !node || !instance || entry.scope?.node !== node || entry.controller !== original.controller
        || tab?.tabId !== tabId || tab?.contentInstance !== instance
        || conversation.forkedFromId != null && conversation.forkedFromId !== parentThreadId
        || latest?.forkedFromId != null && latest.forkedFromId !== parentThreadId
        || latest?.ephemeral === false || subagentIdentity(latest)
        || typeof entry.controller.updateTab !== "function")
      throw failure(409, "原生侧边标签或父关系已变化，请刷新后核对");
    const actualTitle = String(tab.title || "").trim();
    if (actualTitle !== expectedTitle
        && !(automatic && isDefaultSideThreadTitle(expectedTitle) && isDefaultSideThreadTitle(actualTitle))) return false;
    onDispatch();
    entry.controller.updateTab(entry.scope, tabId, { title });
    const confirmed = readNativeSideTab(entry, id);
    if (!current() || manager !== sourceManager || sourceManager.disposed
        || confirmed?.contentInstance !== instance || confirmed?.title !== title)
      throw failure(409, "原生侧边名称尚未确认，请刷新后核对");
    nativeSideThreads.set(id, { ...entry, tab: confirmed, title, ephemeral: true });
    const remembered = requestClientSideThreads.get(id);
    if (remembered) requestClientSideThreads.set(id, { ...remembered, title, ephemeral: true });
    emit({ method: "remote/sidebarChanged", params: { threadId: id, parentThreadId, title } });
    emit({ method: "remote/relatedThreadsChanged", params: { threadId: parentThreadId } });
    return true;
  }
  function syncNativeSideThreadTitle(id, conversation, fallback = "") {
    const entry = nativeSideThreads.get(id), remembered = requestClientSideThreads.get(id);
    if (!entry && !remembered) return false;
    const title = sideThreadTitle(conversation, fallback || entry?.title || remembered?.title);
    if (!title) return false;
    let changed = false;
    for (const entries of [nativeSideThreads, requestClientSideThreads]) {
      const current = entries.get(id);
      if (!current || current.title === title) continue;
      entries.set(id, { ...current, title }); changed = true;
    }
    return changed;
  }
  function firstSideThreadMessageTitle(conversation) {
    for (const turn of nativeConversationTurns(conversation)) {
      for (const source of array(turn?.items)) {
        const item = nativeDisplayMessageItem(source);
        if (item?.type !== "userMessage" || item.remotePendingInput || item.remoteCompactionInput) continue;
        const text = array(item.content).filter(part => part?.type === "text" && typeof part.text === "string")
          .map(part => part.text).join(" ").replace(/\s+/g, " ").trim();
        // 用户要求：默认侧边名由首条有效文字生成，最多 15 个 Unicode 字符；按码点截取以保留完整代理对。
        if (text) return Array.from(text).slice(0, 15).join("");
      }
    }
    return "";
  }
  function maybeNameSideThreadFromFirstMessage(id, conversation) {
    if (!conversation || subagentIdentity(conversation) || sideThreadTitleWrites.has(id)) return;
    const entry = nativeSideThreads.get(id) || requestClientSideThreads.get(id);
    const currentTitle = sideThreadTitle(conversation, entry?.title);
    if (currentTitle == null || !isDefaultSideThreadTitle(currentTitle)) return;
    const title = firstSideThreadMessageTitle(conversation);
    if (!title) return;
    const sourceManager = manager;
    let dispatched = false;
    const current = () => manager === sourceManager && !sourceManager.disposed
      && sideThreadTitleWrites.get(id) === write;
    const write = Promise.resolve().then(async () => {
      if (!current()) return;
      if (isEphemeralSideThread(conversation, id)) {
        await setEphemeralSideThreadTitle(id, title, conversation, entry, currentTitle, current, {
          automatic: true, onDispatch: () => { dispatched = true; }
        });
        return;
      }
      // 原生条件写入会拒绝迟到的首条命名；持久化失败必须传回，不能只采用乐观标题。
      dispatched = true;
      await sourceManager.setThreadTitle(id, title, { requireAcknowledgement: true,
        ...(conversation.title == null ? { onlyIfUntitled: true } : { expectedTitle: conversation.title }) });
      if (!current()) return;
      const latest = sourceManager.__codexElvesRequestClientManager
        ? await refreshRequestClientConversation(id, { fresh: true, includeHistory: false })
        : sourceManager.getConversation(id);
      if (current() && latest) syncSidebarConversationState(id, latest);
    }).catch(error => {
      if (!current()) return;
      if (!dispatched && error.sideTitleNotReady === true) sideThreadTitleWrites.delete(id);
      reportSideFeatureDegraded("side_thread_first_message_title", error, { threadId: id });
    });
    // 已调用原生写入后保留尝试；仅标签未就绪且尚未写入时允许到位事件补齐。
    sideThreadTitleWrites.set(id, write);
  }
  function summary(thread, activityTimes) {
    const id = thread.conversationId || thread.id;
    const current = nativeListSummary(id) || thread;
    const cached = nativeCatalog.get(id);
    const conversation = manager.getConversation(id);
    // 目录读取可能比桌面缓存新；随后采用旧摘要时不能把更新时间回退。
    const updatedAt = Math.max(timestamp(current.updatedAt), timestamp(thread.updatedAt),
      cached?.updatedAt || 0);
    const unread = nativeUnreadState(thread, id);
    const { status, ...priority } = sidebarPriority(id, current, unread, activityTimes);
    const side = conversation?.sideConversation === true || thread?.sideConversation === true
      || nativeSideThreads.has(id) || manager.__codexElvesRequestClientManager && requestClientSideThreads.has(id);
    const title = side
      ? sideThreadTitle(conversation || thread, current.title || current.name || cached?.title
        || thread.title || thread.name)
      : mainThreadTitle(current.title, current.name, thread.name, thread.title, thread.preview,
        cached?.title, cached?.name, conversation?.title, conversation?.name);
    const result = {
      id, title,
      cwd: current.cwd || thread.cwd || "",
      createdAt: timestamp(current.createdAt ?? thread.createdAt), updatedAt,
      recencyAt: timestamp(current.recencyAt ?? current.updatedAt ?? thread.recencyAt ?? thread.updatedAt), hasUnreadTurn: unread,
      status, priority,
      threadSource: current.threadSource ?? thread.threadSource, projectId: null, pinned: false
    };
    sidebarSignatures.set(id, JSON.stringify([result.title, updatedAt, result.recencyAt, unread, status, priority]));
    return result;
  }
  function syncSidebarConversationState(id, conversation) {
    if (isDeletedThread(id)) return;
    if (!conversation) {
      if (sidebarSignatures.delete(id))
        emit({ method: "remote/sidebarChanged", params: { threadId: id } });
      return;
    }
    const side = conversation.sideConversation === true || nativeSideThreads.has(id)
      || manager.__codexElvesRequestClientManager && requestClientSideThreads.has(id);
    if (side) maybeNameSideThreadFromFirstMessage(id, conversation);
    const titleChanged = side && syncNativeSideThreadTitle(id, conversation);
    if (!side && !nativeCatalog.has(id)) return;
    const previous = sidebarSignatures.get(id);
    const item = summary(conversation);
    if (side) nativeCatalog.delete(id);
    else nativeCatalog.set(id, item);
    if (previous !== sidebarSignatures.get(id) || titleChanged) emit({ method: "remote/sidebarChanged",
      params: { threadId: id, ...(side ? { title: item.title } : {}), parentThreadId: side
        ? conversation.forkedFromId ?? nativeSideThreads.get(id)?.parentThreadId : undefined } });
    if (titleChanged) {
      const parentThreadId = conversation.forkedFromId || nativeSideThreads.get(id)?.parentThreadId;
      if (parentThreadId) emit({ method: "remote/relatedThreadsChanged", params: { threadId: parentThreadId } });
    }
  }
  function completedSideThreadUnread(thread, item = summary(thread)) {
    const status = typeof item.status === "string" ? item.status : item.status?.type;
    return item.hasUnreadTurn === true && !item.priority.active
      && item.priority.attentionState !== "waiting" && !["waiting", "systemError"].includes(status);
  }
  function recentSideThreadEntries() {
    if (manager.__codexElvesRequestClientManager)
      return [...nativeSideThreads.values()].filter(entry => !entry.closed
        && !isDeletedThread(entry.threadId) && !isDeletedThread(entry.parentThreadId));
    // 原生关闭侧边标签时会丢弃内存会话，直接读取内存即可覆盖生命周期，
    // 不让最近列表首屏依赖侧边面板模块的动态加载。
    const conversations = new Map();
    for (const entry of array(manager.threadStore?.getCachedConversations?.())) {
      const conversation = Array.isArray(entry) && entry.length === 2 ? entry[1] : entry;
      const id = conversation?.conversationId || conversation?.id;
      if (typeof id === "string" && id) conversations.set(id, conversation);
    }
    for (const id of nativeSideThreads.keys()) {
      const conversation = manager.getConversation(id);
      if (conversation) conversations.set(id, conversation);
    }
    return [...conversations.values()].flatMap(conversation => {
      const threadId = conversation.conversationId || conversation.id;
      const parentThreadId = conversation.forkedFromId;
      if (conversation.sideConversation !== true || !threadId || typeof parentThreadId !== "string"
          || !parentThreadId || threadId === parentThreadId || isDeletedThread(threadId)
          || isDeletedThread(parentThreadId) || subagentIdentity(conversation)) return [];
      return [{ threadId, parentThreadId }];
    });
  }
  function sideThreadAttentionByParent(entries, activityTimes) {
    const parents = new Map();
    for (const entry of entries) {
      if (entry.scope && !readNativeSideTab(entry, entry.threadId)) continue;
      const conversation = manager.getConversation(entry.threadId) || entry.conversation;
      if (!conversation) continue;
      const item = summary(conversation, activityTimes);
      const active = item.priority.active === true;
      const waiting = item.priority.attentionState === "waiting";
      const unread = completedSideThreadUnread(conversation, item);
      if (!active && !waiting && !unread) continue;
      const attention = parents.get(entry.parentThreadId) || { active: false, waiting: false, unread: false };
      attention.active ||= active; attention.waiting ||= waiting; attention.unread ||= unread;
      parents.set(entry.parentThreadId, attention);
    }
    return parents;
  }
  function sortMode(value, preferences) {
    return value === "manual" && preferences.manualSortVersion === 1 ? "manual" : "updated_at";
  }
  async function sidebar(payload = {}) {
    const readGeneration = catalogGeneration;
    await prepareAsyncQuestionState().catch(() => {});
    const recentOnly = payload.recentOnly === true;
    let sideEntries = [];
    try {
      const cachedSides = recentOnly
        && (payload.cachedOnly === true || !manager.__codexElvesRequestClientManager);
      sideEntries = cachedSides ? recentSideThreadEntries()
        : (await sideThreadEntries()).map(entry => ({ ...entry }));
      // 新版临时会话不在侧栏索引中；沿原生标签确认关系，再复用元数据读取及状态订阅。
      if (manager.__codexElvesRequestClientManager && !cachedSides)
        for (const entry of sideEntries) {
          try {
            // 本次读取保留结果，不让后续侧边读取触发的缓存淘汰丢失先前状态。
            entry.conversation = await refreshRequestClientConversation(entry.threadId, { includeHistory: false });
          } catch (error) {
            reportSideFeatureDegraded("sidebar.side_thread_state", error, { threadId: entry.threadId });
          }
        }
    } catch (error) {
      reportSideFeatureDegraded("sidebar.side_threads", error);
    }
    const sideIds = new Set(sideEntries.map(entry => entry.threadId));
    for (const id of sideIds) nativeCatalog.delete(id);
    const keys = recentOnly ? ["local-projects", "thread-project-assignments", "projectless-thread-ids"]
      : ["local-projects", "pinned-project-ids", "project-order",
        "sidebar-project-thread-orders", "thread-project-assignments", "projectless-thread-ids"];
    const stored = Object.fromEntries(await Promise.all(keys.map(async key => {
      try { return [key, await state(key)]; }
      catch (error) {
        reportSidebarFeatureDegraded(`state.${key}`, error);
        return [key, {}];
      }
    })));
    if (readGeneration !== catalogGeneration) throw failure(409, "会话列表读取已取消");
    const preferences = manager.storage.readValue("flat-project-sidebar-preferences-v1") || {};
    const sharedSort = manager.storage.readValue("codex-sidebar-sort-mode-v1");
    const pinnedSort = manager.storage.readValue("pinned-sidebar-sort-mode-v1") === "manual" ? "manual" : "updated_at";
    const activityTimes = manager.storage.readValue("thread-user-activity-times-v1") || {};
    const scheduledIds = new Set();
    for (const query of appScope?.queryClient?.getQueryCache?.()?.getAll?.() || []) {
      if (!JSON.stringify(query.queryKey).includes('"inbox-items"')) continue;
      const data = query.state?.data?.data || query.state?.data;
      for (const item of array(data?.items))
        if (item.automationId != null && item.threadId) scheduledIds.add(item.threadId);
      for (const item of array(data?.unreadRunCounts?.unreadRuns))
        if (item.threadId) scheduledIds.add(item.threadId);
    }
    sidebarPriorityOptions = {
      enabled: manager.storage.readValue("sidebar-priority-show-priority-v1") !== false,
      showPinned: manager.storage.readValue("sidebar-priority-show-pinned-v1") === true,
      showScheduled: manager.storage.readValue("sidebar-priority-show-scheduled-v1") === true,
      scheduledIds
    };
    const chatSort = sharedSort == null ? sortMode(preferences.chatSortMode, preferences) : "updated_at";
    const projectSort = sharedSort == null ? sortMode(preferences.projectSortMode, preferences) : "updated_at";
    const observeThreads = threads => {
      if (readGeneration !== catalogGeneration) return;
      for (const thread of array(threads)) {
        const id = thread.conversationId || thread.id;
        if (isDeletedThread(id)) continue;
        if (sideIds.has(id) || thread.sideConversation === true
            || manager.getConversation(id)?.sideConversation === true) {
          sideIds.add(id); nativeCatalog.delete(id); continue;
        }
        if (thread.threadSource === "subagent" || thread.source?.subAgent || thread.source?.subagent) {
          nativeCatalog.delete(id);
          continue;
        }
        nativeCatalog.set(id, summary(thread, activityTimes));
      }
    };
    if (recentOnly && payload.cachedOnly === true) {
      // 首屏复用原生内存摘要；完整目录由下一次后台读取补齐。
    } else if (!catalogLoaded) {
      if (!catalogLoading) {
        const loading = (async () => {
          let cursor = null;
          const cursors = new Set();
          do {
            const page = await manager.requestClient.sendRequest("thread/list", { cursor, limit: 100, sortKey: "updated_at", archived: false });
            if (readGeneration !== catalogGeneration) throw failure(409, "会话列表读取已取消");
            observeThreads(page.data);
            cursor = page.nextCursor || null;
            if (cursor && cursors.has(cursor)) throw failure(503, "原生会话目录分页游标重复");
            if (cursor) cursors.add(cursor);
          } while (cursor);
          catalogLoaded = true;
        })().finally(() => { if (catalogLoading === loading) catalogLoading = null; });
        catalogLoading = loading;
      }
      await catalogLoading;
    } else {
      const page = await manager.requestClient.sendRequest("thread/list", { cursor: null, limit: 100, sortKey: "updated_at", archived: false });
      observeThreads(page.data);
    }
    if (readGeneration !== catalogGeneration) throw failure(409, "会话列表读取已取消");
    observeThreads(manager.getRecentThreadSummaries());
    let pinnedState = { threadIds: [] };
    try {
      const pins = await manager.runtime.pinnedThreads();
      if (!pins) throw failure(503, "当前 Codex 版本的原生置顶服务不可用");
      pinnedState = await pins.list({ hostId: "local", useAppServerPins: true });
      if (!pinnedState || !Array.isArray(pinnedState.threadIds))
        throw failure(503, "原生置顶服务返回结果无效");
    } catch (error) {
      if (manager.__codexElvesRequestClientManager)
        pinnedState = { threadIds: requestClientSnapshotState("pinned-thread-ids") };
      reportSidebarFeatureDegraded("pinned_threads", error);
    }
    if (readGeneration !== catalogGeneration) throw failure(409, "会话列表读取已取消");
    const pinnedIds = array(pinnedState.threadIds);
    if (recentOnly) for (const id of nativeCatalog.keys()) {
      if (manager.getConversation(id)?.sideConversation === true) {
        sideIds.add(id); nativeCatalog.delete(id);
      }
    }
    const projects = Object.entries(stored["local-projects"] || {}).map(([id, project]) => ({
      id, name: project.name || project.rootPaths?.[0]?.split(/[\\/]/).at(-1) || id,
      roots: array(project.rootPaths), pinned: array(stored["pinned-project-ids"]).includes(id),
      recencyAt: 0, threads: []
    }));
    const assignments = stored["thread-project-assignments"] || {};
    const projectless = new Set(array(stored["projectless-thread-ids"]));
    const sideAttention = sideThreadAttentionByParent(sideEntries, activityTimes);
    const threads = [...nativeCatalog.values()].filter(thread => !sideIds.has(thread.id)).map(thread => {
      // 跳过状态独立于会话摘要，旧目录条目也须读取当前内存状态。
      const conversation = manager.getConversation(thread.id);
      if (conversation) {
        thread = summary(conversation, activityTimes);
        nativeCatalog.set(thread.id, thread);
      }
      const assigned = assignments[thread.id];
      let projectId = typeof assigned === "string" ? assigned : assigned?.projectKind === "local" ? assigned.projectId : null;
      if (assigned == null && !projectless.has(thread.id)) {
        const matches = projects.filter(project => project.roots.some(root => normalizePath(root) === normalizePath(thread.cwd)));
        if (matches.length === 1) projectId = matches[0].id;
      }
      const pinned = pinnedIds.includes(thread.id);
      return { ...thread, projectId, pinned,
        ...(sideAttention.has(thread.id) ? { sideThreadAttention: sideAttention.get(thread.id) } : {}),
        priority: { ...thread.priority, eligible: thread.priority.eligible && (!pinned || !sidebarPriorityOptions.showPinned) } };
    });
    const manualOrder = stored["sidebar-project-thread-orders"] || {};
    const updatedAt = thread => thread.updatedAt || thread.recencyAt || thread.createdAt || 0;
    const compareUpdatedAt = (a, b) => updatedAt(b) - updatedAt(a)
      || b.createdAt - a.createdAt || a.id.localeCompare(b.id);
    const sortThreads = (items, ids, mode) => items.sort((a, b) => mode === "manual"
      ? (ids.indexOf(a.id) < 0 ? ids.length : ids.indexOf(a.id)) - (ids.indexOf(b.id) < 0 ? ids.length : ids.indexOf(b.id)) || compareUpdatedAt(a, b)
      : compareUpdatedAt(a, b));
    if (recentOnly) return {
      ok: true, complete: catalogLoaded,
      projects: projects.map(project => ({ id: project.id, name: project.name })),
      recent: threads.filter(thread => !thread.pinned),
      pinnedThreads: threads.filter(thread => thread.pinned),
      sideThreadIds: [...sideIds]
    };
    for (const project of projects) {
      const children = threads.filter(thread => thread.projectId === project.id);
      project.recencyAt = Math.max(0, ...children.map(updatedAt));
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
    const unreadSideThreads = sideEntries.filter(entry => !entry.closed && !isDeletedThread(entry.threadId)
      && completedSideThreadUnread(manager.getConversation(entry.threadId) || { id: entry.threadId }))
      .map(entry => ({ id: entry.threadId, parentThreadId: entry.parentThreadId }));
    return { ok: true, projects, pinnedThreads, recent, nextCursor: null, sideThreadIds: [...sideIds], unreadSideThreads,
      creationAvailable: !!appScope,
      preferences: { mode: preferences.mode === "list" ? "list" : "project", chats: chatSort, projects: projectSort, pinned: pinnedSort } };
  }
  async function readThread(id) {
    id = requiredId(id);
    if (isDeletedThread(id)) throw failure(404, "会话已永久删除");
    const sourceManager = manager;
    const knownSideThread = nativeSideThreads.get(id);
    let response;
    try { response = await readThreadMetadata(id); }
    catch (error) {
      if (isDeletedThread(id)) throw failure(404, "会话已永久删除");
      if (knownSideThread && sideThreadLoadError(error)) {
        const entries = await sideThreadEntries().catch(() => null);
        if (entries && !entries.some(entry => entry.threadId === id)) {
          throw failure(410, "侧边会话已过期或已关闭", {
            reason: "side_thread_expired",
            threadId: id,
            parentThreadId: knownSideThread.parentThreadId
          });
        }
      }
      if (missingThreadError(error)) throw failure(404, "会话已关闭或不存在");
      throw error;
    }
    const thread = response.thread || response;
    if (!thread?.id) throw failure(404, "会话不存在或已删除");
    if (manager !== sourceManager || sourceManager.disposed)
      throw failure(409, "原生会话连接已变化，请重新读取");
    if (sourceManager.__codexElvesRequestClientManager && thread.ephemeral === true
        && !subagentIdentity(thread) && !subagentIdentity(sourceManager.getConversation(id))) {
      const hasSideIdentity = () => {
        const confirmed = requestClientSideThreads.get(id);
        return confirmed?.threadId === id && typeof confirmed.parentThreadId === "string"
          && confirmed.parentThreadId.length > 0 && confirmed.parentThreadId !== id
          || !!readNativeSideTab(nativeSideThreads.get(id), id);
      };
      const assertCurrent = () => {
        if (manager !== sourceManager || sourceManager.disposed)
          throw failure(409, "原生会话连接已变化，请重新读取");
        if (isDeletedThread(id)) throw failure(404, "会话已永久删除");
      };
      if (!hasSideIdentity()) {
        const rpc = await requestClientNativeRpc();
        assertCurrent();
        if (!hasSideIdentity() && !subagentIdentity(sourceManager.getConversation(id))) {
          const request = rpc.getConversation(id);
          try {
            const loaded = await request;
            assertCurrent();
            if (loaded !== null && loaded?.id !== id)
              throw failure(409, "原生会话读取结果不匹配，请重新读取");
            if (loaded === null && !hasSideIdentity() && !subagentIdentity(sourceManager.getConversation(id))) {
              const entries = await sideThreadEntries();
              assertCurrent();
              const side = entries.some(entry => entry.threadId === id && !entry.closed
                && typeof entry.parentThreadId === "string"
                && entry.parentThreadId.length > 0 && entry.parentThreadId !== id);
              if (!side && !hasSideIdentity() && !subagentIdentity(sourceManager.getConversation(id)))
                throw failure(410, "侧边会话已过期或已关闭", {
                  reason: "side_thread_expired", threadId: id
                });
            }
          } finally { request?.[Symbol.dispose]?.(); }
        }
      }
    }
    if (thread.sideConversation === true || manager.getConversation(id)?.sideConversation === true) {
      const side = nativeSideIdentity(thread, await sideThreadEntries());
      if (!side) throw failure(404, "原生临时侧边会话已关闭或不存在");
    }
    return thread;
  }
  async function readThreadMetadata(id) {
    const sourceManager = manager;
    const createdAt = timestamp(sourceManager.getConversation(id)?.createdAt
      ?? requestClientSideThreads.get(id)?.createdAtMs);
    const now = Date.now(), expiresAt = createdAt + INITIAL_THREAD_METADATA_WINDOW_MS;
    if (!Number.isFinite(createdAt) || createdAt <= 0 || createdAt > now || now >= expiresAt)
      return sourceManager.readThread(id, { includeTurns: false });
    const current = pendingThreadMetadataReads.get(id);
    if (current?.manager === sourceManager) return current.promise;
    const pending = { manager: sourceManager, promise: null };
    const active = () => manager === sourceManager && !sourceManager.disposed && !isDeletedThread(id)
      && pendingThreadMetadataReads.get(id) === pending;
    // 首轮接受可能早于元数据写入；只恢复刚创建会话的精确空文件错误，并合并设置与历史的并发读取。
    pending.promise = Promise.resolve().then(async () => {
      for (let attempt = 0; ; attempt++) {
        try { return await sourceManager.readThread(id, { includeTurns: false }); }
        catch (error) {
          const delay = INITIAL_THREAD_METADATA_RETRY_DELAYS[attempt];
          const recentRequestClientSide = manager.__codexElvesRequestClientManager
            && requestClientSideThreads.has(id);
          const retryable = initialThreadMetadataError(error, id)
            || recentRequestClientSide && (missingThreadError(error) || sideThreadLoadError(error));
          if (delay == null || !retryable || !active()
              || Date.now() + delay >= expiresAt) throw error;
          await new Promise(resolve => setTimeout(resolve, delay));
          if (!active() || Date.now() >= expiresAt) throw error;
        }
      }
    });
    pendingThreadMetadataReads.set(id, pending);
    try { return await pending.promise; }
    finally {
      if (pendingThreadMetadataReads.get(id) === pending) pendingThreadMetadataReads.delete(id);
    }
  }
  function initialThreadMetadataError(error, id) {
    const message = typeof error?.message === "string" ? error.message : "";
    return (error?.code ?? error?.jsonRpcCode) === -32603
      && message.includes("failed to read session metadata ") && message.includes("rollout at ")
      && message.toLowerCase().includes(`-${id.toLowerCase()}.jsonl`) && message.endsWith(" is empty");
  }
  function missingThreadError(error) {
    if (error?.code === 404 || error?.status === 404) return true;
    const code = error?.code ?? error?.jsonRpcCode;
    const rpcError = Number.isInteger(code) && (code >= -32099 && code <= -32000
      || code === -32700 || code >= -32603 && code <= -32600);
    // 与原生 thread_not_found 分类一致；普通超时、配置及 rollout 读取失败仍向上传播。
    return rpcError && typeof error?.message === "string"
      && (error.message.includes("thread not found:")
        || error.message.includes("Client specified an invalid argument: unknown thread:")
        || error.message.includes("no rollout found for thread id "));
  }
  function sideThreadLoadError(error) {
    const message = typeof error?.message === "string" ? error.message.toLowerCase() : "";
    return message.includes("thread not loaded:")
      || message.includes("unknown thread:")
      || message.includes("no rollout found for thread id ");
  }
  function subagentIdentity(thread) {
    if (!thread || typeof thread !== "object") return null;
    const source = thread.source;
    const spawn = source && typeof source === "object"
      ? (source.subAgent ?? source.subagent)?.thread_spawn : null;
    const parentThreadId = thread.parentThreadId ?? spawn?.parent_thread_id ?? null;
    const marked = parentThreadId != null || thread.threadSource === "subagent"
      || source === "subagent" || !!(source && typeof source === "object"
        && (Object.hasOwn(source, "subAgent") || Object.hasOwn(source, "subagent")));
    if (!marked) return null;
    const name = thread.agentNickname ?? spawn?.agent_nickname;
    return { readOnly: true, viewKind: "subagent",
      parentThreadId: typeof parentThreadId === "string" && parentThreadId ? parentThreadId : null,
      agentName: typeof name === "string" ? name.trim().replace(/^@/, "") : "" };
  }
  function nativeThreadEphemeral(thread) {
    const flags = [thread?.ephemeral, manager.getConversation(thread?.id)?.ephemeral];
    return flags.includes(true) ? true : flags.includes(false) ? false : undefined;
  }
  async function assertWritableThread(id) {
    id = requiredId(id);
    const cached = manager.getConversation(id);
    if (subagentIdentity(cached) || subagentIdentity(manager.getThreadListSummary?.(id)))
      throw failure(403, "子代理会话仅支持只读查看");
    const thread = await readThread(id);
    if (subagentIdentity(thread)) throw failure(403, "子代理会话仅支持只读查看");
    return thread;
  }
  const nativeReadActions = new Set(["events", "notifications", "sidebar", "history", "thread",
    "models", "skills", "queue", "queueEdit", "sendState", "promptOptimization",
    "workspaceContext", "relatedThreads"]);
  const requestClientReadActions = new Set([
    "events", "notifications", "sidebar", "history", "thread", "models", "skills", "workspaceContext",
    "relatedThreads", "createSideThread", "closeSideThread", "queue", "send", "sendState", "markRead", "sort",
    "removeQueued", "reorderQueued", "beginQueuedEdit", "editQueued", "cancelQueuedEdit",
    "queueEdit", "dismissQueuedEdit", "steerQueued", "create", "stop", "compact",
    "rename", "deleteThread", "pinThread", "pinProject", "approve", "answer", "editMessage", "setGoal", "updateGoal"
  ]);
  async function guardThreadAction(action, payload) {
    if (payload?.threadId == null || nativeReadActions.has(action) || action === "dispose") return null;
    const id = requiredId(payload.threadId);
    const thread = await readThread(id);
    const identity = subagentIdentity(thread) || subagentIdentity(manager.getConversation(id));
    if (!identity) return null;
    if (action === "markRead") return { ok: true, threadId: id, skipped: true, readOnly: true };
    throw failure(403, "子代理会话仅支持只读查看");
  }
  async function nativeResource(name) {
    const rules = {
      initial: {
        expression: /^app:\/\/-\/assets\/app-initial-[A-Za-z0-9_-]+\.js$/,
      },
      primary: {
        expression: /^app:\/\/-\/assets\/app-primary-[A-Za-z0-9_-]+\.js$/,
      },
      shared: {
        expression: /^app:\/\/-\/assets\/app-shared-[A-Za-z0-9_-]+\.js$/,
      },
      background: {
        expression: /^app:\/\/-\/assets\/local-conversation-background-agents-signals-[A-Za-z0-9_-]+\.js$/,
        matches: source => source.includes("getIndexedSubagentProgress:")
          && source.includes("sourceLinkedThreadsDiscoveryComplete:"),
      },
      side: {
        expression: /^app:\/\/-\/assets\/local-conversation-side-chat-(?!tab-)[A-Za-z0-9_-]+\.js$/,
        matches: source => source.includes("Cannot create a side chat for an archived conversation")
          && source.includes("sourceConversationId"),
      },
      sideTab: {
        expression: /^app:\/\/-\/assets\/local-conversation-side-chat(?:-tab)?-[A-Za-z0-9_-]+\.js$/,
        matches: source => source.includes("LocalConversationSideChatTab") && source.includes("import("),
      },
    };
    const rule = rules[name];
    if (!rule) throw failure(503, "原生关联会话模块名称无效");
    const observed = [...document.querySelectorAll('link[rel="modulepreload"]')].map(link => link.href)
      .concat(performance.getEntriesByType("resource").map(entry => entry.name),
        Array.from(document.scripts || [], script => script.src));
    const resources = [...new Set(observed.filter(value => rule.expression.test(value)))];
    if (resources.length === 1) return resources[0];
    if (resources.length > 1) throw failure(503, `原生关联会话模块版本不唯一（${name}）`);
    if (rule.matches) {
      const candidates = [...new Set(observed.filter(value =>
        /^app:\/\/-\/assets\/.+\.js$/.test(value)
        && (value.includes("local-conversation") || value.includes("app-"))))];
      const matches = [];
      for (const resource of candidates) {
        try {
          if (rule.matches(await nativeResourceSource(resource))) matches.push(resource);
        } catch {}
      }
      if (matches.length === 1) return matches[0];
      if (matches.length > 1) throw failure(503, `原生关联会话模块版本不唯一（${name}）`);
    }
    // 静态依赖不一定出现在 DOM 或当前 Resource Timing 中；沿可信入口递归解析
    // index -> app-main 或 app-primary -> app-initial -> local-conversation-*，兼容新版懒加载模块。
    const roots = [...new Set(observed.filter(value =>
      /^app:\/\/-\/assets\/(?:index-|app-main|app-primary|app-route-components|primary-app-route-components|app-initial|local-conversation)[A-Za-z0-9_.-]*\.js$/.test(value)))];
    const importPattern = /(?:from|import)\s*(?:\(\s*)?[`"'](\.\/[^`"']+\.js)[`"']/g;
    const visited = new Set();
    const queue = roots.map(resource => ({ resource, depth: 0 }));
    const resolved = [];
    while (queue.length && resolved.length < 2) {
      const { resource, depth } = queue.shift();
      if (visited.has(resource) || depth > 5) continue;
      visited.add(resource);
      let source;
      try {
        source = await nativeResourceSource(resource);
      } catch {
        continue;
      }
      if ((!rule.matches || rule.matches(source)) && rule.expression.test(resource)) {
        resolved.push(resource);
        continue;
      }
      for (const match of source.matchAll(importPattern)) {
        const dependency = new URL(match[1], resource).href;
        if (rule.expression.test(dependency)) queue.push({ resource: dependency, depth: depth + 1 });
        else if (depth < 5 && /(?:app-(?:main|primary|initial)|local-conversation)/.test(dependency))
          queue.push({ resource: dependency, depth: depth + 1 });
      }
    }
    const uniqueResolved = [...new Set(resolved)];
    if (uniqueResolved.length === 1) return uniqueResolved[0];
    if (uniqueResolved.length > 1)
      throw failure(503, `原生关联会话模块版本不唯一（${name}）`);
    throw failure(503, `当前 Codex 版本的原生关联会话模块不可用（${name}）`);
  }
  async function nativeResourceSource(resource) {
    // 打包资源按带哈希的 URL 复用源码；当前来源与版本歧义仍由 nativeResource 每次检查。
    let pending = nativeResourceSources.get(resource);
    if (!pending) {
      pending = (async () => {
        const response = await fetch(resource);
        if (!response.ok) throw failure(503, "无法读取原生关联会话接口");
        return await response.text();
      })();
      nativeResourceSources.set(resource, pending);
      pending.catch(() => {
        if (nativeResourceSources.get(resource) === pending) nativeResourceSources.delete(resource);
      });
    }
    return await pending;
  }
  async function nativeModuleSnapshot(name) {
    const resource = await nativeResource(name);
    if (!nativeModuleSnapshots.has(resource)) {
      const pending = (async () => {
        const [module, source] = await Promise.all([import(resource), nativeResourceSource(resource)]);
        return { module, source, resource };
      })();
      nativeModuleSnapshots.set(resource, pending);
      pending.catch(() => { if (nativeModuleSnapshots.get(resource) === pending) nativeModuleSnapshots.delete(resource); });
    }
    return await nativeModuleSnapshots.get(resource);
  }
  function nativeFunction(module, predicate, label) {
    const matches = Object.values(module).filter(value => {
      if (typeof value !== "function") return false;
      try { return predicate(Function.prototype.toString.call(value), value); } catch { return false; }
    });
    if (matches.length !== 1) throw failure(503, `当前 Codex 版本的原生${label}接口无法唯一识别`);
    return matches[0];
  }
  function nativeExport(snapshot, symbol) {
    if (typeof symbol !== "string" || !/^[\w$]+$/.test(symbol))
      throw failure(503, "原生关联会话接口标识无效");
    const escaped = symbol.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const exports = [...snapshot.source.matchAll(/\bexport\s*\{([^{}]*)\}/g)].at(-1)?.[1] ?? "";
    const name = exports.match(new RegExp(`(?:^|,)\\s*${escaped}\\s+as\\s+([\\w$]+)\\s*(?=,|$)`))?.[1];
    if (!name || snapshot.module[name] == null)
      throw failure(503, "当前 Codex 版本的原生关联会话接口未导出");
    return snapshot.module[name];
  }
  function nativeFamilyBefore(source, marker) {
    const index = source.lastIndexOf(marker);
    if (index < 0) throw failure(503, "原生关联会话状态标记缺失");
    const assignments = [...source.slice(0, index).matchAll(
      /([\w$]+)=[\w$]+\([\w$]+,\([\w$]+,\{get:[\w$]+(?:,scope:[\w$]+)?\}\)=>\{/g)];
    const symbol = assignments.at(-1)?.[1];
    if (!symbol) throw failure(503, "当前 Codex 版本的原生关联会话状态无法识别");
    return symbol;
  }
  async function nativeRelatedState() {
    if (!appScope) discover();
    if (!appScope || typeof appScope.get !== "function")
      throw failure(503, "原生关联会话上下文尚未就绪");
    if (!relatedNativeState) {
      relatedNativeState = (async () => {
        const [initial, primary] = await Promise.all([
          nativeModuleSnapshot("initial"), nativeModuleSnapshot("primary")
        ]);
        const background = await nativeModuleSnapshot("background").catch(() => null);
        const rowsMarker = "sourceLinkedThreadsDiscoveryComplete:";
        const rowsSnapshot = [background, primary].find(snapshot =>
          snapshot?.source.includes(rowsMarker) && snapshot.source.includes("getIndexedSubagentProgress:"));
        if (!rowsSnapshot) throw failure(503, "当前 Codex 版本的原生子代理列表无法识别");
        const beforeRows = rowsSnapshot.source.slice(0, rowsSnapshot.source.lastIndexOf(rowsMarker));
        const initializer = [...beforeRows.matchAll(/function\s+([\w$]+)\(\)\{return\(\1=/g)].at(-1);
        if (!initializer || !beforeRows.slice(initializer.index).includes("getIndexedSubagentProgress:"))
          throw failure(503, "当前 Codex 版本的原生子代理列表无法识别");
        // 打包器首次执行后会改写初始化函数，必须从静态源码识别导出，不能检查当前函数体。
        const initializeRows = nativeExport(rowsSnapshot, initializer[1]);
        if (typeof initializeRows !== "function") throw failure(503, "原生子代理列表初始化接口无效");
        const rowsSymbol = nativeFamilyBefore(rowsSnapshot.source, rowsMarker);
        initializeRows();
        const rowsFamily = nativeExport(rowsSnapshot, rowsSymbol);
        if (typeof rowsFamily?.read !== "function") throw failure(503, "原生子代理列表状态无效");
        const sortRows = nativeFunction(rowsSnapshot.module, source => source.length < 240
          && source.includes(".sort(") && source.includes("recencyAtMs"), "子代理排序");
        const formatAgentPath = nativeFunction(initial.module, source => source.length < 500
          && source.includes(".split(`/`)") && source.includes("`root`")
          && source.includes(".toUpperCase()"), "子代理名称");
        // 新版由可挂载的 signal 通过桌面 RPC 加载后代；只 get 行列表不会启动该生命周期。
        const discoveryIndex = rowsSnapshot.source.indexOf(".rpc.discoverSubagentDescendantSnapshot(");
        const snapshotSymbol = discoveryIndex < 0 ? null : [...rowsSnapshot.source.slice(0, discoveryIndex)
          .matchAll(/([\w$]+)=[\w$]+\([\w$]+,[\w$]+=>null,\{onMount:/g)].at(-1)?.[1];
        const snapshotFamily = snapshotSymbol ? nativeExport(rowsSnapshot, snapshotSymbol) : null;
        return { rowsFamily, sortRows, formatAgentPath, snapshotFamily };
      })();
      relatedNativeState.catch(() => { relatedNativeState = null; });
    }
    return await relatedNativeState;
  }
  async function nativeSideState() {
    if (!nativeSideBindings) {
      nativeSideBindings = (async () => {
        const initial = await nativeModuleSnapshot("initial");
        const side = await nativeModuleSnapshot("side").catch(() => null);
        const sideSnapshot = side?.source.includes("`sidechat-loading:`") ? side : initial;
        const marker = sideSnapshot.source.indexOf("`sidechat-loading:`");
        const initializer = [...sideSnapshot.source.slice(0, marker).matchAll(
          /function\s+([\w$]+)\(\)\{return\(\1=/g)].at(-1);
        if (marker < 0 || !initializer) throw failure(503, "当前 Codex 版本的原生侧边会话入口无法识别");
        nativeExport(sideSnapshot, initializer[1])();
        const open = nativeFunction(sideSnapshot.module, source =>
          source.includes("Cannot create a side chat for an archived conversation")
          && source.includes("sourceConversationId"), "侧边会话创建");
        const featureSymbol = open.toString().match(/if\(![\w$]+\.get\(([\w$]+)\)\)return null/)?.[1];
        const threadSymbol = initial.source.match(/([\w$]+)=[\w$]+\(`ThreadScope`,/)?.[1];
        const routeSymbol = initial.source.match(/([\w$]+)=[\w$]+\(`RouteScope`,/)?.[1];
        const sideTab = await nativeModuleSnapshot("sideTab").catch(() => null);
        const componentPath = sideTab?.source.match(
          /import\([`"'](\.\/[\w-]+\.js)[`"']\)/)?.[1] || initial.source.match(
            /let\{LocalConversationSideChatTab:[\w$]+\}=await import\([`"'](\.\/[\w-]+\.js)[`"']\)/)?.[1];
        const sharedPaths = [...new Set([...initial.source.matchAll(
          /\bfrom["'](\.\/app-shared-[\w-]+\.js)["']/g)].map(match => match[1]))];
        if (!componentPath || sharedPaths.length !== 1)
          throw failure(503, "原生侧边会话页面依赖无法识别");
        const shared = await import(new URL(sharedPaths[0], initial.resource).href);
        const lookupScope = nativeFunction(shared, source => source.length < 700
          && source.includes(".retainedScopeEntries.get(") && source.includes("new Map("), "会话页面上下文");
        const panel = nativeFunction(initial.module, source => source.length < 120
          && /return [\w$]+===`bottom`\?[\w$]+:[\w$]+/.test(source), "侧边面板");
        const allTabs = nativeFunction(initial.module, source => source.length < 600
          && source.includes("excludeTab:") && source.includes(".tabs$"), "侧边标签");
        let feature = null;
        if (featureSymbol) {
          try { feature = nativeExport(initial, featureSymbol); } catch {}
        }
        return { open, lookupScope, panel, allTabs,
          feature,
          threadScope: nativeExport(initial, threadSymbol),
          routeScope: nativeExport(initial, routeSymbol),
          componentUrl: new URL(componentPath, initial.resource).href };
      })();
      nativeSideBindings.catch(() => { nativeSideBindings = null; });
    }
    return await nativeSideBindings;
  }
  function nativeSideFeatureEnabled(native, scope) {
    if (!scope) return !!nativeNavigate;
    if (!native.feature) return true;
    try { return scope.get(native.feature) === true; } catch { return false; }
  }
  function nativeSideScopes(native) {
    const root = appScope?.chain?.get(native.threadScope.parent.id);
    const scopes = [];
    for (const [key, entry] of root?.retainedScopeEntries.get(native.threadScope) || []) {
      const threadScope = native.lookupScope(appScope, native.threadScope, key);
      if (!threadScope) continue;
      for (const [routeKey] of entry.node.retainedScopeEntries.get(native.routeScope) || []) {
        const scope = native.lookupScope(threadScope, native.routeScope, routeKey);
        if (scope?.value.routeKind === "local-thread"
            && [null, undefined, "local"].includes(scope.value.hostId)) scopes.push(scope);
      }
    }
    return scopes;
  }
  function domSideThreadEntries(parentThreadId = null) {
    const entries = [];
    for (const element of document.querySelectorAll(
      '[data-app-shell-tab-controller][data-tab-id^="sidechat:"]'
    )) {
      const tabId = element.getAttribute("data-tab-id") || "";
      const threadId = tabId.slice("sidechat:".length);
      if (!/^[a-zA-Z0-9_-]{1,120}$/.test(threadId)) continue;
      const known = nativeSideThreads.get(threadId) || requestClientSideThreads.get(threadId);
      const parentId = known?.parentThreadId || parentThreadId;
      if (!parentId || parentId === threadId) continue;
      const label = element.textContent?.replace(/\s+/g, " ").trim();
      const conversation = manager.getConversation(threadId);
      const ephemeral = isEphemeralSideThread(conversation, threadId);
      const title = ephemeral
        ? cleanThreadTitle(readNativeSideTab(nativeSideThreads.get(threadId), threadId)?.title)
          || cleanThreadTitle(label) || known?.title || DEFAULT_SIDE_THREAD_TITLE
        : sideThreadTitle({ ...conversation, id: threadId, sideConversation: true }, label, known?.title);
      const entry = {
        threadId, parentThreadId: parentId,
        title: title || known?.title || "侧边聊天",
        ephemeral,
        createdAtMs: known?.createdAtMs || Date.now()
      };
      if (manager.__codexElvesRequestClientManager)
        requestClientSideThreads.set(threadId, entry);
      else if (!known) nativeSideThreads.set(threadId, entry);
      entries.push(entry);
    }
    return entries;
  }
  function rememberRequestClientSideThread(threadId, parentThreadId, title = "侧边聊天") {
    if (!manager?.__codexElvesRequestClientManager || typeof threadId !== "string"
        || typeof parentThreadId !== "string" || !threadId || !parentThreadId
        || threadId === parentThreadId) return null;
    const current = requestClientSideThreads.get(threadId);
    if (current?.parentThreadId && current.parentThreadId !== parentThreadId) return null;
    const entry = {
      threadId, parentThreadId,
      title: typeof title === "string" && title.trim() ? title.trim() : current?.title || "侧边聊天",
      createdAtMs: current?.createdAtMs || Date.now()
    };
    requestClientSideThreads.set(threadId, entry);
    return entry;
  }
  function requestClientSideView(threadId, parentThreadId, title = "侧边聊天") {
    const entry = rememberRequestClientSideThread(threadId, parentThreadId, title);
    if (!entry) return null;
    return { readOnly: false, viewKind: "side", parentThreadId: entry.parentThreadId,
      title: entry.title, ephemeral: true, sideThreadClosed: false,
      sideHistoryVersion: 1, inheritedTurnIds: [] };
  }
  function syncNativeSideThreads(native) {
    const sourceManager = manager, scopes = nativeSideScopes(native), next = new Map(), nodes = new Set();
    for (const scope of scopes) {
      const parentThreadId = scope.value.conversationId;
      nodes.add(scope.node);
      for (const placement of ["right", "bottom"]) {
        const controller = native.panel(placement);
        for (const tab of scope.get(controller.tabs$)) {
          const id = tab.props?.content?.conversationId ?? tab.props?.conversationId;
          const conversation = id && sourceManager.getConversation(id);
          if (tab.tabType?.kind !== "local-side-chat" || typeof id !== "string" || !id
              || id === parentThreadId || subagentIdentity(conversation)) continue;
          // 新版 request-client 的列表快照不包含临时会话；父关系由原生页面及其标签确认。
          if (!sourceManager.__codexElvesRequestClientManager
              && (!conversation?.sideConversation || conversation.forkedFromId !== parentThreadId)) continue;
          const previous = nativeSideThreads.get(id);
          const ephemeral = conversation?.ephemeral ?? previous?.ephemeral;
          const title = ephemeral === true ? cleanThreadTitle(tab.title) || DEFAULT_SIDE_THREAD_TITLE
            : sideThreadTitle({ ...conversation, id, sideConversation: true }, tab.title, previous?.title);
          next.set(id, { threadId: id, parentThreadId, title,
            createdAtMs: timestamp(conversation?.createdAt) || previous?.createdAtMs || Date.now(),
            ephemeral, closed: false, scope, controller, tab });
        }
      }
      if (!sideScopeWatchers.has(scope.node)) {
        const watcher = { dispose: null };
        sideScopeWatchers.set(scope.node, watcher);
        watcher.dispose = scope.watch(({ get }) => {
          get(native.panel("right").tabs$); get(native.panel("bottom").tabs$);
          queueMicrotask(() => {
            if (manager === sourceManager && !sourceManager.disposed && sideScopeWatchers.get(scope.node) === watcher)
              syncNativeSideThreads(native);
          });
        });
      }
    }
    for (const [node, watcher] of sideScopeWatchers) if (!nodes.has(node)) {
      watcher.dispose?.(); sideScopeWatchers.delete(node);
    }
    const parents = new Set([...nativeSideThreads.values(), ...next.values()].map(entry => entry.parentThreadId));
    for (const parentThreadId of parents) {
      const before = [...nativeSideThreads.values()].filter(entry => entry.parentThreadId === parentThreadId);
      const after = [...next.values()].filter(entry => entry.parentThreadId === parentThreadId);
      if (JSON.stringify(before.map(entry => [entry.threadId, entry.title]))
          === JSON.stringify(after.map(entry => [entry.threadId, entry.title]))) continue;
      emit({ method: "remote/relatedThreadsChanged", params: { threadId: parentThreadId,
        closedThreadIds: before.filter(entry => !next.has(entry.threadId)).map(entry => entry.threadId) } });
    }
    nativeSideThreads.clear();
    for (const [id, entry] of next) nativeSideThreads.set(id, entry);
    for (const [id] of next) {
      const conversation = sourceManager.getConversation(id);
      if (conversation) maybeNameSideThreadFromFirstMessage(id, conversation);
    }
    return [...next.values()];
  }
  async function sideThreadEntries() {
    const entries = syncNativeSideThreads(await nativeSideState());
    if (manager.__codexElvesRequestClientManager) {
      const known = new Set(entries.map(entry => entry.threadId));
      for (const entry of domSideThreadEntries()) {
        if (!known.has(entry.threadId)) entries.push(entry);
      }
    }
    return entries;
  }
  async function nativeSideScope(parentThreadId, native) {
    const find = () => nativeSideScopes(native).find(scope => scope.value.conversationId === parentThreadId);
    let scope = find();
    if (scope) return scope;
    if (!nativeNavigate) throw failure(503, "原生导航尚未就绪，请先在 Codex 打开此主会话");
    await nativeNavigate(`/local/${encodeURIComponent(parentThreadId)}`);
    const deadline = Date.now() + 10000;
    while (!(scope = find()) && Date.now() < deadline)
      await new Promise(resolve => setTimeout(resolve, 100));
    if (!scope) throw failure(503, "Codex 主会话页面尚未就绪，请稍后重试");
    return scope;
  }
  function nativeSideIdentity(thread, entries) {
    const entry = entries.find(entry => entry.threadId === thread.id);
    if (!entry) return null;
    if (manager.__codexElvesRequestClientManager) {
      return { readOnly: false, viewKind: "side", parentThreadId: entry.parentThreadId,
        title: entry.title, ephemeral: true, sideThreadClosed: false,
        sideHistoryVersion: 1, inheritedTurnIds: [] };
    }
    const conversation = manager.getConversation(thread.id);
    if (!conversation) return null;
    // 临时会话的 thread/read 不返回 fork 来源；父关系由原生内存会话和侧边标签共同确认。
    if (conversation?.sideConversation !== true || conversation.forkedFromId !== entry.parentThreadId
        || subagentIdentity(thread) || subagentIdentity(conversation))
      throw failure(503, "原生侧边会话父子关系已变化");
    return { readOnly: false, viewKind: "side", parentThreadId: entry.parentThreadId,
      title: entry.title, ephemeral: conversation.ephemeral === true,
      sideThreadClosed: false, sideHistoryVersion: 1, inheritedTurnIds: [] };
  }
  async function threadViewIdentity(thread) {
    const identity = subagentIdentity(thread) || subagentIdentity(manager.getConversation(thread.id));
    if (identity) {
      let name = identity.agentName;
      try {
        const native = await nativeRelatedState();
        const spawn = (thread.source?.subAgent ?? thread.source?.subagent)?.thread_spawn;
        name = (typeof spawn?.agent_path === "string" ? native.formatAgentPath(spawn.agent_path) : null) || name;
      } catch (error) {
        reportSideFeatureDegraded("thread.subagent_name", error, { threadId: thread.id });
      }
      return { ...identity, agentName: name || "子代理", ephemeral: nativeThreadEphemeral(thread) };
    }
    if (manager.__codexElvesRequestClientManager) {
      let entries = [];
      try { entries = [...await sideThreadEntries(), ...domSideThreadEntries()]; }
      catch (error) {
        if (thread.ephemeral === true) throw error;
        reportSideFeatureDegraded("thread.side_identity", error, { threadId: thread.id });
      }
      const side = nativeSideIdentity(thread, entries);
      if (side) return side;
      const confirmed = requestClientSideThreads.get(thread.id);
      if (thread.ephemeral === true && confirmed)
        return requestClientSideView(thread.id, confirmed.parentThreadId, confirmed.title);
      return { readOnly: false, viewKind: "main", parentThreadId: null };
    }
    let sideEntries = [];
    try {
      sideEntries = await sideThreadEntries();
    } catch (error) {
      const conversation = manager.getConversation(thread.id);
      const isSide = thread?.sideConversation === true || conversation?.sideConversation === true;
      if (isSide) throw error;
      reportSideFeatureDegraded("thread.side_identity", error, { threadId: thread.id });
    }
    const side = nativeSideIdentity(thread, sideEntries);
    if (!side && thread.ephemeral === true) throw failure(404, "原生临时侧边会话已关闭");
    return side ?? { readOnly: false, viewKind: "main", parentThreadId: null };
  }
  function disposeRequestClientSubagentState(record) {
    record.disposed = true;
    record.cancel?.();
    try { record.request?.[Symbol.dispose]?.(); } catch {}
    try { record.watch?.(); } catch {}
    record.request = null;
    record.watch = null;
  }
  function clearRequestClientSubagentStates() {
    for (const record of requestClientSubagentStates.values()) disposeRequestClientSubagentState(record);
    requestClientSubagentStates.clear();
  }
  async function requestClientSubagentSnapshot(threadId, native, timeoutMs = 4000) {
    const sourceManager = manager, scope = appScope;
    if (typeof scope?.watch !== "function" || typeof native.snapshotFamily?.read !== "function")
      throw failure(503, "当前 Codex 版本的原生子代理状态订阅不可用");
    for (const [id, previous] of requestClientSubagentStates) {
      if (previous.manager === sourceManager && previous.scope === scope) continue;
      requestClientSubagentStates.delete(id);
      disposeRequestClientSubagentState(previous);
    }
    let record = requestClientSubagentStates.get(threadId);
    if (record && (record.manager !== sourceManager || record.scope !== scope || record.native !== native)) {
      requestClientSubagentStates.delete(threadId);
      disposeRequestClientSubagentState(record);
      record = null;
    }
    if (!record) {
      if (requestClientSubagentStates.size >= 8) {
        const [oldId, oldest] = requestClientSubagentStates.entries().next().value;
        requestClientSubagentStates.delete(oldId);
        disposeRequestClientSubagentState(oldest);
      }
      record = { manager: sourceManager, scope, native, disposed: false, snapshot: null,
        rows: [], nativeSnapshot: null, updatedAt: 0, promise: null, signature: null };
      requestClientSubagentStates.set(threadId, record);
      const key = { conversationId: threadId, hostId: "local" };
      const current = () => !record.disposed && manager === sourceManager && !sourceManager.disposed
        && scope === appScope && requestClientSubagentStates.get(threadId) === record && !isDeletedThread(threadId);
      try {
        record.watch = scope.watch(({ get }) => {
          if (!current()) return;
          record.nativeSnapshot = get(native.snapshotFamily, key);
          record.rows = get(native.rowsFamily, key);
          const signature = JSON.stringify([record.nativeSnapshot?.isComplete, record.nativeSnapshot?.isLoading,
            record.nativeSnapshot?.descendantThreads?.map(thread => thread.id), record.rows]);
          if (signature === record.signature) return;
          const initialized = record.signature != null;
          record.signature = signature;
          if (initialized && !record.queued) {
            record.queued = true;
            queueMicrotask(() => {
              record.queued = false;
              if (current()) emit({ method: "remote/relatedThreadsChanged", params: { threadId } });
            });
          }
        });
      } catch (error) {
        requestClientSubagentStates.delete(threadId);
        disposeRequestClientSubagentState(record);
        throw error;
      }
    }
    const current = () => !record.disposed && manager === sourceManager && !sourceManager.disposed
      && scope === appScope && requestClientSubagentStates.get(threadId) === record && !isDeletedThread(threadId);
    if (!record.promise && (!record.snapshot || Date.now() - record.updatedAt >= 500)) {
      record.promise = (async () => {
        let timer, active = true;
        const discovery = Promise.resolve().then(async () => {
          const rpc = await requestClientNativeRpc();
          if (!active || !current()) throw failure(409, "原生子代理连接已变化，请重新读取");
          record.request = rpc.discoverSubagentDescendantSnapshot(threadId, { reconcile: true });
          return await record.request;
        });
        try {
          const snapshot = await Promise.race([discovery, new Promise((_, reject) => {
            record.cancel = () => reject(failure(409, "原生子代理连接已变化，请重新读取"));
            timer = setTimeout(() => reject(failure(503, "原生子代理列表读取超时，请重试")), timeoutMs);
          })]);
          if (!current()) throw failure(409, "原生子代理连接已变化，请重新读取");
          if (!Array.isArray(snapshot?.descendantThreads))
            throw failure(503, "原生子代理关系读取结果无效");
          record.snapshot = snapshot;
          record.updatedAt = Date.now();
        } finally {
          active = false;
          clearTimeout(timer);
          record.cancel = null;
          try { record.request?.[Symbol.dispose]?.(); } catch {}
          record.request = null;
        }
      })().finally(() => { record.promise = null; });
    }
    if (record.promise) await record.promise;
    if (!current()) throw failure(409, "原生子代理连接已变化，请重新读取");
    // 原生行可以先合入新活动，而其 discovery signal 仍保留旧成员集合。
    // 此次 RPC 是新的关系快照；是否完整还须在下游核对它的成员均已有原生行。
    return { snapshot: record.snapshot, rows: record.rows,
      isLoading: record.snapshot.isLoading === true,
      isComplete: record.snapshot.isComplete === true };
  }
  async function relatedSubagents(threadId) {
    const sourceManager = manager, scope = appScope;
    const deadline = Date.now() + 4000;
    let native, timer;
    try {
      native = await Promise.race([nativeRelatedState(), new Promise((_, reject) => {
        timer = setTimeout(() => reject(failure(503, "原生子代理接口加载超时，请重试")), 4000);
      })]);
    } finally { clearTimeout(timer); }
    if (manager !== sourceManager || sourceManager.disposed || appScope !== scope)
      throw failure(409, "原生子代理连接已变化，请重新读取");
    let snapshot, rows, isLoading, isComplete;
    if (sourceManager.__codexElvesRequestClientManager) {
      ({ snapshot, rows, isLoading, isComplete } = await requestClientSubagentSnapshot(threadId, native,
        Math.max(1, deadline - Date.now())));
    } else {
      if (typeof sourceManager.discoverSubagentDescendantSnapshot !== "function")
        throw failure(503, "当前 Codex 版本不支持原生子代理关系读取");
      snapshot = await sourceManager.discoverSubagentDescendantSnapshot(threadId);
      if (!sourceManager.getConversation(threadId)) {
        if (typeof sourceManager.hydrateBackgroundThreads !== "function")
          throw failure(503, "当前 Codex 版本不支持只读加载关联会话");
        await sourceManager.hydrateBackgroundThreads([threadId], { includeTurns: true });
      }
      rows = scope.get(native.rowsFamily, { conversationId: threadId, hostId: "local" });
      isLoading = snapshot?.isLoading === true;
      isComplete = snapshot?.isComplete === true;
    }
    if (manager !== sourceManager || sourceManager.disposed || appScope !== scope)
      throw failure(409, "原生子代理连接已变化，请重新读取");
    if (!Array.isArray(snapshot?.descendantThreads))
      throw failure(503, "原生子代理关系读取结果无效");
    if (!Array.isArray(rows)) throw failure(503, "原生子代理列表尚未就绪");
    const threads = new Map(snapshot.descendantThreads.map(thread => [thread.id, thread]));
    const belongsToParent = id => {
      if (isDeletedThread(id) || !threads.has(id)) return false;
      const seen = new Set([id]);
      for (let current = id; current;) {
        const thread = threads.get(current) || sourceManager.getConversation(current);
        const parentId = subagentIdentity(thread)?.parentThreadId;
        if (parentId === threadId) return true;
        if (!parentId || seen.has(parentId)) return false;
        seen.add(parentId);
        current = parentId;
      }
      return false;
    };
    const sorted = native.sortRows(rows);
    if (!Array.isArray(sorted) || sorted.length !== rows.length
        || sorted.some(row => typeof row?.conversationId !== "string"
          || !["active", "waiting", "done"].includes(row.status)))
      throw failure(503, "原生子代理列表无效");
    const confirmed = sorted.filter(row => belongsToParent(row.conversationId));
    const visibleIds = new Set(confirmed.map(row => row.conversationId));
    const missingRows = snapshot.descendantThreads.some(thread =>
      belongsToParent(thread.id) && !visibleIds.has(thread.id));
    if (snapshot.isComplete === true && missingRows) isLoading = true;
    const now = Date.now();
    const toSubagent = row => {
      const startedAtMs = Number.isFinite(row.startedAtMs) ? row.startedAtMs : null;
      return { id: row.conversationId, name: row.displayName || "子智能体", status: row.status, startedAtMs,
        parentThreadId: subagentIdentity(threads.get(row.conversationId))?.parentThreadId ?? null,
        ephemeral: nativeThreadEphemeral(threads.get(row.conversationId) || { id: row.conversationId }),
        ...(row.status === "done" || startedAtMs == null ? {} : { elapsedMs: Math.max(0, now - startedAtMs) }) };
    };
    const subagents = confirmed.filter(row => row.status !== "done").map(toSubagent);
    const completedSubagents = confirmed.filter(row => row.status === "done").map(toSubagent);
    return { subagents, completedSubagents, completedSubagentCount: completedSubagents.length,
      subagentsComplete: snapshot.isComplete === true && isComplete && !isLoading && !missingRows,
      subagentsLoading: isLoading, subagentsError: null };
  }
  async function relatedThreads(payload) {
    const threadId = requiredId(payload.threadId);
    const parent = await readThread(threadId);
    // 子代理能力独立失败；不能因此丢失侧边会话、阻塞主会话或返回“完整空列表”。
    const agents = relatedSubagents(threadId).catch(error => {
      reportSideFeatureDegraded("related_threads.subagents", error, { threadId });
      return { subagents: [], completedSubagents: [], completedSubagentCount: 0,
        subagentsComplete: false, subagentsLoading: false,
        subagentsError: error?.message || "原生子代理列表暂时不可用" };
    });
    const sideThreads = [];
    let canCreateSideThread = false;
    try {
      const entries = await sideThreadEntries();
      if (manager.__codexElvesRequestClientManager) for (const entry of domSideThreadEntries(threadId)) {
        if (!entries.some(current => current.threadId === entry.threadId)) entries.push(entry);
      }
      for (const entry of entries.filter(entry => entry.parentThreadId === threadId)) {
        const thread = manager.getConversation(entry.threadId)
          || (manager.__codexElvesRequestClientManager ? await readThread(entry.threadId).catch(() => null) : null);
        if (!thread || subagentIdentity(thread)) continue;
        nativeSideIdentity(thread, entries);
        const item = summary(thread);
        sideThreads.push({ id: entry.threadId, title: entry.title, createdAtMs: entry.createdAtMs,
          running: item.priority.active && item.priority.attentionState !== "waiting",
          hasUnreadTurn: completedSideThreadUnread(thread, item) });
      }
      const sideNative = await nativeSideState();
      const sideScope = nativeSideScopes(sideNative).find(scope => scope.value.conversationId === threadId);
      canCreateSideThread = nativeSideFeatureEnabled(sideNative, sideScope)
        && typeof nativeIntl?.formatMessage === "function"
        && !subagentIdentity(parent) && parent.ephemeral !== true && parent.sideConversation !== true
        && !entries.some(entry => entry.threadId === threadId);
    } catch (error) {
      reportSideFeatureDegraded("related_threads.side_threads", error, { threadId });
    }
    return { ok: true, threadId, ...await agents, sideThreads, canCreateSideThread };
  }
  async function createSideThread(payload) {
    const parentThreadId = requiredId(payload.threadId);
    const parent = await assertWritableThread(parentThreadId);
    const existingEntries = await sideThreadEntries();
    if (parent.ephemeral === true || existingEntries.some(entry => entry.threadId === parentThreadId))
      throw failure(409, "请从主会话创建侧边会话");
    const native = await nativeSideState();
    const scope = await nativeSideScope(parentThreadId, native);
    if (typeof nativeIntl?.formatMessage !== "function" || !nativeSideFeatureEnabled(native, scope))
      throw failure(503, "当前 Codex 页面尚未提供原生侧边会话入口");
    const { LocalConversationSideChatTab } = await import(native.componentUrl);
    if (typeof LocalConversationSideChatTab !== "function")
      throw failure(503, "原生侧边会话页面尚未就绪");
    let threadId = null, submitted = false;
    const sourceManager = manager;
    try {
      const conversation = manager.getConversation(parentThreadId);
      submitted = true;
      const id = await native.open(scope, LocalConversationSideChatTab, {
        sourceConversationId: parentThreadId, threadSource: "user", cwd: parent.cwd,
        hostId: "local", collaborationMode: conversation?.latestCollaborationMode ?? null,
        intl: nativeIntl, revealAndFocus: false
      });
      if (id == null) throw failure(409, "Codex 已取消侧边会话创建，请重新打开菜单");
      threadId = requiredId(id);
      if (manager.__codexElvesRequestClientManager) {
        let thread = sourceManager.getConversation(threadId);
        if (!thread) {
          try { thread = await readThread(threadId); }
          catch (error) {
            // request-client 的 thread/list 和 DOM 标签可能晚于原生 open 返回；
            // 原生 open 已返回稳定线程 ID时，不能把这个异步窗口误报为创建未知。
            reportSideFeatureDegraded("create_side_thread.read", error, { threadId, parentThreadId });
          }
        }
        const title = thread?.title || thread?.name || "侧边聊天";
        const confirmed = requestClientSideView(threadId, parentThreadId, title);
        if (!confirmed) throw new Error("原生侧边会话父子关系无法确认");
        try { syncNativeSideThreads(native); }
        catch (error) {
          reportSideFeatureDegraded("create_side_thread.snapshot", error, { threadId, parentThreadId });
        }
        // 标签和 request-client 快照随后会异步出现，后台补齐标题及菜单数据。
        queueMicrotask(() => {
          void sideThreadEntries().then(() => {
            emit({ method: "remote/relatedThreadsChanged", params: { threadId: parentThreadId } });
            emit({ method: "remote/sidebarChanged", params: { threadId, parentThreadId } });
          }).catch(error => reportSideFeatureDegraded(
            "create_side_thread.refresh", error, { threadId, parentThreadId }));
        });
        return { ok: true, threadId, parentThreadId,
          thread: { ...(thread ? summary(thread) : { id: threadId, title: confirmed.title }),
            ...(thread ? runtimeThread(threadId, thread, confirmed) : {}), ...confirmed } };
      }
      const entries = await sideThreadEntries();
      const domEntries = domSideThreadEntries(parentThreadId);
      for (const entry of domEntries) {
        if (!entries.some(current => current.threadId === entry.threadId)) entries.push(entry);
      }
      const thread = sourceManager.getConversation(threadId)
        || await readThread(threadId).catch(() => null);
      if (manager !== sourceManager || sourceManager.disposed || !thread || thread.forkedFromId !== parentThreadId
          || thread.sideConversation !== true || subagentIdentity(thread))
        throw new Error("原生侧边会话身份尚未确认");
      const view = nativeSideIdentity(thread, entries);
      if (!view) throw new Error("原生侧边标签尚未确认");
      return { ok: true, threadId, parentThreadId,
        thread: { ...summary(thread), ...runtimeThread(threadId, thread, view), ...view } };
    } catch (error) {
      if (submitted && (threadId != null || !error.remoteValidationFailure))
        return { ok: false, code: 503, outcomeUnknown: true, threadId, parentThreadId,
          message: "原生侧边会话创建结果未确认，请检查 Codex 侧边标签，不要重复新建" };
      throw error;
    }
  }
  async function closeSideThread(payload) {
    const threadId = requiredId(payload.threadId), parentThreadId = requiredId(payload.parentThreadId);
    if (threadId === parentThreadId) throw failure(400, "不能将主会话作为侧边会话关闭");
    const entries = await sideThreadEntries();
    const entry = entries.find(entry => entry.threadId === threadId && entry.parentThreadId === parentThreadId);
    if (!entry) throw failure(404, "该原生侧边会话已关闭或不属于此主会话");
    const identity = nativeSideIdentity(await readThread(threadId), entries);
    if (identity?.parentThreadId !== parentThreadId) throw failure(409, "侧边会话父子关系已变化");
    const sourceManager = manager;
    // 远程页面已确认关闭。使用原生确认后的 continuation，保留 onClose 的停止、丢弃和关系清理。
    try {
      entry.controller.closeTab(entry.scope, entry.tab.tabId, { recordCloseUndo: false },
        new Set([entry.tab.contentInstance]));
      const deadline = Date.now() + 30000;
      const stillOpen = async () => {
        if (!sourceManager.__codexElvesRequestClientManager) return !!sourceManager.getConversation(threadId);
        const rpc = await requestClientNativeRpc(), request = rpc.getConversation(threadId);
        try { return (await request) != null; }
        finally { request?.[Symbol.dispose]?.(); }
      };
      while (await stillOpen() && Date.now() < deadline) {
        if (sourceManager.disposed || manager !== sourceManager) throw new Error("原生连接已变化");
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      if (sourceManager.disposed || manager !== sourceManager) throw new Error("原生连接已变化");
      if (await stillOpen()) throw new Error("原生侧边会话尚未关闭");
      const cached = requestClientConversations.get(threadId);
      if (cached) { requestClientConversations.delete(threadId); disposeRequestClientConversation(cached); }
      const queued = requestClientQueues.get(threadId);
      if (queued) { requestClientQueues.delete(threadId); disposeRequestClientQueue(queued); }
      requestClientSideThreads.delete(threadId);
      await sideThreadEntries();
      return { ok: true, threadId, parentThreadId, closed: true };
    } catch {
      return { ok: false, code: 503, outcomeUnknown: true, threadId, parentThreadId,
        message: "原生侧边会话关闭结果未确认，请检查 Codex 侧边标签，不要重复关闭" };
    }
  }
  function forgetDeletedThread(id) {
    if (typeof id !== "string" || !id) return;
    const canonicalId = id.toLowerCase();
    deletedThreadIds.add(canonicalId);
    const matches = value => typeof value === "string" && value.toLowerCase() === canonicalId;
    for (const cache of [nativeCatalog, sidebarSignatures, asyncQuestionSignatures, pendingThreadMetadataReads,
      blockingRequestSnapshots, asyncQuestionRecoveries, observedRequestThreads, queuedReorders]) {
      for (const key of cache.keys()) if (matches(key)) cache.delete(key);
    }
    for (const key of questionReplyStore.keys()) {
      try { if (matches(JSON.parse(key)?.[0])) questionReplyStore.delete(key); } catch {}
    }
    for (const [editId, entry] of queuedEditStore.sessions)
      if (matches(entry?.threadId)) queuedEditStore.sessions.delete(editId);
    const conversation = requestClientConversations.get(id);
    if (conversation) {
      requestClientConversations.delete(id);
      disposeRequestClientConversation(conversation);
    }
    const queued = requestClientQueues.get(id);
    if (queued) {
      requestClientQueues.delete(id);
      disposeRequestClientQueue(queued);
    }
    for (const [key, record] of requestClientSubagentStates) if (matches(key)) {
      requestClientSubagentStates.delete(key);
      disposeRequestClientSubagentState(record);
    }
  }
  async function deleteRequestClientThread(id) {
    const sourceManager = manager;
    const client = sourceManager.requestClient;
    const rpc = await requestClientNativeRpc();
    if (manager !== sourceManager || sourceManager.disposed)
      throw failure(409, "原生会话连接已变化，请核对会话后再删除");
    // 原生 RPC 同时处理 attached threads；部分删除或送达不明绝不重放。
    try {
      await rpc.deleteConversation(id);
    } catch (error) {
      const missingId = String(error?.message || error)
        .match(/no rollout found for thread id ([0-9a-f-]{36})$/i)?.[1];
      if (missingId?.toLowerCase() !== id.toLowerCase()
          || typeof client.sendAppServerExtensionRequest !== "function") throw error;
      await client.sendAppServerExtensionRequest("thread/delete", {
        threadId: id, missingRolloutRecovery: true
      });
    }
    return { status: "local_deleted", session_id: id };
  }
  async function deleteThread(payload) {
    if (!payload || typeof payload !== "object" || Array.isArray(payload)
        || Object.keys(payload).length !== 1 || !Object.hasOwn(payload, "threadId")
        || typeof payload.threadId !== "string"
        || !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(payload.threadId))
      throw failure(400, "永久删除需要已创建完成的本机会话 ID");
    const id = payload.threadId;
    if (isDeletedThread(id)) return { ok: true, threadId: id, deleted: true, status: "local_deleted" };
    const remove = manager.__codexElvesRequestClientManager
      ? () => deleteRequestClientThread(id) : window.__codexElvesNativeDeleteThread;
    if (typeof remove !== "function" || manager?.hostId !== "local"
        || typeof manager.requestClient?.sendRequest !== "function")
      throw failure(503, "当前 Codex 版本的原生永久删除服务不可用，请在桌面检查增强功能");
    let result;
    try {
      result = await remove({ session_id: `local:${id}` });
    } catch (error) {
      if (error?.delivery?.stage === "not-sent")
        throw failure(503, error.message || "永久删除请求未发送");
      return { ok: false, threadId: id, outcomeUnknown: true, code: 503,
        message: "永久删除结果未确认，请在桌面检查会话，不要重复删除" };
    }
    if (!["local_deleted", "partial"].includes(result?.status)
        || typeof result.session_id !== "string" || result.session_id.toLowerCase() !== id.toLowerCase())
      return { ok: false, threadId: id, outcomeUnknown: true, code: 503,
        message: "原生永久删除回执无法确认，请在桌面检查会话，不要重复删除" };
    emit({ method: "thread/deleted", params: { threadId: id } });
    emit({ method: "remote/sidebarChanged", params: { threadId: id } });
    return { ok: true, threadId: id, deleted: true, status: result.status,
      ...(result.status === "partial" ? {
        message: typeof result.message === "string" ? result.message : "会话已永久删除，但最近列表更新失败"
      } : {}) };
  }
  function optionalCursor(value, label) {
    if (value == null || value === "") return null;
    if (typeof value !== "string" || value.length > 4096) throw failure(400, `${label}无效`);
    return value;
  }
  async function nativeItemsPage(threadId, turnId, cursor, sortDirection = "desc") {
    let limit = HISTORY_ITEM_PAGE_LIMIT, page;
    for (;;) {
      try {
        page = await manager.requestClient.sendRequest("thread/items/list", {
          threadId, turnId, cursor, limit, sortDirection
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
      const displayItem = nativeDisplayMessageItem(item);
      if (typeof displayItem.id === "string" && Number.isFinite(entry?.startedAtMs))
        itemStartedAtMsById[displayItem.id] = entry.startedAtMs;
      return Number.isFinite(entry?.completedAtMs)
        ? { ...displayItem, completedAtMs: entry.completedAtMs } : displayItem;
    });
    if (sortDirection === "desc") items.reverse();
    return {
      items, nextCursor, itemStartedAtMsById,
      pageLimit: limit, newestSnapshotItemId: items.at(-1)?.id ?? null
    };
  }
  function addVisibleTranscriptContext(itemPage, firstPage) {
    const loadedIds = new Set(itemPage.items.map(item => item.id));
    const firstUser = firstPage.items.findIndex(item => item.type === "userMessage");
    const context = firstPage.items.filter((item, index) =>
      !loadedIds.has(item.id)
      && (index <= firstUser || item.type === "agentMessage")
    ).map(item => ({ ...item, remoteHistoryContext: true }));
    if (!context.length) return;
    itemPage.items.unshift(...context);
    itemPage.itemStartedAtMsById = { ...firstPage.itemStartedAtMsById, ...itemPage.itemStartedAtMsById };
  }
  function executingTurnStatus(value) {
    const status = typeof value === "string" ? value : value?.type;
    return ["active", "inProgress", "running", "pending"].includes(status);
  }
  async function nativeCompleteItemsPage(threadId, turnId, cursor = null) {
    const pages = [], seenCursors = new Set(), itemStartedAtMsById = {};
    let nextCursor = cursor, firstPage = null, pageLimit = HISTORY_ITEM_PAGE_LIMIT;
    for (;;) {
      if (nextCursor != null) {
        if (seenCursors.has(nextCursor))
          throw failure(503, "原生会话消息分页游标循环");
        seenCursors.add(nextCursor);
      }
      const page = await nativeItemsPage(threadId, turnId, nextCursor);
      firstPage ||= page;
      pages.unshift(page.items);
      Object.assign(itemStartedAtMsById, page.itemStartedAtMsById);
      pageLimit = Math.min(pageLimit, page.pageLimit);
      nextCursor = page.nextCursor;
      if (nextCursor == null) break;
    }
    const items = [], loadedIds = new Set();
    for (const page of pages) {
      for (const item of page) {
        const id = typeof item?.id === "string" ? item.id : null;
        if (id && loadedIds.has(id)) continue;
        if (id) loadedIds.add(id);
        items.push(item);
      }
    }
    return {
      items, nextCursor: null, itemStartedAtMsById, pageLimit,
      newestSnapshotItemId: firstPage?.newestSnapshotItemId ?? items.at(-1)?.id ?? null
    };
  }
  function enrichHistoryTurn(threadId, turn, itemPage, { readOnly = false } = {}) {
    const items = [...itemPage.items];
    const input = compactionInputItem(threadId, turn.id, items);
    if (input) {
      const index = items.findIndex(item => item.id === input.id);
      if (index >= 0) items.splice(index, 1);
      let before = items.findIndex(item => item.type === "contextCompaction");
      if (before < 0) before = items.findIndex(item => !["modelChanged", "modelRerouted"].includes(item.type));
      items.splice(before < 0 ? items.length : before, 0, input);
    }
    const enriched = enrichMessageMetadata(threadId, [{
      ...turn,
      items,
      itemsView: itemPage.nextCursor == null ? "full" : "summary"
    }], { itemStartedAtMsById: itemPage.itemStartedAtMsById, readOnly })[0];
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
  function nativeDisplayMessageItem(item) {
    if (!item || item.type !== "steeringUserMessage"
        || item.status !== "accepted" && typeof item.serverUserMessageId !== "string") return item;
    const id = typeof item.serverUserMessageId === "string" && item.serverUserMessageId
      ? item.serverUserMessageId : item.id;
    if (typeof id !== "string" || !id) return item;
    const content = Array.isArray(item.input) ? item.input
      : Array.isArray(item.content) ? item.content : [];
    return {
      ...item,
      type: "userMessage",
      id,
      ...(typeof (item.clientUserMessageId ?? item.clientId) === "string"
        ? { clientId: item.clientUserMessageId ?? item.clientId } : {}),
      content
    };
  }
  function nativeSideItemsPage(turn, cursor = null) {
    const byId = new Map();
    for (const item of array(turn.items)) {
      const displayItem = nativeDisplayMessageItem(item);
      if (typeof displayItem?.id === "string") byId.set(displayItem.id, displayItem);
    }
    const all = [...byId.values()], prefix = "native-side-item:";
    const end = cursor == null ? all.length
      : cursor.startsWith(prefix) ? all.findIndex(item => item.id === cursor.slice(prefix.length)) : -1;
    if (end < 0) throw failure(409, "侧边会话消息已变化，请重新读取");
    const start = Math.max(0, end - HISTORY_ITEM_PAGE_LIMIT), items = all.slice(start, end);
    if (cursor == null && start > 0) {
      const input = all.slice(0, start).find(item => item.type === "userMessage");
      if (input) items.unshift({ ...input, remoteHistoryContext: true });
    }
    return { items, nextCursor: start > 0 ? `${prefix}${all[start].id}` : null,
      itemStartedAtMsById: turn.commandExecutionStartedAtMsById || {},
      newestSnapshotItemId: items.at(-1)?.id ?? null, pageLimit: HISTORY_ITEM_PAGE_LIMIT };
  }
  function nativeCompleteSideItemsPage(turn, cursor = null) {
    const byId = new Map();
    for (const item of array(turn.items)) {
      const displayItem = nativeDisplayMessageItem(item);
      if (typeof displayItem?.id === "string") byId.set(displayItem.id, displayItem);
    }
    const all = [...byId.values()], prefix = "native-side-item:";
    const end = cursor == null
      ? all.length
      : cursor.startsWith(prefix) ? all.findIndex(item => item.id === cursor.slice(prefix.length)) : -1;
    if (end < 0) throw failure(409, "侧边会话消息已变化，请重新读取");
    const items = all.slice(0, end);
    return {
      items, nextCursor: null, itemStartedAtMsById: turn.commandExecutionStartedAtMsById || {},
      newestSnapshotItemId: items.at(-1)?.id ?? null, pageLimit: HISTORY_ITEM_PAGE_LIMIT
    };
  }
  async function requestClientNativeRpc() {
    if (!requestClientConversationReader) {
      const initial = await nativeModuleSnapshot("initial");
      requestClientConversationReader = nativeFunction(initial.module, source =>
        source.includes("AppServerManager RPC is unavailable for hostId:")
        && source.includes(".forHost("), "会话数据读取");
    }
    return requestClientConversationReader(appScope, "local");
  }
  async function requestClientRuntime() {
    const { module } = await nativeModuleSnapshot("shared");
    const candidates = Object.values(module).filter(value => value && typeof value === "object"
      && typeof value.pinnedThreads?.set === "function" && typeof value.pinnedThreads?.list === "function"
      && "primaryRuntime" in value && "applicationState" in value);
    if (candidates.length !== 1) throw failure(503, "当前 Codex 版本的原生应用状态服务不可用",
      { nativeOnly: true });
    return candidates[0];
  }
  async function requestClientGlobalState(key) {
    let pending = requestClientGlobalStateReader;
    if (!pending) {
      // 只合并原生读取函数的解析；每个键仍在下方读取本次最新值。
      pending = (async () => {
        const { module } = await nativeModuleSnapshot("initial");
        return nativeFunction(module, source => source.includes(".readPinnedProjects(")
          && source.includes(".readMemberships("), "应用状态读取");
      })();
      requestClientGlobalStateReader = pending;
      pending.catch(() => {
        if (requestClientGlobalStateReader === pending) requestClientGlobalStateReader = null;
      });
    }
    const read = await pending;
    const result = await read(key);
    if (!result || !Object.hasOwn(result, "value"))
      throw failure(503, "原生应用状态读取结果无效", { nativeOnly: true });
    return result.value;
  }
  function disposeRequestClientConversation(record) {
    record.disposed = true;
    for (const resource of record.subscriptions || []) {
      try { resource?.[Symbol.dispose]?.(); } catch {}
    }
    record.subscriptions = null;
  }
  function mergeRequestClientState(value, fields) {
    if (!value) return value;
    const state = fields.get("listState");
    if (state) value = { ...value, ...state.summary,
      requests: state.requests, resumeState: state.resumeState,
      currentPermissions: state.currentPermissions,
      latestThreadSettings: state.latestThreadSettings,
      remoteListState: state };
    if (fields.has("runtimeStatus")) value = { ...value, threadRuntimeStatus: fields.get("runtimeStatus") };
    const goal = fields.get("goalState");
    if (goal) value = { ...value, threadGoal: goal.goal,
      completedThreadGoal: goal.completedGoal, threadGoalResumeConfirmation: goal.resumeConfirmation };
    const model = fields.get("modelSettings");
    if (model) value = { ...value, latestModel: model.model, latestReasoningEffort: model.reasoningEffort };
    if (fields.has("tokenUsageInfo")) value = { ...value, latestTokenUsageInfo: fields.get("tokenUsageInfo") };
    return value;
  }
  function subscribeRequestClientConversation(threadId, record, rpc) {
    if (record.subscriptions) return;
    const resources = [];
    record.subscriptions = resources;
    const current = () => !record.disposed && manager === record.manager && !manager.disposed
      && requestClientConversations.get(threadId) === record && record.subscriptions === resources;
    const failed = error => {
      if (!current()) return;
      for (const resource of resources) {
        try { resource?.[Symbol.dispose]?.(); } catch {}
      }
      record.subscriptions = null;
      record.fields.clear();
      record.fieldRevisions.clear();
      record.updatedAt = 0;
      reportNativeCallbackFailure("request_client_thread_subscription", error);
    };
    for (const field of ["listState", "runtimeStatus", "goalState", "modelSettings", "tokenUsageInfo"]) {
      const request = rpc.subscribe({ type: "thread", key: { hostId: "local", threadId }, field,
        listener: safeNativeCallback(`request_client_${field}`, value => {
          if (!current()) return;
          const previousQuestions = record.fields.get("listState")?.unansweredAsyncQuestions;
          record.fields.set(field, value);
          record.fieldRevisions.set(field, ++record.revision);
          if (field === "goalState" && value) record.coldGoal = value.goal;
          record.value = mergeRequestClientState(record.value, record.fields);
          if (field === "listState" && record.value) {
            syncBlockingRequests(threadId, record.value);
            syncAsyncQuestions(threadId, record.value);
            syncSidebarConversationState(threadId, record.value);
            if (JSON.stringify(previousQuestions) !== JSON.stringify(value?.unansweredAsyncQuestions)) {
              record.updatedAt = 0;
              void refreshRequestClientConversation(threadId, { fresh: true })
                .catch(error => reportNativeCallbackFailure("request_client_question_history", error));
            }
          }
          if (field === "runtimeStatus" && value != null)
            emit({ method: "thread/status/changed", params: { threadId, status: value } });
          else if (field === "goalState" && value)
            emit({ method: value.goal ? "thread/goal/updated" : "thread/goal/cleared",
              params: { threadId, goal: value.goal ?? null } });
          else if (field === "tokenUsageInfo" && value)
            emit({ method: "thread/tokenUsage/updated", params: { threadId, tokenUsage: value } });
        })
      });
      resources.push(request);
      request.onRpcBroken?.(failed);
      Promise.resolve(request).then(subscription => {
        if (current()) {
          resources.push(subscription);
          subscription?.onRpcBroken?.(failed);
        } else subscription?.[Symbol.dispose]?.();
      }).catch(failed);
    }
  }
  async function refreshRequestClientConversation(threadId, {
    fresh = false, includeHistory = true, requireHistory = false
  } = {}) {
    const sourceManager = manager;
    let record = requestClientConversations.get(threadId);
    if (record?.manager !== sourceManager) record = null;
    while (record?.promise) {
      await record.promise;
      if (!fresh && (!includeHistory || record.historyKnown)) return record.value;
    }
    if (!fresh && record?.value && (!includeHistory || record.historyKnown)
        && Date.now() - record.updatedAt < 500) return record.value;
    if (!record) {
      if (requestClientConversations.size >= 32) {
        const [id, oldest] = requestClientConversations.entries().next().value;
        requestClientConversations.delete(id); disposeRequestClientConversation(oldest);
      }
      record = { manager: sourceManager, value: null, updatedAt: 0, promise: null,
        fields: new Map(), fieldRevisions: new Map(), revision: 0,
        subscriptions: null, disposed: false, historyKnown: false };
      requestClientConversations.set(threadId, record);
    }
    const current = () => manager === sourceManager && !sourceManager.disposed
      && requestClientConversations.get(threadId) === record && !isDeletedThread(threadId);
    record.promise = (async () => {
      const rpc = await requestClientNativeRpc();
      if (!current()) throw failure(409, "原生会话连接已变化，请重新读取");
      const startedRevision = record.revision;
      const metadata = rpc.getConversation(threadId);
      const history = includeHistory ? rpc.getLoadedConversationHistory(threadId) : null;
      try {
        const [conversation, loaded] = await Promise.all([metadata,
          history == null ? null : Promise.resolve(history).catch(error => {
            if (requireHistory) throw error;
            reportNativeCallbackFailure("request_client_loaded_history", error);
            return null;
          })]);
        if (!current()) throw failure(409, "原生会话连接已变化，请重新读取");
        const currentFields = new Map([...record.fields].filter(([field]) =>
          record.fieldRevisions.get(field) > startedRevision));
        record.fields = currentFields;
        record.fieldRevisions = new Map([...record.fieldRevisions].filter(([, revision]) => revision > startedRevision));
        record.historyKnown = Array.isArray(loaded?.turns);
        record.value = conversation == null ? null : mergeRequestClientState({
          ...conversation,
          // RPC 返回的 canonical history 需要原生 reader 物化，不能把空的 turns 当成没有历史。
          turns: Array.isArray(loaded?.turns) ? loaded.turns
            : !includeHistory ? record.value?.turns ?? array(conversation.turns) : array(conversation.turns)
        }, currentFields);
        if (record.value && Object.hasOwn(record.value, "threadGoal"))
          record.coldGoal = record.value.threadGoal;
        record.updatedAt = Date.now();
        subscribeRequestClientConversation(threadId, record, rpc);
        if (record.value) {
          syncBlockingRequests(threadId, record.value);
          syncAsyncQuestions(threadId, record.value);
          syncSidebarConversationState(threadId, record.value);
        }
        return record.value;
      } finally {
        metadata?.[Symbol.dispose]?.();
        history?.[Symbol.dispose]?.();
      }
    })();
    try { return await record.promise; }
    finally { record.promise = null; }
  }
  function ensureRequestClientEvents() {
    if (requestClientEventSubscription?.manager === manager) return;
    requestClientEventSubscription?.dispose();
    const sourceManager = manager;
    const record = { manager: sourceManager, disposed: false, request: null, subscription: null,
      dispose() {
        if (record.disposed) return;
        record.disposed = true;
        try { record.subscription?.[Symbol.dispose]?.(); } catch {}
        try { record.request?.[Symbol.dispose]?.(); } catch {}
      }
    };
    requestClientEventSubscription = record;
    const current = () => !record.disposed && manager === sourceManager
      && !sourceManager.disposed && requestClientEventSubscription === record;
    const failed = error => {
      record.dispose();
      if (requestClientEventSubscription === record) requestClientEventSubscription = null;
      reportNativeCallbackFailure("request_client_subscription", error);
    };
    void requestClientNativeRpc().then(async rpc => {
      if (!current()) return;
      record.request = rpc.subscribe({ type: "notification", key: { hostId: "local" }, methods,
        listener: safeNativeCallback("request_client_notification", event => {
          if (!current()) return;
          const threadId = event.params?.threadId;
          const cached = requestClientConversations.get(threadId);
          if (cached?.manager === sourceManager) cached.updatedAt = 0;
          if (event.method === "thread/name/updated" && (cached?.manager === sourceManager
              || nativeSideThreads.has(threadId) || requestClientSideThreads.has(threadId))) {
            void refreshRequestClientConversation(threadId, { fresh: true, includeHistory: false })
              .then(conversation => {
                if (current() && conversation) syncSidebarConversationState(threadId, conversation);
              }).catch(error => reportNativeCallbackFailure("request_client_thread_title", error));
          }
          const queued = requestClientQueues.get(event.params?.threadId);
          if (event.method === "thread/tokenUsage/updated" && queued)
            queued.tokenUsage = event.params.tokenUsage;
          if (event.method !== "thread/queue/changed" || !queued) emit(event);
        })
      });
      record.request.onRpcBroken?.(failed);
      const subscription = await record.request;
      if (current()) {
        record.subscription = subscription;
        subscription?.onRpcBroken?.(failed);
      }
      else subscription?.[Symbol.dispose]?.();
    }).catch(failed);
  }
  async function requestClientSideHistory(threadId) {
    const sourceManager = manager, scope = appScope;
    const rpc = await requestClientNativeRpc();
    if (manager !== sourceManager || sourceManager.disposed || scope !== appScope)
      throw failure(409, "原生连接已变化，请重新读取侧边会话");
    // 临时会话不提供 thread/turns/list；通过桌面 RPC 读取其已物化的原生历史。
    const request = rpc.getLoadedConversationHistory(threadId);
    try {
      const history = await request;
      if (manager !== sourceManager || sourceManager.disposed || !Array.isArray(history?.turns))
        throw failure(409, "侧边会话历史正在同步，请稍后重试");
      return { id: threadId, ephemeral: true, turns: history.turns };
    } finally {
      request?.[Symbol.dispose]?.();
    }
  }
  function disposeRequestClientQueue(record) {
    if (record.disposed) return;
    record.disposed = true;
    record.cancel?.();
    clearTimeout(record.timer);
    try { record.subscription?.[Symbol.dispose]?.(); } catch {}
    try { record.request?.[Symbol.dispose]?.(); } catch {}
  }
  async function readRequestClientQueue(threadId, { fresh = false } = {}) {
    const sourceManager = manager;
    let record = requestClientQueues.get(threadId);
    const tokenUsage = record?.manager === sourceManager ? record.tokenUsage : null;
    if (record && (fresh || record.manager !== sourceManager)) {
      requestClientQueues.delete(threadId);
      disposeRequestClientQueue(record);
      record = null;
    }
    if (record) { record.accessed = Date.now(); await record.ready; return record.messages; }
    if (requestClientQueues.size >= 64) {
      const [id, oldest] = [...requestClientQueues].sort((a, b) => a[1].accessed - b[1].accessed)[0];
      requestClientQueues.delete(id); disposeRequestClientQueue(oldest);
    }
    record = { manager: sourceManager, messages: null, tokenUsage, accessed: Date.now(), signature: null };
    let resolve, reject;
    record.ready = new Promise((yes, no) => { resolve = yes; reject = no; });
    record.cancel = () => reject(failure(409, "原生队列订阅已变化，请重新读取"));
    // 同一订阅持续接收桌面队列变化，既覆盖本地队列，也覆盖服务端队列。
    record.ready.catch(() => {});
    requestClientQueues.set(threadId, record);
    const current = () => !record.disposed && manager === sourceManager && !sourceManager.disposed
      && requestClientQueues.get(threadId) === record;
    const fail = error => {
      if (requestClientQueues.get(threadId) === record) requestClientQueues.delete(threadId);
      reject(failure(503, error?.message || "原生队列读取暂不可用"));
      disposeRequestClientQueue(record);
    };
    record.timer = setTimeout(() => fail(new Error("原生队列状态读取超时")), 10000);
    try {
      const rpc = await requestClientNativeRpc();
      if (!current()) throw failure(409, "会话连接已变化，请重新读取队列");
      record.request = rpc.subscribe({
        type: "thread", key: { hostId: "local", threadId }, field: "queuedMessages",
        listener(messages) {
          if (!current() || !Array.isArray(messages)) return;
          const signature = JSON.stringify(messages);
          record.messages = messages;
          clearTimeout(record.timer);
          resolve(messages);
          if (signature === record.signature) return;
          record.signature = signature;
          pushNotification({ method: "thread/queue/changed",
            params: { threadId, ...queueSnapshotFields(threadId, messages) } });
        }
      });
      record.request.onRpcBroken?.(fail);
      Promise.resolve(record.request).then(subscription => {
        if (current()) {
          record.subscription = subscription;
          subscription?.onRpcBroken?.(fail);
        }
        else subscription?.[Symbol.dispose]?.();
      }).catch(fail);
    } catch (error) { fail(error); }
    await record.ready;
    return record.messages;
  }
  function requestClientTurnCoordinator(sourceManager) {
    const call = async (method, id, ...params) => {
      const rpc = await requestClientNativeRpc();
      const coordinator = await rpc.getTurnCoordinator();
      try {
        if (manager !== sourceManager || sourceManager.disposed)
          throw failure(409, "原生连接已变化，请重新读取队列");
        const result = await coordinator[method](id, ...params);
        // 原生已确认发送；保留现有订阅接收后续变化，不让首帧快照阻塞引导回执。
        if (method === "sendQueuedMessageNow" && result?.status === "sent")
          return { ...result, queueSyncPending: true };
        try {
          await readRequestClientQueue(id, { fresh: true });
        } catch (error) {
          throw Object.assign(error instanceof Error ? error : new Error(String(error)), {
            queueMutationAcknowledged: true, queueMutationResult: result,
            queueConfirmationStage: "queue-snapshot"
          });
        }
        return result;
      } finally { coordinator?.[Symbol.dispose]?.(); }
    };
    const prepare = async (id, message, phase, verifiedClientUserMessageId = null) => {
      if (!requestClientQueuePrepare) {
        const initial = await nativeModuleSnapshot("initial");
        requestClientQueuePrepare = nativeFunction(initial.module, source =>
          source.includes("inheritNativeWorkspacePermissions")
          && source.includes("allowStartAfterNoActiveTurn"), "队列消息准备");
      }
      const rpc = await requestClientNativeRpc();
      const context = await rpc.getFollowUpContext(id, false);
      try {
        const submission = await requestClientQueuePrepare({
          scope: appScope, managerHostId: "local", hostId: "local", targetConversationId: id,
          context: message.context, cwd: message.cwd || context?.cwd,
          clientUserMessageId: requiredId(verifiedClientUserMessageId ?? message.clientUserMessageId
            ?? message.submissionOptions?.clientUserMessageId ?? message.id),
          turnTrigger: "send_user_message", agentMode: "auto", permissionProfileId: null,
          shouldSendPermissionOverrides: false, restoreMessage: message,
          activeCollaborationMode: message.submissionOptions?.collaborationMode ?? context?.latestCollaborationMode
        }, phase);
        return { status: "ready", submission };
      } finally { context?.[Symbol.dispose]?.(); }
    };
    return {
      readMessages: id => requestClientQueues.get(id)?.messages,
      loadMessages: id => readRequestClientQueue(id),
      removeQueuedMessage: (id, messageId) => call("removeQueuedMessage", id, messageId),
      restoreQueuedMessage: (id, token) => call("restoreQueuedMessage", id, token),
      reorderQueuedMessages: (id, messageIds) => call("reorderQueuedMessages", id, messageIds),
      sendQueuedMessageNow: (id, messageId, options, callback, ...admission) =>
        call("sendQueuedMessageNow", id, messageId, options, callback, ...admission),
      sendMessage: async (request, callback, onMessageAdded, assertRequestCurrent, assertAdmission, completion = null) => {
        const rpc = await requestClientNativeRpc();
        const coordinator = await rpc.getTurnCoordinator();
        const context = await rpc.getFollowUpContext(request.conversationId, false);
        try {
          const requiresOwner = context?.ephemeral === true || context?.sideConversation === true
            || request.message?.requiresExistingOwner === true;
          const queuedRequest = requiresOwner ? { ...request, acceptLocally: false,
            message: { ...request.message, requiresExistingOwner: true } } : request;
          const result = await coordinator.sendMessage(queuedRequest,
            callback ?? (async (queued, phase) => (await prepare(queued.conversationId, queued.message, phase)).submission),
            onMessageAdded, assertRequestCurrent, assertAdmission);
          // sent 可由调用方继续核对接受历史；queued 仍等待队列快照。
          if (completion?.deferQueueSnapshotOnSent === true && result?.status === "sent")
            return { ...result, queueSyncPending: true };
          try {
            await readRequestClientQueue(request.conversationId, { fresh: true });
          } catch (error) {
            throw Object.assign(error instanceof Error ? error : new Error(String(error)), {
              queueMutationAcknowledged: true, queueMutationResult: result,
              queueConfirmationStage: "queue-snapshot"
            });
          }
          return result;
        } finally {
          context?.[Symbol.dispose]?.();
          coordinator?.[Symbol.dispose]?.();
        }
      },
      execution: {
        prepare,
        async queuedEditInputs(message) {
          const initial = await nativeModuleSnapshot("initial");
          let pending = nativeQueuedEditInputContracts.get(initial.module);
          if (!pending) {
            pending = (async () => {
              const from = initial.source.indexOf("function w8t(");
              const to = initial.source.indexOf("function ", from + 13);
              const digest = from < 0 || to < 0 ? null : await crypto.subtle.digest("SHA-256",
                new TextEncoder().encode(initial.source.slice(from, to)));
              if (!digest || hexBytes(new Uint8Array(digest))
                  !== "d81d805c7b3d8cccb567d8a1c4a0db3a7372608e1a5f70a369527820d7cabafc")
                throw failure(503, "当前 Codex 版本的队列取消输入合成合同尚未核准");
              return nativeFunction(initial.module, source => source.includes("isRemoteHost:")
                && source.includes("localTurnMetadata:") && source.includes("promptContextWithoutReviewedMessages:")
                && source.includes("responseItems:"), "队列原始输入序列化");
            })();
            nativeQueuedEditInputContracts.set(initial.module, pending);
            pending.catch(() => {
              if (nativeQueuedEditInputContracts.get(initial.module) === pending)
                nativeQueuedEditInputContracts.delete(initial.module);
            });
          }
          const serialize = await pending;
          return [false, true].map(isDurableThread => serialize({
            context: message.context, isRemoteHost: false, isDurableThread, callId: `untrusted_input_${message.id}`
          }));
        }
      },
      async assertQueuedEditContext(id) {
        const rpc = await requestClientNativeRpc();
        const [hostId, role, context, conversation] = await Promise.all([
          rpc.getHostId(), rpc.getStreamRole(id), rpc.getFollowUpContext(id, false), rpc.getConversation(id)
        ]);
        try {
          if (manager !== sourceManager || sourceManager.disposed || hostId !== "local"
              || role?.role !== "owner" || !context || !conversation || context.isClosedSubagent
              || context.ephemeral || context.sideConversation
              || context.needsResume || ["resuming", "needs_resume"].includes(context.resumeState)
              || context.threadStartKind === "aeon" || context.threadSource === "aeon"
              || nativeTurnStatus(nativeConversationTurns(conversation).at(-1)) === "interrupted")
            throw failure(409, "此会话的原生宿主、队列模式或所有权不支持安全恢复队列消息");
        } finally { context?.[Symbol.dispose]?.(); }
      },
      serverQueue: {
        async isEnabled(id) {
          try { return (await readServerQueuedSubmissions(id)).length > 0; } catch { return false; }
        }
      }
    };
  }
  function nativeSideHistoryPage(payload, readOnly, conversation = manager.getConversation(payload.threadId)) {
    const threadId = requiredId(payload.threadId);
    if (!conversation?.ephemeral) throw failure(404, "原生临时会话已关闭");
    const turns = nativeConversationTurns(conversation).filter(turn => nativeTurnId(turn)).slice().reverse();
    const turnId = payload.turnId == null ? null : requiredId(payload.turnId);
    const toPage = (turn, cursor = null) => {
      const startedAt = Number.isFinite(turn.turnStartedAtMs) ? turn.turnStartedAtMs / 1000 : null;
      const completedAt = startedAt != null && Number.isFinite(turn.durationMs)
        ? startedAt + turn.durationMs / 1000 : null;
      const itemPage = executingTurnStatus(nativeTurnStatus(turn))
        ? nativeCompleteSideItemsPage(turn, cursor) : nativeSideItemsPage(turn, cursor);
      return { itemPage, turn: enrichHistoryTurn(threadId, {
        id: nativeTurnId(turn), status: nativeTurnStatus(turn),
        startedAt, completedAt, error: turn.error ?? null, diff: turn.diff ?? null
      }, itemPage, { readOnly }) };
    };
    if (turnId) {
      const turn = turns.find(turn => nativeTurnId(turn) === turnId);
      if (!turn) throw failure(404, "此轮消息已不在原生临时会话中");
      const page = toPage(turn, optionalCursor(payload.itemCursor, "消息分页游标"));
      return { ok: true, scope: "items", threadId, turnId, data: page.turn.items,
        ephemeral: true,
        itemOrder: "asc", itemDirection: "older", itemsPageComplete: true,
        itemsComplete: page.itemPage.nextCursor == null, itemsNextCursor: page.itemPage.nextCursor,
        newestSnapshotItemId: page.itemPage.newestSnapshotItemId, pageLimit: page.itemPage.pageLimit,
        transcriptEvents: nativeTranscriptEvents(conversation, [turnId]) };
    }
    const cursor = optionalCursor(payload.cursor, "会话分页游标"), prefix = "native-side-turn:";
    const cursorIndex = cursor?.startsWith(prefix)
      ? turns.findIndex(turn => nativeTurnId(turn) === cursor.slice(prefix.length)) : -1;
    if (cursor != null && cursorIndex < 0) throw failure(409, "侧边会话历史已变化，请重新读取");
    const start = cursor == null ? 0 : cursorIndex + 1;
    const pages = turns.slice(start, start + HISTORY_TURN_PAGE_LIMIT).map(turn => toPage(turn));
    const nextCursor = start + pages.length < turns.length && pages.length
      ? `${prefix}${pages.at(-1).turn.id}` : null;
    const data = pages.map(page => page.turn);
    return { ok: true, scope: "turns", threadId, data, nextCursor, backwardsCursor: null,
      turnOrder: "desc", itemOrder: "asc", turnsPageComplete: true, turnsComplete: nextCursor == null,
      ephemeral: true, sideHistoryVersion: 1, inheritedTurnIds: [],
      itemsPaginationByTurnId: Object.fromEntries(pages.map(({ turn, itemPage }) => [turn.id, {
        nextCursor: itemPage.nextCursor, hasMore: itemPage.nextCursor != null,
        complete: itemPage.nextCursor == null, pageComplete: true, direction: "older", order: "asc",
        newestSnapshotItemId: itemPage.newestSnapshotItemId, pageLimit: itemPage.pageLimit
      }])), transcriptEvents: nativeTranscriptEvents(conversation, data.map(turn => turn.id)) };
  }
  async function historyPage(payload = {}) {
    const threadId = requiredId(payload.threadId);
    const nativeThread = await readThread(threadId);
    const readOnly = !!subagentIdentity(nativeThread)
      || !!subagentIdentity(manager.getConversation(threadId));
    if (nativeThreadEphemeral(nativeThread) === true) {
      try {
        const conversation = manager.__codexElvesRequestClientManager
          ? await requestClientSideHistory(threadId) : manager.getConversation(threadId);
        return nativeSideHistoryPage(payload, readOnly, conversation);
      } catch (error) {
        throw failure(error?.code || 503, error?.message || "原生侧边会话历史暂不可用",
          { nativeOnly: true });
      }
    }
    const turnId = payload.turnId == null ? null : requiredId(payload.turnId);
    if (turnId != null) {
      const itemCursor = optionalCursor(payload.itemCursor, "消息分页游标");
      const turnStatus = typeof payload.turnStatus === "string" ? payload.turnStatus : "completed";
      const itemPage = executingTurnStatus(turnStatus)
        ? await nativeCompleteItemsPage(threadId, turnId, itemCursor)
        : await nativeItemsPage(threadId, turnId, itemCursor);
      if (!executingTurnStatus(turnStatus) && itemCursor == null && itemPage.nextCursor != null)
        addVisibleTranscriptContext(itemPage, await nativeItemsPage(threadId, turnId, null, "asc"));
      const turn = enrichHistoryTurn(threadId, {
        id: turnId,
        status: turnStatus,
        startedAt: Number.isFinite(payload.turnStartedAt) ? payload.turnStartedAt : null,
        completedAt: Number.isFinite(payload.turnCompletedAt) ? payload.turnCompletedAt : null
      }, itemPage, { readOnly });
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
    let nextCursor = optionalCursor(page.nextCursor, "原生会话分页游标");
    if (nextCursor != null && nextCursor === cursor)
      throw failure(503, "原生会话历史分页游标未前进");
    const pages = new Array(page.data.length);
    let nextTurn = 0, failed = false;
    // 不同轮次独立读取；按原页索引归并，避免首屏等待逐轮叠加，也限制桌面请求并发。
    await Promise.all(Array.from({ length: Math.min(HISTORY_TURN_READ_CONCURRENCY, page.data.length) }, async () => {
      while (!failed && nextTurn < page.data.length) {
        const index = nextTurn++, turn = page.data[index];
        try {
          const pageTurnId = requiredId(turn?.id);
          const itemPage = executingTurnStatus(nativeTurnStatus(turn))
            ? await nativeCompleteItemsPage(threadId, pageTurnId)
            : await nativeItemsPage(threadId, pageTurnId, null);
          if (failed) return;
          if (!executingTurnStatus(nativeTurnStatus(turn)) && itemPage.nextCursor != null) {
            const firstPage = await nativeItemsPage(threadId, pageTurnId, null, "asc");
            addVisibleTranscriptContext(itemPage, firstPage);
          }
          pages[index] = { turn: enrichHistoryTurn(threadId, turn, itemPage, { readOnly }), itemPage };
        } catch (error) {
          failed = true;
          throw error;
        }
      }
    }));
    const data = pages.map(page => page.turn);
    const itemsPaginationByTurnId = Object.fromEntries(pages.map(({ turn, itemPage }) => [turn.id, {
        nextCursor: itemPage.nextCursor,
        hasMore: itemPage.nextCursor != null,
        complete: itemPage.nextCursor == null,
        pageComplete: true,
        direction: "older",
        order: "asc",
        newestSnapshotItemId: itemPage.newestSnapshotItemId,
        pageLimit: itemPage.pageLimit
    }]));
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
    if (manager.__codexElvesRequestClientManager)
      await refreshRequestClientConversation(id, { fresh: true, includeHistory: false });
    // 已读仅更新已有会话的摘要；保留完整目录，避免下一次侧栏读取重新遍历全部分页。
    emit({ method: "remote/sidebarChanged", params: { threadId: id, reason: "read" } });
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
    let id = null, dispatched = false;
    try {
      if (!payload || typeof payload !== "object" || Array.isArray(payload)
          || Object.keys(payload).some(key => !["threadId", "objective", "expectedCreatedAt",
            "expectedObjective", "expectedStatus", "operation"].includes(key)))
        throw failure(400, "目标操作参数无效");
      id = requiredId(payload.threadId);
      const operation = payload.operation === undefined ? "edit" : payload.operation;
      if (!["edit", "pause", "resume", "delete"].includes(operation))
        throw failure(400, "目标操作无效");
      const objective = typeof payload.objective === "string" ? payload.objective.trim() : "";
      if (!objective || new TextEncoder().encode(objective).byteLength > 16 * 1024)
        throw failure(400, "目标内容不能为空或超过 16 KiB");
      if (!Number.isSafeInteger(payload.expectedCreatedAt) || payload.expectedCreatedAt < 0
          || typeof payload.expectedObjective !== "string" || !payload.expectedObjective.trim()
          || new TextEncoder().encode(payload.expectedObjective.trim()).byteLength > 16 * 1024
          || !["active", "paused", "blocked", "budgetLimited", "usageLimited"].includes(payload.expectedStatus))
        throw failure(400, "目标编辑基线无效");
      if (operation !== "edit" && payload.objective !== payload.expectedObjective)
        throw failure(400, "目标控制操作不能修改目标内容");
      if (operation === "pause" && payload.expectedStatus !== "active"
          || operation === "resume" && !["paused", "blocked"].includes(payload.expectedStatus))
        throw failure(400, "当前目标状态不支持此操作");
      const thread = await readThread(id);
      const { goal: nativeGoal } = await manager.requestClient.sendRequest("thread/goal/get", { threadId: id });
      const current = runtimeGoal(nativeGoal);
      if (!current || current.status === "complete") throw failure(409, "目标已结束或不存在，请重新读取会话");
      if (current.createdAt !== payload.expectedCreatedAt
          || current.objective !== payload.expectedObjective.trim()
          || current.status !== payload.expectedStatus)
        throw failure(409, "目标已发生变化，请重新读取后再操作");
      const method = operation === "delete" ? "clearThreadGoal" : "setThreadGoalStatus";
      if (operation !== "edit" && typeof manager[method] !== "function")
        throw failure(503, "当前 Codex 版本的原生目标控制不可用，请更新桌面端");
      if (!manager.getConversation(id) || manager.getConversation(id).resumeState === "needs_resume") {
        await manager.resumeConversation({
          conversationId: id, model: null, reasoningEffort: null,
          workspaceRoots: [thread.cwd], collaborationMode: null
        });
        const { goal } = await manager.requestClient.sendRequest("thread/goal/get", { threadId: id });
        const resumed = runtimeGoal(goal);
        if (!resumed || resumed.createdAt !== current.createdAt
            || resumed.objective !== current.objective || resumed.status !== current.status)
          throw failure(409, "目标已发生变化，请重新读取后再操作");
      }
      dispatched = true;
      if (operation === "delete") {
        await manager.clearThreadGoal(id);
        const response = await manager.requestClient.sendRequest("thread/goal/get", { threadId: id });
        if (!response || response.goal !== null) throw new Error("原生目标删除结果未确认");
        return { ok: true, threadId: id, goal: null };
      }
      const status = operation === "pause" ? "paused" : operation === "resume" ? "active" : current.status;
      const goal = operation === "edit"
        ? (await manager.requestClient.sendRequest("thread/goal/set", {
          threadId: id, objective, status
        })).goal
        : await manager.setThreadGoalStatus(id, { status });
      const updated = runtimeGoal(goal);
      if (!updated || updated.createdAt !== current.createdAt
          || updated.objective !== objective || updated.status !== status
          || operation !== "edit" && updated.tokenBudget !== current.tokenBudget)
        throw new Error("原生目标更新结果无效");
      return { ok: true, threadId: id, goal: updated };
    } catch (error) {
      if (!dispatched || error.remoteValidationFailure || definiteGoalRejection(error))
        throw failure(error.code || 503, error.message || "原生目标操作失败");
      return { ok: false, threadId: id, outcomeUnknown: true, code: 503,
        message: "原生目标操作未确认完成，请检查会话，不要重复操作" };
    }
  }
  function validateGoalComposerPayload(action, payload) {
    if (!payload || typeof payload !== "object" || Array.isArray(payload))
      throw failure(400, "目标提交参数无效");
    if (Object.prototype.hasOwnProperty.call(payload, "goalMode") && typeof payload.goalMode !== "boolean")
      throw failure(400, "目标模式参数无效");
    if (action === "send" && payload.goalMode === true)
      throw failure(400, "目标必须通过目标提交入口发送");
    if (action !== "setGoal" && !(action === "create" && payload.goalMode === true)) return;
    if (payload.goalMode !== true) throw failure(400, "目标提交需要启用目标模式");
    const fields = ["prompt", "goalMode", "skills", "images", "model", "reasoningEffort",
      action === "setGoal" ? "threadId" : "projectId"];
    if (Object.keys(payload).some(key => !fields.includes(key)))
      throw failure(400, "目标提交包含不支持的字段");
    if (typeof payload.prompt !== "string"
        || new TextEncoder().encode(payload.prompt).byteLength > 128 * 1024)
      throw failure(400, "目标内容不能超过 128 KiB");
    for (const key of ["model", "reasoningEffort"]) {
      if (Object.prototype.hasOwnProperty.call(payload, key)
          && (typeof payload[key] !== "string" || !payload[key].trim()
          || payload[key].length > (key === "model" ? 256 : 32)))
        throw failure(400, "目标模型或思考深度无效");
    }
    if (Object.prototype.hasOwnProperty.call(payload, "skills")
        && (!Array.isArray(payload.skills) || payload.skills.length > 16
        || payload.skills.some(skill => !skill || typeof skill !== "object" || Array.isArray(skill)
          || Object.keys(skill).some(key => !["name", "path"].includes(key))
          || typeof skill.name !== "string" || !skill.name || skill.name.length > 256
          || typeof skill.path !== "string" || !skill.path || skill.path.length > 4096)))
      throw failure(400, "Skill 选择无效，最多选择 16 项");
    if (Object.prototype.hasOwnProperty.call(payload, "images") && (!Array.isArray(payload.images)
        || payload.images.some(image => !image || typeof image !== "object" || Array.isArray(image)
          || Object.keys(image).some(key => !["name", "mime", "dataUrl", "size"].includes(key)))))
      throw failure(400, "目标图片包含不支持的字段");
  }
  async function nativeGoalMaterializer() {
    if (!appScope) discover();
    if (!appScope || typeof manager.setThreadGoal !== "function"
        || ["createThreadGoalAttachmentDirectory", "writeThreadGoalAttachment",
          "removeThreadGoalAttachmentDirectory"].some(key => typeof manager[key] !== "function"))
      throw failure(503, "当前 Codex 版本不支持完整目标提交，请更新桌面端");
    if (goalMaterializer) return goalMaterializer;
    const module = await nativeCreationModule();
    const candidates = Object.values(module).filter(value => {
      if (typeof value !== "function") return false;
      const source = Function.prototype.toString.call(value);
      return source.includes("Goal objective must not be empty")
        && source.includes(".createThreadGoalAttachmentDirectory()")
        && source.includes("attachmentDirectory");
    });
    if (candidates.length !== 1)
      throw failure(503, "当前 Codex 版本的原生目标附件服务不可用，请更新桌面端");
    goalMaterializer = candidates[0];
    return goalMaterializer;
  }
  async function goalThreadSettings(payload, loaded = null) {
    if (payload.model == null && payload.reasoningEffort == null) return undefined;
    const conversation = loaded ? manager.getConversation(loaded.id) : null;
    const current = threadModelSettings(conversation, loaded);
    const currentModel = current.model;
    const model = payload.model ?? currentModel ?? (await defaultModelSettings()).defaultModel;
    if (typeof model !== "string" || !model.trim())
      throw failure(409, "无法确定目标使用的模型，请先选择模型");
    const entry = (await nativeModelCatalog()).find(entry => entry.model === model);
    if (!entry) throw failure(409, "当前模型不在原生模型目录中，请重新选择");
    const effort = payload.reasoningEffort === undefined
      ? payload.model != null && payload.model !== currentModel ? null
        : current.effort
      : payload.reasoningEffort;
    if (effort != null
        && !array(entry.supportedReasoningEfforts).some(item => item.reasoningEffort === effort))
      throw failure(400, "思考深度超出此模型支持的范围");
    return { model, effort };
  }
  function goalSubmission(action, threadId = null) {
    return { action, phase: "preparing", threadId, firstTurnAccepted: false, goalSet: "not-attempted" };
  }
  function goalSubmissionFailure(submission, error, unknown = false) {
    return {
      ok: false, ...(submission.threadId ? { threadId: submission.threadId } : {}),
      ...(unknown ? { outcomeUnknown: true } : {}),
      code: unknown ? 503 : error?.code > 0 ? error.code : 503,
      message: unknown ? submission.firstTurnAccepted
        ? "首条消息已送达，但目标保存结果未知，请打开已创建会话确认，不要重新新建或发送"
        : "原生目标提交结果未知，请查看会话确认，不要重复提交"
        : error?.message || "目标提交失败",
      goalSubmission: { ...submission }
    };
  }
  function definiteGoalRejection(error) {
    const code = error?.jsonRpcCode;
    return ([-32700, -32600, -32601, -32602].includes(code)
      && error?.name === `AppServerRequestError(${code})`)
      || (error?.name === "AppServerRequestDeliveryError" && error?.delivery?.stage === "not-sent");
  }
  async function ensureGoalAbsent(id, sourceManager) {
    const response = await sourceManager.requestClient.sendRequest("thread/goal/get", { threadId: id });
    if (!response || !Object.prototype.hasOwnProperty.call(response, "goal"))
      throw failure(503, "无法确认当前目标，请重新读取会话");
    if (response.goal != null && response.goal.status !== "complete")
      throw failure(409, "此会话已有未完成目标，请使用现有目标编辑入口");
  }
  async function cleanupUnsubmittedGoal(sourceManager, materialized) {
    if (materialized?.attachmentDirectory != null)
      await sourceManager.removeThreadGoalAttachmentDirectory(materialized.attachmentDirectory).catch(() => {});
  }
  async function setComposerGoal(payload) {
    const submission = goalSubmission("setGoal");
    let materialized = null, sourceManager = manager, submitted = false, locked = false;
    try {
      validateGoalComposerPayload("setGoal", payload);
      const id = requiredId(payload.threadId);
      submission.threadId = id;
      const previous = goalSubmissions.get(id);
      if (previous) return goalSubmissionFailure(submission,
        failure(409, "此会话的目标提交正在处理，请等待原请求回执"));
      goalSubmissions.set(id, submission); locked = true;
      const thread = await readThread(id);
      sourceManager = manager;
      await ensureGoalAbsent(id, sourceManager);
      const materialize = await nativeGoalMaterializer();
      const imageAttachments = remoteImageAttachments(payload, "goal");
      const objective = payload.prompt.trim();
      if (!objective && !imageAttachments.length) throw failure(400, "目标不能为空");
      await skillInputs(payload, thread.cwd);
      const threadSettings = await goalThreadSettings(payload, thread);
      const assertRequestCurrent = () => {
        if (manager !== sourceManager || sourceManager.disposed)
          throw failure(409, "原生会话连接已变化，请重新读取后提交目标");
      };
      materialized = await materialize({
        scope: appScope, hostId: "local", assertRequestCurrent,
        draft: { objective, imageAttachments, pastedTextAttachments: [] }
      });
      assertRequestCurrent();
      await ensureGoalAbsent(id, sourceManager);
      assertRequestCurrent();
      await applyThreadModelSettings(id, threadSettings);
      assertRequestCurrent();
      submission.phase = "goalSet"; submitted = true;
      const goal = runtimeGoal(await sourceManager.setThreadGoal(id, {
        objective: materialized.objective
      }));
      if (!goal || goal.objective !== materialized.objective || goal.status !== "active")
        throw new Error("原生目标创建结果未确认");
      submission.goalSet = "succeeded"; submission.phase = "complete";
      return { ok: true, threadId: id, goal, goalSubmission: { ...submission } };
    } catch (error) {
      if (!submitted) await cleanupUnsubmittedGoal(sourceManager, materialized);
      const unknown = submitted && !definiteGoalRejection(error);
      if (submitted) submission.goalSet = unknown ? "unknown" : "failed";
      return goalSubmissionFailure(submission, error, unknown);
    } finally {
      // 已终结的未知结果由 HTTP 原回执和远程 pending 保护；此处只锁真正的在途调用。
      if (locked) goalSubmissions.delete(submission.threadId);
    }
  }
  function runtimeCompaction(id, conversation = manager.getConversation(id)) {
    const turns = nativeConversationTurns(conversation);
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
    const turns = !manager.__codexElvesRequestClientManager && conversation?.turnHistory?.kind === "canonical"
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
  function compactionInputItem(threadId, turnId, items = []) {
    const turn = nativeConversationTurns(manager.getConversation(threadId))
      .find(turn => nativeTurnId(turn) === turnId);
    const clientId = turn?.params?.clientUserMessageId;
    if (typeof clientId !== "string" || !clientId) return null;
    const nativeItems = array(turn.items);
    const compaction = nativeItems.findIndex(item => item.type === "contextCompaction"
      && item.source !== "manual" && item.id !== "pending-manual-context-compaction");
    if (compaction < 0) return null;
    const initial = nativeItems.find(item => item.type === "userMessage" && item.clientId === clientId);
    if (initial && nativeItems.indexOf(initial) < compaction) return null;
    if (!initial && (turn.itemsPagination?.hasLoadedOldest === false
        || nativeItems.slice(0, compaction).some(item => !["modelChanged", "modelRerouted"].includes(item.type)))) return null;
    const existing = items.find(item => item.type === "userMessage"
      && (item.clientId === clientId || initial && item.id === initial.id))
      || (!initial && items.some(item => item.id === nativeItems[compaction].id)
        ? items.find(item => item.type === "userMessage") : null);
    if (existing || initial) return { ...(existing || initial), clientId, remoteCompactionInput: true };
    const content = array(turn.params.input).flatMap(input => {
      if (input.type === "text" && typeof input.text === "string") return [{ ...input }];
      if (input.type === "image" && typeof input.url === "string") return [{ ...input }];
      if (input.type === "localImage" && typeof input.path === "string") return [{ ...input }];
      return [];
    });
    if (!content.length) return null;
    return {
      id: `remote-compaction-input-${clientId}`, type: "userMessage", clientId, content,
      remoteCompactionInput: true, remotePendingInput: true
    };
  }
  function pendingNativeMessage(id, conversation, turns) {
    if (array(conversation?.unconfirmedTurnSubmissions).length) return true;
    const queued = manager.__codexElvesRequestClientManager ? requestClientQueues.get(id)?.messages
      : manager.getTurnCoordinator?.()?.readMessages?.(id) ?? manager.storage.readQueuedFollowUps?.()?.value?.[id];
    if (manager.__codexElvesRequestClientManager && !Array.isArray(queued)) return true;
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
    const statusValue = status?.status || status;
    const statusType = typeof statusValue === "string" ? statusValue : statusValue?.type;
    const statusActive = ["active", "inProgress", "running", "pending"].includes(statusType);
    const statusTerminal = ["idle", "stopped", "completed", "failed", "interrupted", "cancelled", "canceled"]
      .includes(statusType);
    if (statusActive || !statusTerminal && turns.some(turn => nativeTurnStatus(turn) === "inProgress"))
      return { reason: "会话正在执行，不能编辑消息", metadata: null };
    if (runtimeCompaction(id, conversation).state !== "idle")
      return { reason: "会话正在压缩上下文，不能编辑消息", metadata: null };
    if (pendingNativeMessage(id, conversation, turns))
      return { reason: "会话仍有待确认或排队消息，不能编辑消息", metadata: null };
    let target = null, targetIndex = -1;
    for (let index = turns.length - 1; index >= 0; index--) {
      const candidate = turns[index];
      const items = array(candidate?.items);
      if (items.some(item => item?.type === "userMessage") || array(candidate?.params?.input).length
          || typeof candidate?.params?.clientUserMessageId === "string" && candidate.params.clientUserMessageId) {
        target = candidate;
        targetIndex = index;
        break;
      }
      if (!Array.isArray(candidate?.items) || !items.length || candidate.itemsPagination?.hasLoadedOldest === false)
        return { reason: "最新轮次的用户消息尚未完整加载，不能编辑更早消息", metadata: null };
      if (items.some(item => item?.type === "steeringUserMessage"))
        return { reason: "最后一条用户消息是补充消息，不能作为初始请求编辑", metadata: null };
    }
    if (targetIndex >= 0 && turns.slice(targetIndex).some(turn =>
      array(turn?.items).some(item => item?.type === "steeringUserMessage")))
      return { reason: "最后一条用户消息是补充消息，不能作为初始请求编辑", metadata: null };
    const turnId = nativeTurnId(target), clientId = target?.params?.clientUserMessageId;
    const messages = array(target?.items).filter(item => item?.type === "userMessage"
      && (typeof clientId !== "string" || !clientId || item.clientId === clientId));
    const message = messages.length === 1 ? messages[0] : null;
    if (!turnId || typeof message?.id !== "string" || !message.id)
      return { reason: "最后一条用户消息缺少原生编辑标识", metadata: null };
    const input = target?.params?.input;
    if (!Array.isArray(input))
      return { reason: "最新消息缺少原生编辑输入，不能编辑更早消息", metadata: null };
    if (!input.length) {
      // 原生 tdn 支持给空 input 添加新文本，但不会从历史用户项恢复附件、
      // 文本引用或请求包装。仅放行已确认的纯文本，保留原生 params 不作补造。
      const content = array(message.content), text = content[0]?.text;
      if (content.length !== 1 || content[0]?.type !== "text" || typeof text !== "string"
          || content[0].text_elements != null
            && (!Array.isArray(content[0].text_elements) || content[0].text_elements.length > 0)
          || /(^|\n)## My request(?: for Codex)?:\r?\n/.test(text)
          || text.trimStart().startsWith("[from: ") || text.includes("<codex_multi_user_message>"))
        return { reason: "最新消息缺少可安全保留附件与上下文的原生编辑输入", metadata: null };
    }
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
    itemStartedAtMsById = null,
    readOnly = false
  } = {}) {
    messageActionManager();
    const conversation = manager.getConversation(id);
    const nativeTurns = nativeConversationTurns(conversation);
    const nativeById = new Map(nativeTurns.flatMap(turn => {
      const turnId = nativeTurnId(turn);
      return turnId ? [[turnId, turn]] : [];
    }));
    const editable = readOnly || subagentIdentity(conversation) ? null : editableMessageState(id).metadata;
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
            ...(item.type === "userMessage" && typeof (item.clientId ?? source?.clientId) === "string"
              ? { clientId: item.clientId ?? source.clientId } : {}),
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
    const loaded = await readThread(id);
    const view = await threadViewIdentity(loaded);
    if (view.viewKind === "side")
      throw failure(403, "侧边会话不支持编辑已发送消息");
    const state = editableMessageState(id);
    if (!state.metadata) throw failure(409, state.reason || "最后一条用户消息当前不可编辑");
    if (payload.turnId !== state.metadata.turnId || payload.messageId !== state.metadata.messageId)
      throw failure(409, "消息已变化，请刷新会话后重试");
    if (typeof payload.prompt !== "string" || !payload.prompt.trim())
      throw failure(400, "消息不能为空");
    const sourceManager = manager;
    const assertCurrent = () => {
      if (manager !== sourceManager || sourceManager.disposed)
        throw failure(409, "原生会话连接已变化，请刷新后再操作");
      const current = editableMessageState(id);
      if (!current.metadata || current.metadata.turnId !== payload.turnId || current.metadata.messageId !== payload.messageId)
        throw failure(409, current.reason || "消息已变化，请刷新会话后重试");
    };
    const conversation = manager.getConversation(id);
    const threadSettings = await requestedThreadModelSettings(payload, loaded, conversation);
    assertCurrent();
    if (threadSettings) {
      try {
        await applyThreadModelSettings(id, threadSettings);
        assertCurrent();
      } catch (error) {
        throw failure(error.code || 503, error.message || "模型或思考深度切换失败，请稍后重试");
      }
    }
    const options = {
      turnId: state.metadata.turnId,
      message: payload.prompt,
      allowEarlierMessages: false,
      agentMode: "default",
      shouldSendPermissionOverrides: false,
      serviceTier: conversation?.latestServiceTier ?? null
    };
    await sourceManager.editLastUserTurn(id, options);
    emit({ method: "remote/threadReverted", params: { threadId: id } });
    return { ok: true, threadId: id, ...state.metadata };
  }
  function acceptedAsyncQuestionReplies(turn, turns = [turn]) {
    const originals = new Map();
    for (const item of array(turn?.items)) {
      if (item?.type !== "agentMessage" || typeof item.id !== "string" || !item.id) continue;
      array(item.questions).forEach((question, index) => {
        if (typeof question?.title === "string")
          originals.set(JSON.stringify(["request_user_input_async", item.id, index]), question.title);
      });
    }
    const answered = new Map();
    const sourceIndex = turns.findIndex(value => nativeTurnId(value) === nativeTurnId(turn));
    const following = sourceIndex >= 0 ? turns.slice(sourceIndex) : [turn];
    for (const item of following.flatMap(value => array(value?.items))) {
      if (item?.type !== "userMessage" && (item?.type !== "steeringUserMessage" || item.status !== "accepted")) continue;
      const input = item.type === "userMessage" ? item.content : item.input;
      if (!Array.isArray(input) || input.length !== 1 || input[0]?.type !== "text") continue;
      let text = String(input[0].text || "");
      // 与原生 aXt 相同，只解析最后一个正式正文块及原生发送者包装。
      const separator = "\n## My request:\n", boundary = text.lastIndexOf(separator);
      text = (boundary >= 0 ? text.slice(boundary + separator.length) : text).trim();
      const open = "<send_user_message_question_reply>", close = "</send_user_message_question_reply>";
      if (!text.startsWith(open)) {
        const sender = /^\[from: ([^\]\r\n]+)\](?:\r?\n|$)/.exec(text);
        if (sender) {
          try { decodeURIComponent(sender[1]); } catch { continue; }
          text = text.slice(sender[0].length).trim();
        } else {
          const start = text.lastIndexOf("<codex_multi_user_message>");
          const end = text.indexOf("</codex_multi_user_message>", start);
          if (start < 0 || end < 0) continue;
          const wrapped = text.slice(start, end + 27);
          const fields = ["account_user_id", "name", "email", "profile_picture_url", "body"]
            .map(name => new RegExp(`<${name}>([\\s\\S]*?)</${name}>`, "i").exec(wrapped)?.[1]);
          if (fields.some(value => value == null)) continue;
          const body = fields[4].replaceAll("&lt;", "<").replaceAll("&gt;", ">")
            .replaceAll("&quot;", "\"").replaceAll("&apos;", "'").replaceAll("&amp;", "&");
          text = `${text.slice(0, start)}${body}${text.slice(end + 27)}`.trim();
        }
      }
      if (!text.startsWith(open) || !text.endsWith(close)) continue;
      try {
        const parsed = JSON.parse(text.slice(open.length, -close.length));
        const replies = Array.isArray(parsed) ? parsed : [parsed];
        if (!replies.length || new Set(replies.map(reply => reply?.questionItemId)).size !== replies.length
            || !replies.every(reply => typeof reply?.questionItemId === "string"
              && typeof reply.question === "string"
              && typeof reply.answer === "string" && reply.answer.trim())) continue;
        for (const reply of replies)
          if (originals.get(reply.questionItemId) === reply.question) answered.set(reply.questionItemId, reply);
      } catch {}
    }
    return answered;
  }
  function asyncQuestionRequests(id, conversation = manager.getConversation(id)) {
    const turns = nativeConversationTurns(conversation);
    const active = turns.findLast(turn => nativeTurnStatus(turn) === "inProgress");
    const native = asyncQuestionNativeBindings;
    const panel = native && appScope?.get(native.panelFamily, { hostId: "local", threadId: id });
    const selected = panel?.selectedQuestionKey;
    const currentTurnId = selected?.hostId === "local" && selected.threadId === id
      && array(panel.questionIds).includes(selected.itemId)
      ? appScope.get(native.questionFamily, selected)?.turnId : null;
    return turns.flatMap(turn => {
      const turnId = nativeTurnId(turn);
      if (!turnId || turn !== active && turnId !== currentTurnId) return [];
      const answered = acceptedAsyncQuestionReplies(turn, turns);
      return array(turn.items).flatMap(item => {
        if (item.type !== "agentMessage" || typeof item.id !== "string" || !item.id) return [];
        const questions = array(item.questions).flatMap((question, index) => {
          const questionId = JSON.stringify(["request_user_input_async", item.id, index]);
          if (typeof question?.title !== "string" || answered.has(questionId)) return [];
          const entry = nativeAsyncQuestionEntry(id, turn, item, question, index, conversation);
          if (asyncQuestionNativeBindings && appScope && !entry) return [];
          // 原生全局通知为每个会话保留自己的面板，不依赖桌面当前路由。
          // 结束后的题仅延续该面板仍持有的准确逐题状态，不能复活历史题。
          if (turn !== active && (!entry?.current || !entry.state)) return [];
          if (entry?.state?.isSkipped === true || entry?.state?.isSubmitting === true
              || entry?.state?.lastSubmission != null) return [];
          return [{ id: questionId, question: question.title,
            options: array(question.options).filter(option => typeof option === "string").map(label => ({ label })) }];
        });
        if (!questions.length) return [];
        const requestId = JSON.stringify(["async-question", turnId, item.id]);
        return [{ id: requestId, method: "remote/requestUserInputAsync",
          params: { threadId: id, turnId, questions,
            submissionState: questionReplyStore.get(JSON.stringify([id, requestId]))?.state ?? null } }];
      });
    });
  }
  function asyncQuestionRequestIdentity(id, request, conversation = manager.getConversation(id)) {
    if (!asyncQuestionNativeBindings || !appScope) return null;
    const turn = nativeConversationTurns(conversation).find(turn => nativeTurnId(turn) === request.params.turnId);
    const keys = request.params.questions.map(question => {
      const [, sourceId, index] = JSON.parse(question.id);
      const item = array(turn?.items).find(item => item.type === "agentMessage" && item.id === sourceId);
      const source = item?.questions?.[index];
      return item && source ? nativeAsyncQuestionEntry(id, turn, item, source, index, conversation)?.key ?? null : null;
    });
    return JSON.stringify([request.params.turnId, request.params.questions, keys]);
  }
  function nativeAsyncQuestionState(module) {
    const cached = asyncQuestionStateModules.get(module);
    if (cached) return asyncQuestionNativeBindings = cached;
    const functions = Object.values(module).filter(value => typeof value === "function")
      .map(value => ({ value, source: Function.prototype.toString.call(value) }));
    const unique = predicate => {
      const matches = functions.filter(predicate);
      if (matches.length !== 1) throw failure(503, "当前 Codex 版本的原生问答状态接口无法唯一识别");
      return matches[0].value;
    };
    const draftSetter = unique(({ source }) => source.length < 500
      && source.includes(".draft!==") && source.includes("draft:"));
    const selectQuestion = unique(({ source }) => source.length < 700
      && [".selectedQuestionKey.entityKey", ".questionIds.includes(", "selectedQuestionKey:"]
        .every(marker => source.includes(marker)));
    const panelSetters = functions.filter(({ value, source }) => value.length === 3 && source.length < 600
      && [".threadId", "new Map(", ".delete(", ".get(", ".set("]
        .every(marker => source.includes(marker)));
    let setPanel = panelSetters.length === 1 ? panelSetters[0].value : null;
    const skipQuestion = unique(({ source }) => source.length < 900
      && ["isSkipped:!0", ".get(", ".set("].every(marker => source.includes(marker)));
    const key = { hostId: "__remote_question_probe__", threadId: "__remote_question_probe__",
      entityKey: "__remote_question_probe__", itemId: "__remote_question_probe__" };
    if (!setPanel && panelSetters.length === 0 && manager.__codexElvesRequestClientManager) {
      // 新版隐藏了面板 setter，关闭单题入口仍导出。用隔离 scope 捕获其实际 Map family，
      // 不写入桌面，也不把只读的派生 panel reader 当成可写状态。
      const closePanel = unique(({ source }) => source.length < 500
        && [".entityKey===", ".itemId===", ".get("].every(marker => source.includes(marker)));
      const reads = [], writes = [];
      closePanel({
        get(family, fieldKey) {
          reads.push({ family, key: fieldKey });
          return reads.length === 1 ? {} : key;
        },
        set(family, fieldKey, update) { writes.push({ family, key: fieldKey, update }); }
      }, key);
      const write = writes[0];
      const sentinel = {};
      const original = new Map([[key.threadId, sentinel], ["other", sentinel]]);
      const updated = typeof write?.update === "function" ? write.update(original) : null;
      if (reads.length !== 2 || writes.length !== 1 || write.key !== key.hostId
          || !write.family || !(updated instanceof Map) || updated.has(key.threadId)
          || updated.get("other") !== sentinel || original.get(key.threadId) !== sentinel)
        throw failure(503, "当前 Codex 版本的原生问答面板状态无法验证");
      setPanel = (scope, panelKey, panel) => scope.set(write.family, panelKey.hostId, current => {
        if ((current.get(panelKey.threadId) ?? null) === panel) return current;
        const next = new Map(current);
        if (panel == null) next.delete(panelKey.threadId);
        else next.set(panelKey.threadId, panel);
        return next;
      });
    }
    if (!setPanel) throw failure(503, "当前 Codex 版本的原生问答面板服务不可用");
    const draftReads = [], panelReads = [], skipReads = [];
    let writes = 0;
    const probe = reads => ({
      get(family, key) { reads.push({ family, key }); return null; },
      set() { writes++; throw new Error("原生问答隔离探针禁止写入"); }
    });
    try {
      draftSetter(probe(draftReads), key, "");
      selectQuestion(probe(panelReads), key);
      skipQuestion(probe(skipReads), key);
    } catch {
      throw failure(503, "当前 Codex 版本的原生问答状态接口验证失败");
    }
    if (writes || draftReads.length !== 1 || panelReads.length !== 2 || skipReads.length !== 1
        || !draftReads[0].family || !panelReads[0].family
        || !["object", "function"].includes(typeof draftReads[0].family)
        || !["object", "function"].includes(typeof panelReads[0].family)
        || panelReads[0].family === draftReads[0].family
        || panelReads[1].family !== draftReads[0].family
        || skipReads[0].family !== draftReads[0].family || skipReads[0].key !== key
        || draftReads[0].key !== key || panelReads[1].key !== key
        || panelReads[0].key?.hostId !== key.hostId || panelReads[0].key?.threadId !== key.threadId)
      throw failure(503, "当前 Codex 版本的原生问答状态接口验证失败");
    const resolved = { questionFamily: draftReads[0].family, panelFamily: panelReads[0].family,
      setPanel, skipQuestion, setDraft: draftSetter };
    asyncQuestionStateModules.set(module, resolved);
    return asyncQuestionNativeBindings = resolved;
  }
  async function prepareAsyncQuestionState() {
    if (asyncQuestionNativeBindings || !appScope) return;
    const conversations = [...manager.getCachedConversations?.() || []].map(entry =>
      Array.isArray(entry) && entry.length === 2 ? entry[1] : entry);
    if (!conversations.some(conversation => nativeConversationTurns(conversation).some(turn =>
      array(turn.items).some(item => item.type === "agentMessage" && array(item.questions).length)))) return;
    nativeAsyncQuestionState(await nativeCreationModule());
  }
  function nativeAsyncQuestionMatches(state, turn, item, question, index) {
    if (!state || state.turnId !== nativeTurnId(turn)) return false;
    const source = state.question;
    if (source == null) return !manager.__codexElvesRequestClientManager && state.title === question.title;
    return source.id === JSON.stringify(["request_user_input_async", item.id, index])
      && source.sourceItemId === item.id && source.questionIndex === index
      && source.title === question.title && Array.isArray(source.options)
      && JSON.stringify(source.options) === JSON.stringify(array(question.options));
  }
  function nativeAsyncQuestionEntry(id, turn, item, question, index, conversation = manager.getConversation(id)) {
    const native = asyncQuestionNativeBindings;
    if (!native || !appScope || typeof question?.title !== "string") return null;
    const itemId = JSON.stringify(["request_user_input_async", item.id, index]), turnId = nativeTurnId(turn);
    const entityKeys = Object.entries(conversation?.turnHistory?.history?.entitiesByKey || {})
      .filter(([, entity]) => nativeTurnId(entity) === turnId
        && array(entity.items).some(value => value.type === "agentMessage" && value.id === item.id))
      .map(([key]) => key);
    const unanswered = conversation?.remoteListState?.unansweredAsyncQuestions;
    if (unanswered?.turnId === turnId && array(unanswered.questionIds).includes(itemId)
        && typeof unanswered.entityKey === "string") entityKeys.push(unanswered.entityKey);
    const panel = appScope.get(native.panelFamily, { hostId: "local", threadId: id });
    const selected = panel?.selectedQuestionKey;
    const selectedValid = selected?.hostId === "local" && selected.threadId === id
      && typeof selected.entityKey === "string" && !!selected.entityKey
      && Array.isArray(panel.questionIds) && panel.questionIds.includes(selected.itemId)
      && panel.questionIds.every(value => typeof value === "string")
      && new Set(panel.questionIds).size === panel.questionIds.length;
    if (selectedValid && panel.questionIds.includes(itemId))
      entityKeys.push(selected.entityKey);
    const entries = [...new Set(entityKeys)].map(entityKey => {
      const key = { hostId: "local", threadId: id, entityKey, itemId };
      const state = appScope.get(native.questionFamily, key);
      return { key, state, current: !!selectedValid
        && selected.entityKey === entityKey && panel.questionIds.includes(itemId) };
    }).filter(({ state }) => state == null
      || nativeAsyncQuestionMatches(state, turn, item, question, index));
    if (entries.length !== 1) return null;
    return entries[0];
  }
  function nativeAsyncQuestionReplyEntry(id, turnId, reply, conversation = manager.getConversation(id)) {
    let identity;
    try { identity = JSON.parse(reply.questionItemId); } catch { return null; }
    if (!Array.isArray(identity) || identity.length !== 3 || identity[0] !== "request_user_input_async"
        || typeof identity[1] !== "string" || !identity[1] || !Number.isSafeInteger(identity[2]) || identity[2] < 0)
      return null;
    const turn = nativeConversationTurns(conversation).find(turn => nativeTurnId(turn) === turnId);
    const item = array(turn?.items).find(item => item.type === "agentMessage" && item.id === identity[1]);
    const question = item?.questions?.[identity[2]];
    return item && question?.title === reply.question
      ? nativeAsyncQuestionEntry(id, turn, item, question, identity[2], conversation) : null;
  }
  async function skipAsyncQuestion(id, request) {
    if (!appScope || typeof appScope.set !== "function") throw failure(503, "Codex 原生问答状态上下文尚未就绪");
    const sourceManager = manager, scope = appScope;
    const identity = asyncQuestionRequestIdentity(id, request);
    const native = nativeAsyncQuestionState(await nativeCreationModule());
    if (manager !== sourceManager || appScope !== scope || sourceManager.disposed)
      throw failure(409, "Codex 界面上下文已变化，请重新读取问题");
    const key = JSON.stringify([id, request.id]);
    if (questionReplyStore.has(key)) throw failure(409, "此问题已提交或结果待确认，请先核对状态");
    const current = asyncQuestionRequests(id).find(value => value.id === request.id);
    if (!current || current.params.turnId !== request.params.turnId
        || JSON.stringify(current.params.questions) !== JSON.stringify(request.params.questions)
        || identity !== null && asyncQuestionRequestIdentity(id, current) !== identity)
      throw failure(409, "问题状态已变化，请重新读取会话");
    const conversation = manager.getConversation(id);
    const turn = nativeConversationTurns(conversation).find(value => nativeTurnId(value) === request.params.turnId);
    const targets = request.params.questions.map(question => {
      const [, itemId, index] = JSON.parse(question.id);
      const item = array(turn?.items).find(value => value.type === "agentMessage" && value.id === itemId);
      const source = item?.questions?.[index];
      const entry = item && source?.title === question.question
        ? nativeAsyncQuestionEntry(id, turn, item, source, index, conversation) : null;
      if (!entry) throw failure(503, "原生问题状态尚未就绪，请在 Codex 打开此会话后重试");
      if (entry.state?.isSubmitting === true) throw failure(409, "此问题正在提交，请稍后核对状态");
      return { ...entry, title: question.question, question: {
        id: question.id, sourceItemId: item.id, questionIndex: index,
        title: question.question, options: array(source.options)
      } };
    });
    let changed = false;
    try {
      for (const target of targets) {
        changed = true;
        if (!target.state) scope.set(native.questionFamily, target.key, {
          turnId: request.params.turnId,
          ...(manager.__codexElvesRequestClientManager
            ? { question: target.question, status: nativeTurnStatus(turn) } : { title: target.title }),
          draft: "", draftBaseline: "",
          lastSubmission: null, isSubmitting: false, isSkipped: false
        });
        native.skipQuestion(scope, target.key);
        native.setDraft(scope, target.key, "");
        if (scope.get(native.questionFamily, target.key)?.isSkipped !== true)
          throw new Error("原生跳过状态未确认");
      }
      const panelKey = { hostId: "local", threadId: id }, panel = scope.get(native.panelFamily, panelKey);
      if (panel && targets.some(target => target.key.entityKey === panel.selectedQuestionKey?.entityKey)) {
        const ids = new Set(targets.map(target => target.key.itemId));
        const questionIds = array(panel.questionIds).filter(itemId => !ids.has(itemId));
        native.setPanel(scope, panelKey, questionIds.length
          ? { ...panel, questionIds, selectedQuestionKey: questionIds.includes(panel.selectedQuestionKey.itemId)
            ? panel.selectedQuestionKey : { ...panel.selectedQuestionKey, itemId: questionIds[0] } } : null);
      }
      syncAsyncQuestions(id, manager.getConversation(id));
      syncPendingNotifications(id);
      return { ok: true, threadId: id, requestId: request.id, skipped: true };
    } catch (error) {
      syncAsyncQuestions(id, manager.getConversation(id));
      if (!changed) throw error;
      return { ok: false, code: 503, outcomeUnknown: true, threadId: id,
        message: "跳过状态待确认，请重新读取问题，不要重复提交" };
    }
  }
  function asyncQuestionDesktopBaseline(native, scope, id, replies) {
    const panel = scope.get(native.panelFamily, { hostId: "local", threadId: id });
    const selected = panel?.selectedQuestionKey;
    if (selected?.hostId !== "local" || selected.threadId !== id) return null;
    return {
      entityKey: selected.entityKey,
      questions: new Map(replies.flatMap(reply => {
        if (!array(panel.questionIds).includes(reply.questionItemId)) return [];
        const state = scope.get(native.questionFamily, { ...selected, itemId: reply.questionItemId });
        const entry = state && nativeAsyncQuestionReplyEntry(id, state.turnId, reply);
        return entry?.key.entityKey === selected.entityKey && entry.state === state ? [[reply.questionItemId, {
          turnId: state.turnId, lastSubmission: state.lastSubmission, draftBaseline: state.draftBaseline,
          identity: JSON.stringify(state.question ?? { title: state.title })
        }]] : [];
      }))
    };
  }
  function syncAsyncQuestionDesktop(native, scope, sourceManager, id, turnId, replies, submission = null) {
    const current = () => manager === sourceManager && appScope === scope && !sourceManager.disposed;
    const processed = new Set(), ids = replies.map(reply => reply.questionItemId);
    const result = () => ({
      processed: current() ? [...processed] : [],
      pending: ids.filter(id => !current() || !processed.has(id))
    });
    if (!current()) return result();
    const panelKey = { hostId: "local", threadId: id };
    const panel = scope.get(native.panelFamily, panelKey);
    if (panel == null) {
      for (const itemId of ids) processed.add(itemId);
      return result();
    }
    const selected = panel.selectedQuestionKey;
    if (selected?.hostId !== "local" || selected.threadId !== id
        || typeof selected.entityKey !== "string" || !selected.entityKey
        || !Array.isArray(panel.questionIds) || !panel.questionIds.includes(selected.itemId)
        || panel.questionIds.some(value => typeof value !== "string")
        || new Set(panel.questionIds).size !== panel.questionIds.length) return result();
    const matching = new Map();
    for (const reply of replies) {
      if (!current() || scope.get(native.panelFamily, panelKey) !== panel) return result();
      if (!panel.questionIds.includes(reply.questionItemId)) {
        processed.add(reply.questionItemId);
        continue;
      }
      const key = { ...selected, itemId: reply.questionItemId };
      const state = scope.get(native.questionFamily, key);
      const entry = nativeAsyncQuestionReplyEntry(id, turnId, reply);
      // 远程路径不接管桌面另一笔正在提交的状态。
      if (!state || !entry || entry.key.entityKey !== selected.entityKey || entry.state !== state
          || typeof state.draft !== "string" || typeof state.draftBaseline !== "string"
          || state.isSubmitting !== false) continue;
      if (!submission && (typeof state.lastSubmission === "string" && state.lastSubmission.length > 0
            && state.lastSubmission !== reply.answer
          || state.draftBaseline.length > 0 && state.draftBaseline !== reply.answer)) {
        processed.add(reply.questionItemId);
        continue;
      }
      if (submission) {
        const baseline = submission.baseline?.questions.get(reply.questionItemId);
        if (submission.baseline?.entityKey !== selected.entityKey || baseline?.turnId !== turnId
            || baseline.identity !== JSON.stringify(state.question ?? { title: state.title })
            || submission.accepted?.has(reply.questionItemId)
              && submission.accepted.get(reply.questionItemId).answer !== reply.answer) {
          processed.add(reply.questionItemId);
          continue;
        }
        if ((state.lastSubmission !== baseline.lastSubmission || state.draftBaseline !== baseline.draftBaseline)
            && (state.lastSubmission !== reply.answer || state.draftBaseline !== reply.answer)) {
          processed.add(reply.questionItemId);
          continue;
        }
      }
      const draft = state.draft === state.draftBaseline ? reply.answer : state.draft;
      const next = { ...state, lastSubmission: reply.answer, draftBaseline: reply.answer, draft, isSubmitting: false };
      if (!current() || scope.get(native.panelFamily, panelKey) !== panel) return result();
      if (state.lastSubmission !== next.lastSubmission || state.draftBaseline !== next.draftBaseline
          || state.draft !== next.draft) scope.set(native.questionFamily, key, next);
      processed.add(reply.questionItemId);
      matching.set(reply.questionItemId, reply);
    }
    if (!matching.size || !current() || scope.get(native.panelFamily, panelKey) !== panel) return result();
    const questionIds = panel.questionIds.filter(itemId => {
      const reply = matching.get(itemId);
      if (!reply) return true;
      const state = scope.get(native.questionFamily, { ...selected, itemId });
      const entry = nativeAsyncQuestionReplyEntry(id, turnId, reply);
      return !entry || entry.key.entityKey !== selected.entityKey || entry.state !== state || state.isSubmitting !== false
        || state.lastSubmission !== reply.answer || state.draftBaseline !== reply.answer
        || typeof state.draft !== "string" || state.draft.trim() !== reply.answer;
    });
    if (questionIds.length === panel.questionIds.length || !current()
        || scope.get(native.panelFamily, panelKey) !== panel) return result();
    if (!questionIds.length) native.setPanel(scope, panelKey, null);
    else native.setPanel(scope, panelKey, {
      ...panel, questionIds,
      selectedQuestionKey: questionIds.includes(selected.itemId) ? selected : { ...selected, itemId: questionIds[0] }
    });
    return result();
  }
  function syncAsyncQuestionDesktopRecord(key, questionIds, replies, perform) {
    const record = questionReplyStore.get(key);
    const previous = new Set(array(record?.desktopSynchronizedQuestionIds));
    const snapshot = processed => ({
      ...record, questionIds,
      desktopSynchronizedQuestionIds: [...processed],
      desktopSynchronized: questionIds.length > 0 && questionIds.every(id => processed.has(id))
    });
    // 先暂记本次有证据的逐题范围，原生 watcher 重入时不能再处理这些题。
    const provisional = snapshot(new Set([...previous, ...replies.map(reply => reply.questionItemId)]));
    questionReplyStore.set(key, provisional);
    let outcome, failed = false;
    try { outcome = perform(); }
    catch { outcome = { processed: [] }; failed = true; }
    const completed = new Set([...previous, ...array(outcome?.processed)]);
    const final = snapshot(completed);
    if (questionReplyStore.get(key) === provisional) questionReplyStore.set(key, final);
    return { processed: [...completed], pending: questionIds.filter(id => !completed.has(id)), failed };
  }
  function asyncQuestionRecoveryCandidates(id, conversation) {
    const candidates = [];
    const turns = nativeConversationTurns(conversation);
    for (const [key, record] of questionReplyStore) {
      if (!["submitted", "unknown"].includes(record?.state) || record.desktopSynchronized) continue;
      try {
        const [threadId, requestId] = JSON.parse(key);
        const [kind, turnId, itemId] = JSON.parse(requestId);
        if (threadId !== id || kind !== "async-question") continue;
        const turn = turns.find(turn => nativeTurnId(turn) === turnId);
        if (!turn) continue;
        const source = array(turn.items).find(item => item.type === "agentMessage" && item.id === itemId);
        if (!array(source?.questions).length) continue;
        const sourceIds = source.questions.flatMap((question, index) => typeof question?.title === "string"
          ? [JSON.stringify(["request_user_input_async", itemId, index])] : []);
        const questionIds = Array.isArray(record.questionIds)
          ? record.questionIds.filter(id => sourceIds.includes(id)) : sourceIds;
        const processed = new Set(array(record.desktopSynchronizedQuestionIds));
        const accepted = acceptedAsyncQuestionReplies(turn, turns);
        const replies = array(source.questions).flatMap((question, index) => {
          const questionId = JSON.stringify(["request_user_input_async", itemId, index]);
          if (!questionIds.includes(questionId) || processed.has(questionId)) return [];
          const reply = accepted.get(questionId);
          return reply ? [reply] : [];
        });
        if (replies.length) candidates.push({ key, record, turnId, replies, questionIds });
      } catch {}
    }
    return candidates;
  }
  function recoverAsyncQuestionDesktop(id, conversation) {
    if (!appScope || !asyncQuestionRecoveryCandidates(id, conversation).length) return;
    const sourceManager = manager, scope = appScope;
    const active = asyncQuestionRecoveries.get(id);
    if (active?.manager === sourceManager && active.scope === scope) return;
    const ticket = { manager: sourceManager, scope };
    asyncQuestionRecoveries.set(id, ticket);
    void nativeCreationModule().then(module => {
      if (manager !== sourceManager || appScope !== scope || sourceManager.disposed) return;
      const native = nativeAsyncQuestionState(module);
      for (const candidate of asyncQuestionRecoveryCandidates(id, sourceManager.getConversation(id))) {
        if (manager !== sourceManager || appScope !== scope || sourceManager.disposed) return;
        if (questionReplyStore.get(candidate.key) !== candidate.record) continue;
        syncAsyncQuestionDesktopRecord(candidate.key, candidate.questionIds, candidate.replies, () =>
          syncAsyncQuestionDesktop(native, scope, sourceManager, id, candidate.turnId, candidate.replies));
      }
    }).catch(() => {}).finally(() => {
      if (asyncQuestionRecoveries.get(id) === ticket) asyncQuestionRecoveries.delete(id);
    });
  }
  function syncAsyncQuestions(id, conversation) {
    recoverAsyncQuestionDesktop(id, conversation);
    const requests = asyncQuestionRequests(id, conversation);
    const signature = JSON.stringify(requests);
    if (signature === asyncQuestionSignatures.get(id)) return;
    asyncQuestionSignatures.set(id, signature);
    emit({ method: "remote/asyncQuestionsChanged", params: { threadId: id, requests } });
  }
  async function answerAsyncQuestion(payload) {
    const id = requiredId(payload.threadId);
    const request = asyncQuestionRequests(id).find(request => request.id === payload.requestId);
    if (!request) throw failure(409, "此问题已回答或问题状态已变化，请刷新会话");
    const identity = asyncQuestionRequestIdentity(id, request);
    if (payload.response?.skip === true) {
      if (Object.keys(payload.response).some(key => key !== "skip"))
        throw failure(400, "跳过问题不能同时提交回答");
      return await skipAsyncQuestion(id, request);
    }
    if (payload.response && Object.hasOwn(payload.response, "skip")) throw failure(400, "跳过问题参数无效");
    const answers = payload.response?.answers;
    const questions = request.params.questions;
    if (!answers || typeof answers !== "object" || Array.isArray(answers)
        || Object.keys(answers).length !== questions.length
        || Object.keys(answers).some(key => !questions.some(question => question.id === key)))
      throw failure(400, "请回答此次请求中的所有问题");
    const replies = questions.map(question => {
      const values = answers[question.id]?.answers;
      if (!Array.isArray(values) || values.length !== 1 || typeof values[0] !== "string" || !values[0].trim())
        throw failure(400, "请使用选项或文字回答所有问题");
      return { questionItemId: question.id, question: question.question, answer: values[0].trim() };
    });
    const prompt = `<send_user_message_question_reply>\n${JSON.stringify(replies)}\n</send_user_message_question_reply>`;
    if (new TextEncoder().encode(prompt).length > 128 * 1024) throw failure(400, "回答不能超过 128 KiB");
    if (runtimeCompaction(id).state !== "idle") throw failure(409, "上下文压缩完成前不能提交回答");
    if (!appScope) discover();
    if (!appScope) throw failure(503, "Codex 界面上下文尚未就绪，请稍后重试提交回答");
    const sourceManager = manager, scope = appScope;
    if (typeof scope.set !== "function") throw failure(503, "Codex 原生问答状态上下文尚未就绪");
    const module = await nativeCreationModule();
    if (manager !== sourceManager || appScope !== scope || sourceManager.disposed)
      throw failure(409, "Codex 界面上下文已变化，请重新读取问题");
    const native = nativeAsyncQuestionState(module);
    const submits = Object.values(module).filter(value => typeof value === "function"
      && ["isSubmissionCurrent", "getTurnCoordinator", ".sendMessage("]
        .every(marker => Function.prototype.toString.call(value).includes(marker)));
    if (submits.length !== 1) throw failure(503, "当前 Codex 版本的原生异步问答提交服务不可用");
    const submit = submits[0];
    const key = JSON.stringify([id, request.id]);
    if (questionReplyStore.has(key)) throw failure(409, "此回答已提交或结果待确认，请先核对回答状态");
    const isCurrent = () => {
      if (manager !== sourceManager || appScope !== scope || sourceManager.disposed) return false;
      const current = asyncQuestionRequests(id).find(value => value.id === request.id);
      return current?.params.turnId === request.params.turnId
        && JSON.stringify(current.params.questions) === JSON.stringify(questions)
        && (identity === null || asyncQuestionRequestIdentity(id, current) === identity)
        && runtimeCompaction(id).state === "idle";
    };
    if (!isCurrent()) throw failure(409, "问题状态已变化，请重新读取会话");
    const conversation = manager.getConversation(id);
    const cwd = manager.getConversationCwd(id) || "/";
    const context = { ...remoteAttachmentContext(prompt, cwd, []), turnTrigger: "send_user_message_async_question" };
    const message = { id: crypto.randomUUID(), text: prompt, context, cwd, createdAt: Date.now() };
    const submission = { questionIds: replies.map(reply => reply.questionItemId) };
    const desktopBaseline = asyncQuestionDesktopBaseline(native, scope, id, replies);
    questionReplyStore.set(key, { ...submission, state: "submitting" });
    syncAsyncQuestions(id, conversation);
    let sent = false;
    try {
      const result = await submit({
        scope, turnTrigger: "send_user_message_async_question",
        managerHostId: "local", hostId: "local", targetConversationId: id,
        context, cwd, agentMode: "auto", permissionProfileId: null,
        serviceTier: conversation?.latestServiceTier ?? undefined,
        shouldSendPermissionOverrides: false, activeCollaborationMode: conversation?.latestCollaborationMode,
        restoreMessage: message, isSubmissionCurrent: isCurrent
      }, { queueModeOverride: "send-now" });
      if (result?.status !== "sent") {
        questionReplyStore.set(key, { ...submission, state: "unknown" });
        return { ok: false, outcomeUnknown: true, threadId: id, message: "回答送达状态待确认，请核对会话，不要重复提交" };
      }
      sent = true;
      questionReplyStore.set(key, { ...submission, state: "submitted", desktopSynchronized: false });
      let desktopSyncPending = false;
      try {
        const turns = nativeConversationTurns(sourceManager.getConversation(id));
        const turn = turns.find(turn => nativeTurnId(turn) === request.params.turnId);
        const accepted = acceptedAsyncQuestionReplies(turn, turns);
        const outcome = syncAsyncQuestionDesktopRecord(key, submission.questionIds, replies, () =>
          syncAsyncQuestionDesktop(native, scope, sourceManager, id, request.params.turnId, replies,
            { baseline: desktopBaseline, accepted }));
        desktopSyncPending = outcome.failed || outcome.pending.length > 0;
      } catch {
        desktopSyncPending = true;
        questionReplyStore.set(key, { ...submission, state: "submitted", desktopSynchronized: false });
      }
      return { ok: true, threadId: id, requestId: request.id, ...(desktopSyncPending ? { desktopSyncPending: true } : {}) };
    } catch (error) {
      if (sent) {
        questionReplyStore.set(key, { ...submission, state: "submitted", desktopSynchronized: false });
        return { ok: true, threadId: id, requestId: request.id, desktopSyncPending: true };
      }
      if (error.remoteValidationFailure || error.delivery?.stage === "not-sent") {
        questionReplyStore.delete(key);
        throw error;
      }
      questionReplyStore.set(key, { ...submission, state: "unknown" });
      return { ok: false, outcomeUnknown: true, threadId: id, message: "回答送达状态待确认，请核对会话，不要重复提交" };
    } finally {
      try { syncAsyncQuestions(id, manager.getConversation(id)); } catch {}
    }
  }
  function runtimeThread(id, nativeThread = null, { readOnly = false, viewKind, parentThreadId } = {}) {
    const conversation = manager.getConversation(id);
    const requestsSnapshot = readOnly ? null : blockingRequestsSnapshot(id, conversation);
    if (!readOnly) recoverAsyncQuestionDesktop(id, conversation);
    const queued = readOnly ? [] : manager.__codexElvesRequestClientManager
      ? requestClientQueues.get(id)?.messages ?? []
      : manager.getTurnCoordinator?.()?.readMessages?.(id)
        ?? manager.storage.readQueuedFollowUps?.()?.value?.[id];
    const modelSettings = threadModelSettings(conversation, nativeThread);
    const sideEntry = !readOnly && manager.__codexElvesRequestClientManager && viewKind === "side"
      ? nativeSideThreads.get(id) : null;
    const sideContent = sideEntry && sideEntry.parentThreadId === parentThreadId
      ? readNativeSideTab(sideEntry, id)?.props?.content : null;
    const sideSettings = sideContent?.hostId === "local"
      ? conversation?.latestCollaborationMode?.settings
        ?? conversation?.latestThreadSettings?.collaborationMode?.settings
        ?? sideContent.lockedCollaborationMode?.settings : null;
    // 发送准备使用当前会话协作配置；标签锁只在快照缺失时回退继承值。
    if (typeof sideSettings?.model === "string" && sideSettings.model.trim()) {
      modelSettings.model = sideSettings.model;
      if (Object.hasOwn(sideSettings, "reasoning_effort"))
        modelSettings.effort = sideSettings.reasoning_effort ?? null;
    }
    const asyncQuestions = readOnly ? [] : asyncQuestionRequests(id, conversation);
    if (!readOnly) asyncQuestionSignatures.set(id, JSON.stringify(asyncQuestions));
    return {
      id,
      model: modelSettings.model,
      reasoningEffort: modelSettings.effort,
      serviceTier: conversation?.latestServiceTier ?? null,
      tokenUsage: requestClientQueues.get(id)?.tokenUsage ?? conversation?.latestTokenUsageInfo ?? null,
      goal: runtimeGoal(conversation?.threadGoal),
      compaction: runtimeCompaction(id, conversation),
      transcriptEvents: nativeTranscriptEvents(conversation),
      editableMessage: readOnly ? null : editableMessageState(id).metadata,
      status: manager.getThreadRuntimeStatusEvidence(id) || { type: "idle" },
      requestsKnown: readOnly || !!requestsSnapshot,
      ...(readOnly ? { requests: [] } : requestsSnapshot ? { ...requestsSnapshot, requests: [...requestsSnapshot.requests, ...asyncQuestions] }
        : asyncQuestions.length ? { requests: asyncQuestions } : {}),
      ...(!manager.__codexElvesRequestClientManager || readOnly
        || requestClientQueues.get(id)?.messages != null ? queueSnapshotFields(id, queued) : {})
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
  function queueSnapshotFields(threadId, queued) {
    const messages = array(queued?.messages || queued);
    const known = queuedAcceptanceByManager.get(manager), accepted = known?.get(threadId);
    if (accepted) {
      const ids = new Set(messages.map(message => message?.clientUserMessageId));
      for (const id of accepted.keys()) if (!ids.has(id)) accepted.delete(id);
      if (!accepted.size) known.delete(threadId);
    }
    return {
      queueEpoch: epoch,
      queueRevision: ++queueSequence,
      queuedMessages: messages.map(message => {
        if (!message || typeof message !== "object") return message;
        const nativeMessage = { ...message };
        delete nativeMessage.remoteDeliveryState;
        delete nativeMessage.remoteQueueSyncPending;
        const evidence = accepted?.get(nativeMessage.clientUserMessageId);
        const serialized = nativeMessage.submission?.editPosition?.serverSubmission;
        return evidence && serialized?.clientUserMessageId === nativeMessage.clientUserMessageId
          && sameQueuedUserInput(serialized.input, evidence.input) ? {
          ...nativeMessage, canSendNow: false, remoteDeliveryState: "accepted", remoteQueueSyncPending: true
        } : nativeMessage;
      }),
      queuedEdits: queuedEditSummaries(threadId)
    };
  }
  async function queueSnapshot(id) {
    id = requiredId(id);
    const sourceManager = manager, scope = appScope;
    const assertCurrent = () => {
      if (manager !== sourceManager || appScope !== scope || sourceManager.disposed)
        throw failure(409, "原生队列连接已变化，请重新读取");
    };
    if (manager?.__codexElvesRequestClientManager) {
      const queued = { messages: await readRequestClientQueue(id) };
      if (queued.messages.some(message => message?.pausedReason
          && Array.isArray(message.submission?.editPosition?.serverSubmission?.input))) {
        const proof = { manager, scope: appScope, requestClient: manager.requestClient, threadId: id };
        await queuedEditNativeHistory(proof)
          .then(history => rememberQueuedAcceptedCopies(proof, queued.messages, history)).catch(() => null);
      }
      assertCurrent();
      return {
        ok: true,
        threadId: id,
        ...queueSnapshotFields(id, queued)
      };
    }
    if (typeof sourceManager.getTurnCoordinator !== "function")
      throw failure(503, "当前 Codex 版本的原生队列服务不可用");
    const coordinator = await sourceManager.getTurnCoordinator();
    if (typeof coordinator?.readMessages !== "function")
      throw failure(503, "当前 Codex 版本不支持原生队列读取");
    await loadNativeQueue(coordinator, id);
    assertCurrent();
    const queued = coordinator.readMessages(id);
    return {
      ok: true,
      threadId: id,
      ...queueSnapshotFields(id, queued)
    };
  }
  function acceptedQueuedSend(threadId, clientUserMessageId) {
    for (const turn of nativeConversationTurns(manager.getConversation(threadId))) {
      if (!nativeTurnId(turn) || turn.pendingUserMessageAdmission) continue;
      const item = array(turn.items).find(item =>
        (item.type === "userMessage" || item.type === "steeringUserMessage" && item.status === "accepted")
        && !item.remotePendingInput && !item.remoteCompactionInput
        && (item.clientId === clientUserMessageId || item.clientUserMessageId === clientUserMessageId));
      if (item) return {
        ok: true, confirmed: true, deliveryState: "accepted", threadId, clientUserMessageId,
        messageId: item.serverUserMessageId || item.id, turnId: nativeTurnId(turn)
      };
    }
    return null;
  }
  async function queueSendState(payload) {
    const result = await readQueuedSendState(payload);
    if (result.confirmed) {
      const key = JSON.stringify([result.threadId, result.clientUserMessageId]);
      if (queuedSendSubmissions.get(key)?.unconfirmed === true) queuedSendSubmissions.delete(key);
    }
    return result;
  }
  async function readQueuedSendState(payload) {
    const threadId = requiredId(payload.threadId), clientUserMessageId = requiredId(payload.clientUserMessageId);
    if (manager?.__codexElvesRequestClientManager) {
      try {
        const response = await (await requestClientNativeRpc()).getLoadedConversationHistory(threadId);
        const turns = array(response?.turns);
        for (const turn of turns) {
          const item = array(turn?.items).find(item =>
            (item?.type === "userMessage" || item?.type === "steeringUserMessage"
              && item?.status === "accepted")
            && (item.clientUserMessageId === clientUserMessageId || item.clientId === clientUserMessageId));
          if (!item) continue;
          return {
            ok: true, confirmed: true, deliveryState: "accepted", threadId, clientUserMessageId,
            messageId: item.id, turnId: nativeTurnId(turn)
          };
        }
      } catch {}
      try {
        const submissions = await readRequestClientQueue(threadId);
        const queued = submissions.find(value =>
          (value?.clientUserMessageId ?? value?.submissionOptions?.clientUserMessageId ?? value?.id) === clientUserMessageId);
        if (queued) {
          return {
            ok: true, confirmed: true, deliveryState: "queued", threadId, clientUserMessageId,
            messageId: queued.id, ...queueSnapshotFields(threadId, submissions)
          };
        }
      } catch {}
      return { ok: true, confirmed: false, deliveryState: "unknown", threadId, clientUserMessageId };
    }
    const accepted = acceptedQueuedSend(threadId, clientUserMessageId);
    if (accepted) return accepted;
    const coordinator = await manager.getTurnCoordinator?.();
    const queued = coordinator?.readMessages?.(threadId);
    const message = array(queued?.messages || queued).find(message =>
      (message.clientUserMessageId ?? message.submissionOptions?.clientUserMessageId
        ?? (!message.serverQueuedMessageId ? message.id : null)) === clientUserMessageId
      && !["pending", "sending", "outcome-unknown"].includes(message.submission?.status));
    if (message) return {
      ok: true, confirmed: true, deliveryState: "queued", threadId, clientUserMessageId,
      messageId: message.id, ...queueSnapshotFields(threadId, queued)
    };
    if (await coordinator?.serverQueue?.isEnabled?.(threadId)) {
      const submissions = await readServerQueuedSubmissions(threadId);
      const delivered = acceptedQueuedSend(threadId, clientUserMessageId);
      if (delivered) return delivered;
      const submission = submissions.find(value => value.clientUserMessageId === clientUserMessageId);
      if (submission) return {
        ok: true, confirmed: true, deliveryState: "queued", threadId, clientUserMessageId,
        messageId: submission.id, queueSyncPending: true
      };
    }
    return acceptedQueuedSend(threadId, clientUserMessageId)
      || { ok: true, confirmed: false, deliveryState: "unknown", threadId, clientUserMessageId };
  }
  async function loadNativeQueue(coordinator, id) {
    if (typeof coordinator?.loadMessages === "function") return await coordinator.loadMessages(id);
    if (typeof coordinator?.serverQueue?.load === "function"
        && await coordinator.serverQueue.isEnabled?.(id) !== false)
      return await coordinator.serverQueue.load(id);
    throw failure(503, "当前 Codex 版本不支持原生队列读取");
  }
  async function removeQueued(payload = {}) {
    let threadId = null, messageId = null, removeStarted = false, removalResult = null;
    try {
      if (manager?.disposed) {
        for (const dispose of unsubscribe) { try { dispose?.(); } catch {} }
        unsubscribe = [];
        manager = null;
        appScope = null;
      }
      if (!manager) discover();
      threadId = requiredId(payload.threadId);
      messageId = requiredId(payload.messageId);
      await readThread(threadId);
      if (typeof manager.getTurnCoordinator !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      const coordinator = await manager.getTurnCoordinator();
      if (typeof coordinator?.removeQueuedMessage !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      await loadNativeQueue(coordinator, threadId);
      const message = array(coordinator.readMessages?.(threadId)).find(message => message?.id === messageId);
      if (!message) throw failure(409, "此队列消息已不存在，请刷新会话");
      if (message.submission?.status && message.submission.status !== "queued")
        throw failure(409, "此消息已提交或送达尚未确认，不能作为待执行消息取消");
      removeStarted = true;
      removalResult = await coordinator.removeQueuedMessage(threadId, messageId);
      if (removalResult == null) {
        removeStarted = false;
        throw failure(409, "此队列消息已不存在，请刷新会话");
      }
      const queued = coordinator.readMessages?.(threadId)
        ?? manager.storage.readQueuedFollowUps?.()?.value?.[threadId];
      return { ok: true, threadId, messageId, ...queueSnapshotFields(threadId, queued) };
    } catch (error) {
      const acknowledged = error.queueMutationAcknowledged === true ? error.queueMutationResult : removalResult;
      if (removeStarted && acknowledged && typeof acknowledged === "object" && !Array.isArray(acknowledged)
          && acknowledged.message?.id === messageId)
        return { ok: true, threadId, messageId, queueSyncPending: true,
          message: "队列消息已移除，但队列快照暂不可用，请刷新会话" };
      if (removeStarted && error.queueMutationAcknowledged === true && acknowledged === null)
        return { ok: false, code: 409, threadId, messageId, message: "此队列消息已不存在，请刷新会话" };
      const outcomeUnknown = removeStarted && (error.queueMutationAcknowledged === true || !error.remoteValidationFailure);
      return { ok: false, code: error.code || 503,
        ...(threadId ? { threadId } : {}), ...(messageId ? { messageId } : {}),
        ...(outcomeUnknown ? { outcomeUnknown: true } : {}),
        message: outcomeUnknown ? "原生队列移除结果未确认，请刷新会话检查，不要重复删除"
          : error.message || "移除原生队列消息失败" };
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
      if (typeof coordinator?.readMessages !== "function"
          || typeof coordinator?.reorderQueuedMessages !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列排序");
      await loadNativeQueue(coordinator, threadId);
      const currentMessageIds = array(coordinator.readMessages(threadId)).map(message => message?.id);
      if (currentMessageIds.length !== expectedMessageIds.length
          || currentMessageIds.some((messageId, index) => messageId !== expectedMessageIds[index]))
        throw failure(409, "队列已发生变化，请刷新会话后重试");
      await assertQueuedMessagesUnaccepted(threadId, array(coordinator.readMessages(threadId)));
      if (!messageIds.some((messageId, index) => messageId !== expectedMessageIds[index])) {
        return {
          ok: true,
          threadId,
          ...queueSnapshotFields(threadId, coordinator.readMessages(threadId))
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
        ...queueSnapshotFields(threadId, queuedMessages)
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
    // 纯图片的空白文本占位同时覆盖首尾，补正文时只保留尾部，不能重复加到正文前。
    const leading = originalText === "" && contentStart === 0 ? "" : content.match(/^\s*/)?.[0] || "";
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
  function sameQueuedUserInput(left, right) {
    if (!Array.isArray(left) || !Array.isArray(right)) return false;
    // UserInput::Image 会把省略的可选 detail 序列化为 null；其他字段和输入顺序仍逐项核对。
    const canonical = input => input.map(item => item?.type === "image" && item.detail == null
      ? { ...item, detail: null } : item);
    return sameJsonValue(canonical(left), canonical(right));
  }
  async function readServerQueuedSubmissions(threadId, sourceManager = manager) {
    const submissions = [];
    const cursors = new Set();
    let cursor = null;
    do {
      const page = await sourceManager.requestClient.sendRequest("thread/queue/list", { threadId, cursor });
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
  async function queuedEditFingerprint(action, payload) {
    const value = JSON.stringify([action, action === "editQueued" ? payload.prompt.trim() : null,
      payload.retainedImageSources ?? null,
      array(payload.images).map(image => [image.name, image.mime, image.dataUrl, image.size])]);
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
    return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, "0")).join("");
  }
  function queuedEditProofCurrent(proof) {
    return proof && manager === proof.manager && appScope === proof.scope
      && !proof.manager.disposed && proof.manager.requestClient === proof.requestClient;
  }
  async function queuedEditNativeHistory(proof) {
    if (!queuedEditProofCurrent(proof)) throw failure(409, "原生队列连接已变化，请重新核对");
    const thread = await proof.manager.readThread(proof.threadId, { includeTurns: true });
    if (!queuedEditProofCurrent(proof)) throw failure(409, "原生队列连接已变化，请重新核对");
    // 临时会话的完整历史来自桌面内存，没有持久化历史接口的 itemsView 标记。
    const ephemeral = thread?.ephemeral === true;
    if (thread?.id !== proof.threadId || !Array.isArray(thread.turns)
        || thread.turns.some(turn => !nativeTurnId(turn) || !Array.isArray(turn.items)
          || turn.itemsView !== "full" && !(ephemeral && turn.itemsView == null)))
      throw failure(503, "原生完整用户消息历史尚未确认，不能确认队列编辑结果");
    proof.ephemeral = ephemeral;
    const messages = thread.turns.flatMap(turn => turn.items.flatMap(item =>
      item?.type === "userMessage" || item?.type === "steeringUserMessage"
        ? [{ turnId: nativeTurnId(turn), pending: turn.pendingUserMessageAdmission === true, item }] : []));
    return messages;
  }
  function rememberQueuedAcceptedCopies(proof, messages, history) {
    if (!queuedEditProofCurrent(proof)) return;
    const candidates = new Set(messages.filter(message =>
      message?.pausedReason && Array.isArray(message.submission?.editPosition?.serverSubmission?.input))
      .map(message => message.clientUserMessageId));
    const accepted = new Map();
    for (const { item, pending } of history) {
      if (pending || item.type === "steeringUserMessage" && item.status !== "accepted") continue;
      const ids = [...new Set([item.clientId, item.clientUserMessageId]
        .filter(value => typeof value === "string" && value))];
      if (ids.length !== 1 || !candidates.has(ids[0]) || typeof item.id !== "string" || !item.id) continue;
      const id = ids[0];
      accepted.set(id, accepted.has(id) ? null : {
        input: item.type === "userMessage" ? item.content : item.input, messageId: item.id
      });
    }
    for (const [id, evidence] of accepted) if (!evidence) accepted.delete(id);
    let known = queuedAcceptanceByManager.get(proof.manager);
    if (!known && accepted.size) queuedAcceptanceByManager.set(proof.manager, known = new Map());
    if (!known) return;
    if (!accepted.size) known.delete(proof.threadId);
    else {
      if (!known.has(proof.threadId) && known.size >= 64) known.delete(known.keys().next().value);
      known.set(proof.threadId, accepted);
    }
  }
  function queuedEditClientMatches(item, clientId) {
    return item?.clientId === clientId || item?.clientUserMessageId === clientId;
  }
  async function assertQueuedMessagesUnaccepted(threadId, messages, sourceManager = manager) {
    if (sourceManager.__codexElvesRequestClientManager !== true) return;
    const clientIds = new Set(messages.map(message => message?.clientUserMessageId
      ?? message?.submissionOptions?.clientUserMessageId ?? message?.id).filter(Boolean));
    if (!clientIds.size) return;
    const proof = { manager: sourceManager, scope: appScope,
      requestClient: sourceManager.requestClient, threadId };
    let history;
    try { history = await queuedEditNativeHistory(proof); }
    catch (error) {
      throw failure(error.code || 503, "原生已接受消息状态尚未确认，不能重放队列消息",
        { notSent: true, readFailure: true });
    }
    rememberQueuedAcceptedCopies(proof, messages, history);
    if (history.some(({ item }) => clientIds.has(item.clientId) || clientIds.has(item.clientUserMessageId)))
      throw failure(409, "此逻辑消息已进入原生会话，不能再次引导、编辑或重排本地副本", { notSent: true });
  }
  async function prepareQueuedEditAcceptanceProof(entry, expectedClientId, expectedInput) {
    if (manager.__codexElvesRequestClientManager !== true || !Array.isArray(expectedInput)) return null;
    if (manager.hostId !== "local" || manager.requestClient?.hostId !== "local")
      throw failure(503, "原生队列提交归属尚未确认");
    const proof = {
      manager, scope: appScope, requestClient: manager.requestClient, threadId: entry.threadId,
      clientUserMessageId: expectedClientId, input: structuredClone(expectedInput),
      fingerprint: entry.terminalFingerprint, dispatched: false, mutationAcknowledged: false
    };
    const previous = await queuedEditNativeHistory(proof);
    if (previous.some(({ item }) => queuedEditClientMatches(item, expectedClientId)))
      throw failure(409, "此队列消息的逻辑标识已进入会话，不能再次恢复");
    proof.previousMessageIds = new Set(previous.map(({ item }) => item.id));
    if (previous.some(({ item }) => typeof item.id !== "string" || !item.id))
      throw failure(503, "原生用户消息身份尚未完整加载");
    queuedEditAcceptanceProofs.set(entry, proof);
    return proof;
  }
  async function reconcileQueuedEditAcceptance(entry) {
    if (manager.__codexElvesRequestClientManager !== true) return null;
    const proof = queuedEditAcceptanceProofs.get(entry);
    if (!queuedEditProofCurrent(proof) || !proof.dispatched || !proof.mutationAcknowledged
        || proof.fingerprint !== entry.terminalFingerprint
        || !["commit", "cancel"].includes(entry.terminalAction)) return null;
    const queued = submission => submission?.clientUserMessageId === proof.clientUserMessageId
      || submission?.id === entry.messageId || submission?.id === entry.restoredMessageId;
    // 临时会话只使用桌面本地队列，app-server 不提供 thread/queue/list。
    if (!proof.ephemeral && (await readServerQueuedSubmissions(entry.threadId, proof.manager)).some(queued)
        || !queuedEditProofCurrent(proof)) return null;
    const matches = (await queuedEditNativeHistory(proof))
      .filter(({ item }) => queuedEditClientMatches(item, proof.clientUserMessageId));
    if (matches.length !== 1) return null;
    const { turnId, item, pending } = matches[0];
    if (pending || typeof item.id !== "string" || !item.id || proof.previousMessageIds.has(item.id)
        || item.type === "steeringUserMessage" && item.status !== "accepted"
        || [item.clientId, item.clientUserMessageId].some(value =>
          value != null && value !== proof.clientUserMessageId)
        || !sameQueuedUserInput(item.type === "userMessage" ? item.content : item.input, proof.input))
      return null;
    // 服务器消费可以发生在 restore 返回之前。历史证明写入已接受；本地副本仍须单列，
    // 不能再 restore，也不能用不带 CAS 的 remove/reorder 自动清理它。
    if (!proof.ephemeral && (await readServerQueuedSubmissions(entry.threadId, proof.manager)).some(queued)
        || !queuedEditProofCurrent(proof)) return null;
    let messages = null;
    try {
      const coordinator = await proof.manager.getTurnCoordinator();
      if (!queuedEditProofCurrent(proof)) return null;
      const current = coordinator.readMessages?.(entry.threadId);
      if (Array.isArray(current)) messages = current;
    } catch {}
    if (!queuedEditProofCurrent(proof)) return null;
    const queueSyncPending = messages == null || messages.some(message =>
      message?.clientUserMessageId === proof.clientUserMessageId
      || message?.id === entry.messageId || message?.id === entry.restoredMessageId);
    if (messages) rememberQueuedAcceptedCopies(proof, messages, matches);
    entry.state = entry.terminalAction === "commit" ? "committed" : "canceled";
    entry.result = {
      ok: true, threadId: entry.threadId, editId: entry.editId,
      messageId: entry.restoredMessageId ?? entry.messageId, editState: entry.state,
      deliveryState: "accepted", acceptedTurnId: turnId, acceptedMessageId: item.id,
      ...(queueSyncPending ? {
        queueSyncPending: true,
        message: "消息已被原生会话接受，但本地队列仍待同步；不要再次引导或保存"
      } : {})
    };
    entry.token = null;
    entry.beginResult = null;
    queuedEditAcceptanceProofs.delete(entry);
    return { ...entry.result, ...(messages == null ? {} : queueSnapshotFields(entry.threadId, messages)) };
  }
  async function queueEditState(payload = {}) {
    const threadId = requiredId(payload.threadId), editId = requiredId(payload.editId);
    const entry = queuedEditStore.sessions.get(editId);
    if (entry && entry.threadId !== threadId) throw failure(409, "编辑标识不属于此会话");
    const operationPending = !!(entry?.beginPromise || entry?.terminalPromise || entry?.finishRequests);
    if (entry?.state === "outcomeUnknown" && !operationPending)
      await reconcileQueuedEditAcceptance(entry).catch(() => null);
    const snapshot = await queueSnapshot(threadId);
    return {
      ...snapshot, editId,
      editState: entry?.state ?? "missing",
      operationPending,
      operationFingerprint: entry?.terminalFingerprint ?? null,
      terminalAction: entry?.terminalAction ?? null,
      draftText: entry?.terminalPrompt ?? entry?.originalText ?? "",
      canResume: entry?.state === "editing" && !!entry.token && !operationPending,
      canDismiss: !entry || ["outcomeUnknown", "committed", "canceled"].includes(entry.state) && !operationPending,
      result: entry?.result?.ok ? entry.result : null
    };
  }
  async function dismissQueuedEdit(payload = {}) {
    const threadId = requiredId(payload.threadId), editId = requiredId(payload.editId);
    if (payload.confirmed !== true) throw failure(400, "请先核对队列和会话，再结束待确认编辑");
    const entry = queuedEditStore.sessions.get(editId);
    if (entry && entry.threadId !== threadId) throw failure(409, "编辑标识不属于此会话");
    if (entry && (!["outcomeUnknown", "committed", "canceled"].includes(entry.state)
        || entry.beginPromise || entry.terminalPromise || entry.finishRequests))
      throw failure(409, "编辑状态已变化，请重新核对，不会重复保存或恢复消息");
    queuedEditStore.sessions.delete(editId);
    const snapshot = await queueSnapshot(threadId);
    pushNotification({ method: "thread/queue/changed", params: {
      threadId, queueEpoch: snapshot.queueEpoch, queueRevision: snapshot.queueRevision,
      queuedMessages: snapshot.queuedMessages, queuedEdits: snapshot.queuedEdits
    } });
    return { ...snapshot, editId, editState: "dismissed" };
  }
  async function replayQueuedEditResult(entry, result) {
    if (!result?.ok) return result;
    try {
      messageActionManager();
      const sourceManager = manager, scope = appScope;
      const coordinator = await sourceManager.getTurnCoordinator?.();
      if (manager !== sourceManager || appScope !== scope || sourceManager.disposed)
        throw new Error("原生队列连接已变化");
      if (typeof coordinator?.readMessages !== "function")
        throw new Error("原生队列快照不可用");
      return {
        ...result,
        ...queueSnapshotFields(entry.threadId, coordinator.readMessages(entry.threadId))
      };
    } catch {
      return {
        ...result,
        queueSyncPending: true,
        message: "编辑操作已完成，但本机队列快照暂不可用，请刷新会话"
      };
    }
  }
  function queuedEditOutcomeUnknown(entry, message, confirmation = null) {
    const result = {
      ok: false,
      outcomeUnknown: true,
      code: 503,
      threadId: entry.threadId,
      editId: entry.editId,
      messageId: entry.messageId,
      editState: "outcomeUnknown",
      message,
      ...(typeof entry.restoredMessageId === "string" ? { restoredMessageId: entry.restoredMessageId } : {}),
      ...(confirmation ? { confirmation: {
        stage: String(confirmation.stage || "unknown").slice(0, 80),
        mutationAcknowledged: confirmation.mutationAcknowledged === true,
        reason: String(confirmation.reason || "原生确认未完成")
          .replace(/data:[^\s"'<>]+/gi, "[image-data]").slice(0, 300)
      } } : {})
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
      let removalReceipt = null, serverSubmission;
      const rememberRemoval = removed => {
        removalReceipt.result = removed;
        removalReceipt.acknowledged = true;
        if (removed && typeof removed === "object" && !Array.isArray(removed))
          entry.token = { ...removed, serverSubmission: removed.serverSubmission ?? serverSubmission };
      };
      const removalUnknown = (message, error = null) => {
        const token = entry.token;
        const result = queuedEditOutcomeUnknown(entry, message, {
          stage: error?.queueConfirmationStage || "remove",
          mutationAcknowledged: removalReceipt?.acknowledged === true,
          reason: error?.message || message
        });
        // 已确认 remove 的 token 属于原连接；保留原回执，等待重新核对，不能当成新连接的可提交草稿。
        entry.token = token;
        entry.removalReceipt = removalReceipt;
        return result;
      };
      try {
        messageActionManager();
        const sourceManager = manager, scope = appScope;
        const assertCurrent = () => {
          if (manager !== sourceManager || appScope !== scope || sourceManager.disposed)
            throw failure(409, "原生队列连接已变化，请重新核对", { queueConfirmationStage: "connection-scope" });
        };
        const thread = await readThread(entry.threadId);
        assertCurrent();
        if (runtimeCompaction(entry.threadId).state !== "idle")
          throw failure(409, "上下文压缩完成前不能编辑队列消息");
        if (typeof manager.getTurnCoordinator !== "function")
          throw failure(503, "当前 Codex 版本的原生队列服务不可用");
        const coordinator = await manager.getTurnCoordinator();
        const serverQueue = coordinator?.serverQueue;
        if (typeof coordinator?.readMessages !== "function"
            || typeof coordinator?.removeQueuedMessage !== "function"
            || (manager.__codexElvesRequestClientManager === true
              ? typeof coordinator.sendMessage !== "function" || typeof coordinator.execution?.prepare !== "function"
              : typeof coordinator.restoreQueuedMessage !== "function"))
          throw failure(503, "当前 Codex 版本不支持原生队列编辑");
        await loadNativeQueue(coordinator, entry.threadId);
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
        await assertQueuedMessagesUnaccepted(entry.threadId, [current]);
        if (current.submission?.status && current.submission.status !== "queued")
          throw failure(409, "此队列消息正在发送或结果尚未确认，不能编辑");
        let submissions = [];
        if (sourceManager.__codexElvesRequestClientManager === true) {
          // 此适配器的 isEnabled 本身读取完整列表，直接复用一次读取并保留本地队列回退。
          if (thread.ephemeral !== true)
            try { submissions = await readServerQueuedSubmissions(entry.threadId, sourceManager); } catch {}
        } else if (typeof serverQueue?.isEnabled === "function" && await serverQueue.isEnabled(entry.threadId)) {
          submissions = await readServerQueuedSubmissions(entry.threadId, sourceManager);
        }
        const matches = submissions.filter(submission => submission?.id === entry.messageId);
        if (matches.length > 1) throw failure(409, "队列消息身份已变化，请刷新后重试");
        entry.serverQueued = matches.length === 1;
        serverSubmission = matches[0] ?? current.submission?.editPosition?.serverSubmission;
        entry.nativeLocal = manager.__codexElvesRequestClientManager
          && !serverSubmission && !current.serverQueuedMessageId && !current.context?.restoredPromptContext;
        if (!serverSubmission && !entry.nativeLocal)
          throw failure(409, "此队列消息缺少可安全编辑的原生输入");
        if (serverSubmission && serverSubmission.clientUserMessageId !== entry.clientUserMessageId)
          throw failure(409, "队列消息身份已变化，请刷新后重试");
        if (entry.expectedImageSources != null) {
          if (serverSubmission) await buildQueuedImagePlan(current, serverSubmission.input, entry.expectedImageSources);
          else if (!sameJsonValue((await queuedImageAttachmentRecords(current)).map(record => record.reference),
              entry.expectedImageSources)) throw failure(409, "队列图片已变化，请刷新后重试");
        }
        if (sourceManager.__codexElvesRequestClientManager === true && serverSubmission)
          await prepareNativeQueuedCancel(entry, { message: current, serverSubmission,
            preserveSerializedContext: true }, coordinator, assertCurrent);
        assertCurrent();
        entry.state = "removing";
        removeStarted = true;
        removalReceipt = { manager: sourceManager, scope, requestClient: sourceManager.requestClient,
          acknowledged: false, result: undefined };
        const removed = await coordinator.removeQueuedMessage(entry.threadId, entry.messageId);
        rememberRemoval(removed);
        if (removed == null) {
          removeStarted = false;
          throw failure(409, "此队列消息已执行或不存在，请刷新会话");
        }
        assertCurrent();
        if (entry.token.canRestore === false)
          return removalUnknown("原生队列已移除消息，但未提供可恢复状态；请刷新会话检查");
        entry.state = "editing";
        const result = {
          ok: true,
          threadId: entry.threadId,
          editId: entry.editId,
          messageId: entry.messageId,
          editState: "editing",
          ...queueSnapshotFields(entry.threadId, coordinator.readMessages(entry.threadId))
        };
        entry.beginResult = compactQueuedEditResult(result);
        return result;
      } catch (error) {
        if (removeStarted) {
          if (error.queueMutationAcknowledged === true) rememberRemoval(error.queueMutationResult);
          return removalUnknown("原生队列移除结果未确认，请刷新会话检查，不要重复开始编辑", error);
        }
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
    const entry = queuedEditStore.sessions.get(payload.editId);
    if (entry) entry.finishRequests = (entry.finishRequests || 0) + 1;
    try {
      const result = await finishQueuedEditOperation(payload, action);
      // 队列内容可能先于编辑终态变化；其他页面也需要收到已确认的终态快照。
      if (result?.ok === true && ["committed", "canceled"].includes(result.editState)
          && Array.isArray(result.queuedMessages) && Array.isArray(result.queuedEdits))
        pushNotification({ method: "thread/queue/changed", params: {
          threadId: result.threadId, queueEpoch: result.queueEpoch, queueRevision: result.queueRevision,
          queuedMessages: result.queuedMessages, queuedEdits: result.queuedEdits
        } });
      return result;
    }
    finally { if (entry) entry.finishRequests -= 1; }
  }
  function nativeQueuedEditSubmission(entry, token, expectedClientId) {
    // 原生 composer 把 remove token 映射为 editPosition，并通过 submission 承接队列消息。
    // 保存使用改写后的 token，服务器队列取消使用准确原 token；均保留 serverSubmission 和原位置锚点，
    // 避免 Undo restore 在服务器立即消费后再次 reorder 而回填 paused 副本。
    const message = { ...token.message, id: expectedClientId,
      clientUserMessageId: expectedClientId, createdAt: Date.now() };
    for (const key of ["submission", "submissionIntent", "pausedReason", "acceptedAtMs", "canSendNow", "serverQueuedMessageId"])
      delete message[key];
    return {
      conversationId: entry.threadId, message, acceptLocally: false, queueModeOverride: "queue",
      editPosition: {
        messageId: entry.messageId, previousMessageId: token.previousMessageId,
        nextMessageId: token.nextMessageId, serverSubmission: token.serverSubmission,
        preserveSerializedContext: token.preserveSerializedContext,
        multiUserMessageSender: token.message.multiUserMessageSender ?? null,
        isImageEditFollowUp: token.message.context?.isImageEditFollowUp,
        turnTrigger: token.message.context?.turnTrigger,
        restoredPromptContext: token.message.context?.restoredPromptContext
      }
    };
  }
  async function prepareNativeQueuedEdit(request, phase, coordinator, assertCurrent, expectedInput = null) {
    assertCurrent();
    const clientId = request.message.clientUserMessageId;
    const prepared = await retryExecutionConfigLoading(
      () => coordinator.execution.prepare(request.conversationId, request.message, phase, clientId), assertCurrent
    );
    assertCurrent();
    if (prepared?.status === "paused") throw failure(409, prepared.reason || "原生队列编辑暂不可提交");
    if (prepared?.status !== "ready" || prepared.submission?.conversationId !== request.conversationId
        || prepared.submission.steer?.clientUserMessageId !== clientId
        || !Array.isArray(prepared.submission.steer.input)
        || phase === "start" && (prepared.submission.start?.request?.threadId !== request.conversationId
          || prepared.submission.start.request.clientUserMessageId !== clientId
          || !Array.isArray(prepared.submission.start.request.input)))
      throw failure(503, "原生队列编辑准备结果不符合提交身份");
    if (expectedInput && (!sameQueuedUserInput(prepared.submission.steer.input, expectedInput)
        || phase === "start" && !sameQueuedUserInput(prepared.submission.start.request.input, expectedInput)))
      throw failure(409, "原生队列编辑输入已变化，请重新核对");
    return prepared.submission;
  }
  function assertNativeQueuedCancelInput(request, prepared = null) {
    const original = request.editPosition.serverSubmission.input;
    const context = request.message.context;
    const unsupported = /(?:^\[from:|<codex_multi_user_message>|<page-comment-context>|## My request(?: for Codex)?:|# Files (?:mentioned|pasted) by the user:|chatgpt-library-file:\/\/)/;
    const texts = original.filter(item => item.type === "text");
    if (original.some(item => ["audio", "localAudio"].includes(item.type))
        || texts.some(item => item.text_elements?.length || unsupported.test(item.text))
        || context.workPermissions != null || context.worktreeRecovery != null || context.collaborationMode != null)
      throw failure(409, "此队列消息的原生输入或权限暂不支持安全取消，请在 Codex 桌面恢复");
    const options = request.message.submissionOptions;
    if (options && (options.executionHostId != null && options.executionHostId !== "local"
        || options.agentMode != null && options.agentMode !== "auto" || options.permissionProfileId != null
        || options.serviceTier != null || options.shouldSendPermissionOverrides || options.usePermissionSelection
        || options.permissionSelection != null || options.collaborationMode != null))
      throw failure(409, "此队列消息带有独立执行配置，不能按当前默认配置恢复");
    if (!prepared) return;
    if (prepared.executionHostId != null && prepared.executionHostId !== "local")
      throw failure(409, "原生取消准备不属于原本地执行宿主");
    // 只接受已核 w8t 可证明等价的普通文本及图片：原生 prepare 产生完全相同的内容，
    // 其余原输入只能由 w8t 追加一次；不把完整原 input 强填回 prepare。
    const verify = input => {
      if (!Array.isArray(input) || input.some(item => !["text", "image", "localImage"].includes(item.type)))
        throw failure(409, "原生准备已包含额外输入，不能保证取消后原输入只出现一次");
      if (!sameQueuedUserInput(input.filter(item => item.type === "text"), texts))
        throw failure(409, "原生取消准备的文本与原输入不一致");
      const visible = input.map(item => item.type === "image"
        ? original.find(old => old.type === "image" && (old.url != null && old.url === item.url
          || old.fileId != null && old.fileId === item.fileId)) ?? item
        : item.type === "localImage"
          ? original.find(old => old.type === "localImage" && old.path === item.path) ?? item : item);
      const hidden = original.filter(item => item.type !== "text" && item.type !== "localImage"
        && (item.type !== "image" || item.fileId != null && !input.some(value =>
          value.type === "image" && value.fileId === item.fileId)));
      if (!sameQueuedUserInput([...visible, ...hidden], original))
        throw failure(409, "原生取消准备不能按原顺序恢复完整输入");
    };
    verify(prepared.steer.input);
    if (prepared.start) verify(prepared.start.request.input);
  }
  async function prepareNativeQueuedCancel(entry, token, coordinator, assertCurrent) {
    assertCurrent();
    if (token.message.pausedReason != null)
      throw failure(409, "此服务器队列消息原本已暂停，不能通过取消编辑解除暂停");
    const clientId = token.restoreClientUserMessageId ?? token.serverSubmission.clientUserMessageId;
    const request = nativeQueuedEditSubmission(entry, token, clientId);
    assertNativeQueuedCancelInput(request);
    await coordinator.assertQueuedEditContext(entry.threadId);
    const prepared = await prepareNativeQueuedEdit(request, "start", coordinator, assertCurrent);
    assertNativeQueuedCancelInput(request, prepared);
    for (const serialized of await coordinator.execution.queuedEditInputs(request.message)) {
      if (serialized.responseItems?.length)
        throw failure(409, "此队列消息包含额外原生响应输入，不能安全取消");
      assertNativeQueuedCancelInput(request, { steer: { input: serialized.input } });
    }
    assertCurrent();
    return request;
  }
  async function submitNativeQueuedEdit(entry, request, coordinator, assertCurrent, expectedLocalInput = null,
      cancelOriginal = false) {
    if (typeof coordinator.sendMessage !== "function" || typeof coordinator.execution?.prepare !== "function")
      throw failure(503, "当前 Codex 版本不支持原生队列编辑提交");
    const expectedClientId = request.message.clientUserMessageId;
    const assertDispatch = async () => {
      assertCurrent();
      if (cancelOriginal) await coordinator.assertQueuedEditContext(entry.threadId);
      assertCurrent();
    };
    const result = await coordinator.sendMessage(request, async (queued, phase) => {
      assertCurrent();
      if (queued.conversationId !== entry.threadId || queued.message?.id !== expectedClientId
          || queued.message.clientUserMessageId !== expectedClientId)
        throw failure(409, "原生队列编辑提交身份已变化");
      const prepared = await prepareNativeQueuedEdit({ ...request, message: queued.message },
        phase, coordinator, assertCurrent, expectedLocalInput);
      if (cancelOriginal) assertNativeQueuedCancelInput(request, prepared);
      return prepared;
    }, undefined, assertDispatch, assertDispatch, { deferQueueSnapshotOnSent: true });
    assertCurrent();
    if (!["sent", "queued"].includes(result?.status) || typeof result.messageId !== "string" || !result.messageId
        || result.status === "sent" && result.messageId !== expectedClientId
        || result.status === "sent" && (
          result.threadId != null && result.threadId !== entry.threadId
          || result.conversationId != null && result.conversationId !== entry.threadId
          || result.clientUserMessageId != null && result.clientUserMessageId !== expectedClientId)
        || result.status === "queued" && result.serverAccepted !== true && result.messageId !== expectedClientId)
      throw new Error("原生队列编辑提交未确认消息身份");
    return result;
  }
  async function finishQueuedEditOperation(payload = {}, action) {
    let threadId, editId, prompt = null, retainedImageSources = null, addedImages = null;
    try {
      threadId = requiredId(payload.threadId);
      editId = requiredId(payload.editId);
      if (action === "commit") {
        prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
        retainedImageSources = payload.retainedImageSources == null
          ? null : validateQueuedImageSources(payload.retainedImageSources, "队列保留图片");
        addedImages = payload.images == null ? null : remoteImageAttachments(payload, "queue-edit");
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
    const operationFingerprint = await queuedEditFingerprint(
      action === "commit" ? "editQueued" : "cancelQueuedEdit", payload);
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
    entry.terminalFingerprint = operationFingerprint;
    entry.terminalPrompt = prompt;
    entry.terminalRetainedImageSources = retainedImageSources;
    entry.terminalAddedImageFingerprint = addedImageFingerprint;
    entry.state = action === "commit" ? "committing" : "canceling";
    entry.terminalPromise = (async () => {
      let restoreStarted = false;
      let expectedInput = Array.isArray(entry.token?.serverSubmission?.input)
        ? entry.token.serverSubmission.input : null;
      let expectedImageSources = entry.expectedImageSources;
      let confirmationStage = "restore-token";
      let restoreAcknowledged = false;
      let submittedEdit = null;
      let nativeSubmissionStarted = false;
      try {
        messageActionManager();
        const sourceManager = manager, scope = appScope;
        const assertCurrent = () => {
          if (manager !== sourceManager || appScope !== scope || sourceManager.disposed)
            throw failure(409, "原生队列连接已变化，请重新核对", { queueConfirmationStage: "connection-scope" });
        };
        if (typeof sourceManager.getTurnCoordinator !== "function")
          throw failure(503, "当前 Codex 版本的原生队列服务不可用");
        const coordinator = await sourceManager.getTurnCoordinator();
        assertCurrent();
        const nativeSubmission = sourceManager.__codexElvesRequestClientManager === true
          && (action === "commit" || entry.token?.serverSubmission != null);
        if (typeof coordinator?.readMessages !== "function"
            || (nativeSubmission
              ? typeof coordinator.sendMessage !== "function" || typeof coordinator.execution?.prepare !== "function"
              : typeof coordinator.restoreQueuedMessage !== "function"))
          throw failure(503, "当前 Codex 版本不支持恢复原生队列消息");
        let restoreToken = entry.token;
        const tokenMessage = restoreToken.message;
        if (tokenMessage?.id !== entry.messageId
            || (tokenMessage.clientUserMessageId ?? tokenMessage.submissionOptions?.clientUserMessageId
              ?? tokenMessage.id) !== entry.clientUserMessageId
            || !queuedTextMatches(queuedMessageText(tokenMessage), entry.originalText)
            || Number.isFinite(entry.createdAt) && tokenMessage.createdAt !== entry.createdAt
            || restoreToken.threadId != null && restoreToken.threadId !== entry.threadId
            || restoreToken.conversationId != null && restoreToken.conversationId !== entry.threadId
            || entry.serverQueued && (restoreToken.serverSubmission?.id !== entry.messageId
              || restoreToken.serverSubmission.clientUserMessageId !== entry.clientUserMessageId))
          throw failure(409, "原生恢复令牌不属于本次队列消息");
        if (entry.expectedImageSources != null
            && !sameJsonValue((await queuedImageAttachmentRecords(tokenMessage)).map(record => record.reference),
              entry.expectedImageSources))
          throw failure(409, "原生恢复令牌的图片不属于本次编辑基线");
        confirmationStage = "prepare-input";
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
            if (entry.serverQueued === false && message.submission?.editPosition?.serverSubmission) {
              message = { ...message, submission: { ...message.submission,
                editPosition: { ...message.submission.editPosition, serverSubmission } } };
            }
          } else if (retainedImageSources != null || addedImages != null) {
            if (!entry.nativeLocal) throw failure(409, "此队列消息缺少可安全编辑的原生图片输入");
            const plan = await queuedImageAttachmentRecords(message);
            if (!sameJsonValue(plan.map(record => record.reference), entry.expectedImageSources))
              throw failure(409, "队列图片已变化，请刷新后重试");
            message = rewriteQueuedMessageImages(message, plan, effectiveRetainedSources);
            message = appendQueuedMessageImages(message, addedImagePlan.attachments);
            expectedImageSources = [...effectiveRetainedSources, ...addedImagePlan.references];
          }
          restoreToken = {
            ...entry.token,
            message,
            ...(serverSubmission == null ? {} : { serverSubmission })
          };
        }
        confirmationStage = "acceptance-baseline";
        const expectedClientId = restoreToken.restoreClientUserMessageId
          ?? restoreToken.serverSubmission?.clientUserMessageId ?? entry.clientUserMessageId;
        assertCurrent();
        const cancelOriginal = action === "cancel" && nativeSubmission;
        const nativeRequest = cancelOriginal
          ? await prepareNativeQueuedCancel(entry, restoreToken, coordinator, assertCurrent)
          : nativeSubmission ? nativeQueuedEditSubmission(entry, restoreToken, expectedClientId) : null;
        if (action === "cancel" && sourceManager.__codexElvesRequestClientManager === true
            && !nativeSubmission && restoreToken.message.pausedReason == null && !Array.isArray(expectedInput)) {
          const prepared = await prepareNativeQueuedEdit({
            conversationId: entry.threadId, message: restoreToken.message
          }, "start", coordinator, assertCurrent);
          expectedInput = structuredClone(prepared.start.request.input);
        }
        if (nativeSubmission && !Array.isArray(expectedInput)) {
          const prepared = await prepareNativeQueuedEdit(nativeRequest, "start", coordinator, assertCurrent);
          expectedInput = structuredClone(prepared.start.request.input);
        }
        const acceptanceProof = await prepareQueuedEditAcceptanceProof(entry, expectedClientId, expectedInput);
        assertCurrent();
        confirmationStage = "restore";
        restoreStarted = true;
        nativeSubmissionStarted = nativeSubmission;
        if (acceptanceProof) acceptanceProof.dispatched = true;
        const restoredId = nativeSubmission
          ? (submittedEdit = await submitNativeQueuedEdit(entry, nativeRequest, coordinator, assertCurrent,
            entry.nativeLocal ? expectedInput : null, cancelOriginal)).messageId
          : await coordinator.restoreQueuedMessage(entry.threadId, restoreToken);
        restoreAcknowledged = true;
        if (acceptanceProof) acceptanceProof.mutationAcknowledged = true;
        assertCurrent();
        const messageId = requiredId(restoredId);
        entry.restoredMessageId = messageId;
        if (submittedEdit?.status === "sent") {
          confirmationStage = "accepted-history";
          const accepted = await reconcileQueuedEditAcceptance(entry).catch(() => null);
          assertCurrent();
          if (accepted) return accepted;
          throw new Error("原生编辑已提交，完整用户输入接受记录仍待确认");
        }
        confirmationStage = "restored-message";
        const restoredMessage = array(coordinator.readMessages(entry.threadId))
          .find(message => message?.id === messageId);
        if (!restoredMessage
            || restoredMessage.clientUserMessageId !== expectedClientId
              && !(messageId === entry.messageId && restoredMessage.clientUserMessageId === entry.clientUserMessageId)
            || !queuedTextMatches(queuedMessageText(restoredMessage), action === "commit" ? prompt : entry.originalText))
          throw new Error("原生队列恢复后的消息身份或内容未确认");
        if (expectedImageSources != null) {
          confirmationStage = "images";
          const restoredSources = (await queuedImageAttachmentRecords(restoredMessage))
            .map(record => record.reference);
          if (!sameJsonValue(restoredSources, expectedImageSources))
            throw new Error("原生队列恢复后的图片未确认");
        }
        if (!entry.nativeLocal && Array.isArray(expectedInput)) {
          confirmationStage = "server-input";
          const submissions = entry.serverQueued === false
            || submittedEdit?.status === "queued" && submittedEdit.serverAccepted !== true
            ? [restoredMessage.submission?.editPosition?.serverSubmission]
            : (await readServerQueuedSubmissions(entry.threadId)).filter(submission => submission?.id === messageId);
          if (submissions.length !== 1 || !submissions[0]
              || submissions[0].clientUserMessageId !== expectedClientId
              || !sameQueuedUserInput(submissions[0].input, expectedInput))
            throw new Error("原生队列恢复后的真实输入未确认");
          if (expectedImageSources != null) {
            confirmationStage = "image-input-binding";
            await buildQueuedImagePlan(restoredMessage, submissions[0].input, expectedImageSources);
          }
        }
        const accepted = await reconcileQueuedEditAcceptance(entry).catch(() => null);
        assertCurrent();
        if (accepted) return accepted;
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
        queuedEditAcceptanceProofs.delete(entry);
        return {
          ...entry.result,
          ...queueSnapshotFields(entry.threadId, coordinator.readMessages(entry.threadId))
        };
      } catch (error) {
        if (restoreStarted) {
          const proof = manager?.__codexElvesRequestClientManager === true
            ? queuedEditAcceptanceProofs.get(entry) : null;
          let mutationAcknowledged = restoreAcknowledged;
          if (error.queueMutationAcknowledged === true) {
            const result = error.queueMutationResult;
            const nativeSubmission = nativeSubmissionStarted;
            const clientId = proof?.clientUserMessageId;
            const confirmed = nativeSubmission
              ? ["sent", "queued"].includes(result?.status) && typeof result.messageId === "string" && !!result.messageId
                && (result.threadId == null || result.threadId === entry.threadId)
                && (result.conversationId == null || result.conversationId === entry.threadId)
                && (result.clientUserMessageId == null || result.clientUserMessageId === clientId)
                && (result.status === "queued" && result.serverAccepted === true || result.messageId === clientId)
              : typeof result === "string" && !!result;
            if (confirmed) {
              entry.restoredMessageId = nativeSubmission ? result.messageId : result;
              mutationAcknowledged = true;
              if (proof) proof.mutationAcknowledged = true;
            }
          }
          const accepted = await reconcileQueuedEditAcceptance(entry).catch(() => null);
          if (accepted) return accepted;
          return queuedEditOutcomeUnknown(entry, action === "commit"
            ? "原生队列保存结果未确认，请刷新队列检查，不要重复保存"
            : "原生队列恢复结果未确认，请刷新队列检查，不要重复取消", {
              stage: error.queueConfirmationStage || confirmationStage,
              mutationAcknowledged,
              reason: error.message || "原生队列确认未完成"
            });
        }
        entry.terminalAction = null;
        entry.terminalFingerprint = null;
        entry.terminalPrompt = null;
        entry.terminalRetainedImageSources = null;
        entry.terminalAddedImageFingerprint = null;
        entry.state = "editing";
        entry.result = null;
        queuedEditAcceptanceProofs.delete(entry);
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
      await assertQueuedMessagesUnaccepted(threadId, [current]);
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
          || !sameQueuedUserInput(result.queuedSubmission.input, input))
        throw new Error("原生队列未确认原地编辑结果");
      const queued = array(coordinator.readMessages(threadId)).map(message =>
        message?.id === messageId ? queuedMessageWithText(message, prompt) : message);
      return {
        ok: true,
        threadId,
        messageId,
        replacedMessageId: messageId,
        ...queueSnapshotFields(threadId, queued)
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
    let threadId = null, messageId = null;
    let preDispatchError = null;
    // RPC 会重建回调抛出的 Error，不能依赖它保留 notSent 等自定义字段。
    const beforeDispatch = callback => async (...args) => {
      preDispatchError = null;
      try { return await callback(...args); }
      catch (error) { preDispatchError = error; throw error; }
    };
    try {
      const sourceManager = messageActionManager();
      threadId = requiredId(payload.threadId);
      messageId = requiredId(payload.messageId);
      const assertCurrent = () => {
        if (manager !== sourceManager || sourceManager.disposed || isDeletedThread(threadId))
          throw failure(409, "会话连接已变化或会话已删除，请重新核对后引导");
      };
      await readThread(threadId);
      assertCurrent();
      if (runtimeCompaction(threadId).state !== "idle")
        throw failure(409, "上下文压缩完成前不能引导队列消息");
      const coordinator = await sourceManager.getTurnCoordinator?.();
      if (typeof coordinator?.sendQueuedMessageNow !== "function"
          || typeof coordinator.readMessages !== "function"
          || typeof coordinator.execution?.prepare !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列引导");
      const nativeFinalAdmission = sourceManager.__codexElvesRequestClientManager === true;
      const prepare = async (request, phase) => {
        if (request?.conversationId !== threadId || request?.message?.id !== messageId)
          throw failure(409, "队列消息身份已变化，请刷新会话");
        const message = request.message;
        // aEs 会先恢复工作树再构造 submission，此分支仍须在原生 prepare 前核验接受状态。
        if (!nativeFinalAdmission || message.context?.worktreeRecovery?.creationRequest != null)
          await assertQueuedMessagesUnaccepted(threadId, [message], sourceManager);
        assertCurrent();
        const serverQueuedMessage = message.serverQueuedMessageId != null;
        let clientUserMessageId = requiredId(message.clientUserMessageId
          ?? message.submissionOptions?.clientUserMessageId ?? message.id);
        let serverInput = null;
        if (serverQueuedMessage) {
          let submissions;
          try { submissions = await readServerQueuedSubmissions(threadId); }
          catch {
            throw failure(503, "原生队列身份读取失败，请刷新后重试", { notSent: true, readFailure: true });
          }
          const matches = submissions.filter(submission => submission?.id === messageId);
          assertCurrent();
          if (message.serverQueuedMessageId !== messageId || matches.length !== 1
              || typeof matches[0].clientUserMessageId !== "string" || !matches[0].clientUserMessageId
              || request.options?.clientUserMessageId !== matches[0].clientUserMessageId
              || message.clientUserMessageId != null
                && message.clientUserMessageId !== matches[0].clientUserMessageId
              || !Array.isArray(matches[0].input))
            throw failure(409, "服务端队列消息身份无效，请刷新会话");
          clientUserMessageId = matches[0].clientUserMessageId;
          serverInput = structuredClone(matches[0].input);
        } else if (request.options?.clientUserMessageId != null
            && request.options.clientUserMessageId !== clientUserMessageId) {
          throw failure(409, "队列消息身份与原生准备参数不一致，请刷新会话");
        }
        const baseline = structuredClone(message);
        const targetTurnIds = [request.options?.expectedTurnId, request.options?.targetTurnId, message.targetTurnId]
          .filter(value => value != null).map(requiredId);
        if (new Set(targetTurnIds).size > 1)
          throw failure(409, "队列消息的目标轮次已变化，请刷新会话");
        const expectedTurnId = targetTurnIds[0] ?? nativeTurnId(nativeConversationTurns(
          sourceManager.getConversation(threadId)).findLast(turn => nativeTurnStatus(turn) === "inProgress"));
        let prepared;
        try {
          prepared = await retryExecutionConfigLoading(
            () => coordinator.execution.prepare(request.conversationId, request.message, phase, clientUserMessageId),
            assertCurrent
          );
        } catch (error) {
          if (error.remoteValidationFailure) throw error;
          throw failure(error.code || 503, "原生队列准备失败，消息尚未引导，请稍后重试",
            { notSent: true, preparationFailed: true });
        }
        assertCurrent();
        if (prepared?.status === "paused")
          throw failure(409, prepared.reason || "原生队列消息暂时不可用");
        if (prepared?.status !== "ready" || !prepared.submission)
          throw failure(503, "原生队列消息准备失败");
        const submission = prepared.submission;
        const start = submission.start;
        const steer = submission.steer;
        const admissionStart = steer?.userMessageAdmissionStart;
        const preparedClientId = sourceManager.__codexElvesRequestClientManager
          ? clientUserMessageId : message.id;
        const restoreMatches = restored => restored != null && sameJsonValue(restored, {
          ...baseline,
          context: { ...baseline.context, ...(restored.context?.turnTrigger === "send_user_message"
            ? { turnTrigger: "send_user_message" } : {}) }
        });
        const targetMatches = target => target == null || expectedTurnId != null && target === expectedTurnId;
        const startMatches = value => value?.request?.threadId === threadId
          && value.request.clientUserMessageId === preparedClientId
          && sameQueuedUserInput(value.request.input, steer?.input)
          && restoreMatches(value.context?.restoreMessage);
        if (submission.conversationId !== threadId
            || submission.resume?.conversationId != null && submission.resume.conversationId !== threadId
            || steer?.clientUserMessageId !== preparedClientId
            || !restoreMatches(steer.restoreMessage) || !Array.isArray(steer.input)
            || !targetMatches(steer.expectedTurnId) || !targetMatches(steer.targetTurnId)
            || serverInput != null && !sameQueuedUserInput(steer.input, serverInput)
            || start == null && !(sourceManager.__codexElvesRequestClientManager && phase === "steer")
            || start != null && !startMatches(start)
            || admissionStart != null && !startMatches(admissionStart))
          throw failure(503, "原生准备结果不符合队列消息身份、输入或目标轮次契约");
        if (clientUserMessageId === preparedClientId) return submission;
        if (start == null) throw failure(503, "原生队列缺少可确认的开始请求");
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
      const assertAdmission = async () => {
        assertCurrent();
        const current = array(coordinator.readMessages(threadId)).find(message => message?.id === messageId);
        if (!current) throw failure(409, "此队列消息已变化，请重新核对", { notSent: true });
        await assertQueuedMessagesUnaccepted(threadId, [current], sourceManager);
        assertCurrent();
      };
      // request-client 的第五参由原生服务端队列和本地队列在锁内、派发前调用；
      // 完整历史接受核验保留在该最终回调，普通消息避免外层和 prepare 前的重复全量读取。
      if (!nativeFinalAdmission) await assertAdmission();
      submitted = true;
      const result = await coordinator.sendQueuedMessageNow(threadId, messageId, undefined,
        beforeDispatch(prepare), beforeDispatch(assertAdmission));
      if (result?.status !== "sent") throw failure(409, "此队列消息当前无法引导，请刷新会话");
      return { ok: true, threadId, messageId, turnId: result.turnId,
        ...(result.queueSyncPending === true ? { queueSyncPending: true }
          : queueSnapshotFields(threadId, coordinator.readMessages(threadId))) };
    } catch (error) {
      const acknowledged = error.queueMutationAcknowledged && error.queueMutationResult;
      if (acknowledged?.status === "sent" && typeof acknowledged.turnId === "string"
          && (acknowledged.messageId == null || acknowledged.messageId === messageId))
        return { ok: true, threadId, messageId, turnId: acknowledged.turnId,
          queueSyncPending: true, message: "引导已完成，但队列快照暂不可用，请刷新会话" };
      if (preDispatchError) error = preDispatchError;
      return { ok: false, code: error.code || 503,
        ...(preDispatchError || error.notSent === true ? { notSent: true } : {}),
        ...(submitted && !preDispatchError && !error.remoteValidationFailure ? { outcomeUnknown: true } : {}),
        message: error.message || "引导结果未确认，请查看原生队列，不要重复操作" };
    }
  }
  async function skillCatalog(payload, trustedCwd = null, { forceReload = false } = {}) {
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
    const result = await manager.requestClient.sendRequest("skills/list", { cwds: [cwd], forceReload });
    const skills = new Map();
    for (const entry of array(result.data)) {
      if (normalizePath(entry.cwd) !== normalizePath(cwd)) continue;
      for (const skill of array(entry.skills)) {
        if (!skill.enabled || typeof skill.name !== "string" || typeof skill.path !== "string") continue;
        skills.set(skill.path, {
          name: skill.name, path: skill.path, displayName: skill.interface?.displayName || skill.name,
          description: skill.interface?.shortDescription || skill.shortDescription || skill.description || "",
          scope: ["user", "system", "admin"].includes(skill.scope) ? skill.scope : "repo"
        });
      }
    }
    return { ok: true, cwd, data: [...skills.values()] };
  }
  async function skillInputs(payload, cwd = null) {
    if (payload.skills == null) return [];
    if (!Array.isArray(payload.skills) || payload.skills.length > 16
        || payload.skills.some(skill => !skill || typeof skill.name !== "string" || typeof skill.path !== "string"))
      throw failure(400, "Skill 选择无效，最多选择 16 项");
    if (!payload.skills.length) return [];
    // 菜单可长时间复用目录；真正提交前仍重新核验项目适用范围和启用状态。
    const available = (await skillCatalog(payload, cwd, { forceReload: true })).data;
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
  function remoteImageAttachments(payload = {}, phase = "unknown") {
    if (payload.images == null) return [];
    if (!Array.isArray(payload.images) || payload.images.length > 4)
      throw failure(400, "每条消息最多添加 4 张图片");
    let total = 0;
    return payload.images.map((image, index) => {
      if (!image || typeof image.name !== "string" || !image.name.trim() || image.name.length > 255
          || !["image/png", "image/jpeg", "image/webp", "image/gif"].includes(image.mime)
          || !Number.isSafeInteger(image.size) || image.size <= 0 || image.size > remoteInputImageMaxBytes
          || typeof image.dataUrl !== "string" || image.dataUrl.length > remoteInputImageMaxSourceLength
          || !remoteImageDataPattern.test(image.dataUrl)
          || !image.dataUrl.toLowerCase().startsWith(`data:${image.mime};base64,`))
        throw failure(400, "图片附件参数无效");
      total += image.size;
      if (total > 12 * 1024 * 1024) throw failure(413, "图片总大小不能超过 12 MiB");
      const attachment = {
        id: nativeImageAttachmentId(),
        src: image.dataUrl,
        filename: image.name.trim(),
        localPath: null
      };
      if (phase !== "send-validation") {
        reportRemoteImageDiagnostic("remote_image_attachment_normalized", {
          phase,
          index,
          mime: image.mime,
          sizeBytes: image.size,
          sourceLength: image.dataUrl.length,
          idPrefix: "remote-image",
        });
      }
      return attachment;
    });
  }
  function remoteAttachmentContext(prompt, cwd, images) {
    validateNativeImageAttachments(images, "attachment-context");
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
  function hasNativeLocalQueuedMessages(coordinator, serverQueue, threadId) {
    const serverIds = new Set(array(serverQueue?.read?.(threadId)).map(message => message.id));
    return array(coordinator.readMessages(threadId)).some(message => !serverIds.has(message.id));
  }
  function validateCompactionQueueSendPayload(action, payload) {
    if (!Object.prototype.hasOwnProperty.call(payload, "queueOnly")) return;
    if (typeof payload.queueOnly !== "boolean")
      throw failure(400, "排队发送参数无效");
    if (action !== "send" || payload.goalMode === true)
      throw failure(400, "排队发送参数仅适用于普通消息");
  }
  async function queueMessage(payload = {}, allowImmediate = false) {
    try {
      validateCompactionQueueSendPayload("send", payload);
      if (payload.clientUserMessageId == null) return await submitQueuedMessage(payload, allowImmediate);
      const key = JSON.stringify([requiredId(payload.threadId), requiredId(payload.clientUserMessageId)]);
      const identity = JSON.parse(JSON.stringify({
        prompt: typeof payload.prompt === "string" ? payload.prompt.trim() : "",
        images: payload.images ?? [], skills: payload.skills ?? [],
        model: payload.model == null ? null : String(payload.model),
        reasoningEffort: payload.reasoningEffort == null ? null : String(payload.reasoningEffort)
      }));
      const current = queuedSendSubmissions.get(key);
      if (current) {
        if (!sameJsonValue(current.identity, identity))
          throw failure(409, "同一消息 ID 的排队内容或设置已变化，请检查原请求");
        if (current.unconfirmed) {
          try {
            messageActionManager();
            const recovered = await queueSendState({
              threadId: payload.threadId, clientUserMessageId: payload.clientUserMessageId
            });
            if (recovered.confirmed) {
              if (queuedSendSubmissions.get(key) === current) queuedSendSubmissions.delete(key);
              return recovered;
            }
          } catch {}
        }
        return await current.promise;
      }
      const entry = { identity, promise: null, unconfirmed: false };
      queuedSendSubmissions.set(key, entry);
      entry.promise = Promise.resolve().then(() => submitQueuedMessage({
        ...payload, images: identity.images, skills: identity.skills
      }, allowImmediate));
      try {
        const result = await entry.promise;
        // 未知回执只允许查询原生结果；即使暂时读不到消息，也不能重放 add。
        entry.unconfirmed = result?.outcomeUnknown === true;
        return result;
      } finally {
        if (!entry.unconfirmed && queuedSendSubmissions.get(key) === entry) queuedSendSubmissions.delete(key);
      }
    } catch (error) {
      return { ok: false, code: error.code || 503, message: error.message || "加入原生队列失败" };
    }
  }
  async function submitQueuedMessage(payload = {}, allowImmediate = false) {
    let threadId = null;
    let clientUserMessageId = null;
    let queueAddStarted = false;
    let localQueueStarted = false;
    let queuedSubmission = null;
    let sendNow = false;
    try {
      validateCompactionQueueSendPayload("send", payload);
      const sourceManager = messageActionManager();
      threadId = requiredId(payload.threadId);
      const assertCurrent = () => {
        if (manager !== sourceManager || sourceManager.disposed || isDeletedThread(threadId))
          throw failure(409, "会话连接已变化或会话已删除，请重新核对后发送");
      };
      const prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
      const imageAttachments = remoteImageAttachments(payload, "queue-send");
      if (!prompt && !imageAttachments.length) throw failure(400, "消息不能为空");
      const thread = await readThread(threadId);
      assertCurrent();
      if (typeof manager.getTurnCoordinator !== "function"
          || typeof manager.requestClient?.sendRequest !== "function")
        throw failure(503, "当前 Codex 版本的原生队列服务不可用");
      const coordinator = await manager.getTurnCoordinator();
      const serverQueue = coordinator?.serverQueue;
      if (typeof coordinator?.execution?.prepare !== "function"
          || typeof coordinator?.readMessages !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列发送");
      await loadNativeQueue(coordinator, threadId);
      assertCurrent();
      const streamRole = sourceManager.getStreamRole?.(threadId)
        ?? coordinator.getStreamRole?.(threadId)
        ?? coordinator.options?.getStreamRole?.(threadId);
      sendNow = allowImmediate && streamRole?.role !== "follower";
      const cwd = manager.getConversationCwd(threadId) || thread.cwd || "/";
      const message = {
        id: payload.clientUserMessageId == null ? crypto.randomUUID() : requiredId(payload.clientUserMessageId),
        text: prompt,
        context: remoteAttachmentContext(prompt, cwd, imageAttachments),
        cwd,
        createdAt: Date.now()
      };
      clientUserMessageId = message.id;
      message.clientUserMessageId = clientUserMessageId;
      if (payload.clientUserMessageId != null) {
        const previous = await queueSendState({ threadId, clientUserMessageId });
        assertCurrent();
        if (previous.confirmed) return previous;
        const unresolved = array(coordinator.readMessages(threadId)).some(queued =>
          (queued.clientUserMessageId ?? queued.id) === clientUserMessageId
          && ["pending", "sending", "outcome-unknown"].includes(queued.submission?.status));
        if (unresolved) return {
          ok: false, outcomeUnknown: true, code: 503, threadId, clientUserMessageId,
          message: "原生队列加入结果未确认，请检查会话，不要重复操作"
        };
      }
      let useServerQueue = !sendNow && typeof serverQueue?.isEnabled === "function"
        && await serverQueue.isEnabled(threadId)
        && typeof serverQueue.read === "function"
        && !hasNativeLocalQueuedMessages(coordinator, serverQueue, threadId)
        && typeof serverQueue.canEnqueue === "function" && await serverQueue.canEnqueue(threadId, message);
      if (!useServerQueue && (typeof coordinator.sendMessage !== "function" || typeof coordinator.loadMessages !== "function"))
        throw failure(503, "当前 Codex 版本不支持原生本地消息队列");
      const inputItems = await skillInputs({ ...payload, threadId });
      const conversation = sourceManager.getConversation(threadId);
      const threadSettings = await requestedThreadModelSettings(payload, thread, conversation);
      assertCurrent();
      const resumed = await retryExecutionConfigLoading(() => sourceManager.resumeConversation({
        conversationId: threadId,
        model: threadSettings?.model ?? null,
        reasoningEffort: threadSettings?.effort ?? null,
        workspaceRoots: [cwd],
        collaborationMode: conversation?.latestCollaborationMode ?? null
      }), assertCurrent);
      assertCurrent();
      if (resumed?.status === "not-ready") throw failure(409, "会话暂不可用，请稍后重试");
      await applyThreadModelSettings(threadId, threadSettings);
      assertCurrent();
      const nativeQueueIntent = sourceManager.__codexElvesRequestClientManager === true
        && sendNow && !useServerQueue && payload.queueOnly !== true
        && message.requiresExistingOwner !== true && conversation?.threadSource === "user"
        && [thread, conversation].every(current => current?.ephemeral === false
          && ["vscode", "cli", "exec", "appServer"].includes(current.source)
          && current.sideConversation !== true && current.parentThreadId == null
          && current.requiresExistingOwner !== true && current.threadStartKind !== "aeon"
          && current.threadSource !== "aeon");
      const prepareAtDispatch = sourceManager.__codexElvesRequestClientManager === true
        && sendNow && !useServerQueue && inputItems.length === 0
        && message.requiresExistingOwner !== true
        && conversation?.threadSource === "user"
        && [thread, conversation].every(current => current?.ephemeral === false
          && ["vscode", "cli", "exec", "appServer"].includes(current.source)
          && current.sideConversation !== true && current.parentThreadId == null
          && current.forkedFromId == null && current.requiresExistingOwner !== true);
      let input = null;
      if (!prepareAtDispatch) {
        const prepared = await retryExecutionConfigLoading(
          () => coordinator.execution.prepare(threadId, message, "start"),
          assertCurrent
        );
        assertCurrent();
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
        input = [...preparedInput, ...inputItems];
      }
      // server queue 展示和执行均先于 local queue；已有本地消息时交给原生 coordinator 保序。
      useServerQueue &&= !hasNativeLocalQueuedMessages(coordinator, serverQueue, threadId);
      if (!useServerQueue) {
        if (typeof coordinator.sendMessage !== "function" || typeof coordinator.loadMessages !== "function")
          throw failure(503, "当前 Codex 版本不支持原生本地消息队列");
        const request = {
          conversationId: threadId, message,
          // 普通主会话由原生本地接受和发送锁承接；queue 在空闲时直接开始，晚到忙碌时入队。
          queueModeOverride: nativeQueueIntent ? "queue"
            : sendNow ? "send-now" : payload.queueOnly === true ? "queue-only" : "queue",
          acceptLocally: nativeQueueIntent || !sendNow,
          // Skill 等额外输入仍随原生重放上下文保存；普通直接发送由实际 phase 生成输入。
          ...(input == null ? {} : { editPosition: {
            serverSubmission: { id: message.id, clientUserMessageId, input },
            preserveSerializedContext: false
          } })
        };
        const sendOnly = typeof coordinator.submission?.getQueueMode === "function"
          && await coordinator.submission.getQueueMode(request) === "send-only";
        assertCurrent();
        const assertAdmission = () => {
          assertCurrent();
          const conversation = sourceManager.getConversation(threadId)
            || (sourceManager.__codexElvesRequestClientManager ? thread : null);
          if (!conversation)
            throw failure(409, "会话状态已变化，暂不能提交排队消息");
          const status = sourceManager.getThreadRuntimeStatusEvidence?.(threadId)
            || conversation.threadRuntimeStatus || thread.status;
          const runtime = status?.status ?? status;
          const statusType = typeof runtime === "string" ? runtime : runtime?.type;
          const queued = coordinator.readMessages(threadId);
          const compacting = runtimeCompaction(threadId, conversation).state !== "idle";
          const active = compacting
            || ["active", "inProgress", "running", "pending"].includes(statusType)
            || nativeConversationTurns(conversation).some(turn => nativeTurnStatus(turn) === "inProgress")
            || array(queued?.messages || queued).length;
          if (sendOnly && active)
            throw failure(409, "此类会话只支持空闲时直接发送，请等待当前回复或压缩完成后重试");
          // fpn 会在最终回调前建立本消息的 active/null-turn；queue 路径由原生 when-idle 判定忙碌。
          if (sendNow && (nativeQueueIntent ? compacting : active))
            throw Object.assign(failure(409, "会话状态已变化，消息需要进入原生队列"), { queueRequired: true });
        };
        if (sendOnly) {
          if (!sendNow)
            throw failure(409, "此类会话不支持排队发送，请等待当前回复完成后重试");
          assertAdmission();
        }
        queueAddStarted = true;
        localQueueStarted = request.acceptLocally;
        // send-only 不经过本地接受阶段，使用原生提交前的请求校验位置保护压缩交界。
        const result = await coordinator.sendMessage(request, async (request, phase) => {
          assertCurrent();
          if (request.conversationId !== threadId || request.message?.id !== clientUserMessageId)
            throw failure(409, "原生队列消息身份已变化，请刷新会话");
          const current = await retryExecutionConfigLoading(
            () => coordinator.execution.prepare(threadId, request.message, phase),
            assertCurrent
          );
          assertCurrent();
          if (current?.status === "paused") throw failure(409, current.reason || "原生队列消息暂时不可用");
          if (current?.status !== "ready" || current.submission?.conversationId !== threadId)
            throw failure(503, "原生队列消息准备失败");
          if (prepareAtDispatch) {
            const { start, steer } = current.submission;
            const restored = steer?.restoreMessage;
            if (!["steer", "start"].includes(phase)
                || steer?.clientUserMessageId !== clientUserMessageId
                || !Array.isArray(steer.input)
                || !sameJsonValue(restored, { ...request.message, context: {
                  ...request.message.context, ...(restored?.context?.turnTrigger === "send_user_message"
                    ? { turnTrigger: "send_user_message" } : {})
                } })
                || phase === "start" && (start?.request?.threadId !== threadId
                  || start.request.clientUserMessageId !== clientUserMessageId
                  || start.context?.restoreMessage !== request.message
                  || !sameQueuedUserInput(start.request.input, steer.input)))
              throw failure(503, "原生准备结果不符合普通发送身份或输入契约");
          }
          return current.submission;
        }, undefined, sendNow ? assertAdmission : undefined, assertAdmission,
        ...(sourceManager.__codexElvesRequestClientManager === true ? [{ deferQueueSnapshotOnSent: true }] : []));
        if (!["queued", "sent"].includes(result?.status)
            || typeof result.messageId !== "string"
            || result.messageId !== clientUserMessageId && result.serverAccepted !== true)
          throw new Error("原生本地队列未确认消息身份");
        if (result.status === "sent") return {
          ok: true, threadId, messageId: result.messageId, clientUserMessageId, result,
          ...(result.queueSyncPending === true ? { queueSyncPending: true, deliveryState: "accepted" } : {})
        };
        if (result.serverAccepted === true)
          queuedSubmission = { id: result.messageId, clientUserMessageId };
        await loadNativeQueue(coordinator, threadId);
        const confirmed = await queueSendState({ threadId, clientUserMessageId });
        if (confirmed.confirmed) return confirmed;
        throw new Error("原生本地队列尚未确认消息已排队或接受");
      }
      queueAddStarted = true;
      const result = await sourceManager.requestClient.sendRequest("thread/queue/add", {
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
      await serverQueue.load(threadId, { afterMutation: true });
      const queued = coordinator.readMessages?.(threadId);
      return {
        ok: true,
        threadId,
        messageId: queuedSubmission.id,
        clientUserMessageId,
        ...queueSnapshotFields(threadId, queued)
      };
    } catch (error) {
      if (error.queueRequired && sendNow)
        return await submitQueuedMessage(payload, false);
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
      if (queueAddStarted && threadId && clientUserMessageId) {
        try {
          const confirmed = await queueSendState({ threadId, clientUserMessageId });
          if (confirmed.confirmed) return confirmed;
        } catch {}
      }
      if (localQueueStarted || queueAddStarted && !error.remoteValidationFailure)
        return {
          ok: false, outcomeUnknown: true, code: 503, threadId, clientUserMessageId,
          message: "原生队列加入结果未确认，请检查会话，不要重复操作"
        };
      return { ok: false, code: error.code || 503, message: error.message || "加入原生队列失败" };
    }
  }
  async function nativeCreationModule() {
    return await import(await nativeResource("initial"));
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
  async function nativeCreationPermissions(module, workspace, workspaceKind, model) {
    const resolvers = Object.values(module).filter(value => typeof value === "function"
      && ["allowPermissionSelection", "sourcePermissionSelection",
        "permissionsConfig", "useAppServerPermissionDefault"]
        .every(marker => Function.prototype.toString.call(value).includes(marker)));
    if (resolvers.length !== 1)
      throw failure(503, "当前 Codex 版本不支持继承桌面权限设置，请在桌面新建会话");
    // 使用桌面的权限解析流程，保留预设、自定义配置及项目和模型的权限限制。
    const permissions = await resolvers[0]({
      scope: appScope, allowPermissionSelection: true, hostId: "local",
      cwd: workspace.cwd, workspaceRoots: workspace.workspaceRoots,
      workspaceKind, model: model ? String(model) : null,
      permissionProfileId: null, sourcePermissionSelection: null, isDelegated: false
    });
    if (!permissions || (!permissions.permissionSelection && !permissions.permissionsConfig
        && permissions.useAppServerPermissionDefault !== true))
      throw failure(503, "未能读取桌面权限设置，请稍后重试");
    return permissions;
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
    const resolvers = Object.values(creationModule).filter(value => {
      if (typeof value !== "function") return false;
      const source = Function.prototype.toString.call(value);
      return source.includes("projectlessOutputDirectory") && source.includes("workspaceRoots")
        && (source.includes("projectless-thread-cwd")
          || ["directoryName:", "outputDirectory:", "workspaceRoot:"].every(marker => source.includes(marker)));
    });
    const resolver = resolvers.length === 1 ? resolvers[0] : null;
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
  async function call(action, payload = {}, sidebarOnly = false) {
    let nativeDispatch = null;
    try {
      validateCompactionQueueSendPayload(action, payload);
      if (action === "dispose") { window.__codexElvesRemoteAccess.dispose(); return { ok: true }; }
      if (["send", "create", "setGoal"].includes(action)) validateGoalComposerPayload(action, payload);
      resetStaleManager();
      if (!manager || !sidebarOnly && !manager.__codexElvesRequestClientManager
          && (!unsubscribe.length || !appScope || !nativeNavigate
            || typeof nativeIntl?.formatMessage !== "function")) {
        try { discover(sidebarOnly); }
        catch (error) {
          if (error?.reason === "native-manager-unavailable" && error?.code === 503
              && error?.nativeOnly !== true && error?.outcomeUnknown !== true)
            nativeDispatch = { status: "not-dispatched", phase: "manager-discovery" };
          throw error;
        }
      }
      if (manager.__codexElvesRequestClientManager && !requestClientReadActions.has(action))
        throw failure(503, "当前 Codex 版本暂未适配此功能，其他远程会话功能仍可使用",
          { nativeOnly: true });
      if (manager.__codexElvesRequestClientManager && payload.threadId
          && ["thread", "history", "send", "stop", "compact", "approve", "answer", "editMessage",
            "setGoal", "updateGoal"].includes(action)) {
        try {
          const requireHistory = action === "editMessage"
            || action === "answer" && payload.method === "remote/requestUserInputAsync";
          await refreshRequestClientConversation(requiredId(payload.threadId),
            { fresh: !nativeReadActions.has(action),
              includeHistory: !["stop", "compact", "approve"].includes(action)
                && (action !== "answer" || requireHistory), requireHistory });
        } catch (error) {
          if (["editMessage", "approve", "answer"].includes(action)) throw error;
          reportNativeCallbackFailure("request_client_conversation", error);
        }
      }
      if (["events", "notifications", "thread"].includes(action))
        await prepareAsyncQuestionState().catch(() => {});
      if (action === "events" || action === "notifications")
        await sideThreadEntries().catch(error => reportSideFeatureDegraded("events.side_threads", error));
      const skipped = await guardThreadAction(action, payload);
      if (skipped) return skipped;
      switch (action) {
        case "optimizePrompt":
        case "promptOptimization":
        case "cancelPromptOptimize": {
          const runtime = window.__codexElvesPromptOptimizeRemote;
          const method = { optimizePrompt: "start", promptOptimization: "status", cancelPromptOptimize: "cancel" }[action];
          if (typeof runtime?.[method] !== "function")
            throw failure(503, "CodexElves 提示词优化尚未就绪，请在桌面端更新增强功能");
          return await runtime[method](payload);
        }
        case "events": {
          for (const id of asyncQuestionSignatures.keys()) syncAsyncQuestions(id, manager.getConversation(id));
          refreshPendingNotifications();
          const reset = payload.epoch !== epoch || payload.cursor < (notifications[0]?.sequence || 1) - 1;
          return { ok: true, epoch, cursor: eventSequence, reset,
            events: notifications.filter(event => event.sequence > (payload.cursor || 0)).flatMap(({ event }) => {
              const { threadId, turnId = event.params?.turn?.id } = event.params || {};
              if (event.params?.item?.type !== "contextCompaction"
                  && !array(event.params?.turn?.items).some(item => item.type === "contextCompaction")) return [event];
              const input = compactionInputItem(threadId, turnId, array(event.params?.turn?.items));
              if (!input) return [event];
              const item = enrichMessageMetadata(threadId, [{ id: turnId, items: [input] }])[0].items[0];
              return [{ method: "item/started", params: { threadId, turnId, item } }, event];
            }) };
        }
        case "notifications": return notificationSnapshot(payload);
        case "sidebar": return await sidebar(payload);
        case "relatedThreads": return await relatedThreads(payload);
        case "createSideThread": return await createSideThread(payload);
        case "closeSideThread": return await closeSideThread(payload);
        case "queue": return await queueSnapshot(payload.threadId);
        case "queueEdit": return await queueEditState(payload);
        case "markRead": return await markThreadRead(payload.threadId);
        case "deleteThread": return await deleteThread(payload);
        case "removeQueued": return await removeQueued(payload);
        case "reorderQueued": return await reorderQueued(payload);
        case "beginQueuedEdit": return await beginQueuedEdit(payload);
        case "editQueued": return await editQueued(payload);
        case "cancelQueuedEdit": return await cancelQueuedEdit(payload);
        case "dismissQueuedEdit": return await dismissQueuedEdit(payload);
        case "steerQueued": return await steerQueued(payload);
        case "editMessage": return await editLastMessage(payload);
        case "updateGoal": return await updateThreadGoal(payload);
        case "setGoal": return await setComposerGoal(payload);
        case "history": return await historyPage(payload);
        case "workspaceContext": {
          if (payload.threadId) {
            const id = requiredId(payload.threadId);
            const current = await readThread(id);
            const cwd = manager.getConversationCwd?.(id) || current.cwd;
            if (typeof cwd !== "string" || !cwd)
              throw failure(404, "当前会话没有可读取的工作目录");
            return { ok: true, hostId: "local", threadId: id, cwd };
          }
          const projectId = requestedProjectId(payload.projectId);
          if (!projectId) throw failure(400, "请先选择会话或本机项目");
          const workspace = await localProjectWorkspace(projectId);
          return { ok: true, hostId: "local", projectId, cwd: workspace.cwd };
        }
        case "thread": {
          const thread = await readThread(payload.threadId);
          const view = await threadViewIdentity(thread);
          const coldConversation = manager.__codexElvesRequestClientManager && !view.readOnly
            && view.viewKind === "main" && thread.ephemeral === false
            ? requestClientConversations.get(thread.id) : null;
          if (coldConversation?.manager === manager && coldConversation.value === null) {
            const sourceManager = manager;
            const goalReadVersion = coldConversation.coldGoalReadVersion =
              (coldConversation.coldGoalReadVersion || 0) + 1;
            const goalRevision = coldConversation.fieldRevisions.get("goalState");
            const current = () => manager === sourceManager && !sourceManager.disposed
              && requestClientConversations.get(thread.id) === coldConversation
              && !coldConversation.disposed && !isDeletedThread(thread.id);
            try {
              // 未物化的持久会话仍有原生目标；只读详情无需为此 resume 或发送消息。
              const response = await sourceManager.requestClient.sendRequest("thread/goal/get", { threadId: thread.id });
              if (!current()) throw failure(409, "原生会话状态已变化，请重新读取");
              if (coldConversation.coldGoalReadVersion === goalReadVersion && coldConversation.value === null
                  && coldConversation.fieldRevisions.get("goalState") === goalRevision) {
                if (!response || !Object.hasOwn(response, "goal")
                    || response.goal !== null && (response.goal?.threadId !== thread.id || !runtimeGoal(response.goal)))
                  throw failure(503, "原生目标读取结果不匹配或不完整");
                coldConversation.coldGoal = response.goal;
              }
            } catch (error) {
              if (!current()) throw failure(409, "原生会话状态已变化，请重新读取");
              reportNativeCallbackFailure("thread_goal_snapshot", error);
            }
          }
          if (!manager.__codexElvesRequestClientManager && !view.readOnly
              && (!manager.getConversation(thread.id)
              || manager.getConversation(thread.id).resumeState === "needs_resume")) {
            await manager.resumeConversation({
              conversationId: thread.id, model: null, reasoningEffort: null,
              workspaceRoots: [thread.cwd], collaborationMode: null
            });
          }
          if (manager.__codexElvesRequestClientManager && !view.readOnly) {
            // 模型和上下文不等待队列首帧；未知队列仍省略字段，由现有订阅补发准确快照。
            try {
              void Promise.resolve(readRequestClientQueue(thread.id))
                .catch(error => reportNativeCallbackFailure("thread_queue_snapshot", error));
            }
            catch (error) {
              reportNativeCallbackFailure("thread_queue_snapshot", error);
            }
          } else if (!view.readOnly) {
            const coordinator = await manager.getTurnCoordinator?.();
            if (typeof coordinator?.loadMessages === "function") await coordinator.loadMessages(thread.id);
          }
          const runtime = runtimeThread(thread.id, thread, view);
          if (coldConversation?.manager === manager && coldConversation.value === null
              && Object.hasOwn(coldConversation, "coldGoal"))
            runtime.goal = runtimeGoal(coldConversation.coldGoal);
          const status = manager.getThreadRuntimeStatusEvidence(thread.id)
            || thread.threadRuntimeStatus || thread.status || runtime.status;
          return { ok: true, thread: { ...summary(thread), ...runtime, status, ...view } };
        }
        case "models": {
          const models = (await nativeModelCatalog()).map(model => ({
            id: model.id, model: model.model, displayName: model.displayName,
            supportedReasoningEfforts: array(model.supportedReasoningEfforts),
            defaultReasoningEffort: model.defaultReasoningEffort
          }));
          return { ok: true, data: models,
            ultraReasoningEnabled: models.some(model => model.supportedReasoningEfforts.some(effort => effort.reasoningEffort === "ultra")),
            ...await defaultModelSettings() };
        }
        case "skills": return await skillCatalog(payload, null, { forceReload: payload.forceReload === true });
        case "sendState": return await queueSendState(payload);
        case "send": {
          const id = requiredId(payload.threadId);
          const loaded = await readThread(id);
          const prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
          const imageAttachments = remoteImageAttachments(payload, "send-validation");
          if (!prompt && !imageAttachments.length) throw failure(400, "消息不能为空");
          const conversation = manager.getConversation(id);
          const status = manager.getThreadRuntimeStatusEvidence?.(id)
            || conversation?.threadRuntimeStatus || loaded.status;
          const runtime = status?.status ?? status;
          const statusType = typeof runtime === "string" ? runtime : runtime?.type;
          const queued = manager.getTurnCoordinator?.()?.readMessages?.(id);
          if (payload.queueOnly === true || runtimeCompaction(id, conversation).state !== "idle"
              || ["active", "inProgress", "running", "pending"].includes(statusType)
              || nativeConversationTurns(conversation).some(turn => nativeTurnStatus(turn) === "inProgress")
              || array(queued?.messages || queued).length)
            return await queueMessage({ ...payload, threadId: id });
          // 普通空闲发送也经过同一准备和身份保护；由原生在异步准备后决定立即发送或入队。
          return await queueMessage({ ...payload, threadId: id }, true);
        }
        case "create": {
          let startConversationSubmitted = false;
          let materialized = null, sourceManager = manager;
          const goalMode = payload.goalMode === true;
          const submission = goalMode ? goalSubmission("create") : null;
          try {
            const projectId = requestedProjectId(payload.projectId);
            const materialize = goalMode ? await nativeGoalMaterializer() : null;
            const goalSettings = goalMode ? await goalThreadSettings(payload) : null;
            if (!goalMode) await validateReasoningEffort(payload.model, payload.reasoningEffort);
            const imageAttachments = remoteImageAttachments(payload, "create");
            const prompt = typeof payload.prompt === "string" ? payload.prompt.trim() : "";
            if (!prompt && !imageAttachments.length) throw failure(400, "消息不能为空");
            const module = await nativeCreationModule();
            const provider = nativeCreationProvider(module);
            const workspace = projectId == null
              ? await nativeProjectlessWorkspace(prompt, module)
              : await localProjectWorkspace(projectId);
            const permissions = await nativeCreationPermissions(module, workspace,
              projectId == null ? "projectless" : "project", goalSettings?.model ?? payload.model);
            const skills = await skillInputs(payload, workspace.cwd);
            if (goalMode) {
              materialized = await materialize({
                scope: appScope, hostId: "local",
                assertRequestCurrent: () => {
                  if (manager !== sourceManager || sourceManager.disposed)
                    throw failure(409, "原生会话连接已变化，请重新提交目标");
                },
                draft: { objective: prompt, imageAttachments, pastedTextAttachments: [] }
              });
              submission.phase = "firstTurn";
            }
            const submittedPrompt = goalMode ? `/goal ${materialized.objective}` : prompt;
            startConversationSubmitted = true;
            const result = await manager.startConversation({
              input: [
                ...(submittedPrompt ? [{ type: "text", text: submittedPrompt, text_elements: [] }] : []),
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
              ...permissions, mode: "default", threadStartKind: "default",
              ...(goalSettings || payload.model ? {
                collaborationMode: {
                  mode: "default",
                  settings: { model: goalSettings?.model ?? String(payload.model),
                    reasoning_effort: goalSettings ? goalSettings.effort : payload.reasoningEffort || null,
                    developer_instructions: null }
                }
              } : {})
            }, { readThreadCreationInputs: (params, mode) => provider.readCreationInputs(params, mode) });
            if (goalMode) {
              submission.threadId = typeof result.conversationId === "string" ? result.conversationId : null;
              submission.firstTurnAccepted = result.status === "created" && result.firstTurn?.status === "accepted";
              if (!submission.firstTurnAccepted) {
                const unknown = result.status === "outcome-unknown" || result.firstTurn?.status === "outcome-unknown";
                return {
                  ...result, ok: false, threadId: submission.threadId, outcomeUnknown: unknown,
                  code: 409,
                  message: result.firstTurn?.message || "首条目标消息尚未确认送达，请查看已创建会话，不要重复新建",
                  goalSubmission: { ...submission }
                };
              }
              const id = requiredId(submission.threadId);
              submission.phase = "goalSet";
              if (goalSubmissions.has(id)) return goalSubmissionFailure(submission,
                failure(409, "已创建会话的另一个目标提交正在处理，请查看原请求回执"));
              goalSubmissions.set(id, submission);
              try {
                const goal = runtimeGoal(await sourceManager.setThreadGoal(id, {
                  objective: materialized.objective, appendTranscriptItem: false
                }));
                if (!goal || goal.objective !== materialized.objective || goal.status !== "active")
                  throw new Error("原生目标创建结果未确认");
                submission.goalSet = "succeeded"; submission.phase = "complete";
                return { ...result, ok: true, threadId: id, goal, goalSubmission: { ...submission } };
              } catch (error) {
                const unknown = !definiteGoalRejection(error);
                submission.goalSet = unknown ? "unknown" : "failed";
                return { ...result, ...goalSubmissionFailure(submission, error, unknown) };
              } finally {
                goalSubmissions.delete(id);
              }
            }
            const accepted = result.status === "created" && ["accepted", "not-requested"].includes(result.firstTurn?.status);
            return {
              ...result, ok: accepted, threadId: result.conversationId,
              outcomeUnknown: result.status === "outcome-unknown" || result.firstTurn?.status === "outcome-unknown",
              ...(!accepted ? { code: 409, message: result.firstTurn?.message || `会话 ${result.conversationId || ""} 的首条消息未确认，请先查看会话，不要重复新建` } : {})
            };
          } catch (error) {
            if (goalMode) {
              if (!startConversationSubmitted) await cleanupUnsubmittedGoal(sourceManager, materialized);
              return goalSubmissionFailure(submission, error, startConversationSubmitted);
            }
            if (!startConversationSubmitted && !error?.remoteValidationFailure)
              throw failure(error?.code || 503, error?.message || "原生创建输入准备失败");
            throw error;
          }
        }
        case "stop": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          await manager.stopThread(id, "user");
          try {
            await manager.cleanBackgroundTerminals(id);
            const terminals = await manager.requestClient.sendRequest("thread/backgroundTerminals/list", { threadId: id });
            return { ok: true, threadId: id, backgroundTerminals: terminals };
          } catch (error) {
            reportNativeCallbackFailure("stop_background_terminals", error);
            return { ok: true, threadId: id, cleanupPending: true,
              message: "会话已停止，后台终端清理尚未确认" };
          }
        }
        case "compact": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          if (runtimeCompaction(id).state !== "idle") throw failure(409, "此会话正在压缩上下文，请等待完成后再试");
          await manager.compactThread(id);
          if (manager.__codexElvesRequestClientManager) {
            await refreshRequestClientConversation(id, { fresh: true })
              .catch(error => reportNativeCallbackFailure("compact_state_refresh", error));
            const compaction = runtimeCompaction(id);
            return { ok: true, threadId: id, compaction: compaction.state !== "idle" ? compaction
              : { state: "pending", turnId: null, itemId: "pending-manual-context-compaction" } };
          }
          return { ok: true, threadId: id, compaction: runtimeCompaction(id) };
        }
        case "pinProject": {
          const id = requiredId(payload.projectId);
          if (!(await state("local-projects"))?.[id]) throw failure(404, "项目不存在");
          await manager.storage.updateGlobalState("pinned-project-ids", current => {
            const pins = array(current).filter(value => value !== id);
            return payload.pinned === true ? [...pins, id] : pins;
          });
          if (manager.__codexElvesRequestClientManager
              && array(await requestClientGlobalState("pinned-project-ids")).includes(id) !== (payload.pinned === true))
            throw failure(409, "原生项目置顶状态尚未确认，请刷新后核对");
          emit({ method: "remote/sidebarChanged" });
          return { ok: true };
        }
        case "pinThread": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          const pins = await manager.runtime.pinnedThreads();
          if (!pins) throw failure(503, "原生置顶服务不可用");
          await pins.set({ hostId: "local", useAppServerPins: true, threadId: id, pinned: payload.pinned === true });
          const current = await pins.list({ hostId: "local", useAppServerPins: true });
          if (array(current.threadIds).includes(id) !== (payload.pinned === true))
            throw failure(409, "原生置顶状态尚未确认，请刷新后核对");
          await manager.setPinnedThreadIds(array(current.threadIds));
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
        case "rename": {
          const id = requiredId(payload.threadId), sourceManager = manager;
          const initialEntry = nativeSideThreads.get(id);
          const initialTitle = String(readNativeSideTab(initialEntry, id)?.title || initialEntry?.title || "").trim();
          const thread = await readThread(id);
          if (!payload.title?.trim() || payload.title.length > 200) throw failure(400, "标题需为 1–200 个字符");
          if (manager !== sourceManager || sourceManager.disposed)
            throw failure(409, "原生会话连接已变化，请核对名称后再操作");
          // 使旧自动命名的完成回调失效；原生 expectedTitle 同时保护实际持久化顺序。
          const write = Promise.resolve();
          sideThreadTitleWrites.set(id, write);
          const current = () => manager === sourceManager && !sourceManager.disposed
            && sideThreadTitleWrites.get(id) === write;
          const identity = sourceManager.getConversation(id) || thread;
          if (isEphemeralSideThread(identity, id)) {
            const entry = initialEntry || nativeSideThreads.get(id);
            const expectedTitle = initialEntry ? initialTitle
              : String(readNativeSideTab(entry, id)?.title || entry?.title || "").trim();
            const applied = await setEphemeralSideThreadTitle(id, payload.title.trim(), identity, entry, expectedTitle, current);
            if (!applied) throw failure(409, "原生侧边名称已变化，请刷新后核对");
            return { ok: true };
          }
          const applied = await sourceManager.setThreadTitle(id, payload.title.trim(), { requireAcknowledgement: true });
          if (manager !== sourceManager || sourceManager.disposed || sideThreadTitleWrites.get(id) !== write)
            throw failure(409, "原生会话名称已变化，请刷新后核对");
          if (applied === false) throw failure(409, "原生会话名称未应用，请刷新后核对");
          const conversation = sourceManager.__codexElvesRequestClientManager
            ? await refreshRequestClientConversation(id, { fresh: true, includeHistory: false })
            : sourceManager.getConversation(id);
          if (manager !== sourceManager || sourceManager.disposed || sideThreadTitleWrites.get(id) !== write)
            throw failure(409, "原生会话名称已变化，请刷新后核对");
          if (conversation) syncSidebarConversationState(id, conversation);
          return { ok: true };
        }
        case "approve":
        case "answer": {
          if (action === "answer" && payload.method === "remote/requestUserInputAsync")
            return await answerAsyncQuestion(payload);
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
          return { ok: true, ...blockingRequestsSnapshot(id) };
        }
        default: throw failure(404, "不支持的原生会话操作");
      }
    } catch (error) {
      reportNativeCompatibilityDiagnostic(action, error);
      if (action === "setGoal" || action === "create" && payload?.goalMode === true) {
        return goalSubmissionFailure(goalSubmission(action,
          typeof payload?.threadId === "string" ? payload.threadId : null), error);
      }
      if (["send", "create", "editMessage"].includes(action) && !error.remoteValidationFailure) {
        return { ok: false, outcomeUnknown: true, code: 503,
          message: "原生操作未确认完成，请检查会话，不要重复发送或新建" };
      }
      return {
        ok: false,
        code: error.code || 503,
        message: error.message || "原生会话操作失败",
        ...(nativeDispatch ? { nativeDispatch } : {}),
        ...(error.nativeOnly === true ? { nativeOnly: true } : {}),
        ...(error.reason ? { reason: error.reason } : {}),
        ...(error.threadId ? { threadId: error.threadId } : {}),
        ...(error.parentThreadId ? { parentThreadId: error.parentThreadId } : {})
      };
    }
  }
  window.__codexElvesRemoteAccess = {
    version: 128, call,
    async recentSidebar(payload = {}) {
      try {
        resetStaleManager();
        if (!manager || !unsubscribe.length && !sidebarUnsubscribe.length
            || !payload.cachedOnly && (!appScope || !nativeNavigate)) discover(true, payload.cachedOnly === true);
        return await sidebar({ recentOnly: true, cachedOnly: payload.cachedOnly === true });
      } catch (error) {
        return { ok: false, code: error.code || 503, message: error.message || "原生会话列表读取失败" };
      }
    },
    subscribeSidebar(listener) {
      sidebarListeners.add(listener);
      return () => sidebarListeners.delete(listener);
    },
    recentAction(action, payload) {
      if (!["pinThread", "deleteThread"].includes(action))
        return Promise.resolve({ ok: false, code: 400, message: "不支持的会话列表操作" });
      return call(action, payload, true);
    },
    releaseSidebar() {
      if (sidebarListeners.size || unsubscribe.length) return;
      for (const dispose of sidebarUnsubscribe) { try { dispose?.(); } catch {} }
      sidebarUnsubscribe = [];
      nativeCatalog.clear(); sidebarSignatures.clear(); catalogLoaded = false;
      catalogLoading = null; catalogGeneration++;
    },
    async openThread(threadId) {
      try {
        resetStaleManager();
        if (!manager || !nativeNavigate) discover(true);
        const id = requiredId(threadId);
        if (isDeletedThread(id)) throw failure(404, "会话已永久删除");
        if (!nativeNavigate) throw failure(503, "原生导航尚未就绪，请重试");
        await nativeNavigate(`/local/${encodeURIComponent(id)}`);
        return { ok: true, threadId: id };
      } catch (error) {
        return { ok: false, code: error.code || 503, message: error.message || "无法打开此会话" };
      }
    },
    removeQueued: payload => call("removeQueued", payload),
    reorderQueued: payload => call("reorderQueued", payload),
    beginQueuedEdit: payload => call("beginQueuedEdit", payload),
    editQueued: payload => call("editQueued", payload),
    cancelQueuedEdit: payload => call("cancelQueuedEdit", payload),
    queueMessage: (payload, allowImmediate = false) => call("send", { ...payload, queueOnly: !allowImmediate }),
    dispose() {
      requestClientEventSubscription?.dispose();
      requestClientEventSubscription = null;
      clearRequestClientSubagentStates();
      for (const record of requestClientQueues.values()) disposeRequestClientQueue(record);
      requestClientQueues.clear();
      for (const record of requestClientConversations.values()) disposeRequestClientConversation(record);
      requestClientConversations.clear();
      window.__codexElvesRemoteImageErrorDiagnostics?.dispose?.();
      for (const dispose of [...unsubscribe, ...sidebarUnsubscribe]) { try { dispose?.(); } catch {} }
      unsubscribe = []; sidebarUnsubscribe = []; sidebarListeners.clear();
      for (const watcher of sideScopeWatchers.values()) watcher.dispose?.();
      sideScopeWatchers.clear(); pendingThreadMetadataReads.clear();
      sideThreadTitleWrites.clear();
      if (window.__codexElvesRemoteAccess?.epoch === epoch) delete window.__codexElvesRemoteAccess;
    },
    epoch
  };
})();
