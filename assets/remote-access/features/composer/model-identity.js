// 模型身份比对只依赖目录和当前后端；与目录加载、会话快照写入分开，避免设置与压缩展示形成循环依赖。
import { models } from "./options-state.js";
import { isClaude } from "../session/capabilities.js";

export function modelForId(id) {
  if (typeof id !== "string" || !id) return null;
  // 用户要求 Claude 完整模型 ID 与原生目录别名共用推理等级；只采用显式 resolvedModel，Codex 仍按原值精确匹配。
  return models.find(model => model.model === id)
    || (isClaude() && models.find(model => model.resolvedModel === id)) || null;
}
export function modelIdentityMatches(left, right) {
  if (!left || !right) return false;
  if (left === right) return true;
  const model = modelForId(left);
  return !!model && model === modelForId(right);
}
