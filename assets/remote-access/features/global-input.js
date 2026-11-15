// 页面级输入路由：点击/焦点切换时收起弹层与提示、Esc 与原生返回键的分层处理。

import { $ } from "../ui/dom.js";
import { notice } from "../ui/notice.js";
import { activePopup, closePopup } from "../ui/popup.js";
import { closeComposerMenus } from "../core/actions.js";
import { selectedId, selectionToken } from "../core/state.js";
import { claudeUsageState, closeClaudeUsageTip } from "./composer/claude-usage.js";
import { closeModelMenu } from "./composer/composer-menus.js";
import { closeContextUsageTip, contextUsageTip } from "./composer/metrics.js";
import { finishQueueDrag } from "./queue/queue.js";
import { queueDrag } from "./queue/state.js";
import {
  relatedNavigationOwner, relatedSessionView, relatedThreadOperations, sideCreationHistoryKey,
  sideCreationView
} from "./session/related-state.js";
import {
  dismissSideCreation, handleSideThreadShortcut, handleTransientHistoryPop, ownsRelatedOperation,
  requestSideCreationBack, returnToParentSession, showSideCreation, updateSideCreationStatus
} from "./session/related.js";
import { closeMobileSidebar } from "./shell-state.js";

// 点击、焦点切换与键盘输入时收起弹层和提示，并处理 Tab 输入模式与 Esc 分层关闭。
export function installGlobalInputHandlers() {
  document.addEventListener("pointerdown", event => {
    document.documentElement.dataset.inputModality = "pointer";
    if (activePopup && !activePopup.node.contains(event.target) && !activePopup.trigger.contains(event.target)) closePopup();
    if (contextUsageTip && !event.target.closest("#context-info,#context-usage-tooltip")) closeContextUsageTip();
    if (claudeUsageState?.tip && !event.target.closest("#claude-usage,#claude-usage-tooltip")) closeClaudeUsageTip();
    if (!event.target.closest("#model-button,#model-settings,#skill-menu,#prompt,#context-info,#context-usage-tooltip")) closeComposerMenus();
  });
  document.addEventListener("focusin", event => {
    if (activePopup && !activePopup.node.contains(event.target) && !activePopup.trigger.contains(event.target)) closePopup();
    if (contextUsageTip && !event.target.closest("#context-info,#context-usage-tooltip")) closeContextUsageTip();
    if (claudeUsageState?.tip && !event.target.closest("#claude-usage,#claude-usage-tooltip")) closeClaudeUsageTip();
    if (!event.target.closest("#model-button,#model-settings")) closeModelMenu();
  });
  document.addEventListener("focusout", event => {
    if (event.target.closest("#model-button,#model-settings")
        && !event.relatedTarget?.closest?.("#model-button,#model-settings")) checkModelMenuFocus();
  });
  window.addEventListener("blur", checkModelMenuFocus);
  document.addEventListener("keydown", event => {
    if (event.isComposing) return;
    if (handleSideThreadShortcut(event)) return;
    if (event.key === "Tab") document.documentElement.dataset.inputModality = "keyboard";
    if (event.key === "Escape") {
      if (queueDrag) { event.preventDefault(); finishQueueDrag(); return; }
      if (contextUsageTip) { event.preventDefault(); closeContextUsageTip(); return; }
      if (claudeUsageState?.tip) { event.preventDefault(); closeClaudeUsageTip(); return; }
      if (activePopup) { event.preventDefault(); event.stopPropagation(); closePopup(true); return; }
      const modelOpen = $("#model-settings") && !$("#model-settings").hidden;
      const skillOpen = $("#skill-menu") && !$("#skill-menu").hidden;
      closeComposerMenus();
      if (modelOpen) { event.preventDefault(); $("#model-button").focus(); return; }
      if (skillOpen) { event.preventDefault(); return; }
      if (!document.querySelector("dialog[open]") && $(".workspace")?.classList.contains("sidebar-open")) {
        event.preventDefault(); closeMobileSidebar();
        return;
      }
      if (!event.defaultPrevented && relatedSessionView && !document.querySelector("dialog[open]")) {
        event.preventDefault();
        void returnToParentSession().catch(error => notice(error.message, true));
      }
      if (!event.defaultPrevented && sideCreationView && !document.querySelector("dialog[open]")) {
        event.preventDefault(); requestSideCreationBack();
      }
    }
  });
}
function checkModelMenuFocus() {
  queueMicrotask(() => {
    const menu = $("#model-settings");
    if (!menu || menu.hidden) return;
    if (document.hasFocus() && document.activeElement?.closest("#model-button,#model-settings")) return;
    closeModelMenu();
  });
}
// 原生返回键与浏览器后退：依次关闭预览、弹层、侧边会话创建页和关联会话。
export function installBackNavigationHandlers() {
  window.addEventListener("codex-elves-native-back", event => {
    if (event.defaultPrevented) return;
    const preview = document.querySelector("#image-preview-dialog[open], #video-preview-dialog[open]");
    if (preview) { event.preventDefault(); preview.close(); return; }
    if (activePopup) { event.preventDefault(); closePopup(true); return; }
    if (sideCreationView && !document.querySelector("dialog[open]")) {
      event.preventDefault(); requestSideCreationBack(); return;
    }
    if (relatedSessionView && !document.querySelector("dialog[open], .session-tools-screen.session-tools-visible")) {
      event.preventDefault();
      void returnToParentSession().catch(error => notice(error.message, true));
    }
  });
  window.addEventListener("popstate", handleTransientHistoryPop, true);
  window.addEventListener("popstate", event => {
    const target = event.state?.[sideCreationHistoryKey];
    if (sideCreationView && (target?.owner !== relatedNavigationOwner
        || target.requestId !== sideCreationView.record.id))
      dismissSideCreation({ canceled: true, animate: true });
    if (!sideCreationView && target?.owner === relatedNavigationOwner && !relatedSessionView) {
      const record = [...relatedThreadOperations.values()].find(record => record.id === target.requestId
        && ownsRelatedOperation(record) && record.parentThreadId === selectedId);
      if (record) {
        record.autoOpen = true; record.selectionToken = selectionToken;
        showSideCreation(record, { recordHistory: false }); updateSideCreationStatus(record);
      }
    }
  }, true);
}
