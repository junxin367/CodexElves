// 远程会话网页入口：只负责按固定顺序装配各模块并启动连接。
// 各功能模块只做声明，不在加载时产生副作用；页面级事件监听、会话存储恢复等副作用
// 均由下方按原有执行顺序显式调用，调整顺序前须核对监听注册次序与恢复依赖。

import { configureAndroidDiagnostics } from "./diagnostics.js";
import { installAndroidClickHapticFeedback, postPlatformMessage } from "./platform.js";
import { registerAppMenuCloser } from "./ui/popup.js";
import { session } from "./core/state.js";
import { api } from "./features/api.js";
import { installPwaCacheStateListener } from "./features/appearance/dialog.js";
import { installSidePanelAppearanceSync, startAppearance } from "./features/appearance/theme.js";
import { restoreQuestionReceipts } from "./features/approvals.js";
import { closeComposerMenus, installComposerMenuViewportListeners } from "./features/composer/composer-menus.js";
import { restoreCompactions } from "./features/conversation/compaction.js";
import { restoreStoredPending } from "./features/conversation/pending.js";
import { installBackNavigationHandlers, installGlobalInputHandlers } from "./features/global-input.js";
import { restoreGoalComposerRecoveries } from "./features/goal/composer.js";
import { installLocalFileOpenListener } from "./features/local-files.js";
import { installMarkdownRefresh } from "./features/messages/media.js";
import { installNotificationOpenListener } from "./features/notifications.js";
import { restoreQueuedEdits } from "./features/queue/queued-edit.js";
import { installChannelConfigurationObserver } from "./features/session/connection.js";
import { connect, installPageLifecycle } from "./features/session/lifecycle.js";
import {
  handleInitialConnectionError, initialAuthenticationRequired, restoreAuthenticationRequirement,
  showLogin
} from "./features/session/login.js";
import { applyWorkspaceWindowMode, installSidePanelMessageListener } from "./features/session/side-panel.js";
import { installSidebarPreferenceSync, restoreThreadDeletions } from "./features/sidebar/actions.js";

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
