import { requestRemote } from "./channels.js";
import { bindLocalFileReferences, markdown, releaseMermaidBlocks, renderMermaidBlocks, withStableMessageLayout } from "./markdown.js";
import { safeInlineImageSource } from "./attachments.js";

const node = (tag, className = "", text = "") => {
  const result = document.createElement(tag);
  result.className = className; result.textContent = text;
  return result;
};
const control = (label, action, className = "text-button") => {
  const result = node("button", className, label);
  result.type = "button"; result.addEventListener("click", action);
  return result;
};
// 用户要求去掉 Windows 长路径前缀；只处理展示文本，读取参数、目录边界和缓存身份仍使用原路径。
export function displayWorkspacePath(value) {
  const path = String(value || "");
  if (/^(?:\\\\\?\\|\/\/\?\/)UNC[\\/]/iu.test(path))
    return displayWorkspaceName(`\\\\${path.slice(8)}`);
  return displayWorkspaceName(path.replace(/^(?:\\\\\?\\|\/\/\?\/)/u, ""));
}
export function displayWorkspaceName(value) {
  // 用户要求文件名不显示方框：还原 WSL/DrvFS 的 8 类可打印保留字符（U+F000 + ASCII）。
  // 只处理显示文本，不转换其他私用字符；目录分层、请求、缓存和选中身份始终使用原始路径。
  return String(value || "").replace(/[\uf022\uf02a\uf03a\uf03c\uf03e\uf03f\uf05c\uf07c]/gu,
    character => String.fromCharCode(character.charCodeAt(0) - 0xf000));
}
export function workspaceBranchLabel(git) {
  if (!git || typeof git !== "object") return "";
  if (git.unavailable === true) return "分支不可用";
  if (typeof git.branch === "string" && git.branch.length > 0 && git.branch.length <= 16384
      && !/[\u0000-\u001f\u007f]/u.test(git.branch)) return git.branch;
  if (typeof git.detachedHead === "string" && /^(?:[a-f\d]{40}|[a-f\d]{64})$/iu.test(git.detachedHead))
    return `分离 HEAD · ${git.detachedHead.slice(0, 8)}`;
  return git.branch === null && git.detachedHead === null ? "无 Git 分支" : "分支不可用";
}
export function setWorkspaceBranch(badge, git, placeholder = "") {
  const label = workspaceBranchLabel(git) || placeholder;
  badge.hidden = !label;
  badge.querySelector(".workspace-branch-label").textContent = label;
  badge.title = git?.branch === label ? `当前分支：${label}` : label;
  badge.setAttribute("aria-label", badge.title);
}
export function createWorkspaceBranch(git, placeholder = "") {
  const badge = node("span", "workspace-branch");
  const glyph = node("span", "workspace-branch-icon");
  glyph.setAttribute("aria-hidden", "true");
  glyph.innerHTML = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="4" cy="3" r="1.5"/><circle cx="12" cy="4" r="1.5"/><circle cx="4" cy="13" r="1.5"/><path d="M4 4.5v7M12 5.5v1A3.5 3.5 0 0 1 8.5 10H4"/></svg>';
  badge.append(glyph, node("span", "workspace-branch-label"));
  setWorkspaceBranch(badge, git, placeholder);
  return badge;
}
export function createWorkspaceRepository(repository) {
  if (!repository || typeof repository.name !== "string" || !repository.name
      || repository.name.length > 16384 || /[\u0000-\u001f\u007f]/u.test(repository.name)) return null;
  // 用户要求仓库行右侧显示该仓库的真实分支，不能用仓库名或外层项目分支兜底。
  const badge = createWorkspaceBranch(repository.git, "分支不可用");
  badge.classList.add("session-files-repository");
  return badge;
}
export function createWorkspaceLocation(initialPath = "") {
  // 用户要求路径与分支清楚区分：路径和带图标、边框的分支标签使用独立元素，长文本分别省略。
  const element = node("div", "session-tools-heading-location");
  const path = node("span", "session-tools-heading-path");
  const branch = createWorkspaceBranch();
  element.append(path, branch);
  const set = (value, git) => {
    path.textContent = displayWorkspacePath(value);
    path.title = path.textContent;
    setWorkspaceBranch(branch, git);
  };
  set(initialPath);
  return { element, path, branch, set };
}
const parentPath = path => path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
const fileName = path => path.replaceAll("\\", "/").split("/").at(-1) || "文件";
const revisionValid = value => typeof value === "string" && /^[a-f0-9]{64}$/u.test(value);
const imageValid = value => typeof value === "string"
  && /^data:image\/(?:png|jpeg|gif|webp|svg\+xml);base64,[a-zA-Z0-9+/=]+$/u.test(value);
