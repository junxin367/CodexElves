"""按需统计当前 WSL 的 Claude 原生日志；只缓存消息身份及数字，不缓存正文或凭据。"""
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sqlite3
import sys
import time

SAFE_INTEGER = 9007199254740991
MAX_LINE = 32 * 1024 * 1024


def number(value):
    return type(value) in (int, float) and 0 <= value <= SAFE_INTEGER and math.isfinite(value)


def timestamp(value):
    if number(value):
        return int(value * 1000 if value < 1e12 else value)
    if not isinstance(value, str):
        return None
    try:
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
        return int(parsed.timestamp() * 1000) if parsed.tzinfo else None
    except (ValueError, OverflowError):
        return None


def event(row):
    if not isinstance(row, dict) or row.get("type") != "assistant" or row.get("isApiErrorMessage") is True:
        return None
    message = row.get("message")
    if not isinstance(message, dict) or message.get("model") == "<synthetic>":
        return None
    usage, model = message.get("usage"), message.get("model")
    at = timestamp(row.get("timestamp"))
    message_id = message.get("id")
    request_id = row.get("requestId") or row.get("requestRef")
    if isinstance(message_id, str) and message_id.startswith(("msg_", "resp_", "chatcmpl-")):
        identity = message_id
    elif isinstance(request_id, str) and request_id and (message_id is None or isinstance(message_id, str)):
        identity = json.dumps([message_id, request_id], separators=(",", ":"))
    else:
        identity = row.get("uuid")
    if (not isinstance(usage, dict) or not isinstance(model, str) or not model
            or len(model) > 256 or not isinstance(identity, str) or not identity or at is None):
        return None
    iterations = usage.get("iterations")
    # 官方 iterations 含计费用量（包括压缩）；存在时不再累加顶层，避免漏算压缩或重复计算。
    raw_parts = iterations if isinstance(iterations, list) and iterations else [usage]
    parts = []
    for part in raw_parts:
        if not isinstance(part, dict) or part.get("type") not in (None, "message", "compaction"):
            return {"id": identity, "at": at, "invalid": True}
        # 缺失的用量不能冒充真实 0；原生最终记录到达后会接替这条不完整记录。
        if "input_tokens" not in part or "output_tokens" not in part:
            return {"id": identity, "at": at, "invalid": True}
        fields = ["input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]
        counts = [part.get(key, 0) for key in fields]
        if not all(type(value) is int and number(value) for value in counts):
            return {"id": identity, "at": at, "invalid": True}
        creation = part.get("cache_creation") or {}
        hour = creation.get("ephemeral_1h_input_tokens", 0) if isinstance(creation, dict) else 0
        if (not isinstance(creation, dict) or type(hour) is not int or not number(hour) or hour > counts[3]
                or "ephemeral_5m_input_tokens" in creation
                and (type(creation["ephemeral_5m_input_tokens"]) is not int
                     or creation["ephemeral_5m_input_tokens"] != counts[3] - hour)):
            return {"id": identity, "at": at, "invalid": True}
        part_model = part.get("model") or model
        if not isinstance(part_model, str) or len(part_model) > 256 or sum(counts) > SAFE_INTEGER:
            return {"id": identity, "at": at, "invalid": True}
        parts.append({"model": part_model, "input": counts[0], "output": counts[1],
                      "read": counts[2], "write": counts[3] - hour, "hour": hour,
                      "context": counts[0] + counts[2] + counts[3], "tokens": sum(counts),
                      "tier": part.get("service_tier", usage.get("service_tier", "standard")),
                      "speed": part.get("speed", usage.get("speed")),
                      "geo": part.get("inference_geo", usage.get("inference_geo"))})
    # thinkingTokens 已包含在 output_tokens，不能重复累计。
    return {"id": identity, "at": at, "parts": parts, "tokens": sum(part["tokens"] for part in parts)}


def price_row(models, model):
    candidates = [model, re.sub(r"-\d{8}$", "", model)]
    if model.startswith("anthropic/"):
        candidates.extend([model[10:], re.sub(r"-\d{8}$", "", model[10:])])
    for key in candidates:
        row = models.get(key)
        if isinstance(row, dict):
            return row
    return None


def estimate_part(item, models):
    if item["tokens"] == 0:
        return 0.0
    row = price_row(models, item["model"])
    if row is None:
        return None
    thresholds = []
    for key in row:
        match = re.fullmatch(r"input_cost_per_token(_above_(\d+(?:\.\d+)?)([km]?)_tokens)"
                             r"(?:_(?:priority|flex|ultrafast|batches))?", key)
        if match:
            limit = float(match[2]) * {"": 1, "k": 1000, "m": 1000000}[match[3]]
            if item["context"] > limit:
                thresholds.append((limit, match[1]))
    long_suffix = max(thresholds, default=(0, ""))[1]
    tier = item.get("tier")
    if tier is not None and not isinstance(tier, str) or item.get("speed") not in (None, "standard", "fast"):
        return None
    tier_suffix = {"priority": "_priority", "flex": "_flex", "ultrafast": "_ultrafast",
                   "batch": "_batches"}.get(tier, "")
    if tier not in (None, "standard", "default", "auto", "priority", "flex", "ultrafast", "batch"):
        return None

    def rate(key):
        # 长上下文阈值和档位来自价格表；不把 priority/flex 误按普通价格估算。
        for candidate in dict.fromkeys([key + long_suffix + tier_suffix, key + tier_suffix]):
            value = row.get(candidate)
            if number(value):
                return value
        return None

    cost = 0.0
    for field, key in [("input", "input_cost_per_token"), ("output", "output_cost_per_token"),
                       ("read", "cache_read_input_token_cost"), ("write", "cache_creation_input_token_cost"),
                       ("hour", "cache_creation_input_token_cost_above_1hr")]:
        if item[field]:
            value = rate(key)
            if value is None:
                return None
            cost += item[field] * value
    extra = row.get("provider_specific_entry") or {}
    if not isinstance(extra, dict):
        extra = {}
    if item.get("speed") == "fast" and not tier_suffix:
        multiplier = extra.get("fast")
        if not number(multiplier) or multiplier == 0:
            return None
        cost *= multiplier
    region = item.get("geo")
    if region is not None and not isinstance(region, str):
        return None
    geo = extra.get(region, 1)
    if not number(geo) or geo == 0:
        return None
    cost *= geo
    return cost if number(cost) else None


def estimate_cost(item, models):
    if item.get("invalid"):
        return None
    costs = [estimate_part(part, models) for part in item["parts"]]
    return sum(costs) if all(value is not None for value in costs) else None


def merge_event(old, new):
    def quality(item):
        parts = item.get("parts", [])
        return (not item.get("invalid"), item.get("tokens", 0), len(parts),
                any(part.get("speed") == "fast" for part in parts),
                sum(part.get("hour", 0) for part in parts))
    # 参考 ccusage：保留更完整的流式计数及档位/缓存 TTL；重放不能移动原消费时间。
    chosen = dict(new if old is None or quality(new) >= quality(old) else old)
    if old is not None:
        chosen["at"] = min(old["at"], new["at"])
    return chosen


def read_events(path, cutoff, offset, deadline):
    records, complete = {}, True
    with path.open("rb") as source:
        source.seek(offset)
        while True:
            if time.monotonic() >= deadline:
                break
            before = source.tell()
            line = source.readline(MAX_LINE + 1)
            if not line:
                break
            if len(line) > MAX_LINE:
                # 大图片正文无需解析；异常超大 assistant 行保持统计未知，不阻断会话。
                assistant = b'"assistant"' in line[:4096]
                while line and not line.endswith(b"\n") and time.monotonic() < deadline:
                    line = source.readline(MAX_LINE + 1)
                if line and not line.endswith(b"\n"):
                    return list(records.values()), False, before
                complete = complete and not assistant
                offset = source.tell()
                continue
            if not line.endswith(b"\n"):
                offset = before
                break  # 原生日志仍在追加，下一次按新指纹重新读取完整行。
            offset = source.tell()
            if b'"assistant"' not in line:
                continue
            try:
                value = event(json.loads(line))
            except (ValueError, TypeError):
                complete = False
                continue
            if value and value["at"] >= cutoff:
                records[value["id"]] = merge_event(records.get(value["id"]), value)
    return list(records.values()), complete, offset


def anchor(path, offset):
    with path.open("rb") as source:
        source.seek(max(0, offset - 512))
        return hashlib.sha256(source.read(min(offset, 512))).hexdigest()


def collect(request, root=None, cache_path=None):
    now = request["nowMs"]
    windows = {}
    for window in request.get("windows", []):
        end, duration = timestamp(window.get("resetsAt")), window.get("durationMs")
        if end is not None and number(duration) and duration > 0 and end - duration < now < end:
            windows[window["key"]] = (int(end - duration), end)
    if not windows:
        return {}
    # 保留至少七天的数字账本；暂缺周额度时，也不能提前丢掉已经发生的消耗。
    cutoff = min(now - 7 * 86400000, min(window[0] for window in windows.values()))
    config = Path(os.environ.get("CLAUDE_CONFIG_DIR", "~/.claude")).expanduser()
    root = Path(root) if root is not None else config / "projects"
    if cache_path is None:
        scope = hashlib.sha256(str(root.resolve()).encode()).hexdigest()[:16]
        cache_path = Path.home() / ".cache" / "codex-elves" / ("usage-ledger-v1-" + scope + ".sqlite")
    cache_path = Path(cache_path)
    cache_path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    database = sqlite3.connect(cache_path, timeout=0.5)
    try:
        os.chmod(cache_path, 0o600)
        return collect_database(request, database, root, windows, cutoff, now)
    finally:
        database.close()


def collect_database(request, database, root, windows, cutoff, now):
    database.execute("CREATE TABLE IF NOT EXISTS events (id TEXT PRIMARY KEY, at INTEGER NOT NULL, data TEXT NOT NULL)")
    database.execute("CREATE INDEX IF NOT EXISTS event_time ON events(at)")
    database.execute("CREATE TABLE IF NOT EXISTS files (path TEXT PRIMARY KEY, size INTEGER, modified INTEGER, "
                     "inode INTEGER, offset INTEGER, anchor TEXT, valid INTEGER, checked INTEGER)")
    # SQLite 事务避免两个客户端同时更新数字账本时覆盖彼此；原生日志始终只读。
    database.execute("BEGIN IMMEDIATE")
    database.execute("DELETE FROM events WHERE at < ?", (cutoff,))
    database.execute("DELETE FROM files WHERE checked < ?", (cutoff,))
    complete = True
    deadline = time.monotonic() + 12

    def walk_error(failure):
        nonlocal complete
        # 文件夹删除后继续使用已记录账本；权限/读取失败不能把未扫描部分当成零消费。
        if not isinstance(failure, FileNotFoundError):
            complete = False

    for folder, _, names in os.walk(root, followlinks=False, onerror=walk_error):
        for name in names:
            if not name.endswith(".jsonl"):
                continue
            path = Path(folder) / name
            try:
                info = path.stat()
                if path.is_symlink() or info.st_mtime * 1000 < cutoff:
                    continue
                key = str(path.relative_to(root))
                saved = database.execute("SELECT size,modified,inode,offset,anchor,valid FROM files WHERE path=?",
                                         (key,)).fetchone()
                if (saved and saved[:2] == (info.st_size, info.st_mtime_ns) and saved[3] == info.st_size):
                    complete = complete and bool(saved[5])
                    continue
                if time.monotonic() >= deadline:
                    complete = False
                    continue
                offset = saved[3] if (saved and saved[2] == info.st_ino and saved[3] <= info.st_size
                                      and anchor(path, saved[3]) == saved[4]) else 0
                continued = offset > 0
                records, valid, offset = read_events(path, cutoff, offset, deadline)
                valid = valid and (bool(saved[5]) if continued else True)
                complete = complete and valid and offset == info.st_size
                for item in records:
                    old = database.execute("SELECT data FROM events WHERE id=?", (item["id"],)).fetchone()
                    item = merge_event(json.loads(old[0]) if old else None, item)
                    database.execute("INSERT OR REPLACE INTO events VALUES (?,?,?)",
                                     (item["id"], item["at"], json.dumps(item, separators=(",", ":"))))
                database.execute("INSERT OR REPLACE INTO files VALUES (?,?,?,?,?,?,?,?)",
                                 (key, info.st_size, info.st_mtime_ns, info.st_ino, offset,
                                  anchor(path, offset), int(valid), now))
            except (OSError, ValueError, TypeError, KeyError):
                complete = False
    # 会话删除、日志重写和压缩均不删除 events；只有消费时间离开保留窗口后才清理。
    database.commit()
    models = request.get("prices") or {}
    result = {}
    for key, (start, end) in windows.items():
        items = [json.loads(row[0]) for row in database.execute("SELECT data FROM events WHERE at>=? AND at<=?",
                                                               (start, now))]
        valid = complete and not any(item.get("invalid") for item in items)
        tokens = sum(item.get("tokens", 0) for item in items) if valid else None
        costs = [estimate_cost(item, models) for item in items] if valid else [None]
        cost = sum(costs) if all(value is not None for value in costs) else None
        # 用户要求预估总额度改为已消耗 ÷ 原生已用比例；本地账本只导出实耗，由网页结合账号比例计算。
        result[key] = {"source": "claude-wsl-local-journal", "observedAtMs": now,
                       "windowStartMs": start, "windowEndMs": end,
                       "usedCostUsd": cost if number(cost) else None,
                       "usedTokens": tokens if number(tokens) else None}
    return result


if __name__ == "__main__":
    try:
        print(json.dumps(collect(json.load(sys.stdin)), separators=(",", ":"), allow_nan=False))
    except (OSError, ValueError, TypeError, KeyError, sqlite3.Error):
        print("{}")
