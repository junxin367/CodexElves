// 输入区统计：会话 Token 统计与上下文用量提示。

import { $, element } from "../../ui/dom.js";
import { closePopup } from "../../ui/popup.js";
import {
  connected, creating, nativeConnectionVersion, pageLeaving, selectedId, selectionVersion, session,
  thread, turns
} from "../../core/state.js";
import { api } from "../api.js";
import { closeComposerMenus } from "./composer-menus.js";
import { messageVersion } from "../conversation/history.js";
import { activeBackend, isActiveThread, privateInputScope } from "../session/capabilities.js";
import { activeInstanceId } from "../session/instances.js";
import { currentThreadDisplay, rememberThreadDisplay } from "../session/thread-cache.js";

export let contextUsageTip = null;
let composerMetricsState = null;
function composerMetricsNumber(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER;
}
function composerMetricsHasTurn(value) {
  return ["tokensPerSecond", "steps", "turnTokens"].some(key => composerMetricsNumber(value?.[key]));
}
export function normalizeComposerMetrics(value) {
  if (!value || !["codex-rollout", "claude-native"].includes(value.source)
      || typeof value.turnId !== "string" || value.turnId.length > 120) return null;
  const result = { source: value.source, turnId: value.turnId };
  for (const key of ["inputTokens", "outputTokens", "turnTokens", "steps", "totalTokens", "durationMs", "tokensPerSecond"])
    result[key] = composerMetricsNumber(value[key])
      && (key === "tokensPerSecond" || Number.isSafeInteger(value[key])) ? value[key] : null;
  // 用户要求无数据时不显示；只有原生实际返回的可展示数值才算统计，零值仍是有效数据。
  return composerMetricsHasTurn(result) || composerMetricsNumber(result.totalTokens) ? result : null;
}
function composerMetricsCompact(value) {
  if (!composerMetricsNumber(value)) return "—";
  const divisor = value >= 1e6 ? 1e6 : value >= 1e3 ? 1e3 : 1;
  return `${Number((value / divisor).toFixed(divisor === 1 ? 0 : 1))}${divisor === 1e6 ? "m" : divisor === 1e3 ? "k" : ""}`;
}
function composerMetricNumber(state, key, value, format = String) {
  state.metricNumbers ||= new Map();
  let record = state.metricNumbers.get(key);
  const text = format(value);
  if (!record) {
    const node = element("span", "composer-metric-number"), label = element("span", "composer-metric-value", text);
    node.append(label);
    record = { node, label, value, text, dispose: null };
    state.metricNumbers.set(key, record);
    // 用户要求无数据时隐藏；首次取得数值直接展示，不从虚构的零开始计数。
    return node;
  }
  if (record.text === text) { record.value = value; return record.node; }
  const previous = record.value, previousText = record.text;
  record.dispose?.();
  record.value = value; record.text = text; record.label.textContent = text;
  if (typeof record.node.animate !== "function" || typeof matchMedia !== "function"
      || matchMedia("(prefers-reduced-motion: reduce)").matches) return record.node;
  const direction = value > previous ? 1 : -1, animations = [];
  let overlay = null;
  const before = /^(\d+)(?:\.(\d+))?([km]?)$/u.exec(previousText), after = /^(\d+)(?:\.(\d+))?([km]?)$/u.exec(text);
  if (before && after && before[3] === after[3]) {
    const decimals = Math.max(before[2]?.length || 0, after[2]?.length || 0);
    const start = Math.round(Number(`${before[1]}.${before[2] || "0"}`) * 10 ** decimals);
    const end = Math.round(Number(`${after[1]}.${after[2] || "0"}`) * 10 ** decimals);
    const jobs = [];
    overlay = element("span", "composer-metric-rolling");
    overlay.setAttribute("aria-hidden", "true");
    const character = (char, old, place) => {
      const digit = /\d/u.test(char), cell = element("span", `composer-metric-character${digit ? " composer-metric-digit" : ""}`);
      cell.dataset.character = char;
      if (digit) {
        const from = /\d/u.test(old) ? Number(old) : 0, to = Number(char);
        const distance = direction > 0 ? (to - from + 10) % 10 : (from - to + 10) % 10;
        const changes = Math.abs(Math.floor(end / place) - Math.floor(start / place));
        // 用户要求逐位向上递增、向下递减；小变化逐个经过，大跳变最多滚一圈加目标差值，避免手机长时间空转。
        const steps = changes <= 19 && changes % 10 === distance ? changes : 10 + distance;
        if (changes || !/\d/u.test(old)) {
          const sequence = Array.from({ length: steps + 1 }, (_, index) => String((from + direction * index % 10 + 10) % 10));
          if (direction < 0) sequence.reverse();
          const track = element("span", "composer-metric-track");
          track.dataset.digits = sequence.join("\n");
          cell.dataset.character = "";
          cell.append(track);
          jobs.push({ cell, track, steps, position: Math.round(Math.log10(place)), appearing: !/\d/u.test(old) });
        }
      }
      overlay.append(cell);
    };
    for (let index = 0; index < after[1].length; ++index) {
      const position = after[1].length - index - 1;
      character(after[1][index], before[1].at(-position - 1) || "", 10 ** (position + decimals));
    }
    if (after[2]) {
      character(".", ".", 1);
      for (let index = 0; index < after[2].length; ++index)
        character(after[2][index], before[2]?.[index] || "0", 10 ** (decimals - index - 1));
    }
    for (const unit of after[3]) character(unit, unit, 1);
    if (jobs.length) {
      record.node.append(overlay); record.node.classList.add("composer-metric-animating");
      // 最低位先动，高位最多错开 5 × 24ms；总时长限制为 320–700ms，低于既有 2 秒统计刷新间隔。
      const duration = Math.min(700, Math.max(320, Math.max(...jobs.map(job => job.steps)) * 45));
      const lowest = Math.min(...jobs.map(job => job.position));
      for (const job of jobs) {
        const delay = Math.min(5, job.position - lowest) * 24;
        animations.push(job.track.animate([
          { transform: `translateY(${direction > 0 ? 0 : -job.steps}em)` },
          { transform: `translateY(${direction > 0 ? -job.steps : 0}em)` }
        ], { duration: duration - delay, delay, fill: "both", easing: "cubic-bezier(.2,.65,.3,1)" }));
        if (job.appearing) animations.push(job.cell.animate([{ opacity: 0 }, { opacity: 1 }],
          { duration: duration - delay, delay, fill: "both" }));
      }
    } else overlay = null;
  }
  if (!animations.length) {
    // k/m 单位或显示精度变化时整体过渡，不能把旧单位下的数字套在新单位上滚动。
    animations.push(record.node.animate([
      { transform: `translateY(${direction > 0 ? ".4" : "-.4"}em)`, opacity: .4 },
      { transform: "translateY(0)", opacity: 1 }
    ], { duration: 320, easing: "ease-out" }));
  }
  const dispose = () => {
    if (record.dispose !== dispose) return;
    record.dispose = null;
    for (const animation of animations) animation.cancel();
    overlay?.remove(); record.node.classList.remove("composer-metric-animating");
  };
  record.dispose = dispose;
  // 新值在动画完成前到达时取消旧动画；旧 Promise 不得清理新数字，读屏与复制仍采用真实最终值。
  void Promise.all(animations.map(animation => animation.finished)).then(dispose, dispose);
  return record.node;
}
function composerMetricsContext() {
  const owner = privateInputScope(session), instanceId = activeInstanceId();
  if (!owner || !instanceId || creating || !selectedId || thread?.id !== selectedId
      || thread.viewKind === "subagent" || pageLeaving || document.hidden
      || session?.capabilities?.sessionMetrics !== true) return null;
  return { key: JSON.stringify([owner, instanceId, selectedId, selectionVersion, nativeConnectionVersion]),
    payload: { threadId: selectedId, instanceId, backend: activeBackend() } };
}
export function resetComposerMetrics() {
  if (composerMetricsState) {
    clearTimeout(composerMetricsState.timer);
    composerMetricsState.controller?.abort();
    for (const record of composerMetricsState.metricNumbers?.values() || []) record.dispose?.();
  }
  composerMetricsState = null;
  const node = $("#session-metrics");
  if (node) node.hidden = true;
}
export function fitComposerMetrics() {
  const node = $("#session-metrics");
  if (!node || node.hidden) return;
  const quota = $("#claude-usage");
  // 用户要求手机共用已有底行：额度保持原右侧位置；只限制新增统计的可用宽度，避免数字变长后盖住额度。
  node.style.setProperty("--composer-quota-width", `${quota && !quota.hidden ? quota.getBoundingClientRect().width + 8 : 0}px`);
}
function composerMetricsLatestTurn(state) {
  if (state.turnsReference !== turns || state.turnsSize !== turns.size || state.messageVersion !== messageVersion) {
    state.turnsReference = turns; state.turnsSize = turns.size; state.messageVersion = messageVersion;
    state.latestTurn = [...turns.values()].at(-1);
  }
  return state.latestTurn;
}
function renderComposerMetrics() {
  const node = $("#session-metrics"), state = composerMetricsState;
  if (!node) return;
  const value = state?.value;
  node.hidden = !value;
  if (!value) return;
  const latest = composerMetricsLatestTurn(state);
  // 用户要求新轮次等待统计时保留上一轮完整显示；快照仍保留原 turnId，不能伪造为新轮次实测。
  const sameTurn = !latest || !value.turnId || latest.id === value.turnId;
  const metrics = value, labels = [], round = [];
  // 未提供的单项也不放“—”占位；已有实测值继续显示，直到收到可展示的新轮次统计。
  if (composerMetricsNumber(metrics.tokensPerSecond)) labels.push(`${Math.round(metrics.tokensPerSecond)}tk/s`);
  if (composerMetricsNumber(metrics.steps)) round.push(`${metrics.steps}步`);
  if (composerMetricsNumber(metrics.turnTokens)) round.push(composerMetricsCompact(metrics.turnTokens));
  if (round.length) labels.push(`本轮 ${round.join(" ")}`);
  if (composerMetricsNumber(value.totalTokens)) labels.push(`总 ${composerMetricsCompact(value.totalTokens)}`);
  node.hidden = !labels.length;
  if (node.hidden) return;
  const signature = JSON.stringify(labels);
  if (node.dataset.metrics !== signature) {
    node.dataset.metrics = signature;
    const groups = [], used = new Set();
    const number = (key, amount, format) => { used.add(key); return composerMetricNumber(state, key, amount, format); };
    if (composerMetricsNumber(metrics.tokensPerSecond)) {
      const group = element("span", "composer-metric-group");
      group.append(number("speed", metrics.tokensPerSecond, value => String(Math.round(value))), element("span", "", "tk/s"));
      groups.push(group);
    }
    if (round.length) {
      const group = element("span", "composer-metric-group", "本轮 ");
      if (composerMetricsNumber(metrics.steps)) group.append(number("steps", metrics.steps), element("span", "", "步"));
      if (composerMetricsNumber(metrics.turnTokens)) {
        if (composerMetricsNumber(metrics.steps)) group.append(element("span", "", " "));
        group.append(number("turnTokens", metrics.turnTokens, composerMetricsCompact));
      }
      groups.push(group);
    }
    if (composerMetricsNumber(value.totalTokens)) {
      const group = element("span", "composer-metric-group", "总 ");
      group.append(number("totalTokens", value.totalTokens, composerMetricsCompact)); groups.push(group);
    }
    for (const [key, record] of state.metricNumbers || []) {
      if (!used.has(key)) { record.dispose?.(); state.metricNumbers.delete(key); }
    }
    node.replaceChildren(...groups);
  }
  const exact = number => composerMetricsNumber(number) ? number.toLocaleString("zh-CN") : "尚未提供";
  const description = (sameTurn ? "" : "新轮次统计尚未返回，当前显示上一轮数据。")
    + `${sameTurn ? "本轮" : "上一轮"}输入：${exact(metrics?.inputTokens)} Token；输出：${exact(metrics?.outputTokens)} Token；`
    + `合计：${exact(metrics?.turnTokens)} Token；模型调用：${exact(metrics?.steps)} 次；`
    + `当前会话累计：${exact(value?.totalTokens)} Token。`
    + "速度为本轮输出 Token 除以原生执行时长的平均值，包含思考、工具与等待。m 表示百万 Token，k 表示千 Token。";
  // 复用编辑区现有主题提示组件，不再创建一套 hover 配色或监听器。
  node.title = description;
  node.setAttribute("aria-label", `${labels.join("，")}。${description}`);
  fitComposerMetrics();
}
export function syncComposerMetrics() {
  if (!$("#session-metrics")) return;
  const context = composerMetricsContext();
  if (!context) { resetComposerMetrics(); return; }
  if (composerMetricsState?.key !== context.key) {
    resetComposerMetrics();
    composerMetricsState = { key: context.key, value: normalizeComposerMetrics(currentThreadDisplay()?.sessionMetrics),
      checkedAt: 0, pending: false, controller: null, timer: null, fingerprint: "", retryAt: 0 };
  }
  const state = composerMetricsState, active = isActiveThread();
  const fingerprint = JSON.stringify([composerMetricsLatestTurn(state)?.id, active]);
  if (state.fingerprint !== fingerprint) { state.fingerprint = fingerprint; state.checkedAt = 0; }
  renderComposerMetrics();
  clearTimeout(state.timer); state.timer = null;
  if (!connected) { state.controller?.abort(); return; }
  if (state.pending) return;
  // 统计只在可见的当前会话后台读取；执行中每 2 秒、空闲每 15 秒，发送与停止均不等待统计。
  const interval = active ? 2000 : 15000;
  const remaining = Math.max(state.retryAt - Date.now(), state.checkedAt + interval - Date.now());
  if (remaining > 0) {
    state.timer = setTimeout(() => { if (composerMetricsState === state) syncComposerMetrics(); }, remaining);
    return;
  }
  state.pending = true;
  const controller = new AbortController();
  state.controller = controller;
  void api("/api/read/sessionMetrics", context.payload, undefined, controller.signal, { resourcePriority: "background" })
    .then(result => {
      if (composerMetricsState !== state || state.controller !== controller || controller.signal.aborted
          || composerMetricsContext()?.key !== state.key
          || result?.ok !== true || result.threadId !== context.payload.threadId || result.backend !== context.payload.backend) return;
      const value = normalizeComposerMetrics(result.metrics);
      // 新轮次可能先只返回累计或空统计；这时保留上一轮快照，等本轮速度、步数或 Token 实测值到达再替换。
      if (value && (!composerMetricsHasTurn(state.value) || composerMetricsHasTurn(value))) {
        state.value = value;
        if (thread?.id === result.threadId) thread.sessionMetrics = value;
        rememberThreadDisplay({ id: result.threadId, sessionMetrics: value }, { persist: !isActiveThread() });
      }
      state.retryAt = 0;
    })
    .catch(() => {
      if (composerMetricsState === state && state.controller === controller && !controller.signal.aborted)
        state.retryAt = Date.now() + 15000;
    })
    .finally(() => {
      if (composerMetricsState !== state || state.controller !== controller) return;
      state.checkedAt = !controller.signal.aborted && state.fingerprint === fingerprint ? Date.now() : 0;
      state.pending = false; state.controller = null;
      syncComposerMetrics();
    });
}
export function tokenCount(value) { return value >= 1000 ? `${Number((value / 1000).toFixed(1))}k` : String(Math.round(value)); }
export function contextUsage(info) {
  const modelWindow = info?.modelContextWindow, value = info?.last?.totalTokens;
  const hasModelWindow = typeof modelWindow === "number" && Number.isFinite(modelWindow) && modelWindow > 0;
  // 用户要求所有实例共用 Codex 显示：模型容量优先，缺失时直接采用配置压缩窗口，不按提前压缩比例额外折算。
  const total = hasModelWindow ? modelWindow : info?.compaction?.windowTokens;
  if (typeof total !== "number" || !Number.isFinite(total) || total <= 0
      || typeof value !== "number" || !Number.isFinite(value) || value < 0) return null;
  const used = Math.min(value, total);
  // 用户要求沿用 Codex 原有百分比：统一按用量除以所用容量四舍五入，不另用实例特有的原生百分比。
  return { used, total, percent: Math.round(used / total * 100) };
}
function contextUsageText() {
  const info = currentThreadDisplay()?.tokenUsage, usage = contextUsage(info);
  if (usage) {
    return {
      value: `${tokenCount(usage.used)} / ${usage.percent}%，${tokenCount(usage.total)}`,
      description: `最后一次请求的上下文占用：${usage.used.toLocaleString()} / ${usage.total.toLocaleString()} Token，不是累计用量。`
    };
  }
  // 用户允许容量未知时诚实降级：显示原生已知用量，但不估算容量或百分比；环形指示仍保持“—”。
  const value = info?.last?.totalTokens;
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? { value: `${tokenCount(value)} Token，容量尚未提供`,
      description: `最后一次请求的上下文占用：${value.toLocaleString()} Token，不是累计用量；尚未提供可用容量，无法计算百分比。` }
    : { value: "上下文信息尚未提供", description: "会话尚未提供有效的上下文容量或最后一次请求用量。" };
}
export function closeContextUsageTip() {
  if (!contextUsageTip) return;
  const { node, trigger, closeTimer } = contextUsageTip;
  contextUsageTip = null;
  clearTimeout(closeTimer);
  trigger.setAttribute("aria-expanded", "false");
  trigger.removeAttribute("aria-describedby");
  if (node.hasAttribute("popover") && node.matches(":popover-open")) node.hidePopover();
  node.remove();
}
export function fitContextUsageTip() {
  if (!contextUsageTip) return;
  const { node, trigger } = contextUsageTip;
  if (!trigger.isConnected || trigger.hidden) { closeContextUsageTip(); return; }
  const viewport = window.visualViewport, gap = 4, padding = 8;
  const top = viewport?.offsetTop || 0, left = viewport?.offsetLeft || 0;
  const width = viewport?.width || window.innerWidth, height = viewport?.height || window.innerHeight;
  const box = trigger.getBoundingClientRect();
  if (box.bottom <= top || box.top >= top + height || box.right <= left || box.left >= left + width) {
    closeContextUsageTip(); return;
  }
  const above = Math.max(0, box.top - top - gap - padding);
  const below = Math.max(0, top + height - box.bottom - gap - padding);
  const availableWidth = Math.max(0, width - padding * 2), maxWidth = Math.min(300, availableWidth);
  node.style.width = "max-content";
  node.style.minWidth = `${Math.min(140, maxWidth)}px`;
  node.style.maxWidth = `${maxWidth}px`;
  node.style.maxHeight = `${Math.min(height * .8, Math.max(above, below))}px`;
  const upwards = below < node.getBoundingClientRect().height && above > below;
  node.style.maxHeight = `${Math.min(height * .8, upwards ? above : below)}px`;
  const actual = node.getBoundingClientRect();
  node.style.left = `${Math.max(left + padding, Math.min(box.left, left + width - actual.width - padding))}px`;
  node.style.top = `${Math.max(top + padding, Math.min(upwards ? box.top - gap - actual.height : box.bottom + gap,
    top + height - actual.height - padding))}px`;
  node.dataset.direction = upwards ? "up" : "down";
}
export function updateContextUsage() {
  const trigger = $("#context-info");
  if (!trigger) return;
  const usage = contextUsage(currentThreadDisplay()?.tokenUsage), text = contextUsageText();
  let ring = $(".context-usage-ring", trigger);
  if (!ring) {
    ring = element("span", "context-usage-ring");
    ring.setAttribute("aria-hidden", "true"); trigger.append(ring);
  }
  ring.textContent = usage ? `${usage.percent}%` : "—";
  ring.style.setProperty("--context-progress", `${(usage?.percent || 0) * 3.6}deg`);
  trigger.setAttribute("aria-label", `${text.value}。${text.description}`);
  if (contextUsageTip?.trigger === trigger) {
    const { node } = contextUsageTip;
    if (node.firstElementChild.textContent !== text.value) node.firstElementChild.textContent = text.value;
    fitContextUsageTip();
  }
}
function showContextUsageTip(pinned = false) {
  const trigger = $("#context-info");
  if (!trigger?.isConnected) return;
  if (contextUsageTip?.trigger === trigger) {
    clearTimeout(contextUsageTip.closeTimer);
    contextUsageTip.pinned ||= pinned;
    fitContextUsageTip(); return;
  }
  closePopup(); closeComposerMenus();
  const node = element("div", "ui-popup context-usage-tooltip");
  node.id = "context-usage-tooltip"; node.setAttribute("role", "tooltip");
  // 用户要求与 Codex 一致，只显示用量、百分比和容量，不再增加 Claude 或压缩配置说明行。
  // 用户要求用量文字颜色对齐 Codex 正文，不能沿用 muted 提示色；仍只显示原有单行用量。
  node.append(element("div", "context-usage-value"));
  contextUsageTip = { node, trigger, pinned, closeTimer: null };
  trigger.setAttribute("aria-expanded", "true");
  trigger.setAttribute("aria-describedby", node.id);
  if (typeof node.showPopover === "function") node.setAttribute("popover", "manual");
  document.body.append(node);
  updateContextUsage();
  if (contextUsageTip?.node !== node) return;
  if (node.hasAttribute("popover")) node.showPopover();
  fitContextUsageTip();
  node.onpointerenter = () => clearTimeout(contextUsageTip?.closeTimer);
  node.onpointerleave = scheduleContextUsageTipClose;
}
function scheduleContextUsageTipClose() {
  const tip = contextUsageTip;
  if (!tip || tip.pinned || document.activeElement === tip.trigger
      && document.documentElement.dataset.inputModality === "keyboard") return;
  clearTimeout(tip.closeTimer);
  tip.closeTimer = setTimeout(() => { if (contextUsageTip === tip) closeContextUsageTip(); }, 120);
}
export function mountContextUsage() {
  const trigger = $("#context-info");
  trigger.onclick = () => {
    if (contextUsageTip?.pinned) closeContextUsageTip();
    else showContextUsageTip(true);
  };
  trigger.onpointerenter = event => { if (event.pointerType === "mouse") showContextUsageTip(); };
  trigger.onpointerleave = scheduleContextUsageTipClose;
  trigger.onfocus = () => {
    if (document.documentElement.dataset.inputModality === "keyboard") showContextUsageTip();
  };
  trigger.onblur = () => { if (!contextUsageTip?.pinned) scheduleContextUsageTipClose(); };
  trigger.onpointerdown = event => {
    if (event.isPrimary && event.button === 0 && document.activeElement === $("#prompt")) event.preventDefault();
  };
}
export function compactUsageDescription(usage) {
  return usage ? `（已使用${usage.percent}%/${tokenCount(usage.total)}）` : "";
}
