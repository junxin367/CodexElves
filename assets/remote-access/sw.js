const schemaVersion = 1;
const shellVersion = 1;
const cachePrefix = "codex-elves-pwa-v1-";
const manifestPath = "/app-resources.json";
const hexPattern = /^[a-f0-9]{64}$/;
const resourcePathPattern = /^\/(?:[A-Za-z0-9._-]+\/)*[A-Za-z0-9._-]+$/;
const allowedContentTypes = new Set([
  "application/javascript; charset=utf-8",
  "application/manifest+json; charset=utf-8",
  "image/png",
  "text/css; charset=utf-8",
  "text/html; charset=utf-8"
]);
const maximumFileSize = 24 * 1024 * 1024;
const maximumTotalSize = 64 * 1024 * 1024;
const maximumFiles = 512;
const maximumDownloads = 2;
const manifestTimeoutMs = 30000;
const backgroundQuietMs = 1500;
const activityLeaseMs = 30000;
const optionalRetryMs = 5 * 60 * 1000;
const maximumOptionalInterruptions = 2;
const deferredResourcePaths = new Set([
  "/markdown-renderer.js", "/vendor/marked.esm.js", "/vendor/dompurify.esm.js",
  "/vendor/mermaid.esm.js", "/manifest.webmanifest",
  "/icons/icon-192.png", "/icons/icon-512.png", "/icons/apple-touch-icon.png"
]);
const criticalResourceOrder = [
  "/", "/style.css", "/platform.js", "/pwa.js", "/markdown.js", "/app.js",
  "/events-client.js", "/recovery-store.js", "/attachments.js", "/message-actions.js",
  "/events-worker.js", "/channels.js", "/session-tools.js", "/session-menu.js"
];
const recoveryPaths = new Set(["/cache-recovery.html", "/cache-recovery.js"]);
const metadataPath = "/sw.js?__codex_elves_cache_metadata=manifest";
const readyPath = "/sw.js?__codex_elves_cache_metadata=ready";
const expectedVersion = new URL(self.location.href).searchParams.get("resources");
const currentCacheName = `${cachePrefix}${expectedVersion}`;
const resourceRequests = new Map();
const downloadRequests = new Map();
const recoveryNotices = new Map();
const clientActivities = new Map();
const optionalRetryAt = new Map();
const preparedOptionalPaths = new Set();
const downloadQueue = [];
let downloadsInFlight = 0;
let backgroundResumeTimer = null;
let foregroundQuietUntil = 0;
let installedManifest = null;
let storedManifestPromise = null;
let protectedPreviousCache = null;
let requiredWarmupPromise = null;
let optionalWarmupPromise = null;
let optionalPreparationAuthorized = false;

async function withDeadline(task, timeoutMs, description, interruptSignal = null) {
  const controller = new AbortController();
  let timer;
  let interrupt;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      controller.abort();
      reject(new Error(`${description}超时，请稍后重试`));
    }, timeoutMs);
    interrupt = () => {
      controller.abort();
      reject(interruptSignal.reason);
    };
    if (interruptSignal?.aborted) interrupt();
    else interruptSignal?.addEventListener("abort", interrupt, { once: true });
  });
  try {
    return await Promise.race([task(controller.signal), timeout]);
  } finally {
    clearTimeout(timer);
    interruptSignal?.removeEventListener("abort", interrupt);
  }
}

function hex(bytes) {
  return [...new Uint8Array(bytes)].map(value => value.toString(16).padStart(2, "0")).join("");
}

async function sha256(bytes) {
  return hex(await crypto.subtle.digest("SHA-256", bytes));
}

function appendUint64(parts, value) {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, BigInt(value));
  parts.push(bytes);
}

function appendField(parts, value) {
  const bytes = new TextEncoder().encode(value);
  appendUint64(parts, bytes.byteLength);
  parts.push(bytes);
}

async function manifestVersion(files) {
  const parts = [];
  for (const file of files) {
    appendField(parts, file.path);
    appendField(parts, file.contentType);
    appendUint64(parts, file.size);
    appendField(parts, file.sha256);
  }
  const length = parts.reduce((total, part) => total + part.byteLength, 0);
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    bytes.set(part, offset);
    offset += part.byteLength;
  }
  return sha256(bytes);
}

