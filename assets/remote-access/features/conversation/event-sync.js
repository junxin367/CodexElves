// 实时事件同步状态：已通知的状态与 Token 用量版本，以及延迟重载与侧栏刷新调度。

import { notice } from "../../ui/notice.js";
import { loadSidebar, reloadThread } from "../../core/actions.js";
import { pageLeaving, selectedId, selectionVersion, session } from "../../core/state.js";
import { privateInputScope } from "../session/capabilities.js";
import { foregroundReadGeneration, refreshTimer, setRefreshTimer, setSidebarTimer, sidebarTimer } from "../session/connection-state.js";
import { threadDeleted } from "../sidebar/deletion-state.js";

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
    const id = selectedId, version = selectionVersion, scope = privateInputScope(session),
      generation = foregroundReadGeneration;
    void reloadThread().catch(error => {
      // 用户要求删除成功提示不被迟到错误覆盖；错误传播期间也可能切换会话或授权，展示前须再次核对。
      if (!pageLeaving && id && selectedId === id && version === selectionVersion
          && generation === foregroundReadGeneration && scope === privateInputScope(session) && !threadDeleted(id))
        notice(error.message, true);
    });
  }, 450));
}
export function scheduleSidebarRefresh() {
  clearTimeout(sidebarTimer);
  setSidebarTimer(setTimeout(() => void loadSidebar(null, { fresh: true }).catch(() => {}), 1200));
}
