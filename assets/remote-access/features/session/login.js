// 登录与连接恢复页：访问密钥登录、授权失效处理与初始连接失败的恢复说明；登录状态与登录客户端标识在 connection-state.js。

import { clearRemoteChannels } from "../../channels.js";
import { isAndroidContainer, postPlatformMessage } from "../../platform.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import {
  app, setCreating, setSelectedId, setSelectionToken, setSession, setSidebar, setThread, turns
} from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { openAppearance } from "../appearance/dialog.js";
import { setApprovalSignature } from "../approval-state.js";
import { resetNewChatBranch, setCreationProject } from "../composer/creation-state.js";
import { clearPrivateInputs } from "../composer/draft-restore.js";
import { attachmentDrafts, drafts, goalDrafts, skillDrafts } from "../composer/drafts.js";
import { closeContextUsageTip } from "../composer/metrics.js";
import {
  setModelDefaults, setModels, setModelsLoadedAt, setModelsPromise, setSelectedEffort,
  setSelectedModel
} from "../composer/options-state.js";
import {
  setMessageSkillLoadScope, setSelectedSkills, setSkillChoices, setSkillLoadVersion,
  setSkillLoading, setSkillRenderVersion, setSkillScope, skillLoadVersion, skillRenderVersion
} from "../composer/skill-catalog.js";
import { setNotifiedTokenUsage } from "../conversation/event-sync.js";
import { setHistoryCursor, setHistoryInitialized } from "../conversation/history-state.js";
import { savePending } from "../conversation/pending-store.js";
import {
  setSuppressAutomaticNewChat, setThreadReadVersion, threadReadVersion
} from "../conversation/selection-state.js";
import { transcriptEvents } from "../conversation/transcript-events.js";
import { setSelectedGoal, stopGoalLiveTimer } from "../goal/state.js";
import { setEditingMessage } from "../messages/message-edit.js";
import { setPendingNotificationOpen } from "../notifications.js";
import { setEditingQueuedMessage } from "../queue/state.js";
import {
  connectionErrorMessage, initialAuthenticationRequired, loginPageCleanup, remoteLoginClientId,
  setAuthenticationRequired, setLoginPageCleanup
} from "./connection-state.js";
import { connect, disconnect } from "./lifecycle.js";
import { resetRelatedSessions } from "./related.js";
import {
  announceAppReady, composerScroll, sessionTools, setCloseMobileSidebar, setComposerScroll,
  setSessionTools, setSidebarLayoutCleanup, setWorkspaceOpenIn, sidebarLayoutCleanup, workspaceOpenIn
} from "../shell-state.js";
import { setSidebarBusy, setSidebarPreferenceScope } from "../sidebar/actions.js";
import {
  collapsedSidebarSections, projectVisibleThreadCounts, recentPrioritySelectionSuppressions,
  setRecentPriorityIds, sidebarReadRefreshSuppressions
} from "../sidebar/list-state.js";

