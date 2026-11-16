// 正文只识别带目录信息的路径；纯文件名（包括带行号的文件名）保持普通内容。
export function localFileReference(value, explicit = false, allowFilename = false) {
  let path = String(value ?? "").trim(), line = null;
  if (!path || path.length > 16384 || /[\u0000-\u001f\u007f<>"`]/u.test(path)) return null;
  const position = /(?::([1-9]\d*)(?::[1-9]\d*)?|#L([1-9]\d*)(?:C\d+)?(?:-L?\d+(?:C\d+)?)?)$/u.exec(path);
  if (position) { line = Number(position[1] || position[2]); path = path.slice(0, position.index); }
  if (/^file:/iu.test(path)) {
    try {
      const url = new URL(path);
      if (url.hostname && url.hostname !== "localhost") return null;
      path = decodeURIComponent(url.pathname);
    } catch { return null; }
  } else {
    if (/^[a-z][a-z\d+.-]*:/iu.test(path) && !/^[a-z]:[\\/]/iu.test(path)) return null;
    try { path = decodeURIComponent(path); } catch { return null; }
  }
  path = path.replaceAll("\\", "/");
  if (/^\/[a-z]:\//iu.test(path)) path = path.slice(1);
  // 用户明确要求支持相对路径：src/app.ts、./README.md 和反斜杠路径均可；README.md 本身不算路径。
  if ((!allowFilename && !path.includes("/")) || path.startsWith("//") || path.startsWith("#")
      || /[\u0000-\u001f\u007f?#]/u.test(path)) return null;
  const name = path.split("/").at(-1);
  if (!name || name === "." || name === "..") return null;
  if (!explicit && !/\.(?:md|mdx|txt|rst|log|json|jsonl|ya?ml|toml|xml|ini|conf|config|csv|tsv|js|mjs|cjs|jsx|ts|tsx|vue|svelte|css|scss|less|html?|rs|py|java|kt|kts|go|c|cc|cpp|h|hpp|cs|swift|sh|ps1|bat|sql|proto|gradle|properties|png|jpe?g|gif|webp|svg|pdf)$/iu.test(name)
      && !/^(?:README|LICENSE|Dockerfile|Makefile|Cargo\.lock|\.gitignore|\.env)$/u.test(name)) return null;
  return { path, name, line: Number.isSafeInteger(line) && line <= 1000000 ? line : null };
}
