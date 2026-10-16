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
      || path === "/native.js" || path === manifestPath)
    return false;
  const url = new URL(path, self.location.origin);
  return url.origin === self.location.origin && url.pathname === path && !url.search && !url.hash;
}

async function readManifest(expectedVersion) {
  const response = await fetch(manifestPath, {
    cache: "no-store",
    credentials: "same-origin",
    redirect: "error"
  });
  if (!response.ok || response.redirected || new URL(response.url).origin !== self.location.origin)
    throw new Error("无法读取可信网页资源清单");
  const manifest = await response.json();
  if (manifest?.schemaVersion !== schemaVersion
      || !Number.isInteger(manifest.minShellVersion) || manifest.minShellVersion > shellVersion
      || !hexPattern.test(manifest.version) || manifest.version !== expectedVersion
      || !Array.isArray(manifest.files) || manifest.files.length === 0)
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

async function fetchVerifiedResource(file, version) {
  const url = new URL(file.path, self.location.origin);
  url.searchParams.set("__codex_elves_version", version);
  const response = await fetch(url, {
    cache: "no-store",
    credentials: "same-origin",
    redirect: "error"
  });
  if (!response.ok || response.redirected || response.type !== "basic"
      || new URL(response.url).origin !== self.location.origin)
    throw new Error(`无法下载网页资源：${file.path}`);
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

async function installResources() {
  const expectedVersion = new URL(self.location.href).searchParams.get("resources");
  if (!expectedVersion || !hexPattern.test(expectedVersion))
    throw new Error("缺少网页资源版本");
  const manifest = await readManifest(expectedVersion);
  const cacheName = `${cachePrefix}${manifest.version}`;
  const cache = await caches.open(cacheName);
  try {
    for (const file of manifest.files) {
      const response = await fetchVerifiedResource(file, manifest.version);
      await cache.put(new Request(file.path, { method: "GET" }), response);
    }
  } catch (error) {
    await caches.delete(cacheName);
    throw error;
  }
}

self.addEventListener("install", event => {
  event.waitUntil(installResources());
});

self.addEventListener("activate", event => {
  const expectedVersion = new URL(self.location.href).searchParams.get("resources");
  const currentCache = `${cachePrefix}${expectedVersion}`;
  event.waitUntil(caches.keys().then(names => Promise.all(
    names.filter(name => name.startsWith(cachePrefix) && name !== currentCache)
      .map(name => caches.delete(name))
  )));
});

self.addEventListener("fetch", event => {
  const { request } = event;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin || url.search || !validateResourcePath(url.pathname)) return;
  const expectedVersion = new URL(self.location.href).searchParams.get("resources");
  if (!expectedVersion || !hexPattern.test(expectedVersion)) return;
  event.respondWith(
    caches.open(`${cachePrefix}${expectedVersion}`)
      .then(cache => cache.match(url.pathname))
      .then(response => response || fetch(request))
  );
});
