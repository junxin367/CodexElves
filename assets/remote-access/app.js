// 远程会话网页入口：只负责按固定顺序装配各模块并启动连接。
// 各功能模块只做声明，不在加载时产生副作用；页面级事件监听、会话存储恢复等副作用
// 均由下方按原有执行顺序显式调用，调整顺序前须核对监听注册次序与恢复依赖。

import { configureAndroidDiagnostics } from "./diagnostics.js";
import { installAndroidClickHapticFeedback, postPlatformMessage } from "./platform.js";
import { registerAppMenuCloser } from "./ui/popup.js";
import { registerActions } from "./core/actions.js";
import { session } from "./core/state.js";
import { api } from "./features/api.js";
import { installSidePanelAppearanceSync, startAppearance } from "./features/appearance/apply.js";
import { installPwaCacheStateListener } from "./features/appearance/dialog.js";
import { restoreQuestionReceipts } from "./features/approval-state.js";
import { renderApprovals } from "./features/approvals.js";
import { closeComposerMenus, installComposerMenuViewportListeners } from "./features/composer/composer-menus.js";
import { updateComposer } from "./features/composer/composer.js";
import { loadModels } from "./features/composer/models.js";
import { loadSkillChoices, refreshSkillCatalog } from "./features/composer/skills.js";
import { restoreCompactions } from "./features/conversation/compaction.js";
import { receive } from "./features/conversation/events.js";
import { renderMessages, scheduleMessages } from "./features/conversation/history-view.js";
import { reloadThread } from "./features/conversation/history.js";
import { restoreStoredPending } from "./features/conversation/pending-store.js";
import { mutate, resolvePending } from "./features/conversation/pending.js";
import { selectThread, showWelcome } from "./features/conversation/selection.js";
import { installBackNavigationHandlers, installGlobalInputHandlers } from "./features/global-input.js";
import { restoreGoalComposerRecoveries } from "./features/goal/composer.js";
import { renderGoalStatus } from "./features/goal/status.js";
import { installLocalFileOpenListener } from "./features/local-files.js";
import { renderItem } from "./features/messages/items.js";
import { installMarkdownRefresh } from "./features/messages/media.js";
import { messageEditor } from "./features/messages/message-edit.js";
import { installNotificationOpenListener } from "./features/notifications.js";
import { applyQueuedSendResult, renderQueueArea } from "./features/queue/queue.js";
import { renderQueuedEditRecoveryControls } from "./features/queue/queued-edit-recovery.js";
import { restoreQueuedEdits } from "./features/queue/state.js";
import {
  initialAuthenticationRequired, restoreAuthenticationRequirement
} from "./features/session/connection-state.js";
import { installChannelConfigurationObserver } from "./features/session/connection.js";
import { connect, reconnectChangedAuthorization } from "./features/session/lifecycle.js";
import { handleInitialConnectionError, requireAuthentication, showLogin } from "./features/session/login.js";
import { installPageLifecycle, retryConnectionRecovery } from "./features/session/page-lifecycle.js";
import {
  applyWorkspaceWindowMode, installSessionWindowTitleSync, installSidePanelMessageListener
} from "./features/session/side-panel.js";
import { shell } from "./features/shell.js";
import { installSidebarPreferenceSync } from "./features/sidebar/actions.js";
import { restoreThreadDeletions } from "./features/sidebar/deletion-state.js";
import { loadSidebar, renderSidebar } from "./features/sidebar/list.js";

// 上层动作须在任何安装函数运行前注册：下层模块经 core/actions.js 调用这些实现，见 docs/remote-access-web-architecture.md。
registerActions({
  applyQueuedSendResult, closeComposerMenus, loadModels, loadSidebar, loadSkillChoices, refreshSkillCatalog,
  messageEditor, mutate, receive, reconnectChangedAuthorization, reloadThread, renderApprovals,
  renderGoalStatus, renderItem, renderMessages, renderQueueArea, renderQueuedEditRecoveryControls,
  renderSidebar, requireAuthentication, resolvePending, retryConnectionRecovery, scheduleMessages,
  selectThread, shell, showWelcome, updateComposer
});

installAndroidClickHapticFeedback();
document.documentElement.dataset.inputModality = "pointer";
restoreAuthenticationRequirement();
installPwaCacheStateListener();
restoreThreadDeletions();
installSidebarPreferenceSync();
restoreQuestionReceipts();
restoreStoredPending();
restoreGoalComposerRecoveries();
restoreQueuedEdits();
restoreCompactions();
applyWorkspaceWindowMode();
installSessionWindowTitleSync();
startAppearance();
installLocalFileOpenListener();
installComposerMenuViewportListeners();
installGlobalInputHandlers();
installBackNavigationHandlers();
registerAppMenuCloser(closeComposerMenus);
installMarkdownRefresh();
installNotificationOpenListener();
installSidePanelMessageListener();
installSidePanelAppearanceSync();
installPageLifecycle();
postPlatformMessage("ready");
configureAndroidDiagnostics({
  getSession: () => session,
  upload: (requestId, bundle) => api("/api/diagnostics", { requestId, bundle }, undefined, undefined,
    { resourcePriority: "background" })
});
installChannelConfigurationObserver();
if (initialAuthenticationRequired) showLogin();
else connect().catch(handleInitialConnectionError);
