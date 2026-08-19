#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";

const tag = process.argv[2] || process.env.TAG || process.env.RELEASE_TAG || "";
const repo = process.argv[3] || process.env.REPO || process.env.GITHUB_REPOSITORY || "";

if (!tag.trim()) {
  throw new Error("release tag is required");
}

const version = tag.replace(/^[vV]/, "");
const curatedNotesPath = `.github/release-notes/${version}.md`;
if (
  /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)
  && existsSync(curatedNotesPath)
) {
  process.stdout.write(`${readFileSync(curatedNotesPath, "utf8").trim()}\n`);
  process.exit(0);
}

const previousTag = findPreviousTag(tag);
const range = previousTag ? `${previousTag}..${tag}` : tag;
const changesUrl = previousTag && repo
  ? `https://github.com/${repo}/compare/${previousTag}...${tag}`
  : repo
    ? `https://github.com/${repo}/commits/${tag}`
    : "";

const commits = readCommits(range).filter((commit) => !/^chore\(release\):/.test(commit.subject));
const releaseNotes = [];

for (const commit of commits) {
  const parsed = parseSubject(commit.subject);
  const notes = extractBullets(commit.body);
  const fallbackTitle = sanitizeBullet(parsed.title || commit.subject);
  const items = notes.length ? notes : fallbackTitle ? [fallbackTitle] : [];
  const topic = releaseNoteTopic(parsed.type, parsed.scope, commit.subject);
  for (const item of items) {
    releaseNotes.push({ topic, text: item });
  }
}

const notes = dedupeNotes(releaseNotes);

const lines = [];
lines.push(`## ${tag}`);
lines.push("");
if (notes.length) {
  for (const note of notes) {
    lines.push(`- ${note.topic}: ${note.text}`);
  }
} else {
  lines.push("- 维护: 本版本包含常规维护与兼容性更新。");
}
lines.push("- 安装包: Windows x64、macOS Intel、macOS Apple Silicon。");
if (changesUrl) {
  lines.push(`- 完整变更: ${changesUrl}`);
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
  const match = value.match(/^[vV]?(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$/);
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

function releaseNoteTopic(type, scope, subject) {
  const scopeTopics = {
    session: "会话删除",
    protocol: "协议兼容",
    models: "模型支持",
    model: "模型支持",
    proxy: "本地代理",
    manager: "管理器",
    launcher: "启动器",
    updater: "自动更新",
    installer: "安装",
    build: "构建",
    release: "发布维护",
    ci: "发布流程",
    ui: "界面"
  };
  if (scopeTopics[scope]) return scopeTopics[scope];
  if (/proxy|本地代理/i.test(scope) || /本地代理|proxy/i.test(subject)) return "本地代理";

  const typeTopics = {
    feat: "功能",
    fix: "修复",
    perf: "性能",
    refactor: "维护",
    docs: "文档",
    test: "测试",
    ci: "发布流程",
    chore: "维护"
  };
  return typeTopics[type] || "维护";
}

function sanitizeBullet(text) {
  return text
    .replace(
      /(?:\s*并\s*|[，,]\s*)(?:发布版本升级到|版本升级到|发布(?:版本)?)\s*[vV]?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/g,
      ""
    )
    .replace(/[，,]\s*发布版本升级到\s*\d+\.\d+\.\d+$/g, "")
    .replace(/[，,]\s*版本升级到\s*\d+\.\d+\.\d+$/g, "")
    .trim();
}

function dedupeNotes(items) {
  const seen = new Set();
  const result = [];
  for (const item of items) {
    const text = item.text.trim();
    if (!text) continue;
    const key = `${item.topic}\0${text.replace(/[。；;,.，\s]+$/g, "")}`;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push({ topic: item.topic, text });
  }
  return result;
}

function git(args) {
  return execFileSync("git", args, { encoding: "utf8" });
}
