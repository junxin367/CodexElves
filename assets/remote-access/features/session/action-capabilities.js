// 会话动作能力：已发送消息能否编辑、Claude 原生命令是否可用；依赖关联会话视图与模型目录，因此不放在 capabilities.js。

import { creating, selectedId, thread } from "../../core/state.js";
import { modelContextKey } from "../composer/model-catalog.js";
import { modelDefaults, modelsScope } from "../composer/options-state.js";
import { claudeThreadId, threadReadOnly } from "./capabilities.js";
import { relatedViewForThread } from "./related.js";

export function claudeCommandSupported(name) {
  // 原生命令只接入共享 CLI 会话，旧 print 会话不能借用实例目录的能力开放入口。
  if (!creating && thread?.capabilities?.queue !== true) return false;
  const catalog = modelsScope === modelContextKey() ? modelDefaults.nativeCapabilities : null;
  return typeof catalog?.[name] === "boolean" ? catalog[name]
    : !creating && thread?.capabilities?.[name === "goal" ? "goalComposer" : name] === true;
}
export function threadSentMessageEditAllowed(id = selectedId) {
  if (claudeThreadId(id)) return thread?.id === id && thread.capabilities?.messageEdit === true
    && thread.messageEditReady === true && !threadReadOnly(id);
  if (!id) return false;
  const view = relatedViewForThread(id);
  return view?.kind !== "side" && !(thread?.id === id && thread.viewKind === "side");
}