export function showLogin() {
  loginPageCleanup?.();
  resetNewChatBranch();
  resetRelatedSessions();
  sessionTools?.dispose(); setSessionTools(null);
  workspaceOpenIn?.dispose(); setWorkspaceOpenIn(null);
  stopGoalLiveTimer();
  closePopup(); closeContextUsageTip();
  composerScroll?.dispose(); setComposerScroll(null);
  sidebarLayoutCleanup?.(); setSidebarLayoutCleanup(null); setCloseMobileSidebar(() => {});
  setEditingMessage(null); setEditingQueuedMessage(null);
  setSession(null);
  setSuppressAutomaticNewChat(false);
  setSidebar(null); setRecentPriorityIds(null); recentPrioritySelectionSuppressions.clear();
  sidebarReadRefreshSuppressions.clear();
  setSelectedId(null); setThread(null); setCreating(false);
  projectVisibleThreadCounts.clear(); collapsedSidebarSections.clear();
  setHistoryCursor(null); setHistoryInitialized(false); setApprovalSignature(null);
  turns.clear(); transcriptEvents.clear(); drafts.clear(); skillDrafts.clear(); attachmentDrafts.clear(); goalDrafts.clear();
  setSelectedSkills([]); setSelectedGoal(null); setSelectedModel(setSelectedEffort(""));
  setModels([]); setModelsPromise(null); setModelsLoadedAt(0); setSkillScope(""); setSkillChoices([]); setSkillLoading(null); setSkillLoadVersion(skillLoadVersion + 1); setSelectionToken(randomId());
  setMessageSkillLoadScope(""); setSkillRenderVersion(skillRenderVersion + 1);
  setModelDefaults({ model: "", reasoningEffort: "" });
  setCreationProject(""); setNotifiedTokenUsage(null); setThreadReadVersion(threadReadVersion + 1);
  setSidebarBusy(false);
  setSidebarPreferenceScope(null);
  $("#confirmation-dialog")?.close("cancel");
  $("#appearance-dialog")?.close();
  app.replaceChildren();
  const page = element("div", "pair-page");
  const panel = element("main", "pair-panel");
  const heading = element("header", "pair-heading");
  const mark = element("div", "brand-mark", "✦"); mark.setAttribute("aria-hidden", "true");
  const brand = element("div", "pair-brand");
  brand.append(mark, element("h1", "", "CodexElves"));
  heading.append(brand, element("p", "pair-description muted", "输入访问密钥，继续使用你的 Codex 会话。"));
  const form = element("form", "pair-form");
  form.noValidate = true;
  const label = element("label", "pair-label", "访问密钥"); label.htmlFor = "access-key";
  const field = element("div", "pair-key-field");
  const key = element("input");
  key.id = "access-key"; key.type = "password"; key.placeholder = "粘贴或输入访问密钥"; key.required = true;
  key.autocomplete = "off"; key.minLength = 8; key.spellcheck = false;
  key.setAttribute("autocapitalize", "off"); key.setAttribute("autocorrect", "off");
  key.setAttribute("enterkeyhint", "go"); key.setAttribute("aria-describedby", "pair-key-hint");
  const reveal = button("", "显示密钥", () => {
    const start = key.selectionStart, end = key.selectionEnd;
    setKeyVisible(key.type === "password");
    key.focus({ preventScroll: true }); key.setSelectionRange(start, end);
  }, "pair-key-toggle");
  reveal.setAttribute("aria-controls", key.id);
  function setKeyVisible(visible) {
    key.type = visible ? "text" : "password";
    reveal.setAttribute("aria-label", visible ? "隐藏密钥" : "显示密钥");
    reveal.title = visible ? "隐藏密钥" : "显示密钥";
    reveal.replaceChildren(icon(visible ? "eyeOff" : "eye"));
  }
  setKeyVisible(false);
  field.append(key, reveal);
  const invalidKeyMessage = "访问密钥错误，请重试。";
  const loginLockedMessage = "访问密钥连续输错超过3次，登录已锁定。请在本机管理器重新生成并保存密钥后再登录。";
  const hint = element("p", "pair-hint"); hint.id = "pair-key-hint"; hint.hidden = true;
  hint.setAttribute("aria-live", "polite"); hint.setAttribute("aria-atomic", "true");
  const submit = element("button", "primary pair-submit"); submit.type = "submit"; submit.id = "pair-submit";
  const spinner = element("span", "spinner"); spinner.hidden = true; spinner.setAttribute("aria-hidden", "true");
  const submitText = element("span", "", "连接");
  submit.append(spinner, submitText);
  const status = element("p", "pair-status muted");
  status.id = "pair-status"; status.setAttribute("role", "status");
  form.append(label, field, hint, submit, status);
  let submitting = false, authenticated = false, loginLocked = false, checkingLoginStatus = true;
  let statusRequest = null, statusTimer = null, lockVersion = 0, disposed = false, pageHidden = false, loginStatusChecked = false;
  function setError(message = "", invalid = false) {
    hint.textContent = message;
    hint.hidden = !message;
    hint.classList.toggle("error-text", !!message); hint.classList.toggle("muted", !message);
    key.setAttribute("aria-invalid", String(invalid));
  }
  function updateForm() {
    form.setAttribute("aria-busy", String(submitting || checkingLoginStatus));
    key.readOnly = submitting; key.disabled = authenticated || loginLocked;
    reveal.disabled = submitting || authenticated || loginLocked || !key.value;
    submit.disabled = submitting || checkingLoginStatus || loginLocked || (!authenticated && !key.value.trim());
    spinner.hidden = !submitting && !checkingLoginStatus;
    if (!submitting) submitText.textContent = loginLocked ? "登录已锁定"
      : checkingLoginStatus ? "正在检查…" : authenticated ? "重新加载会话" : "连接";
  }
  function setLoginLocked(locked) {
    if (loginLocked !== locked) {
      ++lockVersion;
      if (!locked) {
        key.value = "";
        key.placeholder = "粘贴或输入访问密钥";
      }
    }
    loginLocked = locked;
    setError(locked ? loginLockedMessage : "");
    status.textContent = "";
    updateForm();
  }
  function stopLoginStatusRead() {
    clearTimeout(statusTimer); statusTimer = null;
    if (statusRequest) {
      clearTimeout(statusRequest.timeout);
      statusRequest.controller.abort();
      statusRequest = null;
    }
  }
  function scheduleLoginStatusRead() {
    clearTimeout(statusTimer); statusTimer = null;
    if (!disposed && panel.isConnected && loginLocked && !authenticated && !document.hidden && !pageHidden)
      statusTimer = setTimeout(() => void readLoginStatus(), 5000);
  }
  async function readLoginStatus() {
    if (disposed || !panel.isConnected || authenticated || document.hidden || pageHidden || statusRequest) return;
    clearTimeout(statusTimer); statusTimer = null;
    const request = { controller: new AbortController(), version: lockVersion, timeout: null };
    statusRequest = request;
    request.timeout = setTimeout(() => request.controller.abort(), 10000);
    try {
      const result = await api("/api/login-status", undefined, undefined, request.controller.signal);
      if (statusRequest !== request || request.controller.signal.aborted || disposed || !panel.isConnected
          || authenticated || request.version !== lockVersion) return;
      if (typeof result.loginLocked === "boolean") setLoginLocked(result.loginLocked);
    } catch {
      // 旧服务的 404 和网络错误均不改变已知锁定状态。
    } finally {
      clearTimeout(request.timeout);
      if (statusRequest === request) {
        statusRequest = null;
        loginStatusChecked = true;
        checkingLoginStatus = false;
        updateForm();
        scheduleLoginStatusRead();
      }
    }
  }
  function suspendLoginStatusRead() {
    stopLoginStatusRead();
    checkingLoginStatus = false;
    updateForm();
  }
  function handleLoginVisibility() {
    if (document.hidden) suspendLoginStatusRead();
    else if ((!loginStatusChecked || loginLocked) && !pageHidden) void readLoginStatus();
  }
  function handleLoginPageHide() {
    pageHidden = true;
    suspendLoginStatusRead();
  }
  function handleLoginPageShow() {
    pageHidden = false;
    if ((!loginStatusChecked || loginLocked) && !document.hidden) void readLoginStatus();
  }
  setLoginPageCleanup(() => {
    disposed = true;
    stopLoginStatusRead();
    document.removeEventListener("visibilitychange", handleLoginVisibility);
    window.removeEventListener("pagehide", handleLoginPageHide);
    window.removeEventListener("pageshow", handleLoginPageShow);
    setLoginPageCleanup(null);
  });
  document.addEventListener("visibilitychange", handleLoginVisibility);
  window.addEventListener("pagehide", handleLoginPageHide);
  window.addEventListener("pageshow", handleLoginPageShow);
  key.addEventListener("input", () => {
    setError(loginLocked ? loginLockedMessage : "");
    status.textContent = "";
    updateForm();
  });
  key.addEventListener("blur", () => { key.value = key.value.trim(); updateForm(); });
  key.addEventListener("paste", event => {
    if (!event.clipboardData || submitting || authenticated || loginLocked) return;
    const value = event.clipboardData.getData("text").trim();
    event.preventDefault();
    if (/[\r\n]/.test(value)) {
      setError(invalidKeyMessage, true); return;
    }
    key.setRangeText(value, key.selectionStart, key.selectionEnd, "end");
    key.dispatchEvent(new Event("input", { bubbles: true }));
  });
  key.addEventListener("keydown", event => {
    if (event.key === "Enter" && (event.isComposing || event.keyCode === 229)) event.preventDefault();
  });
  const hashParameters = new URLSearchParams(location.hash.slice(1));
  if (hashParameters.has("pair") || hashParameters.has("key")) history.replaceState(null, "", location.pathname);
  form.addEventListener("submit", async event => {
    event.preventDefault();
    if (submitting || checkingLoginStatus || loginLocked || disposed) return;
    const value = key.value.trim();
    if (!authenticated && !/^[\x21-\x7e]{8,256}$/.test(value)) {
      setError("访问密钥需为 8–256 个非空格 ASCII 字符。", true);
      key.focus(); return;
    }
    ++lockVersion; stopLoginStatusRead();
    submitting = true; setError(); setKeyVisible(false); updateForm();
    submitText.textContent = authenticated ? "正在打开…" : "连接中…";
    status.textContent = authenticated ? "密钥已验证，正在加载会话…" : "正在验证访问密钥…";
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 20000);
    try {
      if (!authenticated) {
        await api("/api/connect", {
          key: value, clientId: remoteLoginClientId(),
          clientKind: isAndroidContainer() ? "android-app" : "browser"
        }, undefined, controller.signal);
        if (disposed || !panel.isConnected) return;
        stopLoginStatusRead(); checkingLoginStatus = false;
        authenticated = true; key.value = ""; key.placeholder = "密钥已验证"; updateForm();
      }
      submitText.textContent = "正在打开…"; status.textContent = "密钥已验证，正在加载会话…";
      await connect(controller.signal);
    } catch (error) {
      if (!panel.isConnected) return;
      const wasAuthenticated = authenticated;
      if (error.code === 423 && !authenticated) {
        ++lockVersion;
        stopLoginStatusRead(); checkingLoginStatus = false;
        setLoginLocked(true);
        scheduleLoginStatusRead();
      }
      if (error.code === 401) {
        authenticated = false; key.placeholder = "粘贴或输入访问密钥";
      }
      const invalid = !authenticated && [400, 401].includes(error.code);
      const message = loginLocked ? loginLockedMessage
        : controller.signal.aborted ? "连接超时，请检查网络后重试。"
        : error instanceof TypeError ? "无法连接服务，请检查网络和远程访问地址后重试。"
        : wasAuthenticated && error.code === 401 ? "授权未生效，请重新输入访问密钥。"
        : invalid ? invalidKeyMessage
        : error.message || "连接失败，请稍后重试。";
      setError(message, invalid);
      status.textContent = authenticated ? "密钥已验证，可重新加载，无需再次输入。" : "";
    } finally {
      clearTimeout(timeout); submitting = false; updateForm();
      if (panel.isConnected && key.getAttribute("aria-invalid") === "true") {
        key.focus({ preventScroll: true }); key.select();
      }
    }
  });
  const help = element("details", "pair-help");
  const helpTitle = element("summary"); helpTitle.append(document.createTextNode("在哪里获取访问密钥？"), icon("chevron"));
  help.append(helpTitle, element("p", "muted", "访问密钥由本机 CodexElves 管理器的「远程会话」页面设置，请使用设置时保存的密钥。它不是 OpenAI API Key。"));
  const footer = element("footer", "pair-footer");
  const appearanceButton = button("", "设置", openAppearance, "pair-appearance-button text-button");
  appearanceButton.append(icon("settings"), element("span", "", "设置"));
  footer.append(element("small", "muted", "密钥勿外传"), appearanceButton);
  panel.append(heading, form, help, footer); page.append(panel); app.append(page); updateForm();
  if (document.hidden) checkingLoginStatus = false;
  else void readLoginStatus();
  updateForm();
  if (matchMedia("(min-width: 761px) and (hover: hover) and (pointer: fine)").matches) key.focus({ preventScroll: true });
  announceAppReady();
}
export function requireAuthentication() {
  clearRemoteChannels();
  disconnect();
  setPendingNotificationOpen(null);
  setAuthenticationRequired(true);
  void clearPrivateInputs().catch(() => {});
  setSession(null); setSelectedId(null); setThread(null); setSidebar(null);
  postPlatformMessage("auth-required");
  showLogin();
}
export function showConnectionRecovery(error) {
  showLogin();
  loginPageCleanup?.();
  const page = element("div", "pair-page");
  const panel = element("main", "pair-panel");
  const heading = element("header", "pair-heading");
  const mark = element("div", "brand-mark", "✦"); mark.setAttribute("aria-hidden", "true");
  const brand = element("div", "pair-brand");
  brand.append(mark, element("h1", "", "CodexElves"));
  heading.append(brand, element("p", "pair-description muted", "当前无法连接远程服务。"));
  const message = element("p", "pair-status error-text", connectionErrorMessage(error));
  message.setAttribute("role", "alert");
  const retry = element("button", "primary pair-submit", "重新连接");
  retry.type = "button";
  const exit = element("button", "pair-exit", "退出");
  exit.type = "button";
  const status = element("p", "pair-status muted", "设备授权仍保留，恢复连接后会继续打开会话。");
  const controller = new AbortController();
  let exiting = false;
  setLoginPageCleanup(() => {
    controller.abort();
    setLoginPageCleanup(null);
  });
  retry.onclick = async () => {
    if (exiting || !panel.isConnected) return;
    retry.disabled = true;
    retry.textContent = "正在重试…";
    status.textContent = "正在连接远程服务…";
    try {
      await connect(controller.signal);
    } catch (retryError) {
      if (exiting || !panel.isConnected) return;
      if (retryError.code === 401) {
        requireAuthentication();
        return;
      }
      message.textContent = connectionErrorMessage(retryError);
      status.textContent = "设备授权仍保留，恢复连接后会继续打开会话。";
      retry.disabled = false;
      retry.textContent = "重新连接";
    }
  };
  exit.onclick = async () => {
    if (exiting || !await confirmAction("退出", "将清理当前设备的本地草稿并返回连接页面；再次进入需要重新输入访问密钥。", "退出", true)
        || !panel.isConnected || exiting) return;
    exiting = true;
    setAuthenticationRequired(true);
    controller.abort();
    disconnect();
    savePending(null);
    setPendingNotificationOpen(null);
    const cleanup = clearPrivateInputs().catch(() => {});
    setSession(null);
    retry.disabled = true;
    exit.disabled = true;
    exit.textContent = "正在退出…";
    const logoutController = new AbortController();
    const timeout = setTimeout(() => logoutController.abort(), 2000);
    let revoked = false;
    try {
      await api("/api/logout", {}, undefined, logoutController.signal);
      revoked = true;
    } catch {
      // 断网时仍完成本地退出，Android 通过桥清除授权 Cookie。
    } finally { clearTimeout(timeout); }
    await cleanup;
    postPlatformMessage("logged-out");
    showLogin();
    if (!revoked && !isAndroidContainer()) notice("已退出本地会话；设备授权可在电脑端撤销。");
  };
  panel.append(heading, message, retry, exit, status);
  page.append(panel);
  app.replaceChildren(page);
  announceAppReady();
}
export function handleInitialConnectionError(error) {
  if (initialAuthenticationRequired) return;
  if (error.code === 401) {
    requireAuthentication();
    return;
  }
  showConnectionRecovery(error);
}
