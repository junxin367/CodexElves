import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, posix, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { listRemoteWebResources, remoteResourceContentTypes, separatelyServedResources } from "./remote-access-resources.mjs";

const root = resolve(import.meta.dirname, "..");
const assetRoot = resolve(root, "assets/remote-access");

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

function localImports(source, { dynamic = true } = {}) {
  const imports = new Set();
  const patterns = [
    /\bfrom\s*["']([^"']+)["']/g,
    /\bimport\s*["']([^"']+)["']/g,
    ...(dynamic ? [/\bimport\s*\(\s*["']([^"']+)["']\s*\)/g] : [])
  ];
  for (const pattern of patterns) {
    for (const match of source.matchAll(pattern)) {
      if (match[1].startsWith(".")) imports.add(match[1]);
    }
  }
  return imports;
}

// 模块分层：ui 为通用组件，不依赖会话业务；core 为跨领域状态与基础设施，不依赖功能模块；
// features 为业务功能模块；app.js 为入口。根目录的既有独立模块不得反向依赖上述分层。
function layer(file) {
  if (file === "app.js") return "entry";
  const top = file.split("/")[0];
  return ["ui", "core", "features"].includes(top) ? top : "root";
}
const forbiddenLayers = {
  root: ["core", "features", "entry"],
  ui: ["core", "features", "entry"],
  core: ["features", "entry"],
  features: ["entry"],
  entry: [],
};

const errors = [];
const resources = await listRemoteWebResources(assetRoot);
const jsFiles = resources.filter(resource => resource.file.endsWith(".js") || resource.file.endsWith(".mjs"))
  .map(resource => resource.file);
for (const file of separatelyServedResources) if (file.endsWith(".js")) jsFiles.push(file);
const sources = new Map();

for (const file of jsFiles.sort()) {
  const path = resolve(assetRoot, file);
  const source = await readFile(path);
  sources.set(file, source.toString("utf8"));
  const syntaxError = checkSyntax(path, source);
  if (syntaxError) errors.push(syntaxError);

  for (const specifier of localImports(source.toString("utf8"))) {
    const target = resolve(dirname(path), specifier);
    if (!target.startsWith(`${assetRoot}${sep}`)) {
      errors.push(`${relative(root, path)}: import 越过远程资源目录：${specifier}`);
      continue;
    }
    const targetFile = relative(assetRoot, target).split(sep).join("/");
    if (!resources.some(resource => resource.file === targetFile)) {
      errors.push(`${relative(root, path)}: import 目标不是已提供的网页资源：${specifier}`);
      continue;
    }
    if (forbiddenLayers[layer(file)].includes(layer(targetFile)))
      errors.push(`${relative(root, path)}: 分层违规，${layer(file)} 模块不能依赖 ${layer(targetFile)} 模块：${specifier}`);
  }
}

// 静态 import 不得成环（2026-11-15 用户要求完整分层重构、消除循环依赖）：环会让模块求值顺序依赖入口导入次序，
// 加载期读取未初始化的绑定即报错。下层需要调用上层时经 core/actions.js 的注册动作反转依赖，
// 见 docs/remote-access-web-architecture.md；动态 import 的可选模块不参与求值顺序，不计入。
{
  const graph = new Map([...sources].map(([file, text]) => [file, [...localImports(text, { dynamic: false })]
    .map(specifier => posix.normalize(posix.join(posix.dirname(file), specifier))).filter(target => sources.has(target))]));
  let index = 0;
  const order = new Map(), low = new Map(), stack = [], onStack = new Set();
  const visit = file => {
    order.set(file, index); low.set(file, index); index++; stack.push(file); onStack.add(file);
    for (const target of graph.get(file)) {
      if (!order.has(target)) { visit(target); low.set(file, Math.min(low.get(file), low.get(target))); }
      else if (onStack.has(target)) low.set(file, Math.min(low.get(file), order.get(target)));
    }
    if (low.get(file) !== order.get(file)) return;
    const component = [];
    let member;
    do { member = stack.pop(); onStack.delete(member); component.push(member); } while (member !== file);
    if (component.length === 1 && !graph.get(file).includes(file)) return;
    // 给出分量内一条最短的具体环，便于定位应反转或下沉的依赖
    const inside = new Set(component), start = component.sort()[0], previous = new Map([[start, null]]), queue = [start];
    let cycle = null;
    while (queue.length && !cycle) {
      const current = queue.shift();
      for (const target of graph.get(current)) {
        if (!inside.has(target)) continue;
        if (target === start) { cycle = [current]; break; }
        if (!previous.has(target)) { previous.set(target, current); queue.push(target); }
      }
    }
    for (let node = cycle && previous.get(cycle[0]); node; node = previous.get(node)) cycle.unshift(node);
    errors.push(`静态 import 存在循环依赖（${component.length} 个模块）：${[...(cycle || component), start].join(" → ")}`);
  };
  for (const file of [...graph.keys()].sort()) if (!order.has(file)) visit(file);
}

