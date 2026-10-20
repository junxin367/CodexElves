import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { STATUS_CODES, createServer, request } from "node:http";
import { resolve } from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({
  options: {
    port: { type: "string", default: "1432" },
    upstream: { type: "string", default: "http://127.0.0.1:45222" },
    "events-transport": { type: "string", default: "sse" },
    help: { type: "boolean", short: "h" }
  }
});
if (values.help) {
  console.log("用法：node scripts/dev-remote-access.mjs [--port 1432] [--upstream http://127.0.0.1:45222] [--events-transport sse|websocket]");
  process.exit(0);
}
const port = Number(values.port);
if (!/^\d+$/.test(values.port) || !Number.isInteger(port) || port < 1 || port > 65535)
  throw new Error("调试端口必须是 1–65535 之间的整数");
const upstream = new URL(values.upstream);
if (upstream.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(upstream.hostname)
    || upstream.username || upstream.password || upstream.pathname !== "/" || upstream.search || upstream.hash)
  throw new Error("后端地址必须是本机 HTTP 服务的根地址");
if (Number(upstream.port || 80) === port)
  throw new Error("调试端口不能与后端端口相同");
const eventsTransport = values["events-transport"];
if (!["sse", "websocket"].includes(eventsTransport))
  throw new Error("事件传输方式必须是 sse 或 websocket");

