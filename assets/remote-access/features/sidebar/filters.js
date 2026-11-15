// 侧栏会话类型筛选与项目分组的显示上限。

import { $ } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { activePopup, renderPopup } from "../../ui/popup.js";
import { renderSidebar } from "../../core/actions.js";
import { applyAppearance } from "../appearance/apply.js";
import { appearance } from "../appearance/theme.js";
import { availableSessionInstances, conversationInstanceId, sessionInstanceEnabled } from "../session/instances.js";
import { projectVisibleThreadCounts, setRecentLimit } from "./list-state.js";

export function sessionFilterValue(preferences = appearance) {
  const filter = preferences.sessionFilter;
  const values = Array.isArray(filter) ? filter : [filter];
  // 用户要求“全部会话”与实例多选互斥：以 all 或实例 ID 数组保存，旧的单选字符串仍可迁移。
  if (values.includes("all")) return "all";
  const selected = [];
  for (const value of values) {
    if (typeof value !== "string" || !/^[a-z][a-z0-9:-]{0,255}$/.test(value)) continue;
    if (value.includes(":")) { selected.push(value); continue; }
    // 旧类型选择优先迁到已启用的原默认实例，其次同类型首个已启用实例。
    // 同类型全部关闭时保留默认实例筛选为空，不能意外扩大到其它类型。
    const instances = availableSessionInstances().filter(instance => instance.backend === value);
    const enabled = instances.filter(instance => sessionInstanceEnabled(instance.id, preferences));
    const id = (enabled.find(instance => instance.legacyDefault) || enabled[0]
      || instances.find(instance => instance.legacyDefault) || instances[0])?.id;
    if (id) selected.push(id);
  }
  return selected.length ? [...new Set(selected)] : "all";
}
function sessionFilterOptions() {
  // 用户要求未开启的实例不出现在筛选菜单中；“全部会话”也只涵盖已开启实例。
  return [{ value: "all", label: "全部会话", icon: "allSessions" },
    ...availableSessionInstances().filter(instance => sessionInstanceEnabled(instance.id))
      .map(instance => ({ value: instance.id, label: instance.label,
        icon: ["codex", "claude"].includes(instance.backend) ? instance.backend : "allSessions" }))];
}
function toggleSessionFilter(value, preferences = appearance) {
  if (value === "all") { preferences.sessionFilter = "all"; return true; }
  if (!availableSessionInstances().some(instance => instance.id === value
      && sessionInstanceEnabled(value, preferences))) return false;
  // 每次操作读取当前状态，不使用菜单打开时的选择快照；选择 all 同步清空其它项，避免快速点击互相覆盖。
  const filter = sessionFilterValue(preferences);
  const selected = new Set(filter === "all" ? [] : filter.filter(id => sessionInstanceEnabled(id, preferences)));
  if (selected.has(value)) selected.delete(value); else selected.add(value);
  preferences.sessionFilter = selected.size ? [...selected] : "all";
  return true;
}
export function renderSessionFilter() {
  const trigger = $("#sidebar-type-filter"); if (!trigger) return;
  const filter = sessionFilterValue(), options = sessionFilterOptions();
  const instance = filter !== "all" && filter.length === 1
    ? availableSessionInstances().find(value => value.id === filter[0]) : null;
  const label = filter === "all" ? "全部会话" : filter.length === 1
    ? instance?.label || "实例暂不可用" : `已选 ${filter.length} 个会话实例`;
  // 会话状态刷新不重建未变化的多选菜单，避免替换用户正在点击的节点或打断键盘焦点。
  const signature = JSON.stringify([filter, options, label, instance?.backend]);
  if (trigger.sessionFilterSignature === signature) return;
  trigger.sessionFilterSignature = signature;
  trigger.replaceChildren(icon(instance && ["codex", "claude"].includes(instance.backend) ? instance.backend : "allSessions"));
  trigger.dataset.backend = instance?.backend || "all"; trigger.title = label;
  trigger.setAttribute("aria-label", `筛选会话实例：${label}`);
  trigger.value = filter === "all" ? "all" : filter[0];
  trigger.menuOptions = options.map(option => ({
    ...option, checkable: true, selected: filter === "all" ? option.value === "all" : filter.includes(option.value),
    // 用户要求仅一个已开启实例时点击关闭，多个时保留；options 含“全部会话”，故超过两项才表示多个实例。
    keepOpen: options.length > 2,
    action: () => {
      if (!toggleSessionFilter(option.value)) return;
      applyAppearance(true); setRecentLimit(40); projectVisibleThreadCounts.clear(); renderSidebar();
    },
  }));
  if (activePopup?.trigger === trigger) renderPopup();
}
export function sessionTypeVisible(item, preferences = appearance) {
  const instanceId = conversationInstanceId(item), filter = sessionFilterValue(preferences);
  // 最近、项目、置顶和搜索共用实例 ID 过滤，同类型的不同 WSL 实例不能互相混入。
  return sessionInstanceEnabled(instanceId, preferences) && (filter === "all" || filter.includes(instanceId));
}
