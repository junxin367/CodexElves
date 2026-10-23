const IMAGE_LIMIT = 64 * 1024 * 1024;
const imageReference = "__codexElvesRecoveryImage";

export function recoveryStorageMessage(error, submitting = false) {
  const prefix = submitting ? "草稿和回执保存失败，操作未提交。" : "草稿仅保留在本页，刷新可能丢失。";
  if (error?.name === "QuotaExceededError") return `${prefix}浏览器存储空间不足，请释放此站点的空间后重试。`;
  if (["SecurityError", "NotAllowedError"].includes(error?.name))
    return `${prefix}浏览器拒绝了此站点的本地存储权限。`;
  return `${prefix}${error?.message || "本地恢复存储不可用，请保持页面打开。"}`;
}

// 恢复数据只属于当前授权；正文、审批和可执行队列从不写入这里。
export function createRecoveryStore({ indexedDB = globalThis.indexedDB,
  databaseName = "codex-elves-input-recovery-v1", maxImageBytes = IMAGE_LIMIT } = {}) {
  let database = null, opening = null, owner = null, serial = Promise.resolve(), sequence = 0;
  const imageIds = new Map();
  const enqueue = work => {
    const result = serial.then(work);
    serial = result.catch(() => {});
    return result;
  };
  const nextGeneration = () => globalThis.crypto?.randomUUID?.()
    || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  function open() {
    if (database) return Promise.resolve(database);
    if (opening) return opening;
    opening = new Promise((resolve, reject) => {
      if (!indexedDB) { reject(new Error("当前浏览器不支持 IndexedDB 恢复存储。")); return; }
      const request = indexedDB.open(databaseName, 1);
      let expired = false;
      const timer = setTimeout(() => {
        expired = true; reject(new Error("本地恢复存储打开超时，请关闭此站点的旧页面后重试。"));
      }, 10000);
      request.onupgradeneeded = () => {
        for (const name of ["records", "images", "meta"])
          if (!request.result.objectStoreNames.contains(name)) request.result.createObjectStore(name);
      };
      request.onerror = () => { clearTimeout(timer); reject(request.error); };
      request.onsuccess = () => {
        clearTimeout(timer);
        if (expired) { request.result.close(); return; }
        database = request.result;
        database.onversionchange = () => { database.close(); database = null; opening = null; };
        resolve(database);
      };
    }).catch(error => { opening = null; throw error; });
    return opening;
  }
  async function transaction(work) {
    const db = await open();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(["records", "images", "meta"], "readwrite");
      let result, failure;
      tx.oncomplete = () => resolve(result);
      tx.onerror = () => reject(failure || tx.error || new Error("本地恢复存储写入失败。"));
      tx.onabort = () => reject(failure || tx.error || new Error("本地恢复存储写入已中止。"));
      try { work(tx, value => { result = value; }, error => { failure = error; tx.abort(); }); }
      catch (error) { failure = error; tx.abort(); }
    });
  }
  function assertOwner(scope) {
    if (!scope || owner !== scope) throw Object.assign(
      new Error("授权已更换，不能保存到旧授权的恢复存储。"), { name: "RecoveryAuthorizationError" });
  }
  function ownedTransaction(scope, work) {
    return transaction((tx, done, fail) => {
      const read = tx.objectStore("meta").get("owner");
      read.onsuccess = () => {
        try {
          assertOwner(scope);
          if (read.result !== scope) throw Object.assign(
            new Error("授权已在其他页面更换，不能读取或保存旧授权的恢复数据。"), { name: "RecoveryAuthorizationError" });
          work(tx, done, fail);
        } catch (error) { fail(error); }
      };
    });
  }
  function pack(value, images, added) {
    if (typeof value === "string" && /^data:image\/(?:png|jpeg|webp|gif);base64,/iu.test(value)) {
      let id = imageIds.get(value);
      if (!id) {
        id = globalThis.crypto?.randomUUID?.() || `image-${Date.now()}-${++sequence}`;
        imageIds.set(value, id);
      }
      images.set(id, value.length);
      added.set(id, value);
      return { [imageReference]: id };
    }
    if (Array.isArray(value)) return value.map(entry => pack(entry, images, added));
    if (value && typeof value === "object")
      return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, pack(entry, images, added)]));
    return value;
  }
  async function unpack(value, images) {
    if (value && typeof value === "object" && Object.hasOwn(value, imageReference)) {
      const source = images.get(value[imageReference]);
      if (typeof source !== "string") throw new Error("恢复图片缺失，原回执标识仍保留，请先核对回执。");
      imageIds.set(source, value[imageReference]);
      return source;
    }
    if (Array.isArray(value)) return Promise.all(value.map(entry => unpack(entry, images)));
    if (value && typeof value === "object")
      return Object.fromEntries(await Promise.all(Object.entries(value).map(async ([key, entry]) =>
        [key, await unpack(entry, images)])));
    return value;
  }
  function update(scope, values, removals = []) {
    return enqueue(async () => {
      assertOwner(scope);
      const added = new Map();
      const records = values.map(({ kind, id, value }) => {
        const images = new Map(), packed = pack(value, images, added);
        return { key: JSON.stringify([kind, id]), kind, id, value: packed, images: [...images] };
      });
      const usedIds = await ownedTransaction(scope, (tx, done, fail) => {
        const store = tx.objectStore("records"), read = store.getAll();
        read.onsuccess = () => {
          try {
            assertOwner(scope);
            const previous = read.result, next = new Map(previous.map(record => [record.key, record]));
            for (const [kind, id] of removals) next.delete(JSON.stringify([kind, id]));
            for (const record of records) next.set(record.key, record);
            const used = new Map([...next.values()].flatMap(record => record.images));
            if ([...used.values()].reduce((sum, bytes) => sum + bytes, 0) > maxImageBytes)
              throw Object.assign(new Error("恢复图片已达到 64 MiB 上限；请处理已有草稿后重试。"), { name: "QuotaExceededError" });
            for (const [kind, id] of removals) store.delete(JSON.stringify([kind, id]));
            for (const record of records) store.put(record, record.key);
            const images = tx.objectStore("images");
            const existingIds = new Set(previous.flatMap(record => record.images.map(([id]) => id)));
            for (const [id, source] of added) if (used.has(id) && !existingIds.has(id)) images.put(source, id);
            for (const id of existingIds)
              if (!used.has(id)) images.delete(id);
            done([...used.keys()]);
          } catch (error) { fail(error); }
        };
      }).catch(error => { imageIds.clear(); throw error; });
      const used = new Set(usedIds);
      for (const [source, id] of imageIds) if (!used.has(id)) imageIds.delete(source);
    });
  }
  return {
    observe() {
      return enqueue(() => transaction((tx, done) => {
        const meta = tx.objectStore("meta"), currentOwner = meta.get("owner"), generation = meta.get("generation");
        generation.onsuccess = () => done({ owner: currentOwner.result ?? null, generation: generation.result ?? null });
      }));
    },
    activate(scope, { replace = true, expected = null } = {}) {
      return enqueue(() => transaction((tx, done, fail) => {
        const meta = tx.objectStore("meta"), read = meta.get("owner"), generation = meta.get("generation");
        generation.onsuccess = () => {
          if (expected && (expected.owner !== (read.result ?? null)
              || expected.generation !== (generation.result ?? null))) {
            fail(Object.assign(new Error("读取会话期间授权已在其他页面更换，正在核对最新会话。"),
              { name: "RecoveryAuthorizationError", staleSession: true }));
            return;
          }
          if (!replace && read.result && read.result !== scope) {
            fail(Object.assign(new Error("授权已在其他页面更换，请重新连接后再提交。"), { name: "RecoveryAuthorizationError" }));
            return;
          }
          let currentGeneration = generation.result ?? null;
          if (read.result !== scope) {
            tx.objectStore("records").clear(); tx.objectStore("images").clear(); imageIds.clear();
            meta.put(scope, "owner");
            currentGeneration = nextGeneration();
            meta.put(currentGeneration, "generation");
          }
          owner = scope; done({ owner: scope, generation: currentGeneration });
        };
      }));
    },
    put: update,
    remove(scope, kind, id) { return update(scope, [], [[kind, id]]); },
    list(scope, kind) {
      return enqueue(async () => {
        assertOwner(scope);
        const snapshot = await ownedTransaction(scope, (tx, done) => {
          const records = tx.objectStore("records").getAll(), images = new Map();
          records.onsuccess = () => {
            const matches = records.result.filter(record => record.kind === kind);
            for (const id of new Set(matches.flatMap(record => record.images.map(([id]) => id)))) {
              const read = tx.objectStore("images").get(id);
              read.onsuccess = () => images.set(id, read.result);
            }
            done({ records: matches, images });
          };
        });
        return Promise.all(snapshot.records.map(async record =>
          ({ id: record.id, value: await unpack(record.value, snapshot.images) })));
      });
    },
    clear(expectedScope = owner) {
      owner = null; imageIds.clear();
      return enqueue(() => transaction((tx, done) => {
        owner = null;
        const read = tx.objectStore("meta").get("owner");
        read.onsuccess = () => {
          if (expectedScope && read.result === expectedScope) {
            tx.objectStore("records").clear(); tx.objectStore("images").clear();
            const meta = tx.objectStore("meta");
            meta.delete("owner"); meta.put(nextGeneration(), "generation");
          }
          done();
        };
      }));
    }
  };
}

