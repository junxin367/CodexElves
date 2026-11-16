// 上层动作入口：刷新输入区、切换会话、重新读取、渲染侧栏与正文等跨功能调用。
// 下层模块从这里调用，实现由 app.js 启动时 registerActions 注册，避免下层 import 上层形成循环依赖；
// 未注册就调用会直接抛错，暴露装配遗漏。新增动作：在此添加同名转发函数，并在 app.js 的 registerActions 中注册实现。

const implementations = new Map();

export function registerActions(entries) {
  for (const [name, action] of Object.entries(entries)) implementations.set(name, action);
}

function run(name, args) {
  const action = implementations.get(name);
  if (!action) throw new Error(`上层动作尚未注册：${name}`);
  return action(...args);
}

export function applyQueuedSendResult(...args) { return run("applyQueuedSendResult", args); } // features/queue/queue.js
export function closeComposerMenus(...args) { return run("closeComposerMenus", args); } // features/composer/composer-menus.js
export function loadModels(...args) { return run("loadModels", args); } // features/composer/models.js
export function loadSidebar(...args) { return run("loadSidebar", args); } // features/sidebar/list.js
export function loadSkillChoices(...args) { return run("loadSkillChoices", args); } // features/composer/skills.js
export function refreshSkillCatalog(...args) { return run("refreshSkillCatalog", args); } // features/composer/skills.js
export function messageEditor(...args) { return run("messageEditor", args); } // features/messages/message-edit.js
export function mutate(...args) { return run("mutate", args); } // features/conversation/pending.js
export function receive(...args) { return run("receive", args); } // features/conversation/events.js
export function reconnectChangedAuthorization(...args) { return run("reconnectChangedAuthorization", args); } // features/session/lifecycle.js
export function reloadThread(...args) { return run("reloadThread", args); } // features/conversation/history.js
export function renderApprovals(...args) { return run("renderApprovals", args); } // features/approvals.js
export function renderGoalStatus(...args) { return run("renderGoalStatus", args); } // features/goal/status.js
export function renderItem(...args) { return run("renderItem", args); } // features/messages/items.js
export function renderMessages(...args) { return run("renderMessages", args); } // features/conversation/history-view.js
export function renderQueueArea(...args) { return run("renderQueueArea", args); } // features/queue/queue.js
export function renderQueuedEditRecoveryControls(...args) { return run("renderQueuedEditRecoveryControls", args); } // features/queue/queued-edit-recovery.js
export function renderSidebar(...args) { return run("renderSidebar", args); } // features/sidebar/list.js
export function requireAuthentication(...args) { return run("requireAuthentication", args); } // features/session/login.js
export function resolvePending(...args) { return run("resolvePending", args); } // features/conversation/pending.js
export function retryConnectionRecovery(...args) { return run("retryConnectionRecovery", args); } // features/session/page-lifecycle.js
export function scheduleMessages(...args) { return run("scheduleMessages", args); } // features/conversation/history-view.js
export function selectThread(...args) { return run("selectThread", args); } // features/conversation/selection.js
export function shell(...args) { return run("shell", args); } // features/shell.js
export function showWelcome(...args) { return run("showWelcome", args); } // features/conversation/selection.js
export function updateComposer(...args) { return run("updateComposer", args); } // features/composer/composer.js