function validateResourcePath(path) {
  if (path === "/") return true;
  if (!resourcePathPattern.test(path) || path.includes("\\") || path === "/api" || path.startsWith("/api/")
      || path === "/native.js" || path === manifestPath || recoveryPaths.has(path))
    return false;
  const url = new URL(path, self.location.origin);
  return url.origin === self.location.origin && url.pathname === path && !url.search && !url.hash;
}

async function validateManifest(manifest) {
  if (manifest?.schemaVersion !== schemaVersion
      || !Number.isInteger(manifest.minShellVersion) || manifest.minShellVersion < 1
      || manifest.minShellVersion > shellVersion
      || !hexPattern.test(manifest.version) || manifest.version !== expectedVersion
      || !Array.isArray(manifest.files) || manifest.files.length === 0
      || manifest.files.length > maximumFiles)
    throw new Error("网页资源清单版本不受支持");
  let previousPath = "";
  let totalSize = 0;
  const seen = new Set();
  for (const file of manifest.files) {
    if (!file || typeof file.path !== "string" || !validateResourcePath(file.path)
        || seen.has(file.path) || previousPath && previousPath >= file.path
        || !allowedContentTypes.has(file.contentType)
        || !Number.isSafeInteger(file.size) || file.size < 0 || file.size > maximumFileSize
        || !hexPattern.test(file.sha256))
      throw new Error("网页资源清单包含无效条目");
    seen.add(file.path);
    previousPath = file.path;
    totalSize += file.size;
    if (totalSize > maximumTotalSize) throw new Error("网页资源清单超过大小限制");
  }
  if (!seen.has("/") || await manifestVersion(manifest.files) !== manifest.version)
    throw new Error("网页资源清单摘要无效");
  return manifest;
}

async function readManifest() {
  return withDeadline(async signal => {
    const response = await fetch(manifestPath, {
      cache: "no-store",
      credentials: "same-origin",
      redirect: "error",
      priority: "low",
      signal
    });
    if (!response.ok || response.redirected || new URL(response.url).origin !== self.location.origin)
      throw new Error("无法读取可信网页资源清单");
    return validateManifest(await response.json());
  }, manifestTimeoutMs, "读取网页资源清单");
}

async function storedManifest() {
  if (installedManifest) return installedManifest;
  if (!storedManifestPromise) {
    storedManifestPromise = (async () => {
      const cache = await caches.open(currentCacheName);
      const response = await cache.match(metadataPath);
      const ready = await cache.match(readyPath);
      if (!response || !ready || await ready.text() !== expectedVersion)
        throw new Error("当前网页资源缓存尚未完整就绪");
      installedManifest = await validateManifest(await response.json());
      return installedManifest;
    })().catch(error => {
      storedManifestPromise = null;
      throw error;
    });
  }
  return storedManifestPromise;
}

async function verifiedResponse(file, response) {
  if (!response || response.status !== 200 || response.redirected
      || response.type === "opaque" || response.type === "opaqueredirect"
      || response.url && new URL(response.url).origin !== self.location.origin)
    throw new Error(`无法读取网页资源：${file.path}`);
  const contentType = response.headers.get("content-type")?.toLowerCase();
  if (contentType !== file.contentType.toLowerCase())
    throw new Error(`网页资源类型不匹配：${file.path}`);
  const bytes = await response.arrayBuffer();
  if (bytes.byteLength !== file.size || await sha256(bytes) !== file.sha256)
    throw new Error(`网页资源校验失败：${file.path}`);
  const headers = new Headers();
  for (const name of [
    "cache-control",
    "content-security-policy",
    "referrer-policy",
    "x-content-type-options",
    "x-frame-options"
  ]) {
    const value = response.headers.get(name);
    if (value) headers.set(name, value);
  }
  headers.set("Content-Type", file.contentType);
  headers.set("Content-Length", String(file.size));
  return new Response(bytes, {
    status: 200,
    headers
  });
}

function pausedDownload() {
  const error = new Error("网页资源后台准备已让出网络");
  error.code = "CODEX_ELVES_BACKGROUND_PAUSED";
  return error;
}

function deferredDownload() {
  const error = new Error("网页资源多次让出网络，稍后空闲时再准备");
  error.code = "CODEX_ELVES_BACKGROUND_DEFERRED";
  return error;
}

function expireClientActivities() {
  const now = Date.now();
  for (const [id, activity] of clientActivities) {
    if (activity.expiresAt <= now) clientActivities.delete(id);
  }
}

