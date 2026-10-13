(() => {
  if (window.__codexElvesRemoteAccess?.version === 4) return;
  window.__codexElvesRemoteAccess?.dispose?.();
  let manager = null;
  let appScope = null;
  let unsubscribe = [];
  let eventSequence = 0;
  const epoch = crypto.randomUUID();
  const notifications = [];
  const methods = [
    "thread/started", "thread/status/changed", "thread/name/updated", "thread/archived", "thread/unarchived", "thread/deleted",
    "turn/started", "turn/completed", "item/started", "item/completed", "item/agentMessage/delta",
    "item/reasoning/summaryTextDelta", "item/reasoning/textDelta", "item/commandExecution/outputDelta",
    "item/plan/delta", "turn/plan/updated", "thread/tokenUsage/updated", "thread/settings/updated", "skills/changed", "serverRequest/resolved"
  ];
  const isManager = value => value?.hostId === "local" && typeof value.sendFollowUpMessage === "function"
    && typeof value.listThreadTurns === "function" && value.storage?.readGlobalState;
  const emit = event => {
    if (["thread/archived", "thread/deleted"].includes(event.method)) {
      nativeCatalog.delete(event.params?.threadId);
      catalogLoaded = false;
    }
    notifications.push({ sequence: ++eventSequence, event });
    if (notifications.length > 2048) notifications.splice(0, notifications.length - 2048);
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
  const normalizePath = path => String(path || "").replaceAll("\\", "/").replace(/\/+$/, "").toLowerCase();
  function effortAllowed(model, effort) {
    if (!effort) return true;
    if (/(^|[/:\s])(?:gpt|chatgpt|o[134](?:[-_.]|$))/i.test(model)) return ["none", "minimal", "low", "medium", "high", "xhigh"].includes(effort);
    if (/claude/i.test(model)) return ["none", "low", "medium", "high", "xhigh", "max"].includes(effort);
    return true;
  }
  const timestamp = value => typeof value === "number" ? (value < 100_000_000_000 ? value * 1000 : value) : 0;
  let nativeCatalog = new Map();
  let catalogLoaded = false;
  let catalogLoading = null;
  function summary(thread) {
    const id = thread.conversationId || thread.id;
    return {
      id, title: thread.title || thread.name || thread.preview || "新会话", cwd: thread.cwd || "",
      createdAt: timestamp(thread.createdAt), updatedAt: timestamp(thread.updatedAt),
      recencyAt: timestamp(thread.recencyAt ?? thread.updatedAt), hasUnreadTurn: thread.hasUnreadTurn === true,
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
    const observePage = page => {
      for (const thread of array(page.data)) {
        if (thread.threadSource === "subagent" || thread.source?.subAgent) continue;
        nativeCatalog.set(thread.id, summary(thread));
      }
    };
    if (!catalogLoaded) {
      if (!catalogLoading) catalogLoading = (async () => {
        let cursor = null;
        const cursors = new Set();
        do {
          const page = await manager.requestClient.sendRequest("thread/list", { cursor, limit: 100, sortKey: "updated_at", archived: false });
          observePage(page);
          cursor = page.nextCursor || null;
          if (cursor && cursors.has(cursor)) throw failure(503, "原生会话目录分页游标重复");
          if (cursor) cursors.add(cursor);
        } while (cursor);
        catalogLoaded = true;
      })().finally(() => { catalogLoading = null; });
      await catalogLoading;
    } else {
      observePage(await manager.requestClient.sendRequest("thread/list", { cursor: null, limit: 100, sortKey: "updated_at", archived: false }));
    }
    for (const thread of array(manager.getRecentThreadSummaries())) nativeCatalog.set(thread.conversationId, summary(thread));
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
  function runtimeThread(id) {
    const conversation = manager.getConversation(id);
    const queued = manager.storage.readQueuedFollowUps?.()?.value?.[id];
    return {
      id, model: conversation?.latestModel || null, reasoningEffort: conversation?.latestReasoningEffort ?? null,
      serviceTier: conversation?.latestServiceTier ?? null,
      tokenUsage: conversation?.latestTokenUsageInfo ?? null,
      status: manager.getThreadRuntimeStatusEvidence(id) || { type: "idle" },
      requests: array(conversation?.requests).map(request => ({ id: request.id, method: request.method, params: request.params })),
      queuedMessages: array(queued?.messages || queued)
    };
  }
  async function skillCatalog(payload) {
    let cwd;
    if (payload.threadId) cwd = (await readThread(payload.threadId)).cwd;
    else {
      const project = (await state("local-projects") || {})[requiredId(payload.projectId)];
      cwd = project?.rootPaths?.[0];
    }
    if (!cwd) throw failure(400, "请先选择已有会话，或为新会话选择本机项目");
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
  async function skillInputs(payload) {
    if (payload.skills == null) return [];
    if (!Array.isArray(payload.skills) || payload.skills.length > 16
        || payload.skills.some(skill => !skill || typeof skill.name !== "string" || typeof skill.path !== "string"))
      throw failure(400, "Skill 选择无效，最多选择 16 项");
    if (!payload.skills.length) return [];
    const available = (await skillCatalog(payload)).data;
    const selected = new Map();
    for (const skill of payload.skills) {
      const entry = available.find(item => item.name === skill.name && item.path === skill.path);
      if (!entry) throw failure(409, `Skill「${skill.name}」已停用或不属于当前项目，请重新选择`);
      selected.set(entry.path, { type: "skill", name: entry.name, path: entry.path });
    }
    return [...selected.values()];
  }
  async function nativeCreationInputs() {
    if (!appScope) discover();
    if (!appScope) throw failure(503, "当前 Codex 版本的原生创建上下文不可用");
    const resource = [...document.querySelectorAll('link[rel="modulepreload"]')].map(link => link.href)
      .concat(performance.getEntriesByType("resource").map(entry => entry.name))
      .find(name => /\/app-initial-[^/]+\.js$/.test(name));
    if (!resource || !resource.startsWith("app://-/")) throw failure(503, "未发现原生创建模块");
    const module = await import(resource);
    const factory = Object.values(module).find(value => typeof value === "function"
      && /requestClient:\w+\.get\(/.test(Function.prototype.toString.call(value))
      && /dynamicTools:new/.test(Function.prototype.toString.call(value))
      && /window\.dispatchEvent/.test(Function.prototype.toString.call(value)));
    if (!factory) throw failure(503, "当前 Codex 版本不支持原生创建适配，请在桌面新建会话");
    const provider = factory(appScope, "local");
    if (typeof provider?.readCreationInputs !== "function") throw failure(503, "原生创建输入不可用");
    return provider;
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
        case "history": {
          await readThread(payload.threadId);
          const loaded = await manager.listThreadTurns(payload.threadId, {
            cursor: payload.cursor || null, limit: 30, itemsView: "full", sortDirection: "desc"
          });
          const page = loaded.response || loaded;
          return { ok: true, data: array(page.data), nextCursor: page.nextCursor ?? null, backwardsCursor: page.backwardsCursor ?? null };
        }
        case "thread": {
          const thread = await readThread(payload.threadId);
          if (!manager.getConversation(thread.id) || manager.getConversation(thread.id).resumeState === "needs_resume") {
            await manager.resumeConversation({
              conversationId: thread.id, model: null, reasoningEffort: null,
              workspaceRoots: [thread.cwd], collaborationMode: null
            });
          }
          return { ok: true, thread: { ...summary(thread), ...runtimeThread(thread.id), status: thread.status || runtimeThread(thread.id).status } };
        }
        case "models": {
          const data = [], cursors = new Set();
          let cursor = null;
          do {
            const page = await manager.requestClient.sendRequest("model/list", { limit: 100, cursor });
            data.push(...array(page.data));
            cursor = page.nextCursor ?? null;
            if (cursor && cursors.has(cursor)) throw failure(503, "原生模型列表分页无效");
            if (cursor) cursors.add(cursor);
          } while (cursor);
          return { ok: true, data: data.filter(model => !model.hidden).map(model => ({
            id: model.id, model: model.model, displayName: model.displayName,
            supportedReasoningEfforts: array(model.supportedReasoningEfforts).filter(effort => effortAllowed(model.model, effort.reasoningEffort)),
            defaultReasoningEffort: model.defaultReasoningEffort
          })) };
        }
        case "skills": return await skillCatalog(payload);
        case "send": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          if (typeof payload.prompt !== "string" || !payload.prompt.trim()) throw failure(400, "消息不能为空");
          const options = { prompt: payload.prompt, inputItems: await skillInputs(payload) };
          const selectedModel = payload.model || manager.getConversation(id)?.latestModel;
          if (payload.reasoningEffort && !selectedModel) throw failure(409, "请先打开会话，确认模型后再设置思考深度");
          if (!effortAllowed(selectedModel || "", payload.reasoningEffort))
            throw failure(400, "思考深度超出此模型支持的范围");
          if (payload.model != null) options.model = String(payload.model);
          if (payload.reasoningEffort != null) options.reasoningEffort = String(payload.reasoningEffort);
          const result = await manager.sendFollowUpMessage(id, options);
          if (result?.status === "outcome-unknown") return { ok: false, outcomeUnknown: true, threadId: id, message: "原生客户端未确认送达，请查看会话，不要重复发送" };
          return { ok: true, threadId: id, result: result ?? null };
        }
        case "create": {
          const projects = await state("local-projects") || {};
          const project = projects[requiredId(payload.projectId)];
          if (!project?.rootPaths?.length) throw failure(400, "请选择现有本机项目");
          const provider = await nativeCreationInputs();
          if (!effortAllowed(payload.model || "", payload.reasoningEffort)) throw failure(400, "思考深度超出此模型支持的范围");
          const skills = await skillInputs(payload);
          const result = await manager.startConversation({
            input: [{ type: "text", text: String(payload.prompt || ""), text_elements: [] }, ...skills],
            cwd: project.rootPaths[0], workspaceRoots: project.rootPaths,
            projectAssignment: { projectKind: "local", projectId: payload.projectId },
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
        }
        case "stop": {
          const id = requiredId(payload.threadId);
          await readThread(id);
          await manager.stopThread(id, "user");
          await manager.cleanBackgroundTerminals(id);
          const terminals = await manager.requestClient.sendRequest("thread/backgroundTerminals/list", { threadId: id });
          return { ok: true, threadId: id, backgroundTerminals: terminals };
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
      if (["send", "create"].includes(action) && !error.remoteValidationFailure) {
        return { ok: false, outcomeUnknown: true, code: 503, message: "原生操作未确认完成，请检查会话，不要重复发送或新建" };
      }
      return { ok: false, code: error.code || 503, message: error.message || "原生会话操作失败" };
    }
  }
  window.__codexElvesRemoteAccess = {
    version: 4, call,
    dispose() {
      for (const dispose of unsubscribe) { try { dispose?.(); } catch {} }
      unsubscribe = [];
      if (window.__codexElvesRemoteAccess?.epoch === epoch) delete window.__codexElvesRemoteAccess;
    },
    epoch
  };
})();
