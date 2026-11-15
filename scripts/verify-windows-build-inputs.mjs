import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { listRemoteWebResources } from "./remote-access-resources.mjs";

const repositoryRoot = resolve(process.argv[2] || resolve(import.meta.dirname, ".."));
const sourceOnly = process.argv[3] === "--source-only";
const binaryDirectory = resolve(sourceOnly ? resolve(repositoryRoot, "target/release-fast")
  : process.argv[3] || resolve(repositoryRoot, "target/release-fast"));
const assetSource = readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/assets.rs"), "utf8");
const buildId = assetSource.match(/pub const DIAGNOSTIC_BUILD_ID:\s*&str\s*=\s*"([^"]+)"/)?.[1];
if (!buildId) throw new Error("无法读取当前注入构建标识。");

const remoteAccessDirectory = resolve(repositoryRoot, "assets/remote-access");
// 网页资源清单与服务端、Android、Windows 客户端内嵌规则一致，打包核对覆盖全部资源而非手工挑选的部分文件。
const remoteWebResources = await listRemoteWebResources(remoteAccessDirectory);
const remoteWebSources = remoteWebResources.map(resource => `assets/remote-access/${resource.file}`);

function verifyRemoteRuntimeSources({ backend, service, workspace, app }) {
  const failures = [];
  // 用户要求远程 Codex 仅保留服务模式；协议 6 不再提供模式切换或桌面 CDP 后端。
  const protocol = Number(service.match(/const PROTOCOL_VERSION:\s*u32\s*=\s*(\d+);/)?.[1]);
  if (!Number.isSafeInteger(protocol) || protocol < 6)
    failures.push("后台协议不能低于 6");
  if (!backend.includes("self.harness.call(action, payload, Some(authorization))")
      || backend.includes("NativeCdpBridge") || backend.includes("NativePreferredRemoteBridge")
      || app.includes('mutate("setRuntime"') || app.includes('id="appearance-runtime-entry"'))
    failures.push("远程 Codex 必须只使用服务模式，不能保留模式切换入口");
  if (!backend.includes('.route("/api/workspace/context", post(workspace::context))')
      || !workspace.includes("async fn branch_metadata(")
      || !app.includes('api("/api/workspace/context"'))
    failures.push("后台或新建页面缺少真实分支信息读取");
  if (failures.length) throw new Error(`Windows 构建基线不完整：\n${failures.join("\n")}`);
  return { protocol };
}

const runtimeContract = verifyRemoteRuntimeSources({
  backend: readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/remote_access.rs"), "utf8"),
  service: readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/remote_access/service.rs"), "utf8"),
  workspace: readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/remote_access/workspace.rs"), "utf8"),
  // 远程网页已拆分为入口与 ui/core/features 模块，运行时合同按全部网页脚本合并后核对。
  app: remoteWebResources.filter(resource => /\.m?js$/.test(resource.file))
    .map(resource => readFileSync(resolve(remoteAccessDirectory, resource.file), "utf8")).join("\n"),
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
  ["codex-elves-manager.exe", [...rendererSources, ...remoteWebSources]],
  // 用户要求独立客户端内置网页缓存；打包时也核对其内置资源，不能把旧网页包随新 EXE 一起交付。
  ["codex-elves-remote.exe", remoteWebSources, false],
];
const failures = [];
for (const [name, sources, requireBuildId = true] of sourceOnly ? [] : programs) {
  const binary = readFileSync(resolve(binaryDirectory, name));
  if (requireBuildId && !binary.includes(Buffer.from(buildId))) failures.push(`${name} 缺少当前构建标识`);
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
    ? `Windows 源码基线核对通过：后台协议 ${runtimeContract.protocol}，服务模式和分支信息读取完整。`
    : `Windows 构建输入核对通过：${buildId}，远程页面和桌面注入均与当前源码一致。`);
}