function backgroundAllowed(priority) {
  expireClientActivities();
  if (Date.now() < foregroundQuietUntil
      || [...clientActivities.values()].some(activity => activity.busy)) return false;
  return priority !== "optional" || optionalPreparationAuthorized
    && (!clientActivities.size
      || [...clientActivities.values()].some(activity => activity.ready && activity.visible));
}

function scheduleDownloadResume() {
  if (backgroundResumeTimer !== null) clearTimeout(backgroundResumeTimer);
  backgroundResumeTimer = null;
  if (!downloadQueue.some(job => job.priority !== "foreground")) return;
  const now = Date.now();
  const times = [foregroundQuietUntil,
    ...[...clientActivities.values()].map(activity => activity.expiresAt)]
    .filter(time => time > now);
  if (!times.length) return;
  backgroundResumeTimer = setTimeout(() => {
    backgroundResumeTimer = null;
    drainDownloads();
  }, Math.min(...times) - now);
}

function pauseBackgroundDownloads() {
  for (const job of downloadRequests.values()) {
    if (job.running && job.priority !== "foreground" && !job.controller.signal.aborted)
      job.controller.abort(pausedDownload());
  }
}

function noteForegroundDemand(preempt = true) {
  foregroundQuietUntil = Date.now() + backgroundQuietMs;
  if (preempt) pauseBackgroundDownloads();
  scheduleDownloadResume();
}

function promoteDownload(path) {
  const job = downloadRequests.get(path);
  if (job && job.priority !== "foreground") job.priority = "foreground";
  noteForegroundDemand();
  drainDownloads();
}

function drainDownloads() {
  while (downloadsInFlight < maximumDownloads && downloadQueue.length) {
    let index = downloadQueue.findIndex(job => job.priority === "foreground");
    if (index < 0) {
      // 后台只占一个槽，且在前台资源或会话请求期间完全让出网络。
      if (downloadsInFlight) break;
      index = downloadQueue.findIndex(job => job.priority === "background" && backgroundAllowed(job.priority));
      if (index < 0)
        index = downloadQueue.findIndex(job => backgroundAllowed(job.priority));
      if (index < 0) break;
    }
    const job = downloadQueue.splice(index, 1)[0];
    job.running = true;
    job.controller = new AbortController();
    downloadsInFlight += 1;
    Promise.resolve().then(() => downloadResource(job)).then(job.resolve, error => {
      if (error?.code === "CODEX_ELVES_BACKGROUND_PAUSED") {
        job.interruptions += 1;
        if (job.priority === "optional" && job.interruptions >= maximumOptionalInterruptions) {
          downloadRequests.delete(job.file.path);
          optionalRetryAt.set(job.file.path, Date.now() + optionalRetryMs);
          job.reject(deferredDownload());
        } else {
          downloadQueue.push(job);
        }
      } else {
        downloadRequests.delete(job.file.path);
        job.reject(error);
      }
    }).finally(() => {
      job.running = false;
      downloadsInFlight -= 1;
      drainDownloads();
    });
  }
  scheduleDownloadResume();
}

async function downloadResource(job) {
  // 无 query 的 URL 可复用首屏已加载的 HTTP 表示；正文仍须完整校验。
  // 暂停不计为失败，也不消耗一次显式重新回源的重试机会。
  const cacheModes = ["force-cache", "no-store"];
  while (job.cacheModeIndex < cacheModes.length) {
    try {
      const response = await withDeadline(async signal => {
        const response = await fetch(new URL(job.file.path, self.location.origin), {
          cache: cacheModes[job.cacheModeIndex],
          credentials: "same-origin",
          redirect: "error",
          priority: job.priority === "foreground" ? "high" : "low",
          signal
        });
        return verifiedResponse(job.file, response);
      }, 30000 + Math.ceil(job.file.size / (32 * 1024)) * 1000,
      `下载网页资源 ${job.file.path} `, job.controller.signal);
      if (job.controller.signal.aborted) throw job.controller.signal.reason;
      downloadRequests.delete(job.file.path);
      return response;
    } catch (error) {
      if (job.controller.signal.aborted) throw job.controller.signal.reason;
      job.cacheModeIndex += 1;
      if (job.cacheModeIndex === cacheModes.length) throw error;
    }
  }
}

