// 剪贴板写入：优先使用异步剪贴板接口，不可用时回退到临时文本框复制。

import { notice } from "./notice.js";

export async function copyText(text) {
  try {
    if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
    else {
      const previous = document.activeElement;
      const field = document.createElement("textarea");
      field.value = text;
      field.readOnly = true;
      field.style.position = "fixed";
      field.style.opacity = "0";
      document.body.append(field);
      try { field.select(); if (!document.execCommand("copy")) throw new Error("copy failed"); }
      finally { field.remove(); previous?.focus?.({ preventScroll: true }); }
    }
    notice("已复制");
  }
  catch { notice("无法访问剪贴板，请手动选择代码复制", true); }
}
