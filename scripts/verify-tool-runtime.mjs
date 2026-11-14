import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { isAbsolute, join, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";

const repositoryRoot = resolve(process.argv[2] || resolve(import.meta.dirname, ".."));
const mode = process.argv[3] || "--source-only";
const sourceTools = resolve(repositoryRoot, "tools");
const manifestPath = join(sourceTools, "tool-manifest.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const target = "windows-x64";
if (manifest.schemaVersion !== 1) {
  throw new Error(`不支持的工具清单版本: ${manifest.schemaVersion}`);
}
const spec = manifest.tools?.rg?.targets?.[target];
if (!spec?.path || isAbsolute(spec.path) || !/^[0-9a-f]{64}$/i.test(spec.sha256 || "")) {
  throw new Error("rg 工具清单缺少有效的 Windows x64 路径或 SHA-256");
}

function resolveToolPath(base, relativePath) {
  const basePath = resolve(base);
  const path = resolve(basePath, relativePath);
  const prefix = basePath.endsWith(sep) ? basePath : `${basePath}${sep}`;
  if (path !== basePath && !path.startsWith(prefix)) {
    throw new Error(`工具路径越界: ${relativePath}`);
  }
  return path;
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function checkBinary(path, label) {
  if (!existsSync(path) || !statSync(path).isFile()) {
    throw new Error(`${label}不存在: ${path}`);
  }
  const actual = sha256(path);
  if (actual.toLowerCase() !== spec.sha256.toLowerCase()) {
    throw new Error(`${label} SHA-256 不匹配: ${actual}`);
  }
}

const sourcePath = resolveToolPath(sourceTools, spec.path);
checkBinary(sourcePath, "源码 rg");

if (mode !== "--source-only") {
  const installedTools = resolve(mode, "tools");
  const installedPath = resolveToolPath(installedTools, spec.path);
  checkBinary(installedPath, "安装输入 rg");
  const probe = spawnSync(installedPath, ["--version"], {
    encoding: "utf8",
    windowsHide: true,
  });
  if (probe.status !== 0 || !String(probe.stdout || "").includes(manifest.tools.rg.version)) {
    throw new Error(`安装输入 rg 版本探测失败，期望 ${manifest.tools.rg.version}`);
  }
  console.log(`工具运行时核对通过：rg ${manifest.tools.rg.version}，SHA-256 ${spec.sha256}`);
} else {
  console.log(`工具源码核对通过：rg ${manifest.tools.rg.version}，SHA-256 ${spec.sha256}`);
}
