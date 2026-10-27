import { readFile, readdir, stat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, extname, relative, resolve, sep } from "node:path";

const root = resolve(import.meta.dirname, "..");
const assetRoot = resolve(root, "assets/remote-access");
const devServerPath = resolve(root, "scripts/dev-remote-access.mjs");
const productionRoutePath = resolve(root, "crates/codex-elves-core/src/remote_access.rs");
const excludedFromWeb = new Set(["native.js"]);

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) files.push(...await walk(path));
    else files.push(path);
  }
  return files;
}

function checkSyntax(path, source) {
  const result = spawnSync(process.execPath, ["--check", "--input-type=module"], {
    input: source
  });
  if (result.status === 0) return null;
  const details = Buffer.concat([result.stdout || Buffer.alloc(0), result.stderr || Buffer.alloc(0)])
    .toString("utf8")
    .trim();
  return `${relative(root, path)}: ES module 语法检查失败\n${details}`;
}

function localImports(source) {
  const imports = new Set();
  for (const pattern of [
    /\bfrom\s*["']([^"']+)["']/g,
    /\bimport\s*["']([^"']+)["']/g,
    /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g
  ]) {
    for (const match of source.matchAll(pattern)) {
      if (match[1].startsWith(".")) imports.add(match[1]);
    }
  }
  return imports;
}

function routeFiles(source) {
  return new Set(
    [...source.matchAll(/"\/([^"]+\.js)"/g)]
      .map(([, file]) => file.replaceAll("/", sep))
  );
}

const errors = [];
const jsFiles = (await walk(assetRoot))
  .filter(path => extname(path) === ".js")
  .sort();

for (const path of jsFiles) {
  const source = await readFile(path);
  const syntaxError = checkSyntax(path, source);
  if (syntaxError) errors.push(syntaxError);

  for (const specifier of localImports(source.toString("utf8"))) {
    const target = resolve(dirname(path), specifier);
    if (!target.startsWith(`${assetRoot}${sep}`)) {
      errors.push(`${relative(root, path)}: import 越过远程资源目录：${specifier}`);
      continue;
    }
    try {
      if (!(await stat(target)).isFile()) {
        errors.push(`${relative(root, path)}: import 目标不是文件：${specifier}`);
      }
    } catch {
      errors.push(`${relative(root, path)}: import 目标不存在：${specifier}`);
    }
  }
}

const servedFiles = routeFiles(await readFile(devServerPath, "utf8"));
const productionFiles = routeFiles(await readFile(productionRoutePath, "utf8"));
for (const path of jsFiles) {
  const file = relative(assetRoot, path);
  if (excludedFromWeb.has(file)) continue;
  if (!servedFiles.has(file)) {
    errors.push(`scripts/dev-remote-access.mjs 未提供远程 JS：${file}`);
  }
  if (!productionFiles.has(file)) {
    errors.push(`remote_access.rs 未提供远程 JS：${file}`);
  }
}

if (errors.length) {
  console.error(errors.join("\n\n"));
  process.exitCode = 1;
} else {
  console.log(`远程资源检查通过：${jsFiles.length} 个 JS 文件语法、import 和服务路由均有效。`);
}
