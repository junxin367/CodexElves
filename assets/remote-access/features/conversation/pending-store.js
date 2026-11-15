// 待发送持久化：把待发送消息保存到本地，页面重开后恢复。

import { recoveryStorageMessage } from "../../recovery-store.js";
import { notice } from "../../ui/notice.js";
import { recoveryStore } from "../../core/stores.js";
import { recoveryScope } from "../composer/drafts.js";
import { pending, pendingTimer, setPending, validSkillSelection } from "./pending-state.js";
import { goalComposerStorageRecord, validGoalComposerDraft } from "../goal/composer.js";
import { goalComposerAction, goalComposerRecoveries, hydrateGoalComposerRecord } from "../goal/state.js";
import { queuedEditActions } from "../queue/state.js";

// 从会话存储恢复未完成的写操作回执；校验失败时记为无效，交由恢复流程提示用户。
export function restoreStoredPending() {
  try {
    const stored = hydrateGoalComposerRecord(JSON.parse(sessionStorage.getItem("codex-elves-pending") || "null"));
    if (stored) setPending(typeof stored.id === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(stored.id)
      && (stored.action === "reorderQueued"
        ? typeof stored.payload?.threadId === "string" && Array.isArray(stored.payload.messageIds)
          && Array.isArray(stored.payload.expectedMessageIds)
          && stored.payload.messageIds.length > 0
          && stored.payload.messageIds.length === stored.payload.expectedMessageIds.length
          && new Set(stored.payload.messageIds).size === stored.payload.messageIds.length
          && stored.payload.messageIds.every(id => typeof id === "string" && stored.payload.expectedMessageIds.includes(id))
        : queuedEditActions.has(stored.action)
        ? typeof stored.payload?.editId === "string" && typeof stored.payload?.threadId === "string"
        : ["steerQueued", "removeQueued"].includes(stored.action)
        ? typeof stored.payload?.threadId === "string" && typeof stored.payload?.messageId === "string"
        : ["send", "create", "editMessage", "setGoal"].includes(stored.action)
          && typeof stored.payload?.prompt === "string" && validSkillSelection(stored.payload.skills)
          && (stored.action !== "setGoal" || stored.payload.goalMode === true && typeof stored.payload.threadId === "string")
          && (stored.payload.goalMode !== true || goalComposerAction(stored.action, stored.payload)
            && (validGoalComposerDraft(stored.draft) || typeof stored.recoveryRef === "string")))
      ? stored : { invalid: true });
  } catch { setPending({ invalid: true }); }
}
export function savePending(value) {
  if (value) {
    if (!Number.isFinite(value.time)) value.time = Date.now();
    try { sessionStorage.setItem("codex-elves-pending", JSON.stringify(goalComposerStorageRecord(value))); }
    catch (error) {
      if (!value.recoveryRef) throw new Error(recoveryStorageMessage(error, true));
      // 完整记录已先写入 IndexedDB，刷新时也可以通过原 ID 找回。
    }
  } else {
    try { sessionStorage.removeItem("codex-elves-pending"); }
    catch { notice("无法清理浏览器回执记录，重新打开页面后可能仍需人工核对。", true); }
    const previous = pending;
    if (previous?.recoveryRef && recoveryScope)
      void recoveryStore.remove(recoveryScope, "active", previous.recoveryRef).catch(() => {});
    if (previous?.recoveryRef && recoveryScope
        && ![...goalComposerRecoveries.values()].some(record => record.id === previous.id))
      void recoveryStore.remove(recoveryScope, "pending", previous.recoveryRef).catch(() => {});
  }
  clearTimeout(pendingTimer);
  setPending(value);
}