function fetchVerifiedResource(file, priority = "foreground") {
  let job = downloadRequests.get(file.path);
  if (!job) {
    job = { file, priority, cacheModeIndex: 0, interruptions: 0, running: false, controller: null };
    job.promise = new Promise((resolve, reject) => {
      job.resolve = resolve;
      job.reject = reject;
    });
    downloadRequests.set(file.path, job);
    downloadQueue.push(job);
  } else if (priority === "foreground") {
    job.priority = "foreground";
  } else if (priority === "background" && job.priority === "optional") {
    job.priority = "background";
  }
  if (priority === "foreground") noteForegroundDemand();
  drainDownloads();
  return job.promise.then(response => response.clone());
}

async function reusableResource(file, cacheNames) {
  for (const name of cacheNames) {
    const cache = await caches.open(name);
    const response = await cache.match(file.path);
    if (!response) continue;
    try {
      return { response: await verifiedResponse(file, response), cacheName: name };
    } catch {
      // 旧格式缓存没有可信元数据；即使响应头相同也只能靠正文校验复用。
    }
  }
  return null;
}

function isResourceCache(name) {
  return name.startsWith(cachePrefix) && hexPattern.test(name.slice(cachePrefix.length));
}

function workerUsesCache(name) {
  return [self.registration.active, self.registration.waiting, self.registration.installing]
    .some(worker => worker
      && `${cachePrefix}${new URL(worker.scriptURL).searchParams.get("resources")}` === name);
}

async function pruneAbandonedCandidates() {
  let retained = false;
  const names = (await caches.keys()).filter(isResourceCache).reverse();
  for (const name of names) {
    if (name === currentCacheName || workerUsesCache(name)) continue;
    const cache = await caches.open(name);
    const ready = await cache.match(readyPath);
    if (ready && await ready.text() === name.slice(cachePrefix.length)) continue;
    // 至多保留最近一个无引用的历史候选；当前同版续试与所有在用版本始终保留。
    if (!retained) {
      retained = true;
    } else if (!workerUsesCache(name)) {
      await caches.delete(name);
    }
  }
}

async function notifyClients(status, message, path = null) {
  try {
    const clients = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    for (const client of clients) client.postMessage({
      type: "codex-elves-pwa-state",
      state: {
        status,
        version: expectedVersion,
        message,
        retryable: status === "failed" || status === "recovery-required",
        path
      }
    });
  } catch {
    // 状态通知失败不影响完整性校验与缓存事务。
  }
}

async function notifyRecovery(path, error) {
  const now = Date.now();
  if (now - (recoveryNotices.get(path) || 0) < 30000) return;
  recoveryNotices.set(path, now);
  await notifyClients("recovery-required",
    `当前版本资源暂不可用，请保留输入内容后重试或重新打开页面。${error.message}`, path);
}

function requiredResources(manifest) {
  const rank = path => {
    const index = criticalResourceOrder.indexOf(path);
    return index < 0 ? criticalResourceOrder.length : index;
  };
  return manifest.files.filter(file => !deferredResourcePaths.has(file.path))
    .sort((first, second) => rank(first.path) - rank(second.path));
}

async function installResources() {
  if (!expectedVersion || !hexPattern.test(expectedVersion))
    throw new Error("缺少网页资源版本");
  await notifyClients("installing", "正在后台准备离线网页资源");
  try {
    const manifest = await readManifest();
    const cache = await caches.open(currentCacheName);
    await pruneAbandonedCandidates();
    const previousCaches = (await caches.keys()).filter(isResourceCache).reverse();
    protectedPreviousCache = previousCaches.find(name => name !== currentCacheName) || null;
    const existingMetadata = await cache.match(metadataPath);
    let metadataValid = false;
    if (existingMetadata) {
      try {
        await validateManifest(await existingMetadata.json());
        metadataValid = true;
      } catch {
        // 历史格式或中断写入的元数据会在安装完成前重新保存。
      }
    }
    if (!metadataValid) await cache.put(metadataPath, new Response(JSON.stringify(manifest), {
      headers: { "Content-Type": "application/json; charset=utf-8" }
    }));
    const files = requiredResources(manifest);
    let index = 0;
    let failed = false;
    async function prepareNext() {
      while (!failed && index < files.length) {
        const file = files[index++];
        try {
          await cachedManifestResource(file, "background");
          const persisted = await cache.match(file.path);
          if (!persisted) throw new Error(`当前版本资源无法保存：${file.path}`);
          await verifiedResponse(file, persisted);
        } catch (error) {
          failed = true;
          throw error;
        }
      }
    }
    // 等全部任务落定后才报告安装失败，避免失败后的并发任务继续写入。
    const results = await Promise.allSettled(Array.from({ length: maximumDownloads }, prepareNext));
    const failure = results.find(result => result.status === "rejected");
    if (failure) throw failure.reason;
    const ready = await cache.match(readyPath);
    if (!ready || await ready.text() !== expectedVersion)
      await cache.put(readyPath, new Response(expectedVersion));
    installedManifest = manifest;
  } catch (error) {
    // 保留逐项验证过的候选，供同版重试；不修改任何旧版或删除同版活动缓存。
    await notifyClients("failed", `离线网页资源准备失败：${error.message}`);
    throw error;
  }
}

