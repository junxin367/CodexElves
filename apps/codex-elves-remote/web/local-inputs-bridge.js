(() => {
  if (window.top !== window) return;
  // 用户要求仅独立客户端支持本机文件/文件夹；浏览器和 Android 不注入此入口。
  // 原生命令只处理系统选择器或 CF_HDROP，不接收任意本机路径、脚本或目标地址。
  Object.defineProperty(window, "__CODEX_ELVES_LOCAL_INPUTS__", {
    value: Object.freeze({
      pick: folders => window.__TAURI__.core.invoke("remote_choose_inputs", { folders: folders === true }),
      paste: () => window.__TAURI__.core.invoke("remote_paste_inputs")
    })
  });
})();
