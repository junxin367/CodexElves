// 系统通知跳转：从通知打开关联会话，未就绪时暂存并在页面可用后打开。

import { isAndroidContainer } from "../platform.js";
import { $ } from "../ui/dom.js";
import { notice } from "../ui/notice.js";
import { selectThread } from "../core/actions.js";
import { pageLeaving, session } from "../core/state.js";
import { initialAuthenticationRequired } from "./session/connection-state.js";

export let pendingNotificationOpen = null;
export function setPendingNotificationOpen(value) { return pendingNotificationOpen = value; }
function closeDialogsForNotification() {
  for (const dialog of [...document.querySelectorAll("dialog[open]")].reverse()) {
    if (dialog.classList.contains("confirmation-dialog")) dialog.close("cancel");
    else dialog.close();
  }
}
function openNotificationThread(threadId) {
  closeDialogsForNotification();
  if (!session || initialAuthenticationRequired || pageLeaving || !$("#prompt") || !$("#messages")) {
    pendingNotificationOpen = threadId;
    return;
  }
  void selectThread(threadId).catch(error => notice(`无法打开通知关联的会话：${error.message}`, true));
}
export function drainPendingNotificationOpen() {
  if (!pendingNotificationOpen || !session || initialAuthenticationRequired || pageLeaving
      || !$("#prompt") || !$("#messages")) return;
  const threadId = pendingNotificationOpen;
  pendingNotificationOpen = null;
  openNotificationThread(threadId);
}
// Android 容器内点击系统通知时打开对应会话；只接受仅含合法 threadId 的事件。
export function installNotificationOpenListener() {
  if (isAndroidContainer()) {
    window.addEventListener("codex-elves-notification-open", event => {
      const detail = event?.detail;
      if (!event?.cancelable || !detail || typeof detail !== "object" || Array.isArray(detail)
          || Object.keys(detail).length !== 1 || !Object.hasOwn(detail, "threadId")
          || typeof detail.threadId !== "string" || !/^[A-Za-z0-9_-]{1,120}$/.test(detail.threadId)) return;
      event.preventDefault();
      openNotificationThread(detail.threadId);
    });
  }
}
