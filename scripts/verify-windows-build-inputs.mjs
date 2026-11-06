import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const repositoryRoot = resolve(process.argv[2] || resolve(import.meta.dirname, ".."));
const binaryDirectory = resolve(process.argv[3] || resolve(repositoryRoot, "target/release-fast"));
const assetSource = readFileSync(resolve(repositoryRoot, "crates/codex-elves-core/src/assets.rs"), "utf8");
const buildId = assetSource.match(/pub const DIAGNOSTIC_BUILD_ID:\s*&str\s*=\s*"([^"]+)"/)?.[1];
if (!buildId) throw new Error("无法读取当前注入构建标识。");

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
    "assets/remote-access/native.js",
    "assets/remote-access/session-menu.js",
    "assets/remote-access/session-tools.js",
    "assets/remote-access/style.css",
  ]],
];
const failures = [];
for (const [name, sources] of programs) {
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
  console.log(`Windows 构建输入核对通过：${buildId}，远程页面、原生桥和桌面注入均与当前源码一致。`);
}