// 具名导入须在目标模块中存在：用 V8 只链接、不执行全部模块，缺失或冲突的导出会在链接阶段报错。
// 浏览器遇到这类错误会整页无法加载，而截取源码的 vm 测试无法发现，因此作为静态门禁。
const linkCheck = spawnSync(process.execPath, ["--experimental-vm-modules", "--no-warnings", "--input-type=module", "-e", `
  import { readFileSync } from "node:fs";
  import { pathToFileURL } from "node:url";
  import vm from "node:vm";
  const files = JSON.parse(process.argv[1]);
  const context = vm.createContext({});
  const modules = new Map(files.map(file => {
    const identifier = pathToFileURL(file).href;
    return [identifier, new vm.SourceTextModule(readFileSync(file, "utf8"), { identifier, context })];
  }));
  const linker = specifier => (_, referencing) => {
    const found = modules.get(new URL(specifier, referencing.identifier).href);
    if (!found) throw new Error("未找到模块：" + specifier);
    return found;
  };
  const failures = [];
  for (const [identifier, module] of modules) {
    if (module.status !== "unlinked") continue;
    try { await module.link((specifier, referencing) => linker(specifier)(null, referencing)); }
    catch (error) { failures.push(identifier + "：" + error.message); }
  }
  if (failures.length) { console.log(failures.join("\\n")); process.exitCode = 1; }
`, JSON.stringify(jsFiles.map(file => resolve(assetRoot, file)))], { encoding: "utf8" });
if (linkCheck.status !== 0) {
  // 同一处错误会在所有依赖它的模块上重复出现：按错误去重，并按导入语句定位实际写错的模块。
  const escapeRegExp = value => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const lines = `${linkCheck.stdout || ""}${linkCheck.stderr || ""}`.trim().split("\n").filter(Boolean);
  for (const message of new Set(lines.map(line => line.slice(line.indexOf("：") + 1)))) {
    const missing = message.match(/The requested module '([^']+)' does not provide an export named '([^']+)'/);
    const importers = missing ? [...sources].filter(([, text]) => new RegExp(
      `import\\s*\\{[^}]*\\b${escapeRegExp(missing[2])}\\b[^}]*\\}\\s*from\\s*["']${escapeRegExp(missing[1])}["']`).test(text))
      .map(([file]) => `assets/remote-access/${file}`) : [];
    errors.push(`ES module 链接检查失败${importers.length ? `（${importers.join("、")}）` : ""}：${message}`);
  }
}

// 入口的静态依赖须全部在 index.html 中预加载，避免首屏逐层发现模块；动态导入的可选模块不预加载。
const html = await readFile(resolve(assetRoot, "index.html"), "utf8");
const scripts = new Set([...html.matchAll(/<script type="module" src="\/([^"]+)"><\/script>/g)].map(match => match[1]));
const preloads = new Set([...html.matchAll(/<link rel="modulepreload" href="\/([^"]+)">/g)].map(match => match[1]));
const closure = new Set();
(function visit(file) {
  if (closure.has(file) || !sources.has(file)) return;
  closure.add(file);
  for (const specifier of localImports(sources.get(file), { dynamic: false }))
    visit(posix.normalize(posix.join(posix.dirname(file), specifier)));
})("app.js");
for (const file of [...closure].sort()) {
  if (!scripts.has(file) && !preloads.has(file)) errors.push(`index.html 缺少入口静态依赖的预加载：/${file}`);
}
for (const file of preloads) {
  if (!closure.has(file)) errors.push(`index.html 预加载了入口不再静态依赖的模块：/${file}`);
}

// 各端按同一目录规则生成网页资源清单；规则变化时须同步修改下列实现。
const ruleImplementations = [
  "crates/codex-elves-core/build.rs",
  "apps/codex-elves-android/app/build.gradle",
  "apps/codex-elves-remote/src-tauri/build.rs",
];
for (const implementation of ruleImplementations) {
  let text;
  try { text = await readFile(resolve(root, implementation), "utf8"); }
  catch { errors.push(`${implementation}: 无法读取网页资源清单规则实现`); continue; }
  for (const name of separatelyServedResources)
    if (!text.includes(name)) errors.push(`${implementation}: 未排除独立提供的 ${name}`);
  for (const extension of Object.keys(remoteResourceContentTypes))
    if (!text.includes(`"${extension.slice(1)}"`)) errors.push(`${implementation}: 未按统一规则处理 .${extension.slice(1)} 资源`);
}

if (errors.length) {
  console.error(errors.join("\n\n"));
  process.exitCode = 1;
} else {
  console.log(`远程资源检查通过：${resources.length} 个网页资源，${jsFiles.length} 个 JS 文件语法、import 与导出链接、分层、无循环依赖与预加载均有效。`);
}
