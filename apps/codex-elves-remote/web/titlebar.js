const appWindow = window.__TAURI__.window.getCurrentWindow();
const maximize = document.querySelector("#maximize");
let themeVersion = 0;

function applyThemeSnapshot(snapshot) {
  if (!snapshot || !Number.isSafeInteger(snapshot.version) || snapshot.version <= themeVersion) return;
  const theme = snapshot.theme, root = document.documentElement;
  if (!theme || typeof theme.dark !== "boolean" || !["background", "foreground", "border"].every(key =>
    Array.isArray(theme[key]) && theme[key].length === 3
      && theme[key].every(value => Number.isInteger(value) && value >= 0 && value <= 255))) return;
  themeVersion = snapshot.version;
  for (const key of ["background", "foreground", "border"]) {
    root.style.setProperty(`--titlebar-${key}`, `rgb(${theme[key].join(",")})`);
  }
  root.style.colorScheme = theme.dark ? "dark" : "light";
}
// 标题栏启动读取与页面实时更新可能先后到达；版本较旧的快照不能把新配色改回去。
window.__setRemoteTitlebarTheme = applyThemeSnapshot;
void window.__TAURI__.core.invoke("remote_titlebar_state").then(applyThemeSnapshot).catch(() => {});

async function syncMaximized() {
  const maximized = await appWindow.isMaximized();
  document.documentElement.classList.toggle("maximized", maximized);
  maximize.title = maximized ? "还原" : "最大化";
  maximize.setAttribute("aria-label", maximize.title);
}
async function operate(action) {
  try {
    await action();
    await syncMaximized();
  } catch {
    document.querySelector(".titlebar-drag").title = "窗口操作暂时不可用，请使用托盘菜单。";
  }
}
document.querySelector("#minimize").onclick = () => void operate(() => appWindow.minimize());
maximize.onclick = () => void operate(() => appWindow.toggleMaximize());
// 用户要求关闭仍收进托盘；走原窗口 CloseRequested 流程，不直接退出进程。
document.querySelector("#close").onclick = () => void operate(() => appWindow.close());
document.addEventListener("contextmenu", event => event.preventDefault());

try {
  await syncMaximized();
  await appWindow.onResized(() => void syncMaximized().catch(() => {}));
  await appWindow.onFocusChanged(({ payload }) => {
    document.documentElement.classList.toggle("inactive", !payload);
  });
} catch {
  document.querySelector(".titlebar-drag").title = "窗口状态暂时不可用，请使用托盘菜单。";
}
