// 用户要求只在独立远程客户端添加本机文件/文件夹；不根据 __TAURI__ 的存在猜测新命令是否已安装。
export function localInputBridge() {
  let bridge = globalThis.window?.__CODEX_ELVES_LOCAL_INPUTS__;
  // PC 侧边会话沿用同来源主窗口的原生入口，草稿及迟到结果仍由侧边自己的附件代次约束。
  if (!bridge) try { bridge = globalThis.window?.parent?.__CODEX_ELVES_LOCAL_INPUTS__; } catch {}
  return typeof bridge?.pick === "function" && typeof bridge?.paste === "function" ? bridge : null;
}

export function isLocalInput(entry) { return entry?.kind === "file" || entry?.kind === "folder"; }

export function copyLocalInput(entry) {
  return { kind: entry.kind, name: entry.name, size: entry.size,
    files: entry.files.map(({ path, data }) => ({ path, data })), directories: [...entry.directories] };
}

// 与原生 local_inputs.rs、服务端 input_files.rs 同步；根附件 16 项、目录项 512 项、合计 12 MiB。
export function validateLocalInputs(inputs) {
  if (!Array.isArray(inputs) || inputs.length > 16) throw new Error("每条消息最多添加 16 个文件或文件夹");
  let total = 0, entries = inputs.length;
  const component = name => typeof name === "string" && !!name && new TextEncoder().encode(name).length <= 255
    && ![".", ".."].includes(name) && !/[. ]$/u.test(name)
    && !/[\u0000-\u001f\u007f<>:"/\\|?*]/u.test(name)
    && !/^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/iu.test(name);
  const relative = path => typeof path === "string" && new TextEncoder().encode(path).length <= 4096
    && path.split("/").length <= 32 && path.split("/").every(component);
  for (const input of inputs) {
    if (!isLocalInput(input) || !component(input.name) || !Number.isSafeInteger(input.size) || input.size < 0
        || !Array.isArray(input.files) || !Array.isArray(input.directories)
        || input.kind === "file" && (input.directories.length || input.files.length !== 1 || input.files[0]?.path !== input.name))
      throw new Error("文件或文件夹附件无效，请重新选择");
    const paths = [...input.directories, ...input.files.map(file => file?.path)];
    if (paths.some(path => !relative(path)) || new Set(paths.map(path => path.toLowerCase())).size !== paths.length)
      throw new Error("附件包含越界、重复或无效路径");
    const filePaths = new Set(input.files.map(file => file.path.toLowerCase()));
    if (paths.some(path => path.split("/").slice(0, -1).some((_, index, parts) =>
      filePaths.has(parts.slice(0, index + 1).join("/").toLowerCase()))))
      throw new Error("附件文件与目录路径冲突");
    let size = 0;
    for (const file of input.files) {
      if (typeof file.data !== "string" || file.data.length > 16 * 1024 * 1024
          || file.data.length % 4 || !/^[A-Za-z0-9+/]*={0,2}$/u.test(file.data)
          || file.data.length > 0 && file.data.length < 4)
        throw new Error("附件内容编码无效或超过大小限制");
      size += file.data.length / 4 * 3 - (file.data.endsWith("==") ? 2 : file.data.endsWith("=") ? 1 : 0);
    }
    total += size; entries += paths.length;
    if (size !== input.size) throw new Error("附件大小与实际内容不一致");
    if (total > 12 * 1024 * 1024) throw new Error("文件和文件夹总大小不能超过 12 MiB");
    if (entries > 512) throw new Error("每条消息最多包含 512 个文件和目录项");
  }
  return inputs.map(copyLocalInput);
}
