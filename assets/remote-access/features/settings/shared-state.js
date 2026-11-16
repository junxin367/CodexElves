// 用户要求仅外观按客户端保存；非外观偏好以当前服务返回的快照为准，草稿和设备授权不进入此对象。
export function defaultSharedSettings() {
  return {
    conversationWidth: 950, projectThreadLimit: 5,
    sessionTypes: { codex: true, claude: false }, sessionInstances: null, sessionFilter: "all",
    sidebar: { mode: null, chats: null, projects: null, pinned: null, showPinned: false },
    newChat: {}, notifications: { enabled: false, approval: true, question: true, completed: true, failed: false },
    conversationNaming: { enabled: false, instanceId: "codex:local", model: "", systemPrompt: "",
      maxTitleChars: 36, refreshEnabled: true, refreshEveryTurns: 5 }
  };
}
export const sharedSettingsState = {
  scope: null, ready: false, saving: false, notificationsReady: false, error: "", revision: -1, epoch: "", instanceRevision: -1,
  settings: defaultSharedSettings(), stored: {}
};
const sharedSettingsListeners = new Set();
export function subscribeSharedSettings(listener) {
  sharedSettingsListeners.add(listener);
  return () => sharedSettingsListeners.delete(listener);
}
export function notifySharedSettings(change = {}) {
  for (const listener of sharedSettingsListeners) listener(change);
}
export function sharedSettingsWritable(currentSession) {
  return sharedSettingsState.ready && !sharedSettingsState.saving && currentSession && !currentSession.readOnly;
}