export function createTranscriptCache({ maxEntries = 5, maxBytes = 6 * 1024 * 1024,
  maxTurns = 250, maxItems = 3000 } = {}) {
  const entries = new Map();
  let totalBytes = 0;
  const key = (deviceId, threadId) => JSON.stringify([deviceId, threadId]);
  return {
    put(deviceId, threadId, value, view = null) {
      if (!deviceId || !threadId) return;
      const id = key(deviceId, threadId);
      let serialized, bytes, retained, retainedView;
      // 长会话超限时保留服务端最近一页；游标仍来自服务端，不自行截断消息或制造分页缺口。
      const recentWindows = [value?.recentWindow, value?.recentPage].filter(Boolean).map(page => ({
        ...page, title: value.title, cwd: value.cwd,
        followMessages: true, scrollTop: 0, anchor: null, cacheWindowOnly: true
      }));
      for (const candidate of [value, ...recentWindows]) {
        if (!Array.isArray(candidate?.turns) || !candidate.turns.length || candidate.turns.length > maxTurns
            || candidate.transcriptEvents != null && !Array.isArray(candidate.transcriptEvents)
            || !candidate.turns.every(turn => turn && typeof turn.id === "string"
              && (turn.items == null || Array.isArray(turn.items)
                && turn.items.every(item => item && typeof item.id === "string")))
            || candidate.turns.reduce((sum, turn) => sum + (turn.items?.length || 0), 0) > maxItems) continue;
        const { recentPage: _recentPage, recentWindow: _recentWindow, ...snapshot } = candidate;
        let text;
        try { text = JSON.stringify(snapshot); } catch { continue; }
        const size = new TextEncoder().encode(text).length;
        if (size > maxBytes) continue;
        serialized = text; bytes = size; retained = snapshot;
        retainedView = candidate === value ? view : null;
        break;
      }
      if (serialized === undefined) return;
      const previous = entries.get(id);
      if (previous) { totalBytes -= previous.bytes; entries.delete(id); }
      // 渲染节点只留在内存，和对应正文一起淘汰，不进入可持久化的快照。
      entries.set(id, { serialized, bytes, view: retainedView }); totalBytes += bytes;
      while (entries.size > maxEntries || totalBytes > maxBytes) {
        const oldest = entries.keys().next().value;
        totalBytes -= entries.get(oldest).bytes; entries.delete(oldest);
      }
      return retained;
    },
    get(deviceId, threadId) {
      const id = key(deviceId, threadId), entry = entries.get(id);
      if (!entry) return null;
      entries.delete(id); entries.set(id, entry);
      return JSON.parse(entry.serialized);
    },
    takeView(deviceId, threadId) {
      const entry = entries.get(key(deviceId, threadId));
      const view = entry?.view || null;
      if (entry) entry.view = null;
      return view;
    },
    remove(deviceId, threadId) {
      const id = key(deviceId, threadId), entry = entries.get(id);
      if (entry) { totalBytes -= entry.bytes; entries.delete(id); }
    },
    clear() { entries.clear(); totalBytes = 0; }
  };
}

