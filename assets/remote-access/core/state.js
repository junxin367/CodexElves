// 跨领域共享的会话运行状态。
// 只放被多个功能模块共同读写的状态；读取直接使用导出的实时绑定，跨模块写入必须调用对应 setter，
// 便于检索所有写入点。只属于单一功能的状态应放在该功能模块内。

import { $ } from "../ui/dom.js";
import { randomId } from "./util.js";

export const app = $("#app");
export let session = null, sidebar = null, selectedId = null, thread = null;
export function setSession(value) { return session = value; }
export function setSidebar(value) { return sidebar = value; }
export function setSelectedId(value) { return selectedId = value; }
export function setThread(value) { return thread = value; }
export let connected = false, selectionVersion = 0, busy = false, creating = false;
export function setConnected(value) { return connected = value; }
export function setSelectionVersion(value) { return selectionVersion = value; }
export function setBusy(value) { return busy = value; }
export function setCreating(value) { return creating = value; }
export let turns = new Map();
export function setTurns(value) { return turns = value; }
export let nativeConnectionVersion = 0;
export function setNativeConnectionVersion(value) { return nativeConnectionVersion = value; }
export let pageLeaving = false;
export function setPageLeaving(value) { return pageLeaving = value; }
export let stopping = false;
export function setStopping(value) { return stopping = value; }
export let selectionToken = randomId();
export function setSelectionToken(value) { return selectionToken = value; }
