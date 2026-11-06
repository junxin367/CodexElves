(() => {
  "use strict";

  const runtimeKey = "__codexElvesStatsigCache";
  const version = 1;
  const previous = window[runtimeKey];
  if (previous?.version === version && previous.installed) {
    previous.scheduleRecovery();
    return;
  }
  if (typeof LZString?.compressToUTF16 !== "function" || typeof Storage === "undefined") return;

  let storage;
  try {
    storage = window.localStorage;
  } catch (_) {
    return;
  }
  const prototype = Storage.prototype;
  const originalGet = prototype.getItem;
  const originalSet = prototype.setItem;
  const originalRemove = prototype.removeItem;
  const evaluationPrefix = "statsig.cached.evaluations.";
  const compressedPrefix = "codexElves.statsigCache.v1:";
  const marker = "ce-sc1:";
  // 用户要求重启后仍能读回完整功能配置：只压缩 Statsig 评估缓存，保留原键及 SDK 的账号校验。
  // 64 Ki 字符以下保留原格式；16 Mi 字符仅用于拒绝损坏的长度标头，不裁剪任何配置值。
  const compressionThreshold = 64 * 1024;
  const maxDecodedChars = 16 * 1024 * 1024;
  const state = {
    version,
    installed: false,
    writes: 0,
    reads: 0,
    invalidReads: 0,
    lastWrite: null,
    lastRecovery: null,
    recoveryPromise: null,
    timer: null,
  };

  const nativeGet = (key) => originalGet.call(storage, key);
  const nativeSet = (key, value) => originalSet.call(storage, key, value);
  const nativeRemove = (key) => originalRemove.call(storage, key);
  const isEvaluationKey = (key) =>
    typeof key === "string" &&
    key.startsWith(evaluationPrefix) &&
    key.length > evaluationPrefix.length;
  const aliasFor = (key) => compressedPrefix + key;

  function checksum(value) {
    let result = 0x811c9dc5;
    for (let index = 0; index < value.length; index++) {
      result = Math.imul(result ^ value.charCodeAt(index), 0x01000193);
    }
    return (result >>> 0).toString(16).padStart(8, "0");
  }

  function readHeader(encoded) {
    if (typeof encoded !== "string" || !encoded.startsWith(marker)) return null;
    const match = /^ce-sc1:(\d+):([0-9a-f]{8}):/.exec(encoded);
    if (!match) return null;
    const length = Number(match[1]);
    if (!Number.isSafeInteger(length) || length < 0 || length > maxDecodedChars) return null;
    return { length, checksum: match[2], offset: match[0].length };
  }

  function readCompressed(key) {
    const encoded = nativeGet(aliasFor(key));
    if (encoded === null) return null;
    try {
      const header = readHeader(encoded);
      if (header) {
        const decoded = LZString.decompressFromUTF16(encoded.slice(header.offset));
        if (
          typeof decoded === "string" &&
          decoded.length === header.length &&
          checksum(decoded) === header.checksum
        ) {
          state.reads++;
          return decoded;
        }
      }
    } catch (_) {}
    state.invalidReads++;
    return null;
  }

  function getItem(key) {
    if (this !== storage || !isEvaluationKey(key)) {
      return originalGet.apply(this, arguments);
    }
    return readCompressed(key) ?? originalGet.apply(this, arguments);
  }

  function setItem(key, value) {
    if (this !== storage || !isEvaluationKey(key) || typeof value !== "string") {
      return originalSet.apply(this, arguments);
    }
    let encoded = null;
    if (value.length >= compressionThreshold && value.length <= maxDecodedChars) {
      const compressed = LZString.compressToUTF16(value);
      const candidate = `${marker}${value.length}:${checksum(value)}:${compressed}`;
      if (candidate.length < value.length) encoded = candidate;
    }
    if (encoded !== null) {
      // 使用独立副本键：未加载增强的原生客户端只会缓存未命中，不会把压缩文本当成 SDK JSON。
      // 先成功写入新副本才移除旧原文；配额写入失败时，浏览器会保留此前有效的副本。
      nativeSet(aliasFor(key), encoded);
      nativeRemove(key);
      state.lastWrite = { rawChars: value.length, storedChars: encoded.length, compressed: true };
    } else {
      originalSet.apply(this, arguments);
      nativeRemove(aliasFor(key));
      state.lastWrite = { rawChars: value.length, storedChars: value.length, compressed: false };
    }
    state.writes++;
  }

  function removeItem(key) {
    if (this === storage && isEvaluationKey(key)) nativeRemove(aliasFor(key));
    return originalRemove.apply(this, arguments);
  }

  // sessionStorage、其他 localStorage 键和 SDK 的缓存清理策略保持原行为。
  Object.defineProperties(prototype, {
    getItem: { ...Object.getOwnPropertyDescriptor(prototype, "getItem"), value: getItem },
    setItem: { ...Object.getOwnPropertyDescriptor(prototype, "setItem"), value: setItem },
    removeItem: { ...Object.getOwnPropertyDescriptor(prototype, "removeItem"), value: removeItem },
  });
  state.installed = true;
  window[runtimeKey] = state;

  function clients() {
    const sdk = window.__STATSIG__;
    return [...new Set([sdk?.firstInstance, ...Object.values(sdk?.instances || {})])]
      .filter((client) => client && typeof client.getContext === "function");
  }

  function currentEntry(client) {
    const adapter = client.dataAdapter;
    const context = client.getContext();
    if (
      context.options?.disableStorage === true ||
      typeof adapter?.getDataSync !== "function" ||
      typeof adapter?._getCacheKey !== "function"
    ) return null;
    const key = adapter._getCacheKey(context.user);
    if (!isEvaluationKey(key)) return null;
    // 沿用 SDK 自己的 user cache key、stableID 和失效校验，禁止从别的账号或分析快照回填。
    const entry = adapter.getDataSync(context.user);
    return entry && typeof entry.data === "string"
      ? { adapter, context, key, entry }
      : null;
  }

  function persistEntry(record) {
    const { adapter, key, entry } = record;
    if (entry.source === "Cache" && storage.getItem(key) !== null) return true;
    const serialized = JSON.stringify(entry);
    const header = readHeader(nativeGet(aliasFor(key)));
    if (
      header?.length === serialized.length &&
      header.checksum === checksum(serialized)
    ) return true;
    // 经 SDK 的存储提供器写入，保留 disableStorage 等设置，不绕过缓存禁用或兼容边界。
    if (typeof adapter._writeToCache !== "function") return false;
    adapter._writeToCache(key, entry);
    return storage.getItem(key) !== null;
  }

  async function recoverClient(client, refreshIfMissing) {
    // 注入可能晚于 SDK 的首次读取；等待原初始化结束，再按当前身份读缓存，避免覆盖在途切号。
    if (client._initializePromise) await client._initializePromise.catch(() => {});
    let record = currentEntry(client);
    let restored = false;
    if (record && !client.getContext().values && typeof client.updateUserSync === "function") {
      const result = client.updateUserSync(record.context.user, {
        disableBackgroundCacheRefresh: true,
      });
      restored = result.success === true && !!client.getContext().values;
      record = currentEntry(client);
    }
    let refresh = null;
    if (
      !client.getContext().values &&
      !record &&
      refreshIfMissing &&
      typeof client.refreshValuesAsync === "function"
    ) {
      const result = await client.refreshValuesAsync();
      refresh = { success: result.success, source: result.source };
      record = currentEntry(client);
    }
    return {
      hasValues: !!client.getContext().values,
      restored,
      persisted: !!record && persistEntry(record),
      refresh,
    };
  }

  state.recover = ({ refreshIfMissing = false } = {}) => {
    if (state.recoveryPromise) return state.recoveryPromise;
    state.recoveryPromise = (async () => {
      const results = [];
      for (const client of clients()) {
        try {
          results.push(await recoverClient(client, refreshIfMissing));
        } catch (_) {
          // 诊断仅保留状态，不记录配置正文、用户信息或网络凭据。
          results.push({ hasValues: !!client.getContext().values, persisted: false, failed: true });
        }
      }
      state.lastRecovery = { results };
      return state.lastRecovery;
    })().finally(() => {
      state.recoveryPromise = null;
    });
    return state.recoveryPromise;
  };

  state.scheduleRecovery = () => {
    if (state.timer !== null) return;
    const deadline = Date.now() + 60000;
    const check = () => {
      state.timer = null;
      const available = clients();
      if (
        available.some((client) =>
          client.loadingStatus === "Ready" || client._initializePromise
        )
      ) {
        void state.recover();
      } else if (Date.now() < deadline) {
        // 每 100ms 检查，最多等待一分钟；只等待首次加载，不永久轮询或新增联网请求。
        state.timer = setTimeout(check, 100);
      }
    };
    state.timer = setTimeout(check, 0);
  };
  state.scheduleRecovery();
})();