const assets = resolve(import.meta.dirname, "../assets/remote-access");
const hosts = new Set([`localhost:${port}`, `127.0.0.1:${port}`]);
const cookieName = "codex_elves_device";
const devCookieName = "codex_elves_dev_device";
const sseEventsClient = `export function sharedEventsAvailable() {
  return false;
}

export function createRemoteEventStream() {
  const stream = new EventSource("/api/events");
  stream.transport = "fallback";
  return stream;
}
`;
const files = new Map([
  ["/", ["index.html", "text/html; charset=utf-8"]],
  ["/app.js", ["app.js", "application/javascript; charset=utf-8"]],
  ["/attachments.js", ["attachments.js", "application/javascript; charset=utf-8"]],
  ["/recovery-store.js", ["recovery-store.js", "application/javascript; charset=utf-8"]],
  ["/events-client.js", ["events-client.js", "application/javascript; charset=utf-8"]],
  ["/events-worker.js", ["events-worker.js", "application/javascript; charset=utf-8"]],
  ["/channels.js", ["channels.js", "application/javascript; charset=utf-8"]],
  ["/markdown.js", ["markdown.js", "application/javascript; charset=utf-8"]],
  ["/markdown-renderer.js", ["markdown-renderer.js", "application/javascript; charset=utf-8"]],
  ["/message-actions.js", ["message-actions.js", "application/javascript; charset=utf-8"]],
  ["/platform.js", ["platform.js", "application/javascript; charset=utf-8"]],
  ["/pwa.js", ["pwa.js", "application/javascript; charset=utf-8"]],
  ["/sw.js", ["sw.js", "application/javascript; charset=utf-8"]],
  ["/vendor/marked.esm.js", ["vendor/marked.esm.js", "application/javascript; charset=utf-8"]],
  ["/vendor/dompurify.esm.js", ["vendor/dompurify.esm.js", "application/javascript; charset=utf-8"]],
  ["/vendor/mermaid.esm.js", ["vendor/mermaid.esm.js", "application/javascript; charset=utf-8"]],
  ["/style.css", ["style.css", "text/css; charset=utf-8"]],
  ["/manifest.webmanifest", ["manifest.webmanifest", "application/manifest+json; charset=utf-8"]],
  ["/icons/icon-192.png", ["icons/icon-192.png", "image/png"]],
  ["/icons/icon-512.png", ["icons/icon-512.png", "image/png"]],
  ["/icons/apple-touch-icon.png", ["icons/apple-touch-icon.png", "image/png"]]
]);
const recoveryFiles = new Map([
  ["/cache-recovery.html", ["cache-recovery.html", "text/html; charset=utf-8"]],
  ["/cache-recovery.js", ["cache-recovery.js", "application/javascript; charset=utf-8"]]
]);
const requestHeaders = [
  "accept", "accept-encoding", "accept-language", "content-type", "content-length",
  "user-agent", "idempotency-key", "if-none-match", "if-modified-since", "range",
  "if-range", "last-event-id"
];
const websocketRequestHeaders = [
  "accept", "accept-encoding", "accept-language", "cache-control", "pragma",
  "user-agent", "sec-websocket-key", "sec-websocket-version",
  "sec-websocket-extensions", "sec-websocket-protocol"
];
const hopHeaders = new Set([
  "connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
  "te", "trailer", "transfer-encoding", "upgrade"
]);
const securityHeaders = {
  "cache-control": "no-store",
  "x-content-type-options": "nosniff",
  "referrer-policy": "no-referrer",
  "x-frame-options": "DENY",
  "content-security-policy": "default-src 'self'; script-src 'self'; worker-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
};
function sha256(body) {
  return createHash("sha256").update(body).digest("hex");
}
function updateVersionField(hash, value) {
  const bytes = Buffer.from(value);
  const length = Buffer.alloc(8);
  length.writeBigUInt64BE(BigInt(bytes.length));
  hash.update(length);
  hash.update(bytes);
}
async function staticBody(path, file) {
  let body = eventsTransport === "sse" && path === "/events-client.js"
    ? Buffer.from(sseEventsClient)
    : await readFile(resolve(assets, file[0]));
  if (path === "/") body = Buffer.from(body.toString("utf8")
    .replace("<title>CodexElves · 远程会话</title>", "<title>CodexElves · 本机调试</title>"));
  return body;
}
async function appResourcesBody() {
  const resourceFiles = [];
  for (const [path, file] of [...files].sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0)) {
    const body = await staticBody(path, file);
    resourceFiles.push({
      path,
      contentType: file[1],
      size: body.length,
      sha256: sha256(body)
    });
  }
  const version = createHash("sha256");
  for (const file of resourceFiles) {
    updateVersionField(version, file.path);
    updateVersionField(version, file.contentType);
    const size = Buffer.alloc(8);
    size.writeBigUInt64BE(BigInt(file.size));
    version.update(size);
    updateVersionField(version, file.sha256);
  }
  return Buffer.from(JSON.stringify({
    schemaVersion: 1,
    minShellVersion: 1,
    version: version.digest("hex"),
    files: resourceFiles
  }));
}
function fail(res, status, message) {
  if (res.destroyed) return;
  if (res.headersSent) { res.destroy(); return; }
  res.writeHead(status, { ...securityHeaders, "content-type": "application/json; charset=utf-8" });
  res.end(JSON.stringify({ ok: false, message }));
}
function rejectUpgrade(socket, status, message) {
  if (socket.destroyed) return;
  socket.on("error", () => {});
  const body = Buffer.from(JSON.stringify({ ok: false, message }));
  const headers = {
    ...securityHeaders,
    connection: "close",
    "content-type": "application/json; charset=utf-8",
    "content-length": body.length
  };
  const lines = [`HTTP/1.1 ${status} ${STATUS_CODES[status] || "Error"}`];
  for (const [name, value] of Object.entries(headers)) lines.push(`${name}: ${value}`);
  socket.write(`${lines.join("\r\n")}\r\n\r\n`);
  socket.end(body);
}
function allowedApi(method, path) {
  return method === "GET" && (["/api/session", "/api/events"].includes(path)
    || /^\/api\/(?:receipts|images)\/[a-zA-Z0-9_-]+$/.test(path))
    || method === "POST" && (["/api/connect", "/api/logout"].includes(path)
      || /^\/api\/(?:read|action)\/[a-zA-Z][a-zA-Z0-9_-]*$/.test(path));
}
function mappedCookie(req) {
  const token = String(req.headers.cookie || "").split(";").map(value => value.trim())
    .find(value => value.startsWith(`${devCookieName}=`));
  return token ? `${cookieName}=${token.slice(devCookieName.length + 1)}` : null;
}
function upstreamHeaders(req, names) {
  const headers = { host: upstream.host };
  for (const name of names) if (req.headers[name] != null) headers[name] = req.headers[name];
  const cookie = mappedCookie(req);
  if (cookie) headers.cookie = cookie;
  if (req.headers.origin != null) headers.origin = upstream.origin;
  if (req.headers["sec-fetch-site"] != null) headers["sec-fetch-site"] = "same-origin";
  return headers;
}
function mappedSetCookies(cookies) {
  return cookies?.map(value => value.startsWith(`${cookieName}=`)
    ? `${devCookieName}=${value.slice(cookieName.length + 1)}`
    : value);
}
function proxy(req, res, url) {
  const headers = upstreamHeaders(req, requestHeaders);
  const target = new URL(`${url.pathname}${url.search}`, upstream);
  const pending = request(target, { method: req.method, headers }, response => {
    const connectionHeaders = new Set(String(response.headers.connection || "").toLowerCase().split(",").map(value => value.trim()));
    const outgoing = {};
    for (const [name, value] of Object.entries(response.headers))
      if (value != null && !hopHeaders.has(name) && !connectionHeaders.has(name)) outgoing[name] = value;
    if (response.headers["set-cookie"]) outgoing["set-cookie"] = mappedSetCookies(response.headers["set-cookie"]);
    if (res.destroyed) { response.destroy(); return; }
    res.writeHead(response.statusCode || 502, outgoing);
    res.flushHeaders();
    response.on("error", () => res.destroy());
    response.pipe(res);
  });
  pending.on("error", () => fail(res, 502, "无法连接本机远程会话后端，请确认现有 CodexElves 服务仍在运行"));
  req.on("aborted", () => pending.destroy());
  res.on("close", () => pending.destroy());
  req.pipe(pending);
}
function validateRequest(req) {
  const host = req.headers.host;
  const origin = req.headers.origin;
  if (!hosts.has(host) || origin != null && origin !== `http://${host}`
      || req.method === "POST" && origin == null)
    return { error: [403, "只允许从本机调试地址操作"] };
  if (!req.url?.startsWith("/") || req.url.startsWith("//"))
    return { error: [400, "请求地址无效"] };
  let url;
  try { url = new URL(req.url, `http://${host}`); }
  catch { return { error: [400, "请求地址无效"] }; }
  const navigation = ["GET", "HEAD"].includes(req.method) && url.pathname === "/" && origin == null
    && req.headers["sec-fetch-mode"] === "navigate" && req.headers["sec-fetch-dest"] === "document";
  if (req.headers["sec-fetch-site"] === "cross-site" && !navigation)
    return { error: [403, "不接受跨站请求"] };
  return { url };
}
function writeWebSocketUpgrade(client, response) {
  const lines = ["HTTP/1.1 101 Switching Protocols"];
  for (const name of [
    "upgrade", "connection", "sec-websocket-accept", "sec-websocket-protocol",
    "sec-websocket-extensions"
  ]) {
    const value = response.headers[name];
    if (value != null) lines.push(`${name}: ${value}`);
  }
  for (const value of mappedSetCookies(response.headers["set-cookie"]) || [])
    lines.push(`set-cookie: ${value}`);
  client.write(`${lines.join("\r\n")}\r\n\r\n`);
}
function proxyWebSocket(req, client, head, url) {
  const headers = {
    ...upstreamHeaders(req, websocketRequestHeaders),
    connection: "Upgrade",
    upgrade: "websocket"
  };
  const target = new URL(`${url.pathname}${url.search}`, upstream);
  let upstreamSocket = null;
  let upgraded = false;
  const pending = request(target, { method: "GET", headers });
  const closeUpstream = () => {
    if (upstreamSocket) upstreamSocket.destroy();
    else pending.destroy();
  };
  client.on("error", closeUpstream);
  client.on("end", closeUpstream);
  client.on("close", closeUpstream);
  pending.on("upgrade", (response, socket, upstreamHead) => {
    upgraded = true;
    upstreamSocket = socket;
    if (client.destroyed) { socket.destroy(); return; }
    response.on("error", () => {
      socket.destroy();
      client.destroy();
    });
    writeWebSocketUpgrade(client, response);
    if (head.length) socket.write(head);
    if (upstreamHead.length) client.write(upstreamHead);
    socket.on("error", () => client.destroy());
    socket.on("close", () => client.destroy());
    client.pipe(socket);
    socket.pipe(client);
  });
  pending.on("response", response => {
    response.on("error", () => {});
    response.resume();
    if (!upgraded)
      rejectUpgrade(client, response.statusCode || 502, "WebSocket 事件后端不可用");
  });
  pending.on("error", () => {
    if (!upgraded) rejectUpgrade(client, 502, "无法连接本机远程会话后端，请确认现有 CodexElves 服务仍在运行");
    else client.destroy();
  });
  pending.end();
}
const server = createServer(async (req, res) => {
  const validated = validateRequest(req);
  if (validated.error) { fail(res, ...validated.error); return; }
  const { url } = validated;
  if (allowedApi(req.method, url.pathname)) { proxy(req, res, url); return; }
  if (url.pathname === "/favicon.ico" && ["GET", "HEAD"].includes(req.method)) {
    res.writeHead(204, securityHeaders); res.end(); return;
  }
  if (!["GET", "HEAD"].includes(req.method)) { fail(res, 404, "调试入口不存在"); return; }
  const file = files.get(url.pathname) || recoveryFiles.get(url.pathname);
  if (!file && url.pathname !== "/app-resources.json") { fail(res, 404, "调试入口不存在"); return; }
  try {
    const body = url.pathname === "/app-resources.json"
      ? await appResourcesBody()
      : await staticBody(url.pathname, file);
    const contentType = url.pathname === "/app-resources.json"
      ? "application/json; charset=utf-8"
      : file[1];
    res.writeHead(200, { ...securityHeaders, "content-type": contentType, "content-length": body.length });
    res.end(req.method === "HEAD" ? undefined : body);
  } catch {
    fail(res, 500, "无法读取网页源码，请确认在完整的 CodexElves 工作区中运行");
  }
});
server.on("upgrade", (req, socket, head) => {
  const validated = validateRequest(req);
  if (validated.error) { rejectUpgrade(socket, ...validated.error); return; }
  if (eventsTransport !== "websocket" || req.method !== "GET"
      || validated.url.pathname !== "/api/events/ws") {
    rejectUpgrade(socket, 404, "调试入口不存在"); return;
  }
  proxyWebSocket(req, socket, head, validated.url);
});
server.on("error", error => {
  console.error(error.code === "EADDRINUSE" ? `端口 ${port} 已被占用，请用 --port 指定空闲端口`
    : `调试服务启动失败：${error.message}`);
  process.exitCode = 1;
});
server.listen(port, "127.0.0.1", () => {
  console.log(`CodexElves 本机网页调试：http://localhost:${port}/`);
  console.log(`真实会话后端：${upstream.origin}`);
  console.log(`网页源码目录：${assets}`);
  console.log(`事件传输：${eventsTransport === "sse" ? "SSE（兼容旧后端）" : "WebSocket（后端需支持 /api/events/ws）"}`);
  console.log("仅监听本机，沿用访问密钥登录；修改网页后刷新即可。按 Ctrl+C 停止调试入口。");
});
