#!/usr/bin/env node
import { execFileSync } from "node:child_process";

const tag = process.argv[2] || process.env.TAG || process.env.RELEASE_TAG || "";
const repo = process.argv[3] || process.env.REPO || process.env.GITHUB_REPOSITORY || "";

if (!tag.trim()) {
  throw new Error("release tag is required");
}

const version = tag.replace(/^[vV]/, "");
const previousTag = findPreviousTag(tag);
const range = previousTag ? `${previousTag}..${tag}` : tag;
const changesUrl = previousTag && repo
  ? `https://github.com/${repo}/compare/${previousTag}...${tag}`
  : repo
    ? `https://github.com/${repo}/commits/${tag}`
    : "";

const commits = readCommits(range).filter((commit) => !/^chore\(release\):/.test(commit.subject));
const featureNotes = [];
const maintenanceNotes = [];
const testNotes = [];

for (const commit of commits) {
  const parsed = parseSubject(commit.subject);
  const notes = extractBullets(commit.body);
  const items = notes.length ? notes : [parsed.title || commit.subject];
  const target = chooseSection(parsed.type, parsed.scope, commit.subject);
  for (const item of items) {
    target.push(item);
  }
}

const mainNotes = dedupe(featureNotes);
const maintenance = dedupe(maintenanceNotes);
const tests = dedupe(testNotes);
const headline = mainNotes[0]
  ? `CodexElves ${version} 重点更新：${trimSentence(mainNotes[0])}。`
  : `CodexElves ${version} 发布版本。`;

const lines = [];
lines.push(`## ${tag}`);
lines.push("");
lines.push(headline);
lines.push("");
lines.push("### 主要更新");
pushBullets(lines, mainNotes, ["本版本包含应用能力和体验更新。"]);
lines.push("");
lines.push("### 修复与维护");
pushBullets(lines, maintenance, ["无额外维护项。"]);
if (tests.length) {
  lines.push("");
  lines.push("### 测试与发布保障");
  pushBullets(lines, tests, []);
}
lines.push("");
lines.push("### 安装包");
lines.push(`- Windows: \`CodexElves-${version}-windows-x64-setup.exe\``);
lines.push(`- macOS Intel: \`CodexElves-${version}-macos-x64.dmg\``);
lines.push(`- macOS Apple Silicon: \`CodexElves-${version}-macos-arm64.dmg\``);
lines.push("");
lines.push("### 说明");
lines.push("- 本 Release 由 GitHub Actions 自动编译并上传安装资产。");
lines.push("- `latest.json` 会在所有平台资产上传后自动更新。");
if (previousTag) {
  lines.push(`- 自动更新说明基于 \`${previousTag}...${tag}\` 的提交正文生成。`);
} else {
  lines.push("- 自动更新说明基于当前 tag 的提交正文生成。");
}
if (changesUrl) {
  lines.push("");
  lines.push("### 完整变更");
  lines.push(changesUrl);
}

process.stdout.write(`${lines.join("\n")}\n`);

function findPreviousTag(currentTag) {
  const currentVersion = parseVersionTag(currentTag);
  if (!currentVersion) {
    return "";
  }

  return git(["tag", "--list"])
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean)
    .filter((value) => value !== currentTag)
    .map((name) => ({ name, version: parseVersionTag(name) }))
    .filter((item) => item.version && compareVersions(item.version, currentVersion) < 0)
    .sort((left, right) => compareVersions(right.version, left.version))
    .map((item) => item.name)[0] || "";
}

function parseVersionTag(value) {
  const match = value.match(/^[vV](\d+)\.(\d+)\.(\d+)(?:[-+].*)?$/);
  if (!match) return null;
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3])
  };
}

function compareVersions(left, right) {
  return left.major - right.major || left.minor - right.minor || left.patch - right.patch;
}

function readCommits(logRange) {
  const output = git(["log", "--format=%x1e%H%x1f%s%x1f%b", logRange]);
  return output
    .split("\x1e")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [hash = "", subject = "", ...bodyParts] = entry.split("\x1f");
      return { hash, subject: subject.trim(), body: bodyParts.join("\x1f").trim() };
    });
}

function parseSubject(subject) {
  const match = subject.match(/^([a-z]+)(?:\(([^)]+)\))?:\s*(.+)$/);
  if (!match) return { type: "", scope: "", title: subject };
  return { type: match[1], scope: match[2] || "", title: match[3] };
}

function extractBullets(body) {
  return body
    .split(/\r?\n/)
    .map((line) => line.match(/^\s*[-*]\s+(.+)$/)?.[1]?.trim() || "")
    .map(sanitizeBullet)
    .filter(Boolean)
    .filter((line) => !/^版本升级到\s*\d+\.\d+\.\d+$/.test(line));
}

function chooseSection(type, scope, subject) {
  if (type === "test" || type === "ci") return testNotes;
  if (type === "docs") return testNotes;
  if (type === "chore" && /node_modules|依赖/i.test(subject)) return maintenanceNotes;
  if (type === "chore" && /发布|release|workflow/i.test(subject)) return testNotes;
  if (type === "feat" || type === "perf") return featureNotes;
  if (/proxy|本地代理/i.test(scope) || /本地代理|proxy/i.test(subject)) return featureNotes;
  return maintenanceNotes;
}

function sanitizeBullet(text) {
  return text
    .replace(/[，,]\s*发布版本升级到\s*\d+\.\d+\.\d+$/g, "")
    .replace(/[，,]\s*版本升级到\s*\d+\.\d+\.\d+$/g, "")
    .trim();
}

function dedupe(items) {
  const seen = new Set();
  const result = [];
  for (const item of items.map((value) => value.trim()).filter(Boolean)) {
    const key = item.replace(/[。；;,.，\s]+$/g, "");
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(item);
  }
  return result;
}

function pushBullets(lines, items, fallback) {
  const source = items.length ? items : fallback;
  for (const item of source) {
    lines.push(`- ${item}`);
  }
}

function trimSentence(text) {
  return text.replace(/[。；;,.，\s]+$/g, "");
}

function git(args) {
  return execFileSync("git", args, { encoding: "utf8" });
}
