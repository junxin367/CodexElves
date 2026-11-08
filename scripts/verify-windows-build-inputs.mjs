import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const repositoryRoot = resolve(process.argv[2] || resolve(import.meta.dirname, ".."));
const sourceOnly = process.argv[3] === "--source-only";
const binaryDirectory = resolve(sourceOnly ? resolve(repositoryRoot, "target/release-fast")
  : process.argv[3] || resolve(repositoryRoot, "target/release-fast"));
const assetSource = readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/assets.rs"), "utf8");
const buildId = assetSource.match(/pub const DIAGNOSTIC_BUILD_ID:\s*&str\s*=\s*"([^"]+)"/)?.[1];
if (!buildId) throw new Error("无法读取当前注入构建标识。");

function verifyRemoteRuntimeSources({ backend, service, workspace, app, native }) {
  const failures = [];
  // 用户要求服务模式不依赖 CDP，且切回会话恢复压缩记录；保留协议 5，桥接 135 同步压缩生命周期字段。
  const protocol = Number(service.match(/const PROTOCOL_VERSION:\s*u32\s*=\s*(\d+);/)?.[1]);
  const bridge = Number(native.match(/window\.__codexElvesRemoteAccess\?\.version === (\d+)/)?.[1]);
  const readStart = backend.indexOf("async fn read(");
  const readEnd = backend.indexOf('return Err(ApiError::new(404, "不支持的读取操作"))', readStart);
  if (!Number.isSafeInteger(protocol) || protocol < 5)
    failures.push("后台协议不能低于 5，否则可能继续复用缺少服务隔离的旧后台");
  if (readStart < 0 || readEnd <= readStart || !backend.slice(readStart, readEnd).includes('| "runtime"')
      || !backend.includes('if action == "runtime"') || !backend.includes('if action == "setRuntime"'))
    failures.push("后台缺少运行模式读取或切换接口");
  if (!app.includes('api("/api/read/runtime"') || !app.includes('mutate("setRuntime"'))
    failures.push("远程页面缺少运行模式读取或切换入口");
  if (!backend.includes('.route("/api/workspace/context", post(workspace::context))')
      || !workspace.includes("async fn branch_metadata(")
      || !app.includes('api("/api/workspace/context"'))
    failures.push("后台或新建页面缺少真实分支信息读取");
  if (!Number.isSafeInteger(bridge) || bridge < 135 || !native.includes(`version: ${bridge}, call`)
      || !native.includes("const needsSideContext =") || !native.includes("async function compactThreadContext("))
    failures.push("原生桥缺少已交付的侧边会话恢复或上下文压缩恢复");
  if (failures.length) throw new Error(`Windows 构建基线不完整：\n${failures.join("\n")}`);
  return { protocol, bridge };
}

const runtimeContract = verifyRemoteRuntimeSources({
  backend: readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/remote_access.rs"), "utf8"),
  service: readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/remote_access/service.rs"), "utf8"),
  workspace: readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/remote_access/workspace.rs"), "utf8"),
  app: readFileSync(resolve(repositoryRoot, "assets/remote-access/app.js"), "utf8"),
  native: readFileSync(resolve(repositoryRoot, "assets/remote-access/native.js"), "utf8"),
});

// 用户要求更新后远程功能可用；共享 Cargo 产物可能被另一工作树覆盖，
// 打包前须按完整资源字节核对，不能仅凭相同的产品版本号判断代码一致。
const rendererSources = [
  "assets/inject/renderer-features.js",
  "assets/inject/composer-fast.js",
  "assets/inject/statsig-cache.js",
];
const programs = [
  ["codex-elves.exe", rendererSources],
  ["codex-elves-manager.exe", [
    ...rendererSources,
    "assets/remote-access/app.js",
    "assets/remote-access/channels.js",
    "assets/remote-access/native.js",
    "assets/remote-access/session-files.js",
    "assets/remote-access/session-menu.js",
    "assets/remote-access/session-tools.js",
    "assets/remote-access/style.css",
  ]],
];
const failures = [];
for (const [name, sources] of sourceOnly ? [] : programs) {
  const binary = readFileSync(resolve(binaryDirectory, name));
  if (!binary.includes(Buffer.from(buildId))) failures.push(`${name} 缺少当前构建标识`);
  for (const source of sources) {
    if (!binary.includes(readFileSync(resolve(repositoryRoot, source))))
      failures.push(`${name} 内嵌的 ${source} 与当前工作区不一致`);
  }
}
if (failures.length) {
  console.error(failures.join("\n"));
  process.exitCode = 1;
} else {
  console.log(sourceOnly
    ? `Windows 源码基线核对通过：后台协议 ${runtimeContract.protocol}、原生桥 ${runtimeContract.bridge}，分支信息、运行模式和压缩恢复完整。`
    : `Windows 构建输入核对通过：${buildId}，远程页面、原生桥和桌面注入均与当前源码一致。`);
}