self.addEventListener("install", event => {
  event.waitUntil(installResources());
});

self.addEventListener("activate", event => {
  event.waitUntil((async () => {
    await storedManifest();
    // 新 worker 不强行接管旧页面。清理仅针对已完成版本，并保留上一套缓存。
    // 历史未完成候选仅保留最近一个，并始终保护在用版本。
    await pruneAbandonedCandidates();
    const names = (await caches.keys()).filter(isResourceCache).reverse();
    const previous = protectedPreviousCache || names.find(name => name !== currentCacheName);
    for (const name of names) {
      if (name === currentCacheName || name === previous) continue;
      if (workerUsesCache(name)) continue;
      const cache = await caches.open(name);
      const ready = await cache.match(readyPath);
      if (ready && await ready.text() === name.slice(cachePrefix.length) && !workerUsesCache(name))
        await caches.delete(name);
    }
    await notifyClients("ready", "");
  })());
});

async function cachedResource(request, url) {
  let manifest;
  try {
    manifest = await storedManifest();
  } catch (error) {
    await notifyRecovery(url.pathname, error);
    return unavailableResource();
  }
  const file = manifest.files.find(item => item.path === url.pathname);
  // 清单之外的请求保持网络透传，不读取或写入任何缓存。
  if (!file) return fetch(request);
  try {
    return await cachedManifestResource(file);
  } catch (error) {
    await notifyRecovery(file.path, error);
    return unavailableResource();
  }
}

async function cachedManifestResource(file, priority = "foreground") {
  let resource = resourceRequests.get(file.path);
  if (resource) {
    if (priority === "foreground") resource.priority = "foreground";
    else if (priority === "background" && resource.priority === "optional") resource.priority = "background";
  } else {
    resource = { priority };
    resourceRequests.set(file.path, resource);
    resource.promise = (async () => {
      const cache = await caches.open(currentCacheName);
      const response = await cache.match(file.path);
      if (response) {
        try {
          return await verifiedResponse(file, response);
        } catch {
          // 损坏项走相同固定清单回填，不能退回当前线上任意版本。
        }
      }
      const names = (await caches.keys()).filter(isResourceCache).reverse();
      const reused = await reusableResource(file, names);
      const recovered = reused?.response || await fetchVerifiedResource(file, resource.priority);
      try {
        await cache.put(file.path, recovered.clone());
        if (deferredResourcePaths.has(file.path)) preparedOptionalPaths.add(file.path);
      } catch (error) {
        if (resource.priority === "foreground")
          await notifyClients("failed", `资源已校验，但无法保存离线缓存：${error.message}`, file.path);
      }
      return recovered;
    })().finally(() => resourceRequests.delete(file.path));
  }
  if (priority === "foreground") promoteDownload(file.path);
  try {
    return (await resource.promise).clone();
  } catch (error) {
    if (priority !== "foreground") throw error;
    await notifyRecovery(file.path, error);
    return unavailableResource();
  }
}

function unavailableResource() {
  return new Response("当前版本网页资源暂不可用。请保留输入内容后重试或重新打开页面。", {
    status: 503,
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
      "Cache-Control": "no-store",
      "X-Codex-Elves-Cache": "recovery-required",
      "Retry-After": "30"
    }
  });
}

async function warmupRequiredResources() {
  const manifest = await storedManifest();
  const cache = await caches.open(currentCacheName);
  const files = requiredResources(manifest);
  let index = 0;
  let failed = false;
  async function prepareNext() {
    while (!failed && index < files.length) {
      const file = files[index++];
      try {
        const response = await cachedManifestResource(file);
        if (response.status !== 200) throw new Error(`当前版本资源无法补齐：${file.path}`);
        const persisted = await cache.match(file.path);
        if (!persisted) throw new Error(`当前版本资源无法保存：${file.path}`);
        await verifiedResponse(file, persisted);
      } catch (error) {
        failed = true;
        throw error;
      }
    }
  }
  const results = await Promise.allSettled(Array.from({ length: maximumDownloads }, prepareNext));
  const failure = results.find(result => result.status === "rejected");
  if (failure) throw failure.reason;
}