// 与 document_preview.rs 同步：只接收结构化文字，不把办公文档的 HTML、宏或链接作为可执行内容。
export function validRichFilePreview(value) {
  let characters = 0, cells = 0, blocks = 0;
  const text = (value, maximum = 1024 * 1024) => {
    if (typeof value !== "string" || value.length > maximum) return false;
    characters += value.length;
    return characters <= 1024 * 1024 + 60 * 1024;
  };
  const rows = values => Array.isArray(values) && values.length <= 1000
    && values.every(row => Array.isArray(row) && row.length <= 100
      && (cells += row.length) <= 100000 && row.every(value => text(value, 16 * 1024)));
  // 行号递增检查独立于单元格数组，避免将稀疏表格的真实行号误作连续下标。
  const numberedRows = sheet => Array.isArray(sheet.rows) && sheet.rows.length <= 1000
    && sheet.rows.every((row, index) => Number.isSafeInteger(row?.number) && row.number > 0
      && row.number <= 4294967295 && (!index || row.number > sheet.rows[index - 1].number)
      && (row.height == null || Number.isFinite(row.height) && row.height >= 1 && row.height <= 2048)
      && Array.isArray(row.cells) && row.cells.length <= sheet.columnCount
      && (cells += row.cells.length) <= 100000 && row.cells.every(value => text(value, 16 * 1024)));
  const layout = sheet => {
    if (sheet.defaultRowHeight != null && (!Number.isFinite(sheet.defaultRowHeight)
        || sheet.defaultRowHeight < 1 || sheet.defaultRowHeight > 2048)) return false;
    if (sheet.styles != null && (!Array.isArray(sheet.styles) || sheet.styles.length > 4096
        || sheet.styles.some(style => !style
          || [style.color, style.background].some(color => color != null && !/^#[0-9a-f]{6}$/iu.test(color))
          || style.fontSize != null && (!Number.isFinite(style.fontSize) || style.fontSize < 6 || style.fontSize > 72)
          || ["bold", "italic", "underline", "strike", "wrap"].some(key => typeof style[key] !== "boolean")
          || !["left", "center", "right", "justify"].includes(style.horizontal)
          || !["top", "middle", "bottom"].includes(style.vertical)))) return false;
    if (sheet.rows.some(row => row.styleIds != null && (!Array.isArray(row.styleIds)
        || row.styleIds.length !== sheet.columnCount
        || row.styleIds.some(id => !Number.isSafeInteger(id) || id < 0 || id >= (sheet.styles?.length || 0))))) return false;
    // CSV/TSV/ODS 没有原始列宽时后台返回 []，原始布局回退 120px；非空宽度允许窄列，仍须逐列完整且不超过 480px。
    if (sheet.columnWidths != null && (!Array.isArray(sheet.columnWidths)
        || sheet.columnWidths.length !== 0 && sheet.columnWidths.length !== sheet.columnCount
        || sheet.columnWidths.some(width => !Number.isSafeInteger(width) || width < 1 || width > 480))) return false;
    if (sheet.merges == null) return true;
    if (!Array.isArray(sheet.merges) || sheet.merges.length > 10000) return false;
    const rowNumbers = new Set(sheet.rows.map(row => row.number)), occupied = new Set();
    return sheet.merges.every(merge => {
      if (!merge || ![merge.startRow, merge.endRow, merge.startColumn, merge.endColumn].every(Number.isSafeInteger)
          || merge.startRow < 1 || merge.endRow > 4294967295 || merge.startRow > merge.endRow
          || merge.endRow - merge.startRow >= 1000 || merge.startColumn < 0
          || merge.startColumn > merge.endColumn || merge.endColumn >= sheet.columnCount) return false;
      for (let row = merge.startRow; row <= merge.endRow; row++) {
        if (!rowNumbers.has(row)) return false;
        for (let column = merge.startColumn; column <= merge.endColumn; column++) {
          const key = `${row}:${column}`;
          if (occupied.has(key) || occupied.size >= 100000) return false;
          occupied.add(key);
        }
      }
      return true;
    });
  };
  if (value?.kind === "spreadsheet") return Array.isArray(value.sheets) && value.sheets.length <= 20
    && value.sheets.every(sheet => sheet && text(sheet.name, 512)
      && Number.isSafeInteger(sheet.columnCount) && sheet.columnCount >= 0 && sheet.columnCount <= 100
      && typeof sheet.truncated === "boolean" && numberedRows(sheet) && layout(sheet));
  if (value?.kind === "document") return Array.isArray(value.sections) && value.sections.length <= 100
    && value.sections.every(section => section && text(section.title, 512) && Array.isArray(section.blocks)
      && section.blocks.every(block => ++blocks <= 2000 && block
        && (["paragraph", "heading"].includes(block.kind) ? text(block.text)
          : block.kind === "table" && rows(block.rows))));
  return false;
}
function spreadsheetColumn(index) {
  let label = "";
  for (let value = index + 1; value > 0; value = Math.floor((value - 1) / 26))
    label = String.fromCharCode(65 + (value - 1) % 26) + label;
  return label;
}
function spreadsheetTextMeasurer() {
  const context = typeof document !== "undefined" ? document.createElement("canvas").getContext?.("2d") : null;
  const family = typeof getComputedStyle === "function" ? getComputedStyle(document.documentElement).fontFamily : "sans-serif";
  return (text, style) => {
    const size = style?.fontSize ? style.fontSize * 4 / 3 : 12;
    if (context) {
      context.font = `${style?.italic ? "italic " : ""}${style?.bold ? "700" : "400"} ${size}px ${family}`;
      return Math.max(0, ...text.split(/\r\n?|\n/u).map(line => context.measureText(line).width));
    }
    // 无 Canvas 的环境保留有界估算；正常网页使用真实字体测量，不按文件的宽列猜紧凑宽度。
    return Math.max(0, ...text.split(/\r\n?|\n/u).map(line =>
      Array.from(line).reduce((sum, character) => sum + (/[^\x00-\xff]/u.test(character) ? size : size * .65), 0)));
  };
}
export function compactSpreadsheetWidths(rows, columnCount, styles = [], merges = [], measure = spreadsheetTextMeasurer()) {
  const widths = Array(columnCount).fill(0), covered = new Set(), merged = [], sourceRows = new Map(rows.map(row => [row.number, row]));
  for (const merge of merges) {
    for (let row = merge.startRow; row <= merge.endRow; row++)
      for (let column = merge.startColumn; column <= merge.endColumn; column++)
        if (row !== merge.startRow || column !== merge.startColumn || merge.endColumn > merge.startColumn)
          covered.add(`${row}:${column}`);
    if (merge.endColumn > merge.startColumn) merged.push(merge);
  }
  // 按整张已读取工作表计算，翻页不改变列宽；合并标题约束整体跨度，不能单独撑宽第一列。
  const cache = new Map();
  const textWidth = (row, column) => {
    const text = row?.cells[column] || "", id = row?.styleIds?.[column], key = `${id ?? ""}:${text}`;
    if (!cache.has(key)) cache.set(key, Math.ceil(measure(text, styles[id])));
    return cache.get(key);
  };
  for (const row of rows)
    for (let column = 0; column < row.cells.length; column++)
      if (!covered.has(`${row.number}:${column}`)) widths[column] = Math.max(widths[column], textWidth(row, column));
  for (const merge of merged) {
    const count = merge.endColumn - merge.startColumn + 1;
    const available = widths.slice(merge.startColumn, merge.endColumn + 1).reduce((sum, width) =>
      sum + Math.max(48, Math.min(480, width + 10)), 0);
    const needed = Math.min(count * 480, textWidth(sourceRows.get(merge.startRow), merge.startColumn) + 10);
    let remaining = needed - available;
    let expandable = Array.from({ length: count }, (_, index) => merge.startColumn + index)
      .filter(column => widths[column] + 10 < 480);
    while (remaining > .01 && expandable.length) {
      const share = remaining / expandable.length;
      for (const column of expandable) {
        const before = Math.max(48, widths[column] + 10), extra = Math.min(480 - before, share);
        widths[column] = before + extra - 10; remaining -= extra;
      }
      expandable = expandable.filter(column => widths[column] + 10 < 479.99);
    }
  }
  // 最小 48px、最大 480px；10px 包含左右各 4px、1px 边框及 1px 测量余量。
  // 文本宽度以阅读字号 12px 为基准，CSS 按阅读字号缩放后再封顶；内容再长也不会突破最大列宽。
  return widths.map(width => `clamp(48px,calc(${Math.ceil(width) / 12} * var(--file-reading-font-size,12px) + 10px),480px)`);
}
function applySpreadsheetStyle(cell, style) {
  if (!style) return;
  if (style.background) cell.style.backgroundColor = style.background;
  if (style.color) cell.style.color = style.color;
  // 原文件字号以 pt 记录：默认阅读字号 12px 时按 1pt=4/3px 还原，调整阅读字号时同比缩放。
  if (style.fontSize != null) cell.style.fontSize = `calc(var(--file-reading-font-size,12px) * ${style.fontSize / 9})`;
  cell.style.fontWeight = style.bold ? "700" : "400";
  cell.style.fontStyle = style.italic ? "italic" : "normal";
  cell.style.textDecoration = [style.underline && "underline", style.strike && "line-through"].filter(Boolean).join(" ") || "none";
  cell.style.textAlign = style.horizontal;
  cell.style.verticalAlign = style.vertical;
  cell.style.whiteSpace = style.wrap ? "pre-wrap" : "pre";
  cell.style.overflow = "hidden";
}
function previewTable(rows, columnCount, {
  numbered = false, page = 0, onPage = null, merges = [], columnWidths = [], styles = [], onRange = null,
  layout = "compact", wrap = false, defaultRowHeight = 20
} = {}) {
  // 每页最多渲染 100 行；与后台每表 1000 行上限区分，手机翻页不会一次创建整张大表的 DOM。
  const pageSize = 100, pageCount = Math.max(1, Math.ceil(rows.length / pageSize));
  page = Math.min(Math.max(0, page), pageCount - 1);
  const root = node("div", "session-files-table-group");
  const compactWidths = numbered && layout === "compact" ? compactSpreadsheetWidths(rows, columnCount, styles, merges) : null;
  const rowHeight = row => row.height ?? defaultRowHeight;
  const sizeCell = (element, height) => {
    if (layout !== "original") return;
    // 原始布局使用文件行高；主动开启自动换行时，以原行高为下限增高，避免宽列封顶后正文被裁掉。
    element.style[wrap ? "minHeight" : "height"] = `${Math.max(0, height - 1)}px`;
  };
  const draw = () => {
    root.replaceChildren();
    if (!rows.length || !columnCount) {
      if (onRange) onRange(node("span", "", "0 / 0 行"));
      root.append(node("p", "session-files-empty", "此表格为空")); return;
    }
    const start = page * pageSize, visible = rows.slice(start, start + pageSize);
    const pagination = node("div", "session-files-table-pagination");
    const rangeText = node("span", "", `${start + 1}–${start + visible.length} / ${rows.length} 行`);
    rangeText.title = rangeText.textContent; pagination.append(rangeText);
    if (pageCount > 1) {
      const change = next => { page = next; onPage?.(page); draw(); };
      const previous = control(numbered ? "‹" : "上一页", () => change(page - 1));
      const next = control(numbered ? "›" : "下一页", () => change(page + 1));
      for (const [button, label] of [[previous, "上一页"], [next, "下一页"]]) {
        button.title = label; button.setAttribute("aria-label", label);
        if (numbered) button.className += " session-files-page-button";
      }
      previous.disabled = page === 0; next.disabled = page === pageCount - 1;
      pagination.append(previous, next);
    }
    // 用户要求行数与文件大小同一行；文档的小表不再另占一行，长表翻页控件仍可用。
    if (onRange) onRange(pagination);
    else if (numbered || pageCount > 1) root.append(pagination);
    const scroller = node("div", "session-files-table-scroll");
    scroller.tabIndex = 0; scroller.setAttribute("role", "region");
    scroller.setAttribute("aria-label", "表格内容，可横向滚动");
    const table = node("table", "session-files-table");
    const covered = new Set(), anchors = new Map();
    if (numbered) {
      // 用户要求合并单元格与稳定首列：按原文件列宽固定网格，合并标题不参与单列自动撑宽。
      const widths = Array.from({ length: columnCount }, (_, index) => Math.min(480, columnWidths[index] || 120));
      table.className += " session-files-spreadsheet";
      table.setAttribute("data-layout", layout); table.setAttribute("data-wrap", String(wrap));
      // 用户要求手机行号栏紧凑：实际宽度交给 CSS；总宽与行号 col 共用变量，旋转后不挤压数据列。
      // 按整张工作表最大行号的位数预留空间，长行号完整显示，翻页时宽度不跳变。
      table.style.setProperty("--sheet-row-number-digits", String(String(rows.at(-1).number).length));
      table.style.width = `calc(${compactWidths ? compactWidths.join(" + ") : `${widths.reduce((sum, width) => sum + width, 0)}px`} + var(--sheet-row-number-width,44px))`;
      const columns = node("colgroup"), rowNumbers = node("col");
      rowNumbers.style.width = "var(--sheet-row-number-width,44px)"; columns.append(rowNumbers);
      for (const [index, width] of widths.entries()) {
        const column = node("col"); column.style.width = compactWidths?.[index] || `${width}px`; columns.append(column);
      }
      table.append(columns);
      const sourceRows = new Map(rows.map(row => [row.number, row]));
      for (const merge of merges) {
        const included = visible.filter(row => row.number >= merge.startRow && row.number <= merge.endRow);
        if (!included.length) continue;
        const firstRow = included[0].number, key = `${firstRow}:${merge.startColumn}`;
        // 跨页纵向合并在每页可见段延续；内容始终取原始左上角，不能从被覆盖的空格猜值。
        anchors.set(key, { rows: included.length, columns: merge.endColumn - merge.startColumn + 1,
          text: sourceRows.get(merge.startRow)?.cells[merge.startColumn] || "",
          height: included.reduce((sum, row) => sum + rowHeight(row), 0),
          styleId: sourceRows.get(merge.startRow)?.styleIds?.[merge.startColumn] });
        for (const row of included)
          for (let column = merge.startColumn; column <= merge.endColumn; column++)
            if (`${row.number}:${column}` !== key) covered.add(`${row.number}:${column}`);
      }
    }
    if (numbered) {
      const heading = node("tr"), head = node("thead");
      const corner = node("th", "session-files-row-number", "");
      corner.scope = "col"; heading.append(corner);
      for (let column = 0; column < columnCount; column++) {
        const label = node("th", "", spreadsheetColumn(column)); label.scope = "col"; heading.append(label);
      }
      head.append(heading); table.append(head);
    }
    const body = node("tbody");
    for (const value of visible) {
      const row = node("tr"), cells = numbered ? value.cells : value;
      if (numbered) {
        const number = node("th", "session-files-row-number");
        const label = node("div", "session-files-row-number-label", String(value.number));
        sizeCell(label, rowHeight(value)); number.append(label);
        if (layout === "original") row.style.height = `${rowHeight(value)}px`;
        number.scope = "row"; row.append(number);
      }
      for (let column = 0; column < columnCount; column++) {
        const key = `${value.number}:${column}`;
        if (numbered && covered.has(key)) continue;
        const merge = numbered && anchors.get(key);
        const text = merge ? merge.text : cells[column] || "";
        const cell = node("td", "", numbered ? "" : text);
        if (merge) { cell.rowSpan = merge.rows; cell.colSpan = merge.columns; }
        if (numbered) {
          applySpreadsheetStyle(cell, styles[merge ? merge.styleId : value.styleIds?.[column]]);
          cell.style.whiteSpace = wrap ? "pre-wrap" : "pre";
          const content = node("div", "session-files-cell-content", text);
          sizeCell(content, merge ? merge.height : rowHeight(value));
          const vertical = styles[merge ? merge.styleId : value.styleIds?.[column]]?.vertical;
          content.style.justifyContent = vertical === "middle" ? "center" : vertical === "bottom" ? "flex-end" : "flex-start";
          cell.append(content);
          cell.setAttribute("data-row", String(value.number));
          cell.setAttribute("data-column", String(column));
        }
        row.append(cell);
      }
      body.append(row);
    }
    table.append(body); scroller.append(table); root.append(scroller);
  };
  draw(); return root;
}
export function renderRichFilePreview(value, state = {}, onNavigate = null, { onRange = null } = {}) {
  const root = node("div", "session-files-rich-preview");
  if (value.kind === "spreadsheet") {
    root.className += " session-files-spreadsheet-preview";
    if (!value.sheets.length) {
      root.append(node("p", "session-files-empty", "此文件没有可读取的工作表")); return root;
    }
    const draw = () => {
      root.replaceChildren();
      state.sheet = Number.isSafeInteger(state.sheet) ? Math.min(Math.max(0, state.sheet), value.sheets.length - 1) : 0;
      state.page = Number.isSafeInteger(state.page) ? Math.max(0, state.page) : 0;
      const tabs = node("div", "session-files-sheet-tabs"); tabs.setAttribute("role", "tablist");
      tabs.setAttribute("aria-label", "工作表");
      const panel = node("div", "session-files-sheet-panel"); panel.setAttribute("role", "tabpanel");
      value.sheets.forEach((sheet, index) => {
        const button = control(displayWorkspaceName(sheet.name || `工作表 ${index + 1}`), () => {
          state.sheet = index; state.page = 0; onNavigate?.(); draw();
        });
        button.setAttribute("role", "tab"); button.setAttribute("aria-selected", String(index === state.sheet));
        button.title = button.textContent; tabs.append(button);
      });
      const sheet = value.sheets[state.sheet];
      panel.setAttribute("aria-label", displayWorkspaceName(sheet.name || "工作表"));
      if (sheet.truncated) panel.append(node("p", "session-files-empty", "此工作表仅显示限额内的内容"));
      panel.append(previewTable(sheet.rows, sheet.columnCount, {
        numbered: true, page: state.page, merges: sheet.merges || [], columnWidths: sheet.columnWidths || [],
        styles: sheet.styles || [], onRange, defaultRowHeight: sheet.defaultRowHeight ?? 20,
        layout: state.layout === "original" ? "original" : "compact", wrap: state.wrap === true,
        onPage: page => { state.page = page; onNavigate?.(); }
      }));
      root.append(tabs, panel);
    };
    draw();
  } else {
    if (!value.sections.length) root.append(node("p", "session-files-empty", "此文档没有可读取的正文"));
    for (const section of value.sections) {
      const page = node("section", "session-files-document-section");
      if (section.title) page.append(node("h2", "", section.title));
      for (const block of section.blocks) {
        if (block.kind === "table")
          page.append(previewTable(block.rows, Math.max(0, ...block.rows.map(row => row.length))));
        else page.append(node(block.kind === "heading" ? "h3" : "p", "", block.text));
      }
      root.append(page);
    }
  }
  return root;
}
const bytesLabel = size => size < 1024 ? `${size} B`
  : size < 1024 * 1024 ? `${(size / 1024).toFixed(1)} KiB` : `${(size / 1024 / 1024).toFixed(1)} MiB`;
const fileSort = (left, right) => {
  const leftDirectory = left.kind === "directory" ? 0 : 1;
  const rightDirectory = right.kind === "directory" ? 0 : 1;
  return leftDirectory - rightDirectory
    || left.name.localeCompare(right.name, "zh-CN", { numeric: true, sensitivity: "base" })
    || left.path.localeCompare(right.path, "zh-CN", { numeric: true, sensitivity: "base" });
};
const sortedFiles = files => [...(files || [])].sort(fileSort);
const fileIconKind = path => {
  const name = fileName(path).toLowerCase(), lower = path.toLowerCase();
  if (name === "package.json" || name === "tsconfig.json" || name === "cargo.toml") return "package";
  if (name === "package-lock.json" || name === "pnpm-lock.yaml" || name === "yarn.lock"
      || name === "cargo.lock") return "lock";
  if (name === "dockerfile" || name.startsWith(".dockerfile")) return "docker";
  if (name === ".gitignore" || name === ".gitattributes" || name === ".editorconfig") return "git";
  if (/\.(?:md|mdx|rst|txt|log|docx?|odt|pdf)$/u.test(lower)) return "document";
  if (/\.(?:xlsx?|xlsm|xltx|ods|csv|tsv)$/u.test(lower)) return "spreadsheet";
  if (/\.(?:pptx?)$/u.test(lower)) return "presentation";
  if (/\.(?:png|jpe?g|gif|webp|avif|bmp|ico|svg)$/u.test(lower)) return "image";
  if (/\.(?:css|scss|sass|less|styl)$/u.test(lower)) return "style";
  if (/\.(?:html?|xhtml)$/u.test(lower)) return "html";
  if (/\.(?:js|jsx|mjs|cjs)$/u.test(lower)) return "javascript";
  if (/\.(?:ts|tsx)$/u.test(lower)) return "typescript";
  if (/\.(?:vue|svelte|astro)$/u.test(lower)) return "component";
  if (/\.json$/u.test(lower)) return "json";
  if (/\.(?:ya?ml|toml|ini|conf|config|properties|env)$/u.test(lower)) return "config";
  if (/\.(?:rs)$/u.test(lower)) return "rust";
  if (/\.(?:py)$/u.test(lower)) return "python";
  if (/\.(?:java|kt|kts)$/u.test(lower)) return "java";
  if (/\.(?:go)$/u.test(lower)) return "go";
  if (/\.(?:c|h|cc|cpp|hpp|cs|swift)$/u.test(lower)) return "native";
  if (/\.(?:sh|bash|zsh|ps1|bat|cmd)$/u.test(lower)) return "shell";
  if (/\.(?:sql|graphql|gql|proto)$/u.test(lower)) return "data";
  return "file";
};
const syntaxKeywords = {
  java: "abstract assert boolean break byte case catch char class const continue default do double else enum extends final finally float for goto if implements import instanceof int interface long native new package private protected public return short static strictfp super switch synchronized this throw throws transient try void volatile while record sealed permits var yield".split(" "),
  javascript: "as async await break case catch class const continue debugger default delete do else export extends finally for from function get if implements import in instanceof interface let new of package private protected public return set static super switch this throw try typeof undefined var void while with yield".split(" "),
  typescript: "as async await break case catch class const constructor continue debugger declare default delete do else enum export extends finally for from function get if implements import in infer instanceof interface keyof let module namespace never new of package private protected public readonly return satisfies set static super switch this throw try type typeof undefined var void while with yield".split(" "),
  rust: "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while async dyn".split(" "),
  python: "and as assert async await break case class continue def del elif else except finally for from global if import in is lambda match nonlocal not or pass raise return try while with yield".split(" "),
  json: "true false null".split(" "),
  css: "and as calc case container else extends from if import important media not or supports".split(" "),
  html: "async await checked class defer disabled enctype for hidden id is loading media method name rel required selected slot src style target title type value".split(" "),
  sql: "all alter and as asc begin between by case commit create delete desc distinct drop else end exists from group having in insert into is join like not null on or order select set table then union update values when where".split(" "),
  shell: "case do done elif else esac fi for function if in select then time until while".split(" ")
};
const syntaxTypes = {
  java: "Boolean Byte Character Double Exception Float Integer Long Object RuntimeException Short String System Thread Void List Map Set Optional Override".split(" "),
  javascript: "Array Boolean Date Error Function JSON Math Number Object Promise RegExp Set String Symbol TypeError URL WeakMap WeakSet".split(" "),
  typescript: "Array Boolean Date Error Function JSON Math Number Object Promise RegExp Set String Symbol TypeError URL WeakMap WeakSet Record Partial Pick Omit".split(" "),
  rust: "Arc Box Cell HashMap HashSet Option Rc Result String Vec".split(" "),
  python: "Any Dict List Optional Set Tuple Type".split(" "),
  css: "rgb rgba hsl hsla var".split(" "),
  html: "html head body div span script style".split(" "),
  sql: "bigint boolean date decimal integer text timestamp varchar".split(" ")
};
const syntaxLiterals = new Set([
  "true", "false", "null", "undefined", "None", "True", "False", "nil", "Some", "Ok", "Err"
]);
const syntaxHashCommentLanguages = new Set(["python", "shell"]);
function syntaxLanguage(path) {
  const kind = fileIconKind(path);
  return kind === "component" ? "javascript" : kind;
}
function syntaxSet(language, table) {
  return new Set(table[language] || []);
}
function syntaxQuoteEnd(source, start, quote) {
  for (let index = start; index < source.length; index++) {
    if (source[index] === "\\") { index++; continue; }
    if (source.startsWith(quote, index)) return index + quote.length;
  }
  return -1;
}
function highlightSourceLine(source, language, state) {
  const fragment = document.createDocumentFragment();
  const keywords = syntaxSet(language, syntaxKeywords);
  const types = syntaxSet(language, syntaxTypes);
  let plain = "";
  let index = 0;
  const flushPlain = () => {
    if (!plain) return;
    fragment.append(document.createTextNode(plain));
    plain = "";
  };
  const appendToken = (className, value) => {
    if (!value) return;
    flushPlain();
    const token = node("span", `session-files-token-${className}`, value);
    fragment.append(token);
  };
  while (index < source.length) {
    if (state.blockComment) {
      const end = source.indexOf("*/", index);
      if (end < 0) { appendToken("comment", source.slice(index)); return fragment; }
      appendToken("comment", source.slice(index, end + 2));
      state.blockComment = false; index = end + 2; continue;
    }
    if (state.quote) {
      const end = syntaxQuoteEnd(source, index, state.quote);
      if (end < 0) { appendToken("string", source.slice(index)); return fragment; }
      appendToken("string", source.slice(index, end));
      state.quote = ""; index = end; continue;
    }
    if (source.startsWith("/*", index)) {
      const end = source.indexOf("*/", index + 2);
      if (end < 0) {
        state.blockComment = true; appendToken("comment", source.slice(index)); return fragment;
      }
      appendToken("comment", source.slice(index, end + 2)); index = end + 2; continue;
    }
    if (source.startsWith("//", index)
        || source.startsWith("<!--", index)
        || (syntaxHashCommentLanguages.has(language) && source[index] === "#")) {
      appendToken("comment", source.slice(index)); return fragment;
    }
    if (source[index] === "@" && /[A-Za-z_$]/u.test(source[index + 1] || "")) {
      const annotation = source.slice(index).match(/^@[A-Za-z_$][\w$.-]*/u)?.[0];
      if (annotation) {
        appendToken("annotation", annotation); index += annotation.length; continue;
      }
    }
    if (source.startsWith('"""', index) || source.startsWith("'''", index)
        || source[index] === "\"" || source[index] === "'" || source[index] === "`") {
      const quote = source.startsWith('"""', index) || source.startsWith("'''", index)
        ? source.slice(index, index + 3) : source[index];
      const end = syntaxQuoteEnd(source, index + quote.length, quote);
      if (end < 0) state.quote = quote;
      appendToken("string", source.slice(index, end < 0 ? source.length : end));
      if (end < 0) return fragment;
      index = end; continue;
    }
    const number = source.slice(index).match(/^(?:0[xX][\da-fA-F]+|0[bB][01]+|\d+(?:\.\d+)?(?:[eE][+-]?\d+)?[fFdDlL]?)/u)?.[0];
    if (number) { appendToken("number", number); index += number.length; continue; }
    const word = source.slice(index).match(/^[A-Za-z_$][\w$]*/u)?.[0];
    if (word) {
      const after = source.slice(index + word.length).match(/^\s*\(/u);
      const className = syntaxLiterals.has(word) ? "literal"
        : keywords.has(word) ? "keyword"
        : types.has(word) || /^[A-Z][A-Za-z\d_$]*$/u.test(word) ? "type"
        : after ? "function" : "";
      if (className) appendToken(className, word); else plain += word;
      index += word.length; continue;
    }
    const operator = source.slice(index).match(/^(?:===|!==|==|!=|=>|<=|>=|&&|\|\||\+\+|--|::|->|<<|>>|[+\-*%=!<>/&|^~?:])/u)?.[0];
    if (operator) { appendToken("operator", operator); index += operator.length; continue; }
    plain += source[index++];
  }
  flushPlain();
  return fragment;
}
const fileIconPaths = {
  folder: '<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2h4.5A1.5 1.5 0 0 1 17 8.5v7A1.5 1.5 0 0 1 15.5 17h-11A1.5 1.5 0 0 1 3 15.5Z"/>',
  document: '<path d="M5 2.5h6l4 4v11H5z"/><path d="M11 2.5v4h4M7.5 11h5M7.5 14h4"/>',
  spreadsheet: '<path d="M3 3h14v14H3zM3 7h14M3 11h14M3 14h14M7 3v14M12 3v14"/>',
  presentation: '<path d="M3 4h14v10H3zM10 14v4M6 18l4-4 4 4M6 8h3M6 11h7"/>',
  file: '<path d="M5 2.5h6l4 4v11H5z"/><path d="M11 2.5v4h4"/>',
  image: '<path d="M4 3h12v14H4z"/><circle cx="8" cy="8" r="1.2"/><path d="m5.5 14 3.5-3 2.2 2 1.5-1.5 2.3 2.5"/>',
  javascript: '<path d="M4 3h12v14H4z"/><path d="M8 13.5c.5 1 2.5 1 2.5-.2 0-1.6-2.5-.8-2.5-2.4 0-1.2 2-1.5 2.8-.5M12.5 10.5h2M13.5 10.5v4"/>',
  typescript: '<path d="M4 3h12v14H4z"/><path d="M7 11h6M10 11v4M13 13c0 1.6-2.5 1.6-2.5 0"/>',
  style: '<path d="M4 3h12v14H4z"/><path d="m7 7 3 3-3 3M11.5 13H14"/>',
  html: '<path d="M4 3h12v14H4z"/><path d="m7 8-2 2 2 2M13 8l2 2-2 2M11.5 6l-3 8"/>',
  config: '<path d="M4 3h12v14H4z"/><path d="M7 7h6M7 10h6M7 13h4"/>',
  package: '<path d="m4 6 6-3 6 3-6 3zM4 6v8l6 3 6-3V6M10 9v8"/>',
  lock: '<path d="M5 8h10v9H5zM7 8V6a3 3 0 0 1 6 0v2"/>',
  git: '<path d="M10 3a2 2 0 1 0 0 4 2 2 0 0 0 0-4ZM5 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4Zm10 0a2 2 0 1 0 0 4 2 2 0 0 0 0-4ZM7 10h6M10 7v6"/>',
  docker: '<path d="M3 12h13a3 3 0 0 1-3 3H7a4 4 0 0 1-4-3ZM6 8h2v2H6zM9 8h2v2H9zM12 8h2v2h-2zM8 5h2v2H8zM11 5h2v2h-2z"/>',
  rust: '<path d="m10 3 1 2 2-.3.5 2 2 .7-1 1.8 1 1.7-2 .7-.5 2-2-.3-1 2-1-2-2 .3-.5-2-2-.7 1-1.8-1-1.7 2-.7.5-2 2 .3z"/><circle cx="10" cy="10" r="2.2"/>',
  python: '<path d="M10 3H7a2 2 0 0 0-2 2v3h5V6H7.5M10 3v6h3a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2zM10 17h3a2 2 0 0 0 2-2v-3h-5v2h2.5M10 17v-6H7a2 2 0 0 0-2 2v2a2 2 0 0 0 2 2z"/>',
  java: '<path d="M8 4c3 2-1 2 1 4 2 2 4 0 2-2M6 10c-1 2 1 3 4 3s5-1 4-3M7 15c2 1 6 1 8 0M9 17h2"/>',
  go: '<path d="M3 10c2-4 12-4 14 0-2 4-12 4-14 0Z"/><path d="M7 10h6M10 7v6"/>',
  native: '<path d="M4 3h12v14H4z"/><path d="m7 8 2 2-2 2M10.5 12H13"/>',
  shell: '<path d="M4 3h12v14H4z"/><path d="m7 8 2 2-2 2M10 13h3"/>',
  data: '<path d="M5 4c0-1 2-2 5-2s5 1 5 2-2 2-5 2-5-1-5-2ZM5 4v5c0 1 2 2 5 2s5-1 5-2V4M5 9v5c0 1 2 2 5 2s5-1 5-2V9"/>',
  component: '<path d="m10 2 7 4v8l-7 4-7-4V6z"/><path d="M10 2v8m7-4-7 4-7-4"/>'
};
function fileIcon(kind) {
  const result = node("span", `session-files-icon session-files-icon-${kind === "folder" ? "folder" : kind}`);
  result.dataset.icon = kind;
  result.innerHTML = `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${kind === "folder" ? fileIconPaths.folder : fileIconPaths[kind] || fileIconPaths.file}</svg>`;
  return result;
}
function fileTwisty(isFolder, expanded = false) {
  const result = node("span", "session-files-twist");
  result.setAttribute("aria-hidden", "true");
  if (isFolder) {
    result.dataset.expanded = String(expanded);
    result.append(node("span", "session-files-folder-chevron", "›"));
  }
  return result;
}

// 用户要求两种文件预览的第二行随阅读手势渐隐、收起；只处理用户输入，不把缓存恢复和布局变化当成滚动意图。
export function mountPreviewControlsScroll({ body, controls, isActive = () => true }) {
  const shell = node("div", "session-preview-controls");
  const clip = node("div", "session-preview-controls-content");
  controls.before(shell); clip.append(controls); shell.append(clip);
  let reveal = 1, touch = null;
  const setReveal = value => {
    reveal = Math.max(0, Math.min(1, value));
    if (reveal === 1) shell.style.removeProperty("--preview-controls-rows");
    else shell.style.setProperty("--preview-controls-rows", `${reveal}fr`);
    shell.style.setProperty("--preview-controls-reveal", String(reveal));
    shell.inert = reveal === 0;
    shell.style.pointerEvents = reveal === 0 ? "none" : "";
  };
  const expand = () => setReveal(1);
  const scroll = (delta, target) => {
    if (!isActive() || controls.hidden || !delta) return;
    for (let element = target instanceof Element ? target : target?.parentElement;
      element && element !== body; element = element.parentElement) {
      if (element.scrollHeight > element.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(element).overflowY)
          && (delta < 0 ? element.scrollTop > 0 : element.scrollTop + element.clientHeight < element.scrollHeight - 1)) return;
    }
    // 上滑令正文向上、scrollTop 增加时收起，反向回滑展开；短文件和滚动到底后的继续上滑不收起。
    if (delta > 0 && (body.scrollHeight <= body.clientHeight + 1
        || body.scrollTop + body.clientHeight >= body.scrollHeight - 1)) return;
    // 用户要求预览第二行更快收展：滚动距离减半为至少 80px 或一倍工具行高度；
    // 文件与变更预览共用此处，style.css 同步缩短为 .10s，短文件、缩放及内层滚动保护保持有效。
    const distance = Math.max(80, controls.scrollHeight);
    setReveal(reveal - delta / distance);
  };
  const onWheel = event => {
    if (event.ctrlKey || event.defaultPrevented || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? body.clientHeight : 1;
    scroll(event.deltaY * unit, event.target);
  };
  const onTouchStart = event => {
    touch = event.touches.length === 1 ? { x: event.touches[0].clientX, y: event.touches[0].clientY } : null;
  };
  const onTouchMove = event => {
    const previous = touch;
    onTouchStart(event);
    if (!previous || !touch || event.defaultPrevented) return;
    const delta = previous.y - touch.y;
    if (Math.abs(delta) >= Math.abs(previous.x - touch.x)) scroll(delta, event.target);
  };
  const onTouchEnd = () => { touch = null; };
  body.addEventListener("wheel", onWheel, { passive: true });
  body.addEventListener("touchstart", onTouchStart, { passive: true });
  body.addEventListener("touchmove", onTouchMove, { passive: true });
  body.addEventListener("touchend", onTouchEnd, { passive: true });
  body.addEventListener("touchcancel", onTouchEnd, { passive: true });
  controls.addEventListener("focusin", expand);
  return {
    reset() { touch = null; expand(); },
    dispose() {
      body.removeEventListener("wheel", onWheel);
      body.removeEventListener("touchstart", onTouchStart);
      body.removeEventListener("touchmove", onTouchMove);
      body.removeEventListener("touchend", onTouchEnd);
      body.removeEventListener("touchcancel", onTouchEnd);
      controls.removeEventListener("focusin", expand);
      shell.replaceWith(controls);
    }
  };
}

export function mountSessionFiles({
  workspace, trigger, getContext, beforeOpen, onUnauthorized, openImagePreview, copyText,
  onFileContextMenu,
  embedded = null, isAvailable = () => true, request = requestRemote
}) {
  const historyKey = "codexElvesFiles";
  let navigationOwner = Math.random().toString(36).slice(2);
  const screens = [], controllers = new Set(), inertState = new Map();
  const responses = new Map(), views = new Map();
  const pendingDirectories = new Map();
  let disposed = false, contextKey = "", authorizationKey = "", target = null, route = null;
  let listEpoch = 0, fileEpoch = 0, listController = null, fileController = null;
  let listResponse = null, contentResponse = null, currentDirectory = "", renderedFile = "";
  let expanded = new Set(), searchMatches = null, searchEpoch = 0, searchController = null;
  let sourceMode = false, wrap = false, richView = {}, returnView = null, exitTimer = null;
  const createScreen = (id, title) => {
    const screen = node("section", "session-tools-screen session-files-screen");
    if (embedded) id = `${embedded.id}-${id}`;
    screen.id = id; screen.hidden = true; screen.inert = true;
    const heading = node("header", "session-tools-heading");
    const copy = node("div", "session-tools-heading-copy");
    const label = node("h1", "", title), location = createWorkspaceLocation();
    label.id = `${id}-title`; screen.setAttribute("aria-labelledby", label.id);
    copy.append(label, location.element);
    heading.append(copy, control("返回", back, "text-button session-tools-back"));
    screen.append(heading); workspace.append(screen); screens.push(screen);
    return { screen, heading, label, path: location.path, location };
  };
  const listing = createScreen("session-files-list", "文件");
  const content = createScreen("session-files-content", "文件");
  content.screen.classList.add("session-tools-file-screen");
  const search = node("input", "session-tools-file-search");
  search.type = "search"; search.placeholder = "搜索项目文件"; search.autocomplete = "off";
  search.setAttribute("aria-label", search.placeholder);
  const parentButton = control("上级目录", back);
  const listStatus = node("p", "session-tools-status"); listStatus.setAttribute("role", "status");
  const list = node("div", "session-tools-file-list session-files-list");
  list.setAttribute("role", "tree");
  list.setAttribute("aria-label", "项目文件树");
  const listingBody = node("div", "session-tools-page session-tools-changes");
  listingBody.append(search, parentButton, listStatus, list); listing.screen.append(listingBody);
  const contentControls = node("div", "session-tools-detail-controls");
  const metadata = node("span", "session-files-metadata");
  const richRange = node("div", "session-files-range"); richRange.hidden = true;
  const modeButton = control("源码", () => { sourceMode = !sourceMode; renderContent(); });
  modeButton.setAttribute("aria-label", "切换源码和预览");
  const wrapButton = control("自动换行", () => {
    if (contentResponse?.kind === "spreadsheet") {
      richView.wrap = richView.wrap !== true; renderSpreadsheetControlsChange();
    } else { wrap = !wrap; renderContent(); }
  });
  // 用户要求信息栏右侧为“自动换行 → 原始/紧凑布局”；参照 Markdown 源码/预览按钮，文字表示点击后的目标模式，不使用选中高亮。
  // 用户要求表格默认紧凑布局且保留手动切换；缺省状态按紧凑处理，第一次点击切换原始布局。
  // 自动换行默认关闭；选择跟随文件页缓存。PC 切换会话保留所属标签，手机切换目标或更换授权时清空。
  const layoutButton = control("原始布局", () => {
    richView.layout = richView.layout === "original" ? "compact" : "original";
    renderSpreadsheetControlsChange();
  });
  layoutButton.setAttribute("aria-label", "切换原始布局和紧凑布局");
  contentControls.append(metadata, richRange, wrapButton, layoutButton, modeButton);
  const fileStatus = node("p", "session-tools-status"); fileStatus.setAttribute("role", "status");
  const body = node("div", "session-tools-detail session-files-content");
  content.screen.append(contentControls, fileStatus, body);
  const controlsScroll = mountPreviewControlsScroll({
    body, controls: contentControls, isActive: () => !disposed && route?.kind === "file" && !content.screen.inert
  });
  function message(element, text = "", failed = false) {
    element.textContent = text; element.hidden = !text; element.classList.toggle("error-text", failed);
  }
  function saveView() {
    if (route?.kind === "list") views.set("list:root", {
      query: search.value, scroll: list.scrollTop, expanded: [...expanded]
    });
    if (route?.kind === "file") views.set(`file:${route.path}`, {
      scroll: body.scrollTop, sourceMode, wrap, richView: { ...richView }
    });
    while (views.size > 64) views.delete(views.keys().next().value);
  }
  function updateContext() {
    if (disposed) return;
    const value = getContext() || {};
    const nextTarget = value.threadId ? { threadId: value.threadId }
      : value.creating && value.projectId ? { projectId: value.projectId } : null;
    const nextAuthorization = JSON.stringify([value.deviceId || "", value.cacheScope || ""]);
    const nextKey = JSON.stringify([nextAuthorization, nextTarget]);
    if (contextKey === nextKey) return;
    // The conversation may already show another thread when this runs. Do not
    // restore the previous thread's scroll position or reactivate its pages.
    returnView = null;
    hideAll(true); ++listEpoch; ++fileEpoch;
    navigationOwner = Math.random().toString(36).slice(2);
    for (const controller of controllers) controller.abort();
    target = nextTarget; contextKey = nextKey; authorizationKey = nextAuthorization;
    responses.clear(); views.clear(); listResponse = contentResponse = null;
    listing.location.set(""); content.location.set("");
    expanded = new Set(); pendingDirectories.clear(); searchMatches = null; search.value = "";
    currentDirectory = renderedFile = ""; list.replaceChildren(); body.replaceChildren();
  }
  function retain(key, value) {
    responses.delete(key); responses.set(key, value);
    let size = 0, count = 0;
    for (const [id, result] of [...responses].reverse()) {
      size += JSON.stringify(result).length * 2;
      if (size > 16 * 1024 * 1024 || ++count > 24) responses.delete(id);
    }
  }
  async function read(path, kind, controller, query = "") {
    if (!target) throw new Error("请先选择已有会话，或为新建会话选择项目。");
    const normalizedQuery = query.trim();
    const owner = contextKey, authorization = authorizationKey;
    const key = `${kind}:${path}:${normalizedQuery}`;
    const cached = responses.get(key);
    controllers.add(controller);
    const timer = setTimeout(() => controller.abort(new DOMException("读取超时", "TimeoutError")), 45000);
    const current = () => !disposed && owner === contextKey && authorization === authorizationKey && !controller.signal.aborted;
    try {
    const value = await request(`/api/workspace/${kind}`, {
        method: "POST", credentials: "same-origin", redirect: "error", signal: controller.signal,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ...target, path,
          ...(kind === "files" && normalizedQuery ? { query: normalizedQuery } : {}),
          ...(cached ? { ifRevision: cached.revision } : {}) })
      }, async response => {
        const result = await response.json();
        if (!response.ok || result?.ok !== true)
          throw Object.assign(new Error(result?.message || result?.error || `读取失败（HTTP ${response.status}）`), { code: response.status });
        return result;
      });
      if (!current()) throw new DOMException("已取消", "AbortError");
      if (!revisionValid(value.revision) || typeof value.root !== "string") throw new Error("文件响应格式无效");
      if (value.notModified === true) {
        if (!cached || value.revision !== cached.revision || value.root !== cached.root) throw new Error("文件缓存已失效，请重新打开");
        return cached;
      }
      if (typeof value.path !== "string" || typeof value.truncated !== "boolean"
          || kind === "files" && (!Array.isArray(value.files) || value.files.length > 2000
            || value.files.some(file => !file || typeof file.name !== "string" || typeof file.path !== "string"
              || !["directory", "file", "unsupported"].includes(file.kind)))
          || kind === "file" && (!["text", "image", "binary", "unsupported", "spreadsheet", "document"].includes(value.kind)
            || value.kind === "text" && (typeof value.text !== "string" || value.text.length > 4 * 1024 * 1024)
            || ["spreadsheet", "document"].includes(value.kind) && !validRichFilePreview(value)
            || value.kind === "image" && !imageValid(value.dataUrl))) throw new Error("文件响应格式无效");
      retain(key, value); return value;
    } catch (error) {
      if (current() && error.code === 401 && !error.secondaryChannel) {
        responses.clear(); views.clear(); body.replaceChildren(); list.replaceChildren(); onUnauthorized?.();
      }
      if (current() && [403, 404, 409, 422].includes(error.code)) responses.delete(key);
      if (controller.signal.reason?.name === "TimeoutError") throw new Error("读取超时，请检查远程连接后重试。");
      if (error instanceof TypeError) throw new Error("无法连接电脑端服务，请重试。");
      throw error;
    } finally { clearTimeout(timer); controllers.delete(controller); }
  }
  function prepare(next, recordHistory) {
    updateContext(); beforeOpen?.(); saveView(); clearTimeout(exitTimer);
    if (!route) {
      const messages = workspace.querySelector("#messages");
      returnView = { messages, scroll: messages?.scrollTop || 0, key: contextKey };
    }
    route = next;
    for (const element of embedded ? [] : workspace.querySelectorAll(":scope > .conversation, :scope > .sidebar")) {
      if (!inertState.has(element)) inertState.set(element, element.inert);
      element.inert = true;
    }
    if (recordHistory && !embedded) history.pushState({ ...history.state,
      [historyKey]: { owner: navigationOwner, context: contextKey, ...next } }, "", location.href);
  }
  function show(screen) {
    screen.hidden = false; screen.inert = false;
    screen.getBoundingClientRect(); screen.classList.add("session-tools-visible");
    if (!embedded) screen.querySelector(".session-tools-back").focus({ preventScroll: true });
  }
  function hide(screen) {
    if (screen === content.screen) releaseMermaidBlocks(body);
    screen.inert = true; screen.classList.remove("session-tools-visible");
  }
  async function openDirectory(path = "", { recordHistory = true } = {}) {
    if (disposed) return;
    prepare({ kind: "list", path }, recordHistory);
    ++fileEpoch; fileController?.abort(); hide(content.screen);
    const epoch = ++listEpoch; listController?.abort(); listController = new AbortController();
    for (const controller of pendingDirectories.values()) controller.abort();
    pendingDirectories.clear();
    show(listing.screen); currentDirectory = path;
    parentButton.hidden = true;
    const listView = views.get("list:root") || {};
    search.value = listView.query || "";
    expanded = new Set(Array.isArray(listView.expanded) ? listView.expanded : []);
    searchMatches = null;
    if (search.value.trim()) {
      listResponse = responses.get("files::") || null;
      listing.location.set(listResponse?.root || "当前会话工作目录", listResponse?.git);
      renderDirectory(); list.scrollTop = listView.scroll || 0;
      void searchProject(search.value);
      return;
    }
    const cached = responses.get(`files:${path}:`);
    listResponse = cached || null; renderDirectory(); list.scrollTop = listView.scroll || 0;
    if (!cached) message(listStatus, "正在读取目录…");
    listing.location.set(cached?.root ? `${cached.root}${path ? `/${path}` : ""}` : path || "当前会话工作目录", cached?.git);
    try {
      const value = await read(path, "files", listController, "");
      if (disposed || epoch !== listEpoch || route?.kind !== "list" || currentDirectory !== path) return;
      const scroll = list.scrollTop;
      if (value !== listResponse) { listResponse = value; renderDirectory(); list.scrollTop = scroll; }
      // 重新进入文件页也须核对已展开子目录；根目录未变化不能代表子仓库分支未变化。
      for (const row of list.querySelectorAll(".session-tools-file[aria-expanded='true']"))
        void loadDirectoryChildren(row.dataset.path);
      listing.location.set(`${value.root}${path ? `/${path}` : ""}`, value.git);
      message(listStatus, value.truncated ? "目录较大，仅显示前 2000 项。" : value.files.length ? "" : "此目录为空。");
    } catch (error) {
      if (epoch !== listEpoch || error.name === "AbortError") return;
      if ([401, 403, 404, 409, 422].includes(error.code)) {
        listResponse = null; list.replaceChildren(); listing.location.set("");
      }
      message(listStatus, `${listResponse ? "正在显示上次内容：" : ""}${error.message}`, true);
      listStatus.append(control("重试", () => void openDirectory(path, { recordHistory: false })));
    }
  }
  function renderDirectory() {
    list.replaceChildren();
    const query = search.value.trim().toLocaleLowerCase();
    if (query) {
      for (const file of searchMatches || []) renderTreeRow(file, 0, true);
    } else {
      for (const file of sortedFiles(listResponse?.files)) renderTreeRow(file, 0, false);
    }
    if (query && searchMatches && !searchMatches.length) list.append(node("p", "session-tools-empty", "没有匹配的文件。"));
    if (!query && listResponse?.files.length && !list.childElementCount) list.append(node("p", "session-tools-empty", "此目录为空。"));
  }
  function renderTreeRow(file, depth, searchResult) {
    const isFolder = file.kind === "directory";
    const isExpanded = expanded.has(file.path);
    const button = control("", () => isFolder ? toggleDirectory(file.path)
      : embedded?.onOpenFile ? embedded.onOpenFile(file.path) : void openFile(file.path), "session-tools-file");
    button.title = displayWorkspacePath(file.path); button.disabled = file.kind === "unsupported";
    // 用户要求“文件”中的文件右键可独立预览；目录与不支持项仍保持原来的交互。
    if (!isFolder && file.kind !== "unsupported" && typeof onFileContextMenu === "function")
      button.addEventListener("contextmenu", event => onFileContextMenu(event,
        { kind: "file", path: file.path }, { ...getContext() }));
    button.style.setProperty("--file-depth", String(depth));
    button.dataset.path = file.path;
    button.dataset.depth = String(depth);
    button.setAttribute("role", "treeitem");
    button.setAttribute("aria-level", String(depth + 1));
    button.setAttribute("aria-selected", String(!isFolder && renderedFile === file.path));
    if (isFolder) button.setAttribute("aria-expanded", String(isExpanded));
    button.classList.toggle("session-files-selected", !isFolder && renderedFile === file.path);
    button.append(fileTwisty(isFolder, isExpanded), fileIcon(isFolder ? "folder" : fileIconKind(file.path)));
    const copy = node("span", "session-tools-file-copy");
    copy.append(node("strong", "session-tools-file-name", displayWorkspaceName(file.name)));
    if (searchResult) copy.append(node("span", "session-tools-file-directory", displayWorkspacePath(parentPath(file.path)) || "项目根目录"));
    const repository = isFolder ? createWorkspaceRepository(file.repository) : null;
    button.append(copy, repository || node("span", "session-files-metadata",
      isFolder ? "文件夹" : file.kind === "unsupported" ? "不支持预览" : bytesLabel(file.size)));
    list.append(button);
    if (isFolder && isExpanded && !searchResult) {
      const children = sortedFiles(responses.get(`files:${file.path}:`)?.files);
      for (const child of children) renderTreeRow(child, depth + 1, false);
      if (!responses.has(`files:${file.path}:`) && !pendingDirectories.has(file.path)) {
        const loading = node("p", "session-files-tree-status", "正在读取目录…");
        loading.style.setProperty("--file-depth", String(depth + 1)); list.append(loading);
        void loadDirectoryChildren(file.path);
      }
    }
  }
  function toggleDirectory(path) {
    if (expanded.has(path)) expanded.delete(path); else expanded.add(path);
    renderDirectory();
    if (expanded.has(path)) void loadDirectoryChildren(path);
  }
  async function loadDirectoryChildren(path) {
    if (pendingDirectories.has(path)) return;
    const controller = new AbortController(), epoch = listEpoch;
    pendingDirectories.set(path, controller);
    try {
      const value = await read(path, "files", controller, "");
      if (value && epoch === listEpoch && route?.kind === "list" && !search.value.trim()) renderDirectory();
    } catch (error) {
      if (epoch === listEpoch && route?.kind === "list" && error.name !== "AbortError") {
        const failure = node("p", "session-files-tree-status error-text", error.message);
        failure.style.setProperty("--file-depth", String(path.split("/").length + 1));
        list.append(failure);
      }
    } finally {
      if (pendingDirectories.get(path) === controller) pendingDirectories.delete(path);
    }
  }
  async function searchProject(query) {
    const normalized = query.trim();
    if (!normalized) { searchMatches = null; renderDirectory(); return; }
    const epoch = ++searchEpoch; searchController?.abort(); searchController = new AbortController();
    const cached = responses.get(`files::${normalized}`);
    // 首次目录尚未返回时两侧 root 都可能为空，只有真实缓存存在才可命中，避免提前搜索抛错。
    if (cached && cached.root === listResponse?.root) {
      searchMatches = cached.files || [];
      listing.location.set(cached.root, cached.git);
      renderDirectory();
      message(listStatus, cached.truncated
        ? "搜索结果较多，仅显示前 2000 项。" : `${searchMatches.length} 个匹配文件`);
    }
    message(listStatus, "正在搜索项目文件…");
    try {
      const value = await read("", "files", searchController, normalized);
      if (epoch !== searchEpoch || route?.kind !== "list") return;
      searchMatches = value.files || []; renderDirectory();
      listing.location.set(value.root, value.git);
      message(listStatus, value.truncated ? "搜索结果较多，仅显示前 2000 项。" : `${searchMatches.length} 个匹配文件`);
    } catch (error) {
      if (epoch !== searchEpoch || error.name === "AbortError") return;
      message(listStatus, error.message, true);
    }
  }
  async function openFile(path, line = null, { recordHistory = true } = {}) {
    if (disposed || typeof path !== "string") return;
    prepare({ kind: "file", path, line: Number.isSafeInteger(line) && line > 0 ? line : null }, recordHistory);
    ++listEpoch; listController?.abort(); listing.screen.inert = true;
    if (embedded) listing.screen.hidden = true;
    show(content.screen); content.label.textContent = displayWorkspaceName(fileName(path));
    const saved = views.get(`file:${path}`);
    controlsScroll.reset();
    sourceMode = line ? true : saved?.sourceMode ?? !/\.mdx?$/iu.test(path); wrap = saved?.wrap || false;
    richView = { ...saved?.richView };
    contentResponse = responses.get(`file:${path}`) || null; renderedFile = path;
    content.location.set(contentResponse ? `${contentResponse.root}/${contentResponse.path}` : path, contentResponse?.git);
    renderContent(); body.scrollTop = saved?.scroll || 0;
    if (!contentResponse) message(fileStatus, "正在读取文件…");
    const epoch = ++fileEpoch; fileController?.abort(); fileController = new AbortController();
    try {
      const value = await read(path, "file", fileController);
      if (disposed || epoch !== fileEpoch || route?.kind !== "file" || route.path !== path) return;
      const scroll = body.scrollTop;
      if (value !== contentResponse) { contentResponse = value; renderContent(); body.scrollTop = scroll; }
      content.location.set(`${value.root}/${value.path}`, value.git);
      message(fileStatus, value.message);
      if (line) body.querySelector(`[data-line="${line}"]`)?.scrollIntoView({ block: "center" });
    } catch (error) {
      if (epoch !== fileEpoch || error.name === "AbortError") return;
      if ([401, 403, 404, 409, 422].includes(error.code)) {
        contentResponse = null; body.replaceChildren(); content.location.set(path);
      }
      message(fileStatus, `${contentResponse ? "正在显示上次内容：" : ""}${error.message}`, true);
      fileStatus.append(control("重试", () => void openFile(path, line, { recordHistory: false })));
    }
  }
  function resolveDocumentReference(path) {
    if (/^(?:[a-z]:\/|\/)/iu.test(path)) return path;
    const parts = parentPath(contentResponse?.path || renderedFile).split("/").filter(Boolean);
    for (const part of path.replaceAll("\\", "/").split("/")) {
      if (!part || part === ".") continue;
      if (part === "..") {
        if (!parts.length) return null;
        parts.pop();
      } else parts.push(part);
    }
    return parts.join("/");
  }
  function renderContent() {
    releaseMermaidBlocks(body);
    body.replaceChildren(); body.classList.toggle("session-files-wrap", wrap);
    richRange.replaceChildren(); richRange.hidden = true;
    const value = contentResponse;
    const isSpreadsheet = value?.kind === "spreadsheet", compact = richView.layout !== "original";
    contentControls.classList.toggle("session-files-spreadsheet-controls", isSpreadsheet);
    contentControls.hidden = !value;
    modeButton.hidden = value?.kind !== "text" || !/\.mdx?$/iu.test(value.path);
    wrapButton.hidden = !isSpreadsheet && (value?.kind !== "text" || !sourceMode);
    layoutButton.hidden = !isSpreadsheet;
    layoutButton.textContent = compact ? "原始布局" : "紧凑布局";
    layoutButton.title = compact ? "切换到原始布局" : "切换到紧凑布局";
    modeButton.textContent = sourceMode ? "预览" : "源码";
    modeButton.title = sourceMode ? "切换到预览" : "切换到源码";
    wrapButton.setAttribute("aria-pressed", String(isSpreadsheet ? richView.wrap === true : wrap));
    if (!value) return;
    metadata.textContent = bytesLabel(value.size);
    message(fileStatus, value.message);
    if (["spreadsheet", "document"].includes(value.kind)) {
      body.append(renderRichFilePreview(value, richView, saveView, {
        onRange: pagination => { richRange.replaceChildren(pagination); richRange.hidden = false; }
      }));
    } else if (value.kind === "text" && !sourceMode) {
      const document = node("div", "markdown session-files-markdown");
      document.innerHTML = markdown(value.text);
      for (const copy of document.querySelectorAll(".copy-code")) {
        if (copyText) copy.onclick = () => void copyText(copy.closest(".code-block").querySelector("code").textContent);
        else copy.remove();
      }
      bindLocalFileReferences(document, {
        openFile: (reference, line) => {
          const path = resolveDocumentReference(reference);
          if (path) {
            if (embedded?.onOpenFile) embedded.onOpenFile(path, line);
            else void openFile(path, line);
          }
          else message(fileStatus, "该路径超出了当前项目。", true);
        },
        openBrowser: (reference) => {
          const path = resolveDocumentReference(reference);
          if (path) window.dispatchEvent(new CustomEvent("codex-elves-open-local-file", { detail: { path } }));
          else message(fileStatus, "该路径超出了当前项目。", true);
        }
      });
      body.append(document);
      refreshFileDiagrams();
    } else if (value.kind === "text") {
      const code = node("div", "session-files-source");
      code.tabIndex = 0; code.setAttribute("aria-label", "文件源码，可横向滚动");
      const fragment = document.createDocumentFragment();
      const state = { blockComment: false, quote: "" };
      const language = syntaxLanguage(value.path);
      value.text.split("\n").forEach((line, index) => {
        const row = node("div", "session-files-source-line");
        row.dataset.line = index + 1;
        row.classList.toggle("session-files-selected-line", route?.line === index + 1);
        const source = node("code", "");
        source.append(line ? highlightSourceLine(line, language, state) : "\u200b");
        row.append(node("span", "session-tools-line-number", String(index + 1)), source);
        fragment.append(row);
      });
      code.append(fragment); body.append(code);
    } else if (value.kind === "image") {
      const source = safeInlineImageSource(value.dataUrl, { allowSvg: true });
      if (!source) {
        message(fileStatus, "图片预览数据无效，请刷新后重试。", true);
        return;
      }
      const link = node("a", "session-files-image"); link.href = source;
      const image = node("img"); image.src = source; image.alt = displayWorkspaceName(fileName(value.path));
      link.append(image); link.onclick = event => { event.preventDefault(); openImagePreview?.(link, image); };
      body.append(link);
    }
  }
  function renderSpreadsheetControlsChange() {
    const previous = body.querySelector(".session-files-table-scroll");
    const top = previous?.scrollTop || 0, left = previous?.scrollLeft || 0;
    renderContent();
    const next = body.querySelector(".session-files-table-scroll");
    if (next) { next.scrollTop = top; next.scrollLeft = left; }
    saveView();
  }
  function finishExit() {
    if (route || disposed) return;
    const returning = !!returnView;
    for (const { screen } of [listing, content]) screen.hidden = true;
    for (const [element, previous] of inertState) element.inert = previous;
    inertState.clear();
    if (returnView?.key === contextKey && returnView.messages?.isConnected) returnView.messages.scrollTop = returnView.scroll;
    returnView = null;
    if (!embedded && returning && trigger?.isConnected) trigger.focus({ preventScroll: true });
  }
  function hideAll(immediate = false) {
    saveView(); route = null; ++fileEpoch; ++listEpoch;
    fileController?.abort(); listController?.abort();
    for (const screen of screens) hide(screen);
    clearTimeout(exitTimer);
    if (immediate) finishExit(); else exitTimer = setTimeout(finishExit, 250);
  }
  function back() {
    if (!route || disposed) return;
    if (embedded) { embedded.onClose(); return; }
    const image = document.querySelector("#image-preview-dialog[open]");
    if (image) { image.close(); return; }
    saveView();
    if (history.state?.[historyKey]?.owner === navigationOwner) history.back();
    else hideAll();
  }
  function pop(event) {
    if (embedded || !isAvailable()) return;
    const next = event.state?.[historyKey];
    if (next?.owner !== navigationOwner || next.context !== contextKey) { if (route) hideAll(); return; }
    if (next.kind === "file") void openFile(next.path, next.line, { recordHistory: false });
    else if (next.kind === "list") void openDirectory(next.path, { recordHistory: false });
  }
  function key(event) {
    if (!route || disposed || embedded || !isAvailable() || event.isComposing || document.querySelector("dialog[open]")) return;
    if (event.key === "Escape") { event.preventDefault(); event.stopImmediatePropagation(); back(); }
    if (event.key === "Tab") {
      const screen = route.kind === "file" ? content.screen : listing.screen;
      const controls = [...screen.querySelectorAll("button:not(:disabled),input,[tabindex='0'],a[href]")]
        .filter(element => element.getClientRects().length && !element.closest("[hidden],[inert]"));
      const first = controls[0], last = controls.at(-1);
      if (event.shiftKey && (document.activeElement === first || !screen.contains(document.activeElement))) {
        event.preventDefault(); last?.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !screen.contains(document.activeElement))) {
        event.preventDefault(); first?.focus();
      }
    }
  }
  function nativeBack(event) {
    if (!route || disposed || embedded || !isAvailable() || event.defaultPrevented || document.querySelector("dialog[open]")) return;
    event.preventDefault(); back();
  }
  function markdownReady() {
    if (route?.kind === "file" && contentResponse?.kind === "text" && !sourceMode)
      withStableMessageLayout(body, renderContent);
  }
  function refreshFileDiagrams() {
    if (disposed || route?.kind !== "file" || contentResponse?.kind !== "text" || sourceMode) return;
    // 用户要求文件 Markdown 与会话共用 Mermaid；按文件滚动区延迟渲染，主题变化只换图，不重建正文或跳动阅读位置。
    renderMermaidBlocks(body, undefined, change => withStableMessageLayout(body, change));
  }
  const diagramThemeObserver = typeof MutationObserver === "function"
    ? new MutationObserver(refreshFileDiagrams) : null;
  diagramThemeObserver?.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  let searchTimer = null;
  search.addEventListener("input", () => {
    clearTimeout(searchTimer);
    searchEpoch++;
    searchController?.abort();
    searchTimer = setTimeout(() => void searchProject(search.value), 180);
    if (!search.value.trim()) {
      searchMatches = null; message(listStatus, "");
      renderDirectory();
    } else {
      message(listStatus, "正在搜索项目文件…");
      renderDirectory();
    }
  });
  document.addEventListener("keydown", key, true);
  window.addEventListener("popstate", pop, true);
  window.addEventListener("codex-elves-native-back", nativeBack);
  window.addEventListener("codex-elves-markdown-ready", markdownReady);
  updateContext();
  return {
    open: () => openDirectory(currentDirectory), openFile, updateContext, close: () => hideAll(true),
    dispose() {
      if (disposed) return;
      diagramThemeObserver?.disconnect();
      controlsScroll.dispose();
      hideAll(true); disposed = true;
      for (const controller of controllers) controller.abort();
      document.removeEventListener("keydown", key, true);
      window.removeEventListener("popstate", pop, true);
      window.removeEventListener("codex-elves-native-back", nativeBack);
      window.removeEventListener("codex-elves-markdown-ready", markdownReady);
      if (history.state?.[historyKey]?.owner === navigationOwner) {
        const state = { ...history.state }; delete state[historyKey]; history.replaceState(state, "", location.href);
      }
      for (const screen of screens) screen.remove();
      responses.clear(); views.clear();
    }
  };
}
