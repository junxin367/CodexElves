const form = document.querySelector("#connection-form");
const server = document.querySelector("#server-url");
const connect = document.querySelector("#connect");
const status = document.querySelector("#status");
const invoke = (command, payload = {}) => window.__TAURI__.core.invoke(command, payload);
let connecting = false;

async function openConnection() {
  if (connecting || !server.value.trim()) return;
  connecting = true;
  connect.disabled = true;
  status.textContent = "正在打开远程服务…";
  status.classList.remove("error");
  try {
    // 连接页只提交服务首页；配对密钥和设备授权仍由原远程页面处理，不传入原生配置。
    await invoke("connect_remote", { serverUrl: server.value.trim() });
  } catch (error) {
    status.textContent = String(error);
    status.classList.add("error");
    server.focus();
  } finally {
    connecting = false;
    connect.disabled = false;
  }
}
form.addEventListener("submit", event => {
  event.preventDefault();
  void openConnection();
});

try {
  const settings = await invoke("remote_client_state");
  server.value = settings.serverUrl;
  document.querySelector("#version").textContent = `v${settings.version}`;
  status.textContent = settings.error || "连接后将使用此客户端独立保存的登录状态。";
  status.classList.toggle("error", Boolean(settings.error));
  connect.disabled = false;
  if (settings.error) {
    // 地址错误后直接选中原输入，便于立即修正，不再次自动进入错误页面。
    server.focus();
    server.select();
  }
  // 用户要求独立程序直接打开会话；仅首次启动自动恢复已成功连接的地址。
  if (!settings.error && settings.autoConnect && settings.serverUrl) void openConnection();
} catch (error) {
  status.textContent = `读取设置失败：${error}`;
  status.classList.add("error");
}
