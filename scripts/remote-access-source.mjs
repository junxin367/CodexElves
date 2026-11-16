// 远程会话网页源码的测试辅助：按模块读取 assets/remote-access 中的入口与 ui/core/features 模块，
// 供“截取源码片段放进 vm 运行”的测试使用。
//
// 约定：
// - section(start, end) 只在单个模块内截取：start 标记必须且只能出现在一个模块中，end 须在同一模块的
//   start 之后；省略 end 表示截取到模块末尾。跨模块的片段需拆成多段分别截取。上层动作在 core/actions.js
//   有同名转发函数，标记同时命中实现模块时以实现模块为准。
// - 截取结果去掉 export 关键字，可直接作为普通脚本在 vm 中运行；import 语句不会出现在片段中。
// - wireVm(vm) 返回的 vm 在运行片段前补齐入口负责的模块装配：跨模块状态 setter（写入同名全局）
//   与通用弹层的 closeAppMenus 钩子（对应入口 registerAppMenuCloser(closeComposerMenus)）。
import { readFile, readdir } from "node:fs/promises";
import { join, relative, resolve, sep } from "node:path";

const defaultRoot = resolve(import.meta.dirname, "../assets/remote-access");
const moduleDirectories = ["ui", "core", "features"];

async function listModules(root) {
  const files = ["app.js"];
  async function walk(directory) {
    const entries = await readdir(directory, { withFileTypes: true });
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await walk(path);
      else if (entry.name.endsWith(".js")) files.push(relative(root, path).split(sep).join("/"));
    }
  }
  for (const directory of moduleDirectories) await walk(join(root, directory));
  return files;
}

// 去掉 import 声明（单行或多行）并把行首的 export 声明还原为普通声明；保持行号不变以便定位。
function scriptText(source) {
  return source
    .replace(/^import\s[^;]*?from\s*["'][^"']+["'];?[ \t]*$/gms, match => match.replace(/[^\n]/g, ""))
    .replace(/^import\s*["'][^"']+["'];?[ \t]*$/gm, "")
    .replace(/^export (?=(?:async )?function[\s*]|const |let |var |class )/gm, "");
}

export async function loadRemoteAppSource(root = defaultRoot) {
  const modules = [];
  for (const path of await listModules(root)) {
    const original = await readFile(join(root, path), "utf8");
    modules.push({ path, original, text: scriptText(original) });
  }
  const setters = new Map();
  for (const module of modules) {
    for (const match of module.original.matchAll(/^export function (\w+)\(value\) \{ return (\w+) = value; \}$/gm))
      setters.set(match[1], match[2]);
  }
  const corpus = modules.map(module => module.text).join("\n");

  function locate(start) {
    let found = modules.filter(module => module.text.includes(start));
    // core/actions.js 为每个上层动作提供同名转发函数；标记同时命中实现模块时取实现模块，测试截取的是实现源码。
    if (found.length > 1) found = found.filter(module => module.path !== "core/actions.js");
    if (found.length !== 1)
      throw new Error(`源码标记${found.length ? `出现在多个模块（${found.map(module => module.path).join("、")}）` : "不存在"}：${start}`);
    return found[0];
  }
  function section(start, end) {
    const module = locate(start);
    const from = module.text.indexOf(start);
    if (end == null) return module.text.slice(from);
    const to = module.text.indexOf(end, from + start.length);
    if (to < 0) throw new Error(`结束标记不在 ${module.path} 的起始标记之后：${end}`);
    return module.text.slice(from, to);
  }
  function moduleText(path) {
    const module = modules.find(item => item.path === path);
    if (!module) throw new Error(`未找到远程网页模块：${path}`);
    return module.text;
  }
  function wire(code, context, runInContext) {
    // 实例设置新增的模型配置版本由入口模块初始化；源码片段测试只加载函数时也从同一初值开始。
    if (!Object.hasOwn(context, "modelSettingsVersion") && /\bmodelSettingsVersion\b/.test(code))
      context.modelSettingsVersion = 0;
    // 旧的局部函数用例默认没有独立客户端本机附件；专门验证该能力的用例必须注入真实附件状态。
    for (const [name, value] of Object.entries({
      composerLocalInputsPayload: () => [], composerLocalInputsSupported: () => false,
      composerCanAddLocalInputs: () => false, handleComposerLocalPasteShortcut: () => {}, unsupportedFiles: false,
    })) if (!Object.hasOwn(context, name)) context[name] = value;
    const missing = name => !Object.hasOwn(context, name) && new RegExp(`\\b${name}\\(`).test(code);
    const definitions = [];
    for (const [setter, name] of setters) if (missing(setter)) definitions.push(`function ${setter}(value) { return ${name} = value; }`);
    if (missing("closeAppMenus"))
      definitions.push('function closeAppMenus() { if (typeof closeComposerMenus === "function") closeComposerMenus(); }');
    if (definitions.length) runInContext(definitions.join("\n"), context);
  }
  function wireVm(vm) {
    return {
      ...vm,
      runInContext(code, context, options) {
        wire(String(code), context, (source, target) => vm.runInContext(source, target));
        return vm.runInContext(code, context, options);
      },
    };
  }
  return { modules, corpus, setters, section, moduleText, wireVm };
}
