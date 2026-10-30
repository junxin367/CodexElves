(() => {
  if (window.__codexElvesRemoteAccess?.version === 82) return;
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
  let relatedNativeState = null;
  let nativeSideBindings = null;
  let sideThreadsManager = null;
  const nativeSideThreads = new Map();
  const sideThreadTitleWrites = new Map();
  const sideScopeWatchers = new Map();
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
  function minimalManagerStorage() {
    return {
      async readGlobalState() { return {}; },
      readValue() { return null; },
      writeValue() { return false; },
      readQueuedFollowUps() { return null; }
    };
  }
  function requestClientManager(requestClient) {
    const storage = minimalManagerStorage();
    return {
      __codexElvesRequestClientManager: true,
      hostId: "local",
      requestClient,
      storage,
      runtime: null,
      disposed: false,
      getConversation() { return null; },
      getCachedConversations() { return []; },
      getRecentThreadSummaries() { return []; },
      getThreadListSummary() { return null; },
      getThreadRuntimeStatusEvidence() { return null; },
      getConversationCwd() { return null; },
      async readThread(threadId, options = {}) {
        return await requestClient.sendRequest("thread/read", {
          threadId,
          includeTurns: options.includeTurns === true
        });
      }
    };
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
      if (!manager) throw failure(503, "未发现本机原生会话管理器，请检查 CodexElves 注入连接");
      if (!manager.__codexElvesRequestClientManager)
        window.__codexElvesNativeConversationContext = { manager, appScope, nativeIntl, nativeNavigate };
    }
    if (sideThreadsManager !== manager) {
      for (const watcher of sideScopeWatchers.values()) watcher.dispose?.();
      sideScopeWatchers.clear();
      sideThreadsManager = manager;
      nativeSideThreads.clear();
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
      const asyncRequests = nativeConversationTurns(conversation).flatMap(turn => {
        const turnId = nativeTurnId(turn);
        if (!turnId || nativeTurnStatus(turn) !== "inProgress") return [];
        const answered = acceptedAsyncQuestionReplies(turn);
        return array(turn.items).flatMap(item => item?.type === "agentMessage"
          && array(item.questions).some((question, index) => typeof question?.title === "string"
            && !answered.has(JSON.stringify(["request_user_input_async", item.id, index]))
            && !nativeAsyncQuestionSkipped(id, turn, item, question, index, conversation))
          ? [{ id: JSON.stringify(["async-question", turnId, item.id]),
            method: "remote/requestUserInputAsync", params: { turnId } }] : []);
      });
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
    return !title || title === "side chat" || title === DEFAULT_SIDE_THREAD_TITLE || isGenericMainThreadTitle(title);
  }
  function sideThreadTitle(conversation, fallback = "") {
    if (conversation?.sideConversation !== true || subagentIdentity(conversation)) return null;
    const candidate = [conversation?.title, conversation?.name, fallback]
      .map(cleanThreadTitle)
      .find(value => value && !isDefaultSideThreadTitle(value));
    return candidate || DEFAULT_SIDE_THREAD_TITLE;
  }
  function syncNativeSideThreadTitle(id, conversation, fallback = "") {
    const entry = nativeSideThreads.get(id);
    if (!entry) return false;
    const title = sideThreadTitle(conversation, fallback || entry.title);
    if (!title || title === entry.title) return false;
    nativeSideThreads.set(id, { ...entry, title });
    return true;
  }
  function firstSideThreadMessageTitle(conversation) {
    for (const turn of nativeConversationTurns(conversation)) {
      for (const source of array(turn?.items)) {
        const item = nativeDisplayMessageItem(source);
        if (item?.type !== "userMessage" || item.remotePendingInput || item.remoteCompactionInput) continue;
        const text = array(item.content).filter(part => part?.type === "text" && typeof part.text === "string")
          .map(part => part.text).join(" ").replace(/\s+/g, " ").trim();
        if (text) return Array.from(text).slice(0, 10).join("");
      }
    }
    return "";
  }
  function maybeNameSideThreadFromFirstMessage(id, conversation) {
    if (conversation?.sideConversation !== true || subagentIdentity(conversation) || sideThreadTitleWrites.has(id)) return;
    const entry = nativeSideThreads.get(id);
    if (!isDefaultSideThreadTitle(sideThreadTitle(conversation, entry?.title))) return;
    const title = firstSideThreadMessageTitle(conversation);
    if (!title) return;
    const sourceManager = manager;
    const write = (async () => {
      await sourceManager.setThreadTitle(id, title);
      if (manager !== sourceManager || sourceManager.disposed) return;
      const currentEntry = nativeSideThreads.get(id);
      if (currentEntry) nativeSideThreads.set(id, { ...currentEntry, title });
      const parentThreadId = conversation.forkedFromId || currentEntry?.parentThreadId;
      if (parentThreadId) emit({ method: "remote/relatedThreadsChanged", params: { threadId: parentThreadId } });
      syncSidebarConversationState(id, sourceManager.getConversation(id) || conversation);
    })().catch(error => {
      reportSideFeatureDegraded("side_thread_first_message_title", error, { threadId: id });
    }).finally(() => {
      if (sideThreadTitleWrites.get(id) === write) sideThreadTitleWrites.delete(id);
    });
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
    const side = conversation?.sideConversation === true || thread?.sideConversation === true;
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
    const side = conversation.sideConversation === true || nativeSideThreads.has(id);
    if (conversation.sideConversation === true) maybeNameSideThreadFromFirstMessage(id, conversation);
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
      const conversation = manager.getConversation(entry.threadId);
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
      sideEntries = recentOnly ? recentSideThreadEntries() : await sideThreadEntries();
    } catch (error) {
      reportSideFeatureDegraded("sidebar.side_threads", error);
    }
    const sideIds = new Set(sideEntries.map(entry => entry.threadId));
    for (const id of sideIds) nativeCatalog.delete(id);
    const keys = recentOnly ? ["local-projects", "thread-project-assignments", "projectless-thread-ids"]
      : ["local-projects", "pinned-project-ids", "project-order", "pinned-thread-ids",
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
      if (!pinnedState || typeof pinnedState !== "object")
        throw failure(503, "原生置顶服务返回结果无效");
    } catch (error) {
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
    if (thread.sideConversation === true || manager.getConversation(id)?.sideConversation === true) {
      const side = nativeSideIdentity(thread, await sideThreadEntries());
      if (!side) throw failure(404, "原生临时侧边会话已关闭或不存在");
    }
    return thread;
  }
  async function readThreadMetadata(id) {
    const sourceManager = manager;
    const createdAt = timestamp(sourceManager.getConversation(id)?.createdAt);
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
          if (delay == null || !initialThreadMetadataError(error, id) || !active()
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
    "relatedThreads"
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
        expression: /^app:\/\/-\/assets\/local-conversation-side-chat-tab-[A-Za-z0-9_-]+\.js$/,
        matches: source => source.includes("LocalConversationSideChatTab"),
      },
    };
    const rule = rules[name];
    if (!rule) throw failure(503, "原生关联会话模块名称无效");
    const observed = [...document.querySelectorAll('link[rel="modulepreload"]')].map(link => link.href)
      .concat(performance.getEntriesByType("resource").map(entry => entry.name));
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
          const response = await fetch(resource);
          if (!response.ok) continue;
          if (rule.matches(await response.text())) matches.push(resource);
        } catch {}
      }
      if (matches.length === 1) return matches[0];
      if (matches.length > 1) throw failure(503, `原生关联会话模块版本不唯一（${name}）`);
    }
    // 静态依赖不一定出现在 DOM 或当前 Resource Timing 中；只沿已观察到的可信 app-main 导入定位。
    if (name === "initial") {
      const mains = [...new Set(observed.filter(value =>
        /^app:\/\/-\/assets\/app-main-[A-Za-z0-9_-]+\.js$/.test(value)))];
      if (mains.length === 1) {
        const response = await fetch(mains[0]);
        if (!response.ok) throw failure(503, "无法读取原生入口模块");
        const source = await response.text();
        const dependencies = [...new Set([...source.matchAll(
          /\b(?:from|import)\s*["'](\.\/app-initial-[A-Za-z0-9_-]+\.js)["']/g)]
          .map(match => new URL(match[1], mains[0]).href)
          .filter(value => rule.expression.test(value)))];
        if (dependencies.length === 1) return dependencies[0];
      }
    }
    throw failure(503, `当前 Codex 版本的原生关联会话模块不可用（${name}）`);
  }
  async function nativeModuleSnapshot(name) {
    const resource = await nativeResource(name);
    if (!nativeModuleSnapshots.has(resource)) {
      const pending = (async () => {
        const [module, response] = await Promise.all([import(resource), fetch(resource)]);
        if (!response.ok) throw failure(503, "无法读取原生关联会话接口");
        return { module, source: await response.text(), resource };
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
        return { rowsFamily, sortRows, formatAgentPath };
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
          if (tab.tabType?.kind !== "local-side-chat" || !conversation?.sideConversation
              || conversation.forkedFromId !== parentThreadId || subagentIdentity(conversation)) continue;
          const previous = nativeSideThreads.get(id);
          const title = sideThreadTitle(conversation, previous?.title || tab.title);
          next.set(id, { threadId: id, parentThreadId, title,
            createdAtMs: timestamp(conversation.createdAt), closed: false, scope, controller, tab });
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
    return [...next.values()];
  }
  async function sideThreadEntries() {
    return syncNativeSideThreads(await nativeSideState());
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
    if (manager.__codexElvesRequestClientManager) {
      return { readOnly: false, viewKind: "main", parentThreadId: null, nativeBridgeUnavailable: true };
    }
    const identity = subagentIdentity(thread) || subagentIdentity(manager.getConversation(thread.id));
    if (identity) {
      const native = await nativeRelatedState();
      const spawn = (thread.source?.subAgent ?? thread.source?.subagent)?.thread_spawn;
      const pathName = typeof spawn?.agent_path === "string" ? native.formatAgentPath(spawn.agent_path) : null;
      return { ...identity, agentName: pathName || identity.agentName || "子代理",
        ephemeral: nativeThreadEphemeral(thread) };
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
  async function relatedThreads(payload) {
    const threadId = requiredId(payload.threadId);
    const parent = await readThread(threadId);
    if (manager.__codexElvesRequestClientManager) {
      return {
        ok: true, threadId, subagents: [], completedSubagents: [],
        completedSubagentCount: 0, subagentsComplete: true, sideThreads: [],
        canCreateSideThread: false
      };
    }
    const native = await nativeRelatedState();
    if (typeof manager.discoverSubagentDescendantSnapshot !== "function")
      throw failure(503, "当前 Codex 版本不支持原生子代理关系读取");
    const snapshot = await manager.discoverSubagentDescendantSnapshot(threadId);
    if (!Array.isArray(snapshot?.descendantThreads))
      throw failure(503, "原生子代理关系读取结果无效");
    if (!manager.getConversation(threadId)) {
      if (typeof manager.hydrateBackgroundThreads !== "function")
        throw failure(503, "当前 Codex 版本不支持只读加载关联会话");
      await manager.hydrateBackgroundThreads([threadId], { includeTurns: true });
    }
    const rows = appScope.get(native.rowsFamily, { conversationId: threadId, hostId: "local" });
    if (!Array.isArray(rows)) throw failure(503, "原生子代理列表尚未就绪");
    const threads = new Map(snapshot.descendantThreads.map(thread => [thread.id, thread]));
    const belongsToParent = id => {
      if (isDeletedThread(id) || !threads.has(id)) return false;
      const seen = new Set([id]);
      for (let current = id; current;) {
        const thread = threads.get(current) || manager.getConversation(current);
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
    const now = Date.now();
    const toSubagent = row => {
      const startedAtMs = Number.isFinite(row.startedAtMs) ? row.startedAtMs : null;
      return { id: row.conversationId, name: row.displayName || "子智能体", status: row.status, startedAtMs,
        ephemeral: nativeThreadEphemeral(threads.get(row.conversationId) || { id: row.conversationId }),
        ...(row.status === "done" || startedAtMs == null ? {} : { elapsedMs: Math.max(0, now - startedAtMs) }) };
    };
    const subagents = confirmed.filter(row => row.status !== "done").map(toSubagent);
    const completedSubagents = confirmed.filter(row => row.status === "done").map(toSubagent);
    const entries = await sideThreadEntries();
    const sideThreads = [];
    for (const entry of entries.filter(entry => entry.parentThreadId === threadId)) {
      const thread = manager.getConversation(entry.threadId);
      if (!thread) continue;
      nativeSideIdentity(thread, entries);
      const item = summary(thread);
      sideThreads.push({ id: thread.id, title: entry.title, createdAtMs: entry.createdAtMs,
        running: item.priority.active && item.priority.attentionState !== "waiting",
        hasUnreadTurn: completedSideThreadUnread(thread, item) });
    }
    const sideNative = await nativeSideState();
    const sideScope = nativeSideScopes(sideNative).find(scope => scope.value.conversationId === threadId);
    return { ok: true, threadId, subagents, completedSubagents,
      completedSubagentCount: completedSubagents.length,
      subagentsComplete: snapshot.isComplete === true, sideThreads,
      canCreateSideThread: nativeSideFeatureEnabled(sideNative, sideScope)
        && typeof nativeIntl?.formatMessage === "function"
        && !subagentIdentity(parent) && parent.ephemeral !== true
        && !entries.some(entry => entry.threadId === threadId) };
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
      const entries = await sideThreadEntries(), thread = sourceManager.getConversation(threadId);
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
      while (sourceManager.getConversation(threadId) && Date.now() < deadline) {
        if (sourceManager.disposed || manager !== sourceManager) throw new Error("原生连接已变化");
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      if (sourceManager.disposed || manager !== sourceManager) throw new Error("原生连接已变化");
      if (sourceManager.getConversation(threadId)) throw new Error("原生侧边会话尚未关闭");
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
  }
  async function deleteThread(payload) {
    if (!payload || typeof payload !== "object" || Array.isArray(payload)
        || Object.keys(payload).length !== 1 || !Object.hasOwn(payload, "threadId")
        || typeof payload.threadId !== "string"
        || !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(payload.threadId))
      throw failure(400, "永久删除需要已创建完成的本机会话 ID");
    const id = payload.threadId;
    if (isDeletedThread(id)) return { ok: true, threadId: id, deleted: true, status: "local_deleted" };
    const remove = window.__codexElvesNativeDeleteThread;
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
  function nativeSideHistoryPage(payload, readOnly) {
    const threadId = requiredId(payload.threadId), conversation = manager.getConversation(threadId);
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
    if (nativeThreadEphemeral(nativeThread) === true) return nativeSideHistoryPage(payload, readOnly);
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
    catalogLoaded = false;
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
    const conversation = manager.getConversation(id);
    const threadSettings = await requestedThreadModelSettings(payload, loaded, conversation);
    if (threadSettings) {
      try {
        const current = editableMessageState(id);
        if (!current.metadata || current.metadata.turnId !== payload.turnId || current.metadata.messageId !== payload.messageId)
          throw failure(409, current.reason || "消息已变化，请刷新会话后重试");
        await applyThreadModelSettings(id, threadSettings);
        const updated = editableMessageState(id);
        if (!updated.metadata || updated.metadata.turnId !== payload.turnId || updated.metadata.messageId !== payload.messageId)
          throw failure(409, updated.reason || "消息已变化，请刷新会话后重试");
      } catch (error) {
        throw failure(error.code || 503, error.message || "模型或思考深度切换失败，请稍后重试");
      }
    }
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
  function acceptedAsyncQuestionReplies(turn) {
    const originals = new Map();
    for (const item of array(turn?.items)) {
      if (item?.type !== "agentMessage" || typeof item.id !== "string" || !item.id) continue;
      array(item.questions).forEach((question, index) => {
        if (typeof question?.title === "string")
          originals.set(JSON.stringify(["request_user_input_async", item.id, index]), question.title);
      });
    }
    const answered = new Map();
    for (const item of array(turn?.items)) {
      if (item?.type !== "userMessage" && (item?.type !== "steeringUserMessage" || item.status !== "accepted")) continue;
      const input = item.type === "userMessage" ? item.content : item.input;
      if (!Array.isArray(input) || input.length !== 1 || input[0]?.type !== "text") continue;
      const text = String(input[0].text || "").trim();
      const open = "<send_user_message_question_reply>", close = "</send_user_message_question_reply>";
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
    const turn = nativeConversationTurns(conversation).findLast(turn => nativeTurnStatus(turn) === "inProgress");
    const turnId = nativeTurnId(turn);
    if (!turnId) return [];
    const answered = acceptedAsyncQuestionReplies(turn);
    return array(turn.items).flatMap(item => {
      if (item.type !== "agentMessage") return [];
      const questions = array(item.questions).flatMap((question, index) => {
        const questionId = JSON.stringify(["request_user_input_async", item.id, index]);
        return typeof question?.title === "string" && !answered.has(questionId)
          && !nativeAsyncQuestionSkipped(id, turn, item, question, index, conversation)
          ? [{ id: questionId, question: question.title,
            options: array(question.options).filter(option => typeof option === "string").map(label => ({ label })) }]
          : [];
      });
      if (!questions.length) return [];
      const requestId = JSON.stringify(["async-question", turnId, item.id]);
      return [{ id: requestId, method: "remote/requestUserInputAsync",
        params: { threadId: id, turnId, questions,
          submissionState: questionReplyStore.get(JSON.stringify([id, requestId]))?.state ?? null } }];
    });
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
    const setPanel = unique(({ value, source }) => value.length === 3 && source.length < 600
      && [".hostId", ".threadId", "new Map(", ".delete(", ".get(", ".set("]
        .every(marker => source.includes(marker)));
    const skipQuestion = unique(({ source }) => source.length < 900
      && source.includes("isSkipped:!0") && source.includes(".notifications")
      && source.includes(".hide("));
    const key = { hostId: "__remote_question_probe__", threadId: "__remote_question_probe__",
      entityKey: "__remote_question_probe__", itemId: "__remote_question_probe__" };
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
      nativeTurnStatus(turn) === "inProgress"
        && array(turn.items).some(item => item.type === "agentMessage" && array(item.questions).length)))) return;
    nativeAsyncQuestionState(await nativeCreationModule());
  }
  function nativeAsyncQuestionEntry(id, turn, item, question, index, conversation = manager.getConversation(id)) {
    const native = asyncQuestionNativeBindings;
    if (!native || !appScope || typeof question?.title !== "string") return null;
    const itemId = JSON.stringify(["request_user_input_async", item.id, index]), turnId = nativeTurnId(turn);
    const entityKeys = Object.entries(conversation?.turnHistory?.history?.entitiesByKey || {})
      .filter(([, entity]) => nativeTurnId(entity) === turnId
        && array(entity.items).some(value => value.type === "agentMessage" && value.id === item.id))
      .map(([key]) => key);
    const panel = appScope.get(native.panelFamily, { hostId: "local", threadId: id });
    const selected = panel?.selectedQuestionKey;
    if (selected?.hostId === "local" && selected.threadId === id
        && array(panel.questionIds).includes(itemId) && typeof selected.entityKey === "string")
      entityKeys.push(selected.entityKey);
    const entries = [...new Set(entityKeys)].map(entityKey => {
      const key = { hostId: "local", threadId: id, entityKey, itemId };
      const state = appScope.get(native.questionFamily, key);
      return { key, state };
    }).filter(({ state }) => state == null || state.turnId === turnId && state.title === question.title);
    if (entries.length !== 1) return null;
    return entries[0];
  }
  function nativeAsyncQuestionSkipped(id, turn, item, question, index, conversation) {
    return nativeAsyncQuestionEntry(id, turn, item, question, index, conversation)?.state?.isSkipped === true;
  }
  async function skipAsyncQuestion(id, request) {
    if (!appScope || typeof appScope.set !== "function") throw failure(503, "Codex 原生问答状态上下文尚未就绪");
    const sourceManager = manager, scope = appScope;
    const native = nativeAsyncQuestionState(await nativeCreationModule());
    if (manager !== sourceManager || appScope !== scope || sourceManager.disposed)
      throw failure(409, "Codex 界面上下文已变化，请重新读取问题");
    const key = JSON.stringify([id, request.id]);
    if (questionReplyStore.has(key)) throw failure(409, "此问题已提交或结果待确认，请先核对状态");
    const current = asyncQuestionRequests(id).find(value => value.id === request.id);
    if (!current || current.params.turnId !== request.params.turnId
        || JSON.stringify(current.params.questions.map(question => question.id))
          !== JSON.stringify(request.params.questions.map(question => question.id)))
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
      return { ...entry, title: question.question };
    });
    let changed = false;
    try {
      for (const target of targets) {
        changed = true;
        if (!target.state) scope.set(native.questionFamily, target.key, {
          turnId: request.params.turnId, title: target.title, draft: "", draftBaseline: "",
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
        return state ? [[reply.questionItemId, {
          turnId: state.turnId, lastSubmission: state.lastSubmission, draftBaseline: state.draftBaseline
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
      // 远程路径不接管桌面另一笔正在提交的状态。
      if (!state || state.turnId !== turnId || state.title !== reply.question
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
      return state?.turnId !== turnId || state.title !== reply.question || state.isSubmitting !== false
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
    for (const [key, record] of questionReplyStore) {
      if (!["submitted", "unknown"].includes(record?.state) || record.desktopSynchronized) continue;
      try {
        const [threadId, requestId] = JSON.parse(key);
        const [kind, turnId, itemId] = JSON.parse(requestId);
        if (threadId !== id || kind !== "async-question") continue;
        const turn = nativeConversationTurns(conversation).find(turn => nativeTurnId(turn) === turnId);
        if (!turn) continue;
        const source = array(turn.items).find(item => item.type === "agentMessage" && item.id === itemId);
        if (!array(source?.questions).length) continue;
        const sourceIds = source.questions.flatMap((question, index) => typeof question?.title === "string"
          ? [JSON.stringify(["request_user_input_async", itemId, index])] : []);
        const questionIds = Array.isArray(record.questionIds)
          ? record.questionIds.filter(id => sourceIds.includes(id)) : sourceIds;
        const processed = new Set(array(record.desktopSynchronizedQuestionIds));
        const accepted = acceptedAsyncQuestionReplies(turn);
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
    if (!request) throw failure(409, "此问题已回答或当前轮次已结束，请刷新会话");
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
        && questions.every(question => current.params.questions.some(value => value.id === question.id))
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
        const turn = nativeConversationTurns(sourceManager.getConversation(id))
          .find(turn => nativeTurnId(turn) === request.params.turnId);
        const accepted = acceptedAsyncQuestionReplies(turn);
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
  function runtimeThread(id, nativeThread = null, { readOnly = false } = {}) {
    const conversation = manager.getConversation(id);
    const requestsSnapshot = readOnly ? null : blockingRequestsSnapshot(id, conversation);
    if (!readOnly) recoverAsyncQuestionDesktop(id, conversation);
    const queued = readOnly ? [] : manager.getTurnCoordinator?.()?.readMessages?.(id)
      ?? manager.storage.readQueuedFollowUps?.()?.value?.[id];
    const modelSettings = threadModelSettings(conversation, nativeThread);
    const asyncQuestions = readOnly ? [] : asyncQuestionRequests(id, conversation);
    if (!readOnly) asyncQuestionSignatures.set(id, JSON.stringify(asyncQuestions));
    return {
      id,
      model: modelSettings.model,
      reasoningEffort: modelSettings.effort,
      serviceTier: conversation?.latestServiceTier ?? null,
      tokenUsage: conversation?.latestTokenUsageInfo ?? null,
      goal: runtimeGoal(conversation?.threadGoal),
      compaction: runtimeCompaction(id, conversation),
      transcriptEvents: nativeTranscriptEvents(conversation),
      editableMessage: readOnly ? null : editableMessageState(id).metadata,
      status: manager.getThreadRuntimeStatusEvidence(id) || { type: "idle" },
      requestsKnown: readOnly || !!requestsSnapshot,
      ...(readOnly ? { requests: [] } : requestsSnapshot ? { ...requestsSnapshot, requests: [...requestsSnapshot.requests, ...asyncQuestions] }
        : asyncQuestions.length ? { requests: asyncQuestions } : {}),
      ...queueSnapshotFields(id, queued)
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
    return {
      queueEpoch: epoch,
      queueRevision: ++queueSequence,
      queuedMessages: [...array(queued?.messages || queued)],
      queuedEdits: queuedEditSummaries(threadId)
    };
  }
  async function queueSnapshot(id) {
    id = requiredId(id);
    if (typeof manager.getTurnCoordinator !== "function")
      throw failure(503, "当前 Codex 版本的原生队列服务不可用");
    const coordinator = await manager.getTurnCoordinator();
    if (typeof coordinator?.readMessages !== "function")
      throw failure(503, "当前 Codex 版本不支持原生队列读取");
    await loadNativeQueue(coordinator, id);
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
      await loadNativeQueue(coordinator, threadId);
      const message = array(coordinator.readMessages?.(threadId)).find(message => message?.id === messageId);
      if (!message) throw failure(409, "此队列消息已不存在，请刷新会话");
      if (message.submission?.status && message.submission.status !== "queued")
        throw failure(409, "此消息已提交或送达尚未确认，不能作为待执行消息取消");
      const removed = await coordinator.removeQueuedMessage(threadId, messageId);
      if (removed == null) throw failure(409, "此队列消息已不存在，请刷新会话");
      const queued = coordinator.readMessages?.(threadId)
        ?? manager.storage.readQueuedFollowUps?.()?.value?.[threadId];
      return { ok: true, threadId, messageId, ...queueSnapshotFields(threadId, queued) };
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
      if (typeof coordinator?.readMessages !== "function"
          || typeof coordinator?.reorderQueuedMessages !== "function")
        throw failure(503, "当前 Codex 版本不支持原生队列排序");
      await loadNativeQueue(coordinator, threadId);
      const currentMessageIds = array(coordinator.readMessages(threadId)).map(message => message?.id);
      if (currentMessageIds.length !== expectedMessageIds.length
          || currentMessageIds.some((messageId, index) => messageId !== expectedMessageIds[index]))
        throw failure(409, "队列已发生变化，请刷新会话后重试");
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
  async function queuedEditFingerprint(action, payload) {
    const value = JSON.stringify([action, action === "editQueued" ? payload.prompt.trim() : null,
      payload.retainedImageSources ?? null,
      array(payload.images).map(image => [image.name, image.mime, image.dataUrl, image.size])]);
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
    return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, "0")).join("");
  }
  async function queueEditState(payload = {}) {
    const threadId = requiredId(payload.threadId), editId = requiredId(payload.editId);
    const entry = queuedEditStore.sessions.get(editId);
    if (entry && entry.threadId !== threadId) throw failure(409, "编辑标识不属于此会话");
    const snapshot = await queueSnapshot(threadId);
    const operationPending = !!(entry?.beginPromise || entry?.terminalPromise || entry?.finishRequests);
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
      const coordinator = await manager.getTurnCoordinator?.();
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
            || typeof coordinator?.restoreQueuedMessage !== "function")
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
        if (current.submission?.status && current.submission.status !== "queued")
          throw failure(409, "此队列消息正在发送或结果尚未确认，不能编辑");
        const serverEnabled = typeof serverQueue?.isEnabled === "function"
          && await serverQueue.isEnabled(entry.threadId);
        const matches = serverEnabled ? (await readServerQueuedSubmissions(entry.threadId))
          .filter(submission => submission?.id === entry.messageId) : [];
        if (matches.length > 1) throw failure(409, "队列消息身份已变化，请刷新后重试");
        entry.serverQueued = matches.length === 1;
        const serverSubmission = matches[0] ?? current.submission?.editPosition?.serverSubmission;
        if (!serverSubmission) throw failure(409, "此队列消息缺少可安全编辑的原生输入");
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
          ...queueSnapshotFields(entry.threadId, coordinator.readMessages(entry.threadId))
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
    const entry = queuedEditStore.sessions.get(payload.editId);
    if (entry) entry.finishRequests = (entry.finishRequests || 0) + 1;
    try { return await finishQueuedEditOperation(payload, action); }
    finally { if (entry) entry.finishRequests -= 1; }
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
            if (entry.serverQueued === false && message.submission?.editPosition?.serverSubmission) {
              message = { ...message, submission: { ...message.submission,
                editPosition: { ...message.submission.editPosition, serverSubmission } } };
            }
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
        const expectedClientId = restoreToken.restoreClientUserMessageId
          ?? restoreToken.serverSubmission?.clientUserMessageId ?? entry.clientUserMessageId;
        const messageId = requiredId(await coordinator.restoreQueuedMessage(entry.threadId, restoreToken));
        entry.restoredMessageId = messageId;
        const restoredMessage = array(coordinator.readMessages(entry.threadId))
          .find(message => message?.id === messageId);
        if (!restoredMessage
            || restoredMessage.clientUserMessageId !== expectedClientId
              && !(messageId === entry.messageId && restoredMessage.clientUserMessageId === entry.clientUserMessageId)
            || !queuedTextMatches(queuedMessageText(restoredMessage), action === "commit" ? prompt : entry.originalText))
          throw new Error("原生队列恢复后的消息身份或内容未确认");
        if (action === "commit" && expectedImageSources != null) {
          const restoredSources = (await queuedImageAttachmentRecords(restoredMessage))
            .map(record => record.reference);
          if (!sameJsonValue(restoredSources, expectedImageSources))
            throw new Error("原生队列恢复后的图片未确认");
          const submissions = entry.serverQueued === false
            ? [restoredMessage.submission?.editPosition?.serverSubmission]
            : (await readServerQueuedSubmissions(entry.threadId)).filter(submission => submission?.id === messageId);
          if (submissions.length !== 1 || !submissions[0]
              || submissions[0].clientUserMessageId !== expectedClientId
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
          ...queueSnapshotFields(entry.threadId, coordinator.readMessages(entry.threadId))
        };
      } catch (error) {
        if (restoreStarted)
          return queuedEditOutcomeUnknown(entry, action === "commit"
            ? "原生队列保存结果未确认，请刷新队列检查，不要重复保存"
            : "原生队列恢复结果未确认，请刷新队列检查，不要重复取消");
        entry.terminalAction = null;
        entry.terminalFingerprint = null;
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
    try {
      const sourceManager = messageActionManager();
      const threadId = requiredId(payload.threadId);
      const messageId = requiredId(payload.messageId);
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
        const prepared = await retryExecutionConfigLoading(
          () => coordinator.execution.prepare(request.conversationId, request.message, phase),
          assertCurrent
        );
        assertCurrent();
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
      return { ok: true, threadId, messageId, turnId: result.turnId,
        ...queueSnapshotFields(threadId, coordinator.readMessages(threadId)) };
    } catch (error) {
      return { ok: false, code: error.code || 503,
        ...(submitted && !error.remoteValidationFailure ? { outcomeUnknown: true } : {}),
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
      const input = [...preparedInput, ...inputItems];
      // server queue 展示和执行均先于 local queue；已有本地消息时交给原生 coordinator 保序。
      useServerQueue &&= !hasNativeLocalQueuedMessages(coordinator, serverQueue, threadId);
      if (!useServerQueue) {
        if (typeof coordinator.sendMessage !== "function" || typeof coordinator.loadMessages !== "function")
          throw failure(503, "当前 Codex 版本不支持原生本地消息队列");
        const request = {
          conversationId: threadId, message,
          queueModeOverride: sendNow ? "send-now" : "queue",
          acceptLocally: !sendNow,
          // 本地队列重载会重新准备消息，额外输入必须随原生重放上下文保存。
          editPosition: {
            serverSubmission: { id: message.id, clientUserMessageId, input },
            preserveSerializedContext: false
          }
        };
        const sendOnly = typeof coordinator.submission?.getQueueMode === "function"
          && await coordinator.submission.getQueueMode(request) === "send-only";
        assertCurrent();
        const assertAdmission = () => {
          assertCurrent();
          const conversation = sourceManager.getConversation(threadId);
          if (!conversation)
            throw failure(409, "会话状态已变化，暂不能提交排队消息");
          const status = sourceManager.getThreadRuntimeStatusEvidence?.(threadId)
            || conversation.threadRuntimeStatus || thread.status;
          const runtime = status?.status ?? status;
          const statusType = typeof runtime === "string" ? runtime : runtime?.type;
          const queued = coordinator.readMessages(threadId);
          const active = runtimeCompaction(threadId, conversation).state !== "idle"
            || ["active", "inProgress", "running", "pending"].includes(statusType)
            || nativeConversationTurns(conversation).some(turn => nativeTurnStatus(turn) === "inProgress")
            || array(queued?.messages || queued).length;
          if (sendOnly && active)
            throw failure(409, "此类会话只支持空闲时直接发送，请等待当前回复或压缩完成后重试");
          if (sendNow && active)
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
          return current.submission;
        }, undefined, sendNow ? assertAdmission : undefined, assertAdmission);
        if (!["queued", "sent"].includes(result?.status)
            || typeof result.messageId !== "string"
            || result.messageId !== clientUserMessageId && result.serverAccepted !== true)
          throw new Error("原生本地队列未确认消息身份");
        if (result.status === "sent") return {
          ok: true, threadId, messageId: result.messageId, clientUserMessageId, result
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
  async function call(action, payload = {}, sidebarOnly = false) {
    try {
      validateCompactionQueueSendPayload(action, payload);
      if (action === "dispose") { window.__codexElvesRemoteAccess.dispose(); return { ok: true }; }
      if (["send", "create", "setGoal"].includes(action)) validateGoalComposerPayload(action, payload);
      resetStaleManager();
      if (!manager || !sidebarOnly && !manager.__codexElvesRequestClientManager
          && (!unsubscribe.length || !appScope || !nativeNavigate
            || typeof nativeIntl?.formatMessage !== "function")) discover(sidebarOnly);
      if (manager.__codexElvesRequestClientManager && !requestClientReadActions.has(action))
        throw failure(503, "当前 Codex 尚未完成原生注入，远程会话暂只支持只读查看");
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
            ...(manager.__codexElvesRequestClientManager ? { nativeBridge: "read_only" } : {}),
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
          if (!manager.__codexElvesRequestClientManager && !view.readOnly
              && (!manager.getConversation(thread.id)
              || manager.getConversation(thread.id).resumeState === "needs_resume")) {
            await manager.resumeConversation({
              conversationId: thread.id, model: null, reasoningEffort: null,
              workspaceRoots: [thread.cwd], collaborationMode: null
            });
          }
          if (!manager.__codexElvesRequestClientManager && !view.readOnly) {
            const coordinator = await manager.getTurnCoordinator?.();
            if (typeof coordinator?.loadMessages === "function") await coordinator.loadMessages(thread.id);
          }
          const runtime = runtimeThread(thread.id, thread, view);
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
      if (["send", "create", "editMessage", "updateGoal"].includes(action) && !error.remoteValidationFailure) {
        return { ok: false, outcomeUnknown: true, code: 503, message: action === "updateGoal"
          ? "原生目标更新未确认完成，请检查会话，不要重复保存"
          : "原生操作未确认完成，请检查会话，不要重复发送或新建" };
      }
      return {
        ok: false,
        code: error.code || 503,
        message: error.message || "原生会话操作失败",
        ...(error.reason ? { reason: error.reason } : {}),
        ...(error.threadId ? { threadId: error.threadId } : {}),
        ...(error.parentThreadId ? { parentThreadId: error.parentThreadId } : {})
      };
    }
  }
  window.__codexElvesRemoteAccess = {
    version: 82, call,
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
