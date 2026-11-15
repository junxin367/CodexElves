// 文件变更摘要：解析补丁统计增删行数并渲染已编辑文件列表。

import { button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { thread } from "../../core/state.js";

function editedFileStatus(value) {
  return String(typeof value === "string" ? value : value?.type || "").toLowerCase();
}
function editedFilePathKey(path) {
  const normalized = String(path || "").trim().replaceAll("\\", "/");
  return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
}
function editedFileDisplayPath(path) {
  const normalized = String(path || "").trim().replaceAll("\\", "/");
  const rawCwd = String(thread?.cwd || "").trim().replaceAll("\\", "/");
  if (!normalized || !rawCwd) return normalized;
  const cwd = rawCwd.replace(/\/+$/, "");
  const windows = /^[a-z]:\//i.test(normalized) && /^[a-z]:(?:\/|$)/i.test(cwd);
  const comparedPath = windows ? normalized.toLowerCase() : normalized;
  const comparedCwd = windows ? cwd.toLowerCase() : cwd;
  const prefix = comparedCwd === "/" ? "/" : `${comparedCwd}/`;
  return comparedPath.startsWith(prefix) ? normalized.slice(prefix.length) : normalized;
}
function editedFileKind(kind) {
  const value = String(typeof kind === "string" ? kind : kind?.type || "").toLowerCase();
  if (["add", "create"].includes(value)) return "新增";
  if (["delete", "remove"].includes(value)) return "删除";
  if (["move", "rename"].includes(value)) return "移动";
  return "修改";
}
const editedFileDiffLimit = 1_048_576;
const editedFileChangeLimit = 4_096;
const editedFileLineDiffLimit = 4_000_000;
function editedFileDiffPath(value) {
  let path = String(value || "").trim().split("\t", 1)[0];
  if (path.startsWith('"')) {
    try { path = JSON.parse(path); } catch {}
  }
  if (path === "/dev/null") return "";
  return path.replace(/^[ab]\//, "");
}
function editedFileParseDiff(value, fallbackPath = "") {
  if (typeof value !== "string" || value.length > editedFileDiffLimit) return null;
  const lines = value.replaceAll("\r\n", "\n").replaceAll("\r", "\n").split("\n");
  const files = [];
  let file = null, hunk = null;
  const startFile = () => file = {
    oldPath: "", newPath: "", hunks: [], valid: true,
    isBinary: false, isRename: false, isCopy: false, isCreate: false, isDelete: false, oldMode: false, newMode: false
  };
  const finishHunk = () => {
    if (!hunk) return;
    if (hunk.oldSeen !== hunk.oldLines || hunk.newSeen !== hunk.newLines) file.valid = false;
    hunk = null;
  };
  const finishFile = () => {
    if (!file) return;
    finishHunk();
    if (file.hunks.length || file.oldPath || file.newPath || file.isBinary) files.push(file);
    file = null;
  };
  const startHunk = line => {
    const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line);
    if (!match) return false;
    hunk = {
      oldStart: Number(match[1]), oldLines: match[2] == null ? 1 : Number(match[2]),
      newStart: Number(match[3]), newLines: match[4] == null ? 1 : Number(match[4]),
      oldSeen: 0, newSeen: 0, additions: 0, deletions: 0, lines: []
    };
    if (![hunk.oldStart, hunk.oldLines, hunk.newStart, hunk.newLines].every(Number.isSafeInteger)) file.valid = false;
    file.hunks.push(hunk);
    return true;
  };
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    if (line.startsWith("diff --git ")) {
      finishFile(); startFile(); continue;
    }
    if (!file && (line.startsWith("--- ") || line.startsWith("+++ ") || line.startsWith("@@ "))) startFile();
    if (!file) continue;
    if (hunk) {
      if (line.startsWith("@@ ")) {
        finishHunk();
        if (!startHunk(line)) file.valid = false;
      } else if (line.startsWith("\\")) {
        hunk.lines.push(line);
      } else if ([" ", "+", "-"].includes(line[0])) {
        hunk.lines.push(line);
        if (line[0] === " ") { hunk.oldSeen++; hunk.newSeen++; }
        else if (line[0] === "+") { hunk.newSeen++; hunk.additions++; }
        else { hunk.oldSeen++; hunk.deletions++; }
      } else if (hunk.oldSeen === hunk.oldLines && hunk.newSeen === hunk.newLines) {
        finishHunk(); index--;
      } else {
        file.valid = false;
      }
      continue;
    }
    if (startHunk(line)) continue;
    if (line.startsWith("--- ")) file.oldPath = editedFileDiffPath(line.slice(4));
    else if (line.startsWith("+++ ")) file.newPath = editedFileDiffPath(line.slice(4));
    else if (line.startsWith("new file mode ")) file.isCreate = true;
    else if (line.startsWith("deleted file mode ")) file.isDelete = true;
    else if (line.startsWith("rename from ") || line.startsWith("rename to ")) file.isRename = true;
    else if (line.startsWith("copy from ") || line.startsWith("copy to ")) file.isCopy = true;
    else if (line.startsWith("old mode ")) file.oldMode = true;
    else if (line.startsWith("new mode ")) file.newMode = true;
    else if (line.startsWith("Binary files ") || line === "GIT binary patch") file.isBinary = true;
  }
  finishFile();
  if (!files.length && fallbackPath && !value.trim()) {
    startFile(); file.oldPath = fallbackPath; file.newPath = fallbackPath; finishFile();
  }
  return files;
}
function editedFileAddStatistics(statistics, path, additions, deletions) {
  const key = editedFilePathKey(path);
  if (!key || !Number.isSafeInteger(additions) || !Number.isSafeInteger(deletions) || additions < 0 || deletions < 0) return;
  const current = statistics.get(key);
  if (current) {
    current.additions += additions;
    current.deletions += deletions;
  } else {
    statistics.set(key, { additions, deletions });
  }
}
function editedFileDiffStatistics(value, fallbackPath = "") {
  const statistics = new Map();
  const files = editedFileParseDiff(value, fallbackPath);
  if (!files) return statistics;
  for (const file of files) {
    if (!file.valid || file.isBinary) continue;
    const path = file.newPath || file.oldPath || fallbackPath;
    const additions = file.hunks.reduce((total, entry) => total + entry.additions, 0);
    const deletions = file.hunks.reduce((total, entry) => total + entry.deletions, 0);
    editedFileAddStatistics(statistics, path, additions, deletions);
  }
  return statistics;
}
function editedFileContentLines(value) {
  const lines = String(value || "").replaceAll("\r\n", "\n").replaceAll("\r", "\n").split("\n");
  if (lines.at(-1) === "") lines.pop();
  return lines;
}
function editedFilePatchKind(kind) {
  const value = String(typeof kind === "string" ? kind : kind?.type || "").toLowerCase();
  if (["add", "create"].includes(value)) return "add";
  if (["delete", "remove"].includes(value)) return "delete";
  return value === "update" ? "update" : "";
}
function editedFileDirectPatchStatistics(patch) {
  if (patch.kind === "add") return { additions: editedFileContentLines(patch.diff).length, deletions: 0 };
  if (patch.kind === "delete") return { additions: 0, deletions: editedFileContentLines(patch.diff).length };
  const files = editedFileParseDiff(patch.diff, patch.path);
  if (!files || files.length !== 1 || !files[0].valid || files[0].isBinary) return null;
  return {
    additions: files[0].hunks.reduce((total, entry) => total + entry.additions, 0),
    deletions: files[0].hunks.reduce((total, entry) => total + entry.deletions, 0)
  };
}
function editedFileSplitState(state, offset) {
  if (offset < 0) throw new Error("patch starts before file");
  let remaining = offset;
  for (let index = 0; index < state.length; index++) {
    if (remaining === 0) return index;
    const entry = state[index];
    const count = typeof entry === "string" ? 1 : entry.count;
    if (remaining < count && typeof entry !== "string") {
      state.splice(index, 1, { start: entry.start, count: remaining }, { start: entry.start + remaining, count: entry.count - remaining });
      return index + 1;
    }
    remaining -= count;
  }
  if (remaining === 0) return state.length;
  throw new Error("patch lies outside known file");
}
function editedFileLineChanges(before, after) {
  let start = 0, beforeEnd = before.length, afterEnd = after.length;
  while (start < beforeEnd && start < afterEnd && before[start] === after[start]) start++;
  while (beforeEnd > start && afterEnd > start && before[beforeEnd - 1] === after[afterEnd - 1]) { beforeEnd--; afterEnd--; }
  const oldLines = before.slice(start, beforeEnd), newLines = after.slice(start, afterEnd);
  if (oldLines.length * newLines.length > editedFileLineDiffLimit) return null;
  const common = new Uint32Array(newLines.length + 1);
  for (const oldLine of oldLines) {
    let diagonal = 0;
    for (let index = 1; index <= newLines.length; index++) {
      const above = common[index];
      common[index] = oldLine === newLines[index - 1] ? diagonal + 1 : Math.max(above, common[index - 1]);
      diagonal = above;
    }
  }
  const shared = common[newLines.length];
  return { additions: newLines.length - shared, deletions: oldLines.length - shared };
}
function editedFileMergedPatchStatistics(patches) {
  try {
    const parsed = patches.map(patch => {
      if (patch.kind === "add") return {
        isCreate: true, isDelete: false, isBinary: false, isRename: false, isCopy: false, oldMode: false, newMode: false,
        hunks: [{ oldStart: 0, oldLines: 0, newLines: editedFileContentLines(patch.diff).length,
          lines: editedFileContentLines(patch.diff).map(line => `+${line}`) }]
      };
      if (patch.kind !== "update") throw new Error("unsupported patch");
      const files = editedFileParseDiff(patch.diff, patch.path);
      if (!files || files.length !== 1 || !files[0].valid) throw new Error("invalid patch");
      return files[0];
    });
    const created = parsed[0].isCreate;
    if (parsed.some((file, index) => file.isBinary || file.isRename || file.isCopy || file.isDelete || file.oldMode
      || file.newMode && !file.isCreate || file.isCreate && index !== 0)) return null;
    const state = created ? [] : [{ start: 0, count: Infinity }];
    const originals = new Map();
    for (const file of parsed) {
      let delta = 0;
      for (const hunk of file.hunks) {
        const oldOffset = (hunk.oldLines === 0 ? hunk.oldStart : hunk.oldStart - 1) + delta;
        const start = editedFileSplitState(state, oldOffset);
        const end = editedFileSplitState(state, oldOffset + hunk.oldLines);
        const previous = state.slice(start, end).flatMap(entry => typeof entry === "string"
          ? [entry] : Array.from({ length: entry.count }, (_, index) => entry.start + index));
        const replacement = [];
        let consumed = 0;
        for (let index = 0; index < hunk.lines.length; index++) {
          const line = hunk.lines[index];
          if (line.startsWith("\\")) continue;
          const content = line.slice(1) + (hunk.lines[index + 1]?.startsWith("\\") ? "" : "\n");
          if (line.startsWith("+")) { replacement.push(content); continue; }
          const original = previous[consumed++];
          if (typeof original === "number") {
            const saved = originals.get(original);
            if (saved != null && saved !== content) throw new Error("patches disagree");
            originals.set(original, content);
          } else if (original !== content) {
            throw new Error("patches are not sequential");
          }
          if (line.startsWith(" ")) replacement.push(typeof original === "number" ? { start: original, count: 1 } : original);
        }
        if (consumed !== previous.length) throw new Error("patch length mismatch");
        state.splice(start, end - start, ...replacement);
        delta += hunk.newLines - hunk.oldLines;
      }
    }
    let additions = 0, deletions = 0, originalStart = 0, current = [];
    const flush = originalEnd => {
      const before = [];
      for (let index = originalStart; index < originalEnd; index++) {
        if (!originals.has(index)) throw new Error("missing original content");
        before.push(originals.get(index));
      }
      const changes = editedFileLineChanges(before, current);
      if (!changes) throw new Error("line diff limit");
      additions += changes.additions;
      deletions += changes.deletions;
      current = [];
      originalStart = originalEnd;
    };
    for (const entry of state) {
      if (typeof entry === "string") {
        current.push(entry);
      } else if (entry.count === 1 && originals.has(entry.start)) {
        current.push(originals.get(entry.start));
      } else {
        flush(entry.start);
        originalStart += entry.count;
      }
    }
    if (created) flush(0);
    return { additions, deletions };
  } catch {
    return null;
  }
}
function editedFileStatistics(turn) {
  if (typeof turn?.diff === "string" && turn.diff.length > 0 && turn.diff.length <= editedFileDiffLimit)
    return editedFileDiffStatistics(turn.diff);
  const groups = [], active = new Map();
  let size = 0, count = 0;
  for (const item of turn?.items || []) {
    const itemStatus = editedFileStatus(item?.status);
    if (item?.type !== "fileChange" || itemStatus && itemStatus !== "completed") continue;
    for (const change of item.changes || []) {
      const changeStatus = editedFileStatus(change?.status);
      const path = String(change?.path || "").trim();
      const kind = editedFilePatchKind(change?.kind);
      const diff = typeof change?.diff === "string" ? change.diff : "";
      if (!path || !kind || changeStatus && changeStatus !== "completed") continue;
      size += diff.length; count++;
      if (size > editedFileDiffLimit || count > editedFileChangeLimit) return new Map();
      const movePath = kind === "update" ? String(change.kind?.move_path || "").trim() : "";
      const key = editedFilePathKey(path);
      const patch = { kind, path, diff };
      if (kind === "update" && !movePath) {
        let group = active.get(key);
        if (!group) { group = { path, patches: [] }; groups.push(group); active.set(key, group); }
        group.patches.push(patch);
      } else {
        const group = { path: movePath || path, patches: [patch] };
        groups.push(group);
        if (kind === "add") active.set(key, group);
        else {
          active.delete(key);
          if (movePath) active.delete(editedFilePathKey(movePath));
        }
      }
    }
  }
  const statistics = new Map(), unavailable = new Set();
  for (const group of groups) {
    const merged = group.patches.length > 1 ? editedFileMergedPatchStatistics(group.patches) : null;
    if (merged) {
      editedFileAddStatistics(statistics, group.path, merged.additions, merged.deletions);
    } else {
      for (const patch of group.patches) {
        const current = editedFileDirectPatchStatistics(patch);
        if (current) editedFileAddStatistics(statistics, group.path, current.additions, current.deletions);
        else unavailable.add(editedFilePathKey(group.path));
      }
    }
  }
  for (const key of unavailable) statistics.delete(key);
  return statistics;
}
export function renderEditedFiles(turn, previous) {
  const changedItems = (turn?.items || []).filter(item => item.type === "fileChange");
  if (previous?.editedCwd === thread?.cwd && previous?.editedDiff === turn.diff && previous?.editedItemsComplete === turn.itemsComplete
      && previous?.editedItems?.length === changedItems.length
      && changedItems.every((item, index) => previous.editedItems[index] === item)) return previous;
  const files = new Map();
  for (const item of changedItems) {
    const status = editedFileStatus(item.status);
    if (status && status !== "completed") continue;
    for (const change of item.changes || []) {
      const changeStatus = editedFileStatus(change?.status);
      const path = String(change?.path || "").trim();
      if (!path || changeStatus && changeStatus !== "completed") continue;
      const key = editedFilePathKey(path);
      let file = files.get(key);
      if (!file) {
        file = { key, path: editedFileDisplayPath(path), records: [] };
        files.set(key, file);
      }
      file.records.push({ kind: editedFileKind(change.kind), diff: typeof change.diff === "string" ? change.diff : "" });
    }
  }
  if (!files.size) return null;
  const card = previous || element("section", "edited-files-card");
  const open = new Set([...card.querySelectorAll(".edited-files-row[open]")].map(row => row.dataset.pathKey));
  let expanded = card.dataset.expanded === "true" && files.size > 3;
  const heading = element("div", "edited-files-heading");
  const statistics = editedFileStatistics(turn);
  const fileStatistics = file => statistics.get(file.key) || statistics.get(editedFilePathKey(file.path));
  const counts = value => {
    const node = element("span", "edited-files-counts");
    node.setAttribute("aria-label", `新增 ${value.additions} 行，删除 ${value.deletions} 行`);
    node.append(element("span", "diff-added", `+${value.additions}`), element("span", "diff-deleted", `-${value.deletions}`));
    return node;
  };
  const title = element("div", "edited-files-title");
  title.append(element("span", "", turn.itemsComplete === false ? `已加载 ${files.size} 个文件的修改` : `已编辑 ${files.size} 个文件`));
  if ([...files.values()].every(file => fileStatistics(file))) {
    const total = [...files.values()].reduce((sum, file) => {
      const current = fileStatistics(file);
      return { additions: sum.additions + current.additions, deletions: sum.deletions + current.deletions };
    }, { additions: 0, deletions: 0 });
    title.append(counts(total));
  }
  heading.append(icon("fileChange"), title);
  const list = element("div", "edited-files-list");
  const rows = [...files.values()].map(file => {
    const row = element("details", "edited-files-row");
    row.dataset.pathKey = file.key;
    row.open = open.has(file.key);
    const summary = element("summary", "edited-files-summary");
    const path = element("span", "edited-files-path");
    const nameStart = file.path.lastIndexOf("/") + 1;
    path.append(document.createTextNode(file.path.slice(0, nameStart)),
      element("span", "edited-files-name", file.path.slice(nameStart)));
    summary.title = file.path;
    summary.append(path);
    if (fileStatistics(file)) summary.append(counts(fileStatistics(file)));
    row.append(summary);
    const showDetail = () => {
      if (!row.open || row.querySelector(".edited-files-detail")) return;
      const detail = element("div", "edited-files-detail");
      detail.append(element("div", "edited-files-detail-label", "此轮文件修改记录（按操作顺序）"));
      const records = file.records.map((record, index) => {
        const label = file.records.length > 1 ? `记录 ${index + 1} · ${record.kind}` : record.kind;
        return `${label}\n${record.diff.trimEnd() || "该次修改未提供差异内容。"}`;
      });
      detail.append(element("pre", "edited-files-diff", records.join("\n\n"))); row.append(detail);
    };
    row.ontoggle = () => { if (row.isConnected) showDetail(); }; showDetail();
    list.append(row);
    return row;
  });
  const more = files.size > 3 ? button("", "", () => {
    expanded = !expanded;
    update();
  }, "edited-files-more") : null;
  const update = () => {
    card.dataset.expanded = String(expanded);
    rows.forEach((row, index) => { row.hidden = !expanded && index >= 3; });
    if (!more) return;
    const remaining = files.size - 3;
    more.replaceChildren(document.createTextNode(expanded ? "收起" : `再显示 ${remaining} 个文件`), icon("chevron"));
    more.title = expanded ? "收起文件列表" : `再显示 ${remaining} 个文件`;
    more.setAttribute("aria-label", more.title);
    more.setAttribute("aria-expanded", String(expanded));
  };
  card.replaceChildren(heading, list, ...(more ? [more] : []));
  card.editedItems = changedItems; card.editedCwd = thread?.cwd; card.editedDiff = turn.diff;
  card.editedItemsComplete = turn.itemsComplete;
  update();
  return card;
}
