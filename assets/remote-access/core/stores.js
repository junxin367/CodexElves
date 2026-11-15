// 本地恢复存储实例：草稿/回执恢复、会话正文缓存与只读结果缓存，全页面共用同一实例。

import { createReadCache, createRecoveryStore, createTranscriptCache } from "../recovery-store.js";

export const recoveryStore = createRecoveryStore();
export const transcriptCache = createTranscriptCache();
export const readCache = createReadCache();
