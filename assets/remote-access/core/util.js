// 与业务无关的基础工具。

export const randomId = () => globalThis.crypto?.randomUUID?.() || [...crypto.getRandomValues(new Uint8Array(24))].map(byte => byte.toString(16).padStart(2, "0")).join("");
