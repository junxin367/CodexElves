(() => {
  // 用户要求所有新版入口复用原生 RPC；只包装调用，不修改 RPC stub 本身或私有模块源码。
  // 未列出的成员、Symbol.dispose 和序列化用的 Symbol 必须原样转发。
  function createRuntime() {
    let handlers = {};
    const managers = new WeakMap();
    const coordinators = new WeakMap();
    const worktrees = new WeakMap();
    const registryPatches = new Map();
    const clientPatches = new Map();
    const servicePatches = new Map();
    const providerPatches = new Map();
    const providers = new WeakMap();

    const active = () => handlers.enabled?.() === true;
    function forward(target, key) {
      const value = Reflect.get(target, key, target);
      // RpcStub 的原型方法使用私有字段，接收者必须是原对象；动态 RPC 成员则保持原样，
      // 以保留 RpcPromise 的 then/dup 和传输层 Symbol，不把它退化成普通 bound function。
      if (typeof value === "function" && key !== "constructor") {
        for (let proto = Object.getPrototypeOf(target); proto; proto = Object.getPrototypeOf(proto)) {
          if (Object.getOwnPropertyDescriptor(proto, key)?.value === value) return value.bind(target);
        }
      }
      return value;
    }
    function wrapThen(target, wrap) {
      const then = Reflect.get(target, "then", target);
      return typeof then === "function"
        ? (resolve, reject) => then.call(target, (value) => resolve?.(wrap(value)), reject)
        : undefined;
    }
    function wrapCoordinator(target) {
      if (!target || !["object", "function"].includes(typeof target)) return target;
      if (coordinators.has(target)) return coordinators.get(target);
      const wrapped = new Proxy(target, {
        get(original, key) {
          if (key === "then") return wrapThen(original, wrapCoordinator);
          if (key === "dup") return (...args) => wrapCoordinator(original.dup(...args));
          if (key === "sendMessage" || key === "sendQueuedMessageNow") {
            return async (...args) => {
              if (!active()) return original[key](...args);
              const index = key === "sendMessage" ? 1 : 3;
              const prepare = args[index];
              if (typeof prepare !== "function") return original[key](...args);
              const next = [...args];
              next[index] = async (...prepareArgs) => {
                const submission = await prepare(...prepareArgs);
                return handlers.prepareSubmission ? await handlers.prepareSubmission(submission) : submission;
              };
              // 用户要求主流程不受附属功能影响；原生回执不再经过额外完成处理。
              return await original[key](...next);
            };
          }
          return forward(original, key);
        },
      });
      coordinators.set(target, wrapped);
      coordinators.set(wrapped, wrapped);
      return wrapped;
    }
    function wrapManager(target) {
      if (!target || !["object", "function"].includes(typeof target)) return target;
      if (managers.has(target)) return managers.get(target);
      const wrapped = new Proxy(target, {
        get(original, key) {
          if (key === "then") return wrapThen(original, wrapManager);
          if (key === "dup") return (...args) => wrapManager(original.dup(...args));
          if (key === "getTurnCoordinator") return (...args) => wrapCoordinator(original.getTurnCoordinator(...args));
          if (["startTurn", "startTurnWithOutcome", "startEmptyTurn", "startConversation", "editLastUserTurn", "sendFollowUpMessage"].includes(key)) {
            return (...args) => active() && handlers.invokeTurn
              ? handlers.invokeTurn(original, key, args)
              : original[key](...args);
          }
          if (key === "sendRequest") {
            return (...args) => active() && handlers.sendRequest
              ? handlers.sendRequest(original, args)
              : original.sendRequest(...args);
          }
          return forward(original, key);
        },
      });
      managers.set(target, wrapped);
      managers.set(wrapped, wrapped);
      return wrapped;
    }
    function wrapWorktrees(target) {
      if (!target || !["object", "function"].includes(typeof target)) return target;
      if (worktrees.has(target)) return worktrees.get(target);
      const pending = target.pending;
      const wrappedPending = new Proxy(pending, {
        get(original, key) {
          if (key === "create") return (request, ...rest) => original.create(
            active() && handlers.prepareWorktree ? handlers.prepareWorktree(request) : request, ...rest,
          );
          return forward(original, key);
        },
      });
      const wrapped = new Proxy(target, {
        get(original, key) {
          return key === "pending" ? wrappedPending : forward(original, key);
        },
      });
      worktrees.set(target, wrapped);
      worktrees.set(wrapped, wrapped);
      return wrapped;
    }
    function wrapProvider(target) {
      if (providers.has(target)) return providers.get(target);
      const wrapped = new Proxy(target, {
        get(original, key) {
          if (key === "dup") return (...args) => wrapProvider(original.dup(...args));
          if (key === "subscribe") return (hostId, listener, ...rest) => original.subscribe(
            hostId,
            hostId === "local" ? (value) => listener(value?.manager
              ? { ...value, manager: wrapManager(value.manager) } : value) : listener,
            ...rest,
          );
          return forward(original, key);
        },
      });
      providers.set(target, wrapped);
      providers.set(wrapped, wrapped);
      return wrapped;
    }
    const runtime = {
      configure(value) { handlers = value || {}; },
      wrapManager,
      wrapCoordinator,
      patchRegistry(registry) {
        // RPC 属性访问会动态生成任意方法，不能据 typeof 判断是普通 registry。
        if (!registry || typeof Object.getOwnPropertyDescriptor(registry, "forHost")?.value !== "function") return false;
        const prior = registryPatches.get(registry);
        if (prior && registry.forHost === prior.patched) return true;
        const original = registry.forHost;
        const patched = function (hostId, ...rest) {
          const value = original.call(this, hostId, ...rest);
          return hostId === "local" ? wrapManager(value) : value;
        };
        registry.forHost = patched;
        if (registry.forHost !== patched) return false;
        registryPatches.set(registry, { original, patched });
        return true;
      },
      patchProvider(services) {
        if (!services?.appServerManagers) return false;
        const prior = providerPatches.get(services);
        if (prior && services.appServerManagers === prior.patched) return true;
        const original = services.appServerManagers, patched = wrapProvider(original);
        services.appServerManagers = patched;
        if (services.appServerManagers !== patched) return false;
        providerPatches.set(services, { original, patched });
        return true;
      },
      patchClient(client) {
        if (client?.hostId !== "local" || !client.rpc) return false;
        const prior = clientPatches.get(client);
        if (prior && client.rpc === prior.patched) return true;
        const original = client.rpc, patched = wrapManager(original);
        client.rpc = patched;
        if (client.rpc !== patched) return false;
        clientPatches.set(client, { original, patched });
        return true;
      },
      patchWorktrees(services) {
        if (!services?.managedWorktrees?.pending) return false;
        const prior = servicePatches.get(services);
        if (prior && services.managedWorktrees === prior.patched) return true;
        const original = services.managedWorktrees, patched = wrapWorktrees(original);
        services.managedWorktrees = patched;
        if (services.managedWorktrees !== patched) return false;
        servicePatches.set(services, { original, patched });
        return true;
      },
      status: () => ({ registries: registryPatches.size, clients: clientPatches.size, worktrees: servicePatches.size }),
      dispose() {
        handlers = {};
        for (const [registry, {original, patched}] of registryPatches) if (registry.forHost === patched) registry.forHost = original;
        for (const [client, {original, patched}] of clientPatches) if (client.rpc === patched) client.rpc = original;
        for (const [services, {original, patched}] of servicePatches) if (services.managedWorktrees === patched) services.managedWorktrees = original;
        for (const [services, {original, patched}] of providerPatches) if (services.appServerManagers === patched) services.appServerManagers = original;
        registryPatches.clear(); clientPatches.clear(); servicePatches.clear(); providerPatches.clear();
      },
    };
    return runtime;
  }
  if (typeof module === "object" && module.exports) module.exports = { createRuntime };
  if (typeof window === "object" && !window.__codexElvesNativeRpcCompat) {
    window.__codexElvesNativeRpcCompat = createRuntime();
  }
})();
