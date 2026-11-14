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
const toolIds = ["rg", "node_repl"];
const specs = Object.fromEntries(toolIds.map((toolId) => {
  const definition = manifest.tools?.[toolId];
  const spec = definition?.targets?.[target];
  if (!spec?.path || isAbsolute(spec.path) || !/^[0-9a-f]{64}$/i.test(spec.sha256 || "")) {
    throw new Error(`${toolId} 工具清单缺少有效的 Windows x64 路径或 SHA-256`);
  }
  return [toolId, { ...definition, ...spec }];
}));
for (const toolId of toolIds) {
  for (const license of specs[toolId].licenses || []) {
    const licensePath = resolveToolPath(sourceTools, license);
    if (!existsSync(licensePath)) {
      throw new Error(`${toolId} 工具许可证文件不存在: ${licensePath}`);
    }
  }
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

function checkBinary(path, spec, label) {
  if (!existsSync(path) || !statSync(path).isFile()) {
    throw new Error(`${label}不存在: ${path}`);
  }
  const actual = sha256(path);
  if (actual.toLowerCase() !== spec.sha256.toLowerCase()) {
    throw new Error(`${label} SHA-256 不匹配: ${actual}`);
  }
}

function probeTool(toolId, path, spec, label) {
  const args = toolId === "node_repl" ? ["--help"] : ["--version"];
  const expected = toolId === "node_repl"
    ? "node_repl MCP stdio server"
    : spec.version;
  const probe = spawnSync(path, args, {
    encoding: "utf8",
    windowsHide: true,
  });
  const output = `${probe.stdout || ""}\n${probe.stderr || ""}`;
  if (probe.error || probe.status !== 0 || !output.includes(expected)) {
    throw new Error(`${label}版本探测失败，期望 ${expected}`);
  }
}

for (const toolId of toolIds) {
  const spec = specs[toolId];
  const path = resolveToolPath(sourceTools, spec.path);
  checkBinary(path, spec, `源码 ${toolId}`);
  probeTool(toolId, path, spec, `源码 ${toolId}`);
}

if (mode !== "--source-only") {
  const installedTools = resolve(mode, "tools");
  for (const toolId of toolIds) {
    const spec = specs[toolId];
    const installedPath = resolveToolPath(installedTools, spec.path);
    checkBinary(installedPath, spec, `安装输入 ${toolId}`);
    probeTool(toolId, installedPath, spec, `安装输入 ${toolId}`);
  }
  console.log(`工具运行时核对通过：${toolIds.join(", ")}，源码和安装输入 SHA-256 一致`);
} else {
  console.log(`工具源码核对通过：${toolIds.join(", ")}，SHA-256 已核对`);
}
