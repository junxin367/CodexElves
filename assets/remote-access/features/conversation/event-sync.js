// 实时事件同步状态：已通知的状态与 Token 用量版本，以及延迟重载与侧栏刷新调度。

import { notice } from "../../ui/notice.js";
import { loadSidebar, reloadThread } from "../../core/actions.js";
import { refreshTimer, setRefreshTimer, setSidebarTimer, sidebarTimer } from "../session/connection-state.js";

export let tokenUsageVersion = 0, notifiedTokenUsage = null;
export function setTokenUsageVersion(value) { return tokenUsageVersion = value; }
export function setNotifiedTokenUsage(value) { return notifiedTokenUsage = value; }
export let statusVersion = 0, notifiedStatus = null;
export function setStatusVersion(value) { return statusVersion = value; }
export function setNotifiedStatus(value) { return notifiedStatus = value; }
export function scheduleReload() {
  if (refreshTimer) return;
  setRefreshTimer(setTimeout(() => {
    setRefreshTimer(null);
    void reloadThread().catch(error => notice(error.message, true));
  }, 450));
}
export function scheduleSidebarRefresh() {
  clearTimeout(sidebarTimer);
  setSidebarTimer(setTimeout(() => void loadSidebar(null, { fresh: true }).catch(() => {}), 1200));
}
