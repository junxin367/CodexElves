"use strict";
// 独立交互原型：不调用管理器、FRP 或任何真实服务接口。
const $ = id => document.getElementById(id);
const state = { editing: "frp", active: "zrok", scenario: "ready", busy: false, enabled: true, revision: 0 };
let toastTimer;
let notice = null;
const names = { zrok: "zrok", frp: "FRP" };
const addresses = { zrok: "https://demo.shares.zrok.io", frp: "http://hs.wwszxc.tax:39399" };
const pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
function showToast(text) {
  clearTimeout(toastTimer);
  $("toast").querySelector("span").textContent = text;
  $("toast").hidden = false;
  toastTimer = setTimeout(() => { $("toast").hidden = true; }, 4200);
}
function normalizedServer() {
  try {
    const value = $("frp-server").value.trim();
    const url = new URL(value.includes("://") ? value : `http://${value}`);
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url.hostname : "";
  } catch { return ""; }
}
function generatedOrigin() {
  const host = normalizedServer(), port = Number($("frp-remote-port").value);
  if (!/^[a-z0-9][a-z0-9.-]*$/i.test(host) || !Number.isInteger(port) || port < 1 || port > 65535) return "";
  return new URL(`http://${host}:${port}`).origin;
}
function render() {
  $("frp-url").value = generatedOrigin();
  for (const provider of ["zrok", "frp"]) {
    const selected = state.editing === provider;
    $(`tab-${provider}`).setAttribute("aria-selected", String(selected));
    $(`tab-${provider}`).tabIndex = selected ? 0 : -1;
    $(`tab-${provider}`).disabled = state.busy;
    $(`form-${provider}`).hidden = !selected;
  }
  $("active-badge").className = `badge ${state.enabled ? "success" : ""}`;
  $("active-badge").replaceChildren(Object.assign(document.createElement("i"), {}), document.createTextNode(
    state.enabled ? `当前生效：${names[state.active]}` : `已停止 · ${names[state.active]}`));
  $("service-toggle").setAttribute("aria-checked", String(state.enabled));
  $("service-toggle").disabled = state.busy;
  $("service-label").textContent = state.enabled ? "已启用" : "已停用";
  const incomplete = state.editing === "frp" && !generatedOrigin();
  const status = $("provider-status");
  const error = notice?.kind === "error";
  status.className = `provider-status ${error ? "error" : incomplete ? "warning" : ""}`;
  status.replaceChildren(document.createElement("i"), document.createTextNode(
    state.busy ? "正在验证…" : error ? "验证未通过" : incomplete ? "待填写配置"
      : state.editing === state.active ? (state.enabled ? "运行中" : "已保存") : "配置已就绪"));
  $("apply-provider").disabled = state.busy || (state.active === state.editing) || incomplete;
  $("apply-provider").textContent = state.busy && state.applying ? `正在切换到 ${names[state.editing]}…`
    : state.active === state.editing ? "当前主链路" : state.enabled ? "应用为主链路" : "保存为主链路";
  $("test-provider").disabled = state.busy || !state.enabled && state.editing !== "frp";
  $("test-provider").textContent = state.busy && !state.applying ? "正在测试…" : state.enabled ? "保存并测试" : "保存配置";
  $("open-frp-directory").hidden = state.editing !== "frp";
  $("scenario").disabled = state.busy;
  $("save-service").disabled = state.busy;
  for (const input of document.querySelectorAll(".provider-panel input")) input.disabled = state.busy;
  for (const button of document.querySelectorAll(".provider-panel button")) button.disabled = state.busy;
  document.querySelector("[data-copy='frp-url']").disabled = state.busy || !generatedOrigin();
  $("apply-hint").className = notice ? `inline-status ${notice.kind}` : "";
  $("apply-hint").textContent = notice?.text || (incomplete ? "填写服务器地址和公网映射端口后，自动生成固定公网地址。"
    : state.editing === "frp" ? "HTTP 公网传输不加密；验证通过后可应用为主链路。"
    : state.active === state.editing ? "当前正在使用此主链路，可随时查看另一份配置。"
    : "切换公网地址后，远程设备需用新地址重新登录。");
}
function selectProvider(provider) {
  if (state.busy) return;
  state.editing = provider;
  notice = null;
  render();
}
for (const button of document.querySelectorAll("[data-provider]")) {
  button.addEventListener("click", () => selectProvider(button.dataset.provider));
  button.addEventListener("keydown", event => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === "Home" ? "zrok" : event.key === "End" ? "frp" : state.editing === "zrok" ? "frp" : "zrok";
    selectProvider(next);
    $(`tab-${next}`).focus();
  });
}
function reset(scenario = $("scenario").value) {
  state.revision++;
  Object.assign(state, { editing: "frp", active: "zrok", scenario, busy: false, applying: false, enabled: true });
  addresses.frp = "http://hs.wwszxc.tax:39399";
  addresses.zrok = "https://demo.shares.zrok.io";
  $("scenario").value = scenario;
  $("frp-server").value = "hs.wwszxc.tax";
  $("frp-remote-port").value = scenario === "missing" ? "" : "39399";
  notice = null;
  $("toast").hidden = true;
  render();
}
$("scenario").addEventListener("change", () => reset());
$("reset").addEventListener("click", () => reset());
$("frp-server").addEventListener("blur", () => {
  const value = normalizedServer();
  if (value) $("frp-server").value = value;
  render();
});
for (const input of document.querySelectorAll(".provider-panel input:not([type=file])")) {
  input.addEventListener("input", () => { notice = null; render(); });
}
async function validateAndApply(apply) {
  if (state.busy) return;
  const provider = state.editing;
  if (provider === "frp" && (!$("frp-server").value.trim() || !$("frp-token").value.trim()
      || ![$("frp-port"), $("frp-remote-port")].every(input => Number.isInteger(Number(input.value)) && Number(input.value) >= 1 && Number(input.value) <= 65535))) {
    notice = { kind: "error", text: "请完整填写服务器地址、Token 和有效端口。" };
    render(); return;
  }
  if (provider === "frp" && !generatedOrigin()) {
    notice = { kind: "error", text: "请填写有效的服务器地址和公网映射端口。" };
    render(); return;
  }
  if (provider === "frp") $("frp-server").value = normalizedServer();
  if (!state.enabled) {
    if (apply) {
      state.active = provider;
      if (provider === "frp") addresses.frp = new URL($("frp-url").value).origin;
    }
    notice = { kind: "", text: apply ? "主链路选择已保存，启用远程会话网页后验证并启动。" : "FRP 候选配置已保存；启用远程会话网页后可测试入口。" };
    render(); return;
  }
  state.busy = true;
  state.applying = apply;
  const revision = ++state.revision;
  notice = { kind: "", text: `正在验证 ${names[provider]} 入口，当前主链路继续运行…` };
  render();
  await pause(1000);
  if (revision !== state.revision) return;
  if (provider === "frp" && state.scenario === "failure") {
    notice = { kind: "error", text: `FRP 公网验证超时，未应用切换。当前继续使用 ${names[state.active]}，请检查地址或稍后重试。` };
  } else if (apply) {
    notice = { kind: "", text: `入口验证通过，正在应用 ${names[provider]}…` };
    render();
    await pause(650);
    if (revision !== state.revision) return;
    state.active = provider;
    if (provider === "frp") addresses.frp = new URL($("frp-url").value).origin;
    notice = { kind: provider === "frp" ? "warning" : "", text: `已切换为 ${names[provider]} 主链路，请使用固定公网地址登录。${provider === "frp" ? "HTTP 公网传输不加密。" : ""}` };
  } else {
    notice = { kind: "", text: `${names[provider]} 配置已保存，公网访问验证通过。${state.active === provider ? "当前主链路继续运行。" : "点击「应用为主链路」后生效。"}` };
    if (provider === "frp") $("frp-token").type = "password";
  }
  state.busy = false;
  state.applying = false;
  render();
}
$("test-provider").addEventListener("click", () => void validateAndApply(false));
$("open-frp-directory").addEventListener("click", () => showToast("浏览器原型不打开本地目录；桌面版会打开 frp 程序目录。"));
$("apply-provider").addEventListener("click", () => void validateAndApply(true));
$("service-toggle").addEventListener("click", () => { state.enabled = !state.enabled; notice = null; render(); showToast(state.enabled ? "远程网页已启用（演示）。" : "远程网页及隧道已停止（演示）。"); });
$("save-service").addEventListener("click", () => showToast("监听设置已保存（演示）。"));
$("refresh").addEventListener("click", () => { render(); showToast("预览状态已刷新。"); });
$("theme").addEventListener("click", () => document.documentElement.classList.toggle("dark"));
for (const button of document.querySelectorAll("[data-reveal]")) button.addEventListener("click", () => {
  const input = $(button.dataset.reveal);
  const visible = input.type === "password";
  input.type = visible ? "text" : "password";
  button.setAttribute("aria-pressed", String(visible));
  button.setAttribute("aria-label", button.getAttribute("aria-label").replace(visible ? "显示" : "隐藏", visible ? "隐藏" : "显示"));
});
for (const button of document.querySelectorAll("[data-copy]")) button.addEventListener("click", async () => {
  const primaryCopy = ["frp-token", "frp-url"].includes(button.dataset.copy);
  try {
    await navigator.clipboard.writeText($(button.dataset.copy).value);
    if (primaryCopy) { notice = { kind: "", text: "演示内容已复制。" }; render(); }
    else showToast("演示内容已复制。");
  } catch {
    if (primaryCopy) { notice = { kind: "error", text: "复制未完成，请手动选择文本。" }; render(); }
    else showToast("复制未完成，请手动选择文本。");
  }
});
for (const button of document.querySelectorAll("[data-static]")) button.addEventListener("click", () => showToast("此处保留现有界面，本原型仅演示主链路切换。"));
for (const button of document.querySelectorAll("[data-demo-action]")) button.addEventListener("click", () => showToast(button.dataset.demoAction));
$("save-ngrok").addEventListener("click", () => {
  if (!$("ngrok-token").value.trim()) { showToast("请填写 ngrok Token。"); return; }
  $("ngrok-token").type = "password";
  const reveal = document.querySelector('[data-reveal="ngrok-token"]');
  reveal.setAttribute("aria-pressed", "false");
  reveal.setAttribute("aria-label", "显示 ngrok Token");
  showToast("ngrok Token 已保存并通过测试（演示）。");
});
$("generate-key").addEventListener("click", () => {
  $("access-key").value = `demo-${Math.random().toString(36).slice(2, 14)}`;
  showToast("演示密钥已生成，点击保存后生效。");
});
reset(new URLSearchParams(location.search).get("scenario") || "ready");