async function prepareOptionalResources() {
  const manifest = await storedManifest();
  const files = manifest.files.filter(file => deferredResourcePaths.has(file.path))
    .sort((first, second) => first.size - second.size);
  for (const file of files) {
    if (preparedOptionalPaths.has(file.path) || Date.now() < (optionalRetryAt.get(file.path) || 0))
      continue;
    try {
      await cachedManifestResource(file, "optional");
      const cache = await caches.open(currentCacheName);
      const persisted = await cache.match(file.path);
      if (!persisted) throw new Error(`可选资源暂未保存：${file.path}`);
      await verifiedResponse(file, persisted);
      preparedOptionalPaths.add(file.path);
      optionalRetryAt.delete(file.path);
    } catch {
      // 可选项按下一次空闲机会有限续试，不阻止基础页面就绪或触发恢复页。
      optionalRetryAt.set(file.path, Date.now() + optionalRetryMs);
    }
  }
}

async function updateClientActivity(event) {
  if (event.data.version !== expectedVersion || !event.source?.id
      || event.source.type !== "window") return;
  let source;
  try {
    source = new URL(event.source.url);
  } catch {
    return;
  }
  if (source.origin !== self.location.origin) return;
  const client = await self.clients.get(event.source.id);
  if (!client || client.type !== "window" || new URL(client.url).origin !== self.location.origin) return;
  const activity = {
    busy: event.data.busy === true,
    // 没有 preempt 字段的旧页面继续保留原有主动读取优先行为。
    preempt: event.data.preempt !== false,
    ready: event.data.ready === true,
    visible: event.data.visible === true,
    expiresAt: Date.now() + activityLeaseMs
  };
  clientActivities.set(client.id, activity);
  if (activity.busy && activity.preempt) pauseBackgroundDownloads();
  else foregroundQuietUntil = Math.max(foregroundQuietUntil, Date.now() + backgroundQuietMs);
  if (activity.ready && activity.visible) optionalPreparationAuthorized = true;
  drainDownloads();
  if (activity.busy || !activity.ready || !activity.visible) return;
  try {
    await storedManifest();
  } catch {
    // 安装中的候选尚未写入核心就绪标记，等待下一次状态通知或续租。
    return;
  }
  if (!optionalWarmupPromise)
    optionalWarmupPromise = prepareOptionalResources()
      .catch(() => {}).finally(() => { optionalWarmupPromise = null; });
  await optionalWarmupPromise;
}

self.addEventListener("message", event => {
  if (event.data?.type === "codex-elves-resource-activity") {
    event.waitUntil(updateClientActivity(event));
    return;
  }
  if (event.data?.type !== "codex-elves-cache-warmup" || !event.ports[0]) return;
  const port = event.ports[0];
  const reply = (ok, message) => port.postMessage({
    type: "codex-elves-cache-warmup-result",
    version: expectedVersion,
    ok,
    message
  });
  if (event.data.version !== expectedVersion) {
    reply(false, "网页资源版本已变化，请重新检查");
    return;
  }
  if (!requiredWarmupPromise) {
    requiredWarmupPromise = warmupRequiredResources()
      .finally(() => { requiredWarmupPromise = null; });
  }
  event.waitUntil(requiredWarmupPromise.then(
    () => reply(true, "当前版本必需网页资源已校验并补齐"),
    error => reply(false, error.message)
  ));
});

self.addEventListener("fetch", event => {
  const { request } = event;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname === "/api" || url.pathname.startsWith("/api/")) {
    // API 本身不能区分周期轮询和主动恢复。这里只延后新任务，
    // 正在下载的任务是否抢占由页面携带的 busy/preempt 生命周期决定。
    noteForegroundDemand(false);
    return;
  }
  if (request.method !== "GET" || recoveryPaths.has(url.pathname)
      || !validateResourcePath(url.pathname)) return;
  if (!expectedVersion || !hexPattern.test(expectedVersion)) return;
  if (installedManifest && !installedManifest.files.some(file => file.path === url.pathname)) return;
  // 模块失败重试可带有限 query；缓存键及摘要始终绑定当前 worker 清单中的固定路径。
  noteForegroundDemand();
  const response = cachedResource(request, url);
  event.respondWith(response);
  event.waitUntil(response.then(() => {}));
});