const readCacheKinds = new Set(["sidebar", "view", "thread", "transcript", "models", "skills",
  "workspace-list", "workspace-detail", "session-menu"]);
const readCacheDay = 24 * 60 * 60 * 1000;

// 与输入恢复库独立：这里只保存已授权用户的只读展示数据，不保存 Cookie、审批或可执行队列。
export function createReadCache({ indexedDB = globalThis.indexedDB,
  databaseName = "codex-elves-read-cache-v1", maxBytes = 32 * 1024 * 1024,
  maxRecordBytes = 6 * 1024 * 1024, maxEntries = 64, now = () => Date.now() } = {}) {
  let database = null, opening = null, owner = null, generation = 0, serial = Promise.resolve();
  const enqueue = work => {
    const next = serial.then(work);
    serial = next.catch(() => {});
    return next;
  };
  function open() {
    if (database) return Promise.resolve(database);
    if (opening) return opening;
    opening = new Promise((resolve, reject) => {
      if (!indexedDB) { reject(new Error("当前浏览器不支持只读快照缓存")); return; }
      const request = indexedDB.open(databaseName, 1);
      let expired = false;
      const timer = setTimeout(() => { expired = true; reject(new Error("只读快照缓存打开超时")); }, 1000);
      request.onupgradeneeded = () => {
        for (const name of ["records", "meta"])
          if (!request.result.objectStoreNames.contains(name)) request.result.createObjectStore(name);
      };
      request.onerror = () => { clearTimeout(timer); reject(request.error); };
      request.onsuccess = () => {
        clearTimeout(timer);
        if (expired) { request.result.close(); return; }
        database = request.result;
        database.onversionchange = () => { database.close(); database = null; opening = null; };
        resolve(database);
      };
    }).catch(error => { opening = null; throw error; });
    return opening;
  }
  async function transaction(work) {
    const db = await open();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(["records", "meta"], "readwrite");
      let result, failure;
      tx.oncomplete = () => resolve(result);
      tx.onerror = () => reject(failure || tx.error || new Error("只读快照缓存不可用"));
      tx.onabort = () => reject(failure || tx.error || new Error("只读快照缓存已取消"));
      try { work(tx, value => { result = value; }, error => { failure = error; tx.abort(); }); }
      catch (error) { failure = error; tx.abort(); }
    });
  }
  function owned(scope, revision, work) {
    return transaction((tx, done, fail) => {
      const meta = tx.objectStore("meta"), readOwner = meta.get("owner"), readIndex = meta.get("index");
      readIndex.onsuccess = () => {
        try {
          if (!scope || owner !== scope || revision !== generation || readOwner.result !== scope)
            throw new Error("只读快照所属授权已变化");
          work(tx, Array.isArray(readIndex.result) ? readIndex.result : [], done);
        } catch (error) { fail(error); }
      };
    });
  }
  function recordKey(kind, id) {
    if (!readCacheKinds.has(kind) || typeof id !== "string" || id.length > 4096)
      throw new Error("只读快照缓存键无效");
    return JSON.stringify([kind, id]);
  }
  return {
    activate(scope) {
      if (typeof scope !== "string" || !scope) return Promise.reject(new Error("只读快照缺少授权身份"));
      if (owner !== scope) { owner = scope; generation++; }
      const revision = generation;
      return enqueue(() => transaction((tx, done, fail) => {
        const meta = tx.objectStore("meta"), read = meta.get("owner");
        read.onsuccess = () => {
          if (owner !== scope || generation !== revision) { fail(new Error("授权已变化")); return; }
          if (read.result !== scope) {
            tx.objectStore("records").clear();
            meta.put([], "index");
            meta.put(scope, "owner");
          }
          done();
        };
      }));
    },
    get(scope, kind, id) {
      const key = recordKey(kind, id), revision = generation;
      return enqueue(() => owned(scope, revision, (tx, index, done) => {
        const record = index.find(entry => entry.key === key), time = now();
        if (!record || record.expiresAt <= time) {
          if (record) {
            tx.objectStore("records").delete(key);
            tx.objectStore("meta").put(index.filter(entry => entry.key !== key), "index");
          }
          done(null); return;
        }
        const read = tx.objectStore("records").get(key);
        read.onsuccess = () => {
          let value;
          try { value = typeof read.result === "string" ? JSON.parse(read.result) : null; }
          catch {
            tx.objectStore("records").delete(key);
            tx.objectStore("meta").put(index.filter(entry => entry.key !== key), "index");
            done(null); return;
          }
          record.accessedAt = time;
          tx.objectStore("meta").put(index, "index");
          done(value);
        };
      }));
    },
    put(scope, kind, id, value, { maxAge = 7 * readCacheDay } = {}) {
      const key = recordKey(kind, id), revision = generation;
      let serialized;
      try { serialized = JSON.stringify(value); } catch { return Promise.resolve(false); }
      if (serialized === undefined) return Promise.resolve(false);
      const bytes = new TextEncoder().encode(serialized).length;
      if (bytes > maxRecordBytes || bytes > maxBytes || !Number.isFinite(maxAge) || maxAge <= 0)
        return Promise.resolve(false);
      return enqueue(() => owned(scope, revision, (tx, index, done) => {
        const time = now(), store = tx.objectStore("records");
        const next = index.filter(entry => {
          const keep = entry.key !== key && entry.expiresAt > time;
          if (!keep) store.delete(entry.key);
          return keep;
        }).sort((left, right) => left.accessedAt - right.accessedAt);
        let total = next.reduce((sum, entry) => sum + entry.bytes, 0);
        while (next.length >= maxEntries || total + bytes > maxBytes) {
          const expired = next.shift();
          if (!expired) break;
          total -= expired.bytes; store.delete(expired.key);
        }
        store.put(serialized, key);
        next.push({ key, kind, bytes, accessedAt: time, expiresAt: time + maxAge });
        tx.objectStore("meta").put(next, "index");
        done(true);
      }));
    },
    remove(scope, kind, id) {
      const key = recordKey(kind, id), revision = generation;
      return enqueue(() => owned(scope, revision, (tx, index, done) => {
        tx.objectStore("records").delete(key);
        tx.objectStore("meta").put(index.filter(entry => entry.key !== key), "index");
        done();
      }));
    },
    invalidate(scope, kind) {
      if (!readCacheKinds.has(kind)) return Promise.reject(new Error("缓存类型无效"));
      const revision = generation;
      return enqueue(() => owned(scope, revision, (tx, index, done) => {
        for (const entry of index) if (entry.kind === kind) tx.objectStore("records").delete(entry.key);
        tx.objectStore("meta").put(index.filter(entry => entry.kind !== kind), "index");
        done();
      }));
    },
    clear(scope = owner) {
      owner = null; generation++;
      return enqueue(() => transaction((tx, done) => {
        const meta = tx.objectStore("meta"), read = meta.get("owner");
        read.onsuccess = () => {
          if (scope && read.result === scope) {
            tx.objectStore("records").clear(); meta.clear();
          }
          done();
        };
      }));
    }
  };
}
