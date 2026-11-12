"""原版 Claude 后台会话的 WSL 连接端；不运行模型、不修改 CLI、设置或权限。"""
import argparse
import base64
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import socket
import subprocess
import sys
import threading
import time
import unicodedata
import uuid


def emit(value):
    print(json.dumps(value, ensure_ascii=False, separators=(",", ":")), flush=True)


class NativeError(RuntimeError):
    def __init__(self, code, message=None):
        self.code = code
        super().__init__(message or f"原版 Claude 未接受操作（{code}）")


class Screen:
    """只读取原版输出形成正文预览；最终 Markdown 和工具结果仍来自原生消息记录。"""
    def __init__(self):
        self.lines = {}
        self.x = self.y = 0
        self.carry = ""

    def feed(self, chunk):
        text, self.carry = self.carry + chunk, ""
        index = 0
        while index < len(text):
            character = text[index]
            if character == "\x1b":
                start = index
                if index + 1 >= len(text):
                    self.carry = text[start:]
                    break
                kind = text[index + 1]
                if kind in ("]", "P", "_", "^"):
                    ends = [position for position in (text.find("\x07", index + 2),
                            text.find("\x1b\\", index + 2)) if position >= 0]
                    if not ends:
                        self.carry = text[start:]
                        break
                    end = min(ends)
                    index = end + (1 if text[end] == "\x07" else 2)
                    continue
                if kind == "[":
                    end = index + 2
                    while end < len(text) and not "@" <= text[end] <= "~":
                        end += 1
                    if end == len(text):
                        self.carry = text[start:]
                        break
                    raw = text[index + 2:end]
                    numbers = [int(value) if value.isdigit() else 0 for value in raw.split(";")]
                    value = (numbers[0] or 1) if numbers else 1
                    command = text[end]
                    if not raw.startswith("?"):
                        if command in ("H", "f"):
                            self.y = max(0, value - 1)
                            self.x = max(0, (numbers[1] if len(numbers) > 1 and numbers[1] else 1) - 1)
                        elif command == "A":
                            self.y = max(0, self.y - value)
                        elif command == "B":
                            self.y = min(4095, self.y + value)
                        elif command == "C":
                            self.x = min(4095, self.x + value)
                        elif command == "D":
                            self.x = max(0, self.x - value)
                        elif command == "G":
                            self.x = max(0, value - 1)
                        elif command == "J":
                            mode = numbers[0] if numbers else 0
                            if mode in (2, 3):
                                self.lines.clear()
                            elif mode == 0:
                                self.lines = {row: line for row, line in self.lines.items() if row <= self.y}
                        elif command == "K":
                            mode = numbers[0] if numbers else 0
                            if mode == 2:
                                self.lines[self.y] = []
                            elif mode == 0:
                                self.lines[self.y] = self.lines.get(self.y, [])[:self.x]
                            else:
                                line = self.lines.get(self.y, [])
                                self.lines[self.y] = [" "] * min(self.x + 1, len(line)) + line[self.x + 1:]
                    index = end + 1
                    continue
                index += 2
                continue
            if character == "\r":
                self.x = 0
            elif character == "\n":
                self.y = min(4095, self.y + 1)
            elif character == "\b":
                self.x = max(0, self.x - 1)
            elif character >= " " and character != "\x7f":
                line = self.lines.setdefault(self.y, [])
                while len(line) <= self.x:
                    line.append(" ")
                if unicodedata.combining(character):
                    if self.x:
                        line[self.x - 1] += character
                else:
                    line[self.x] = character
                    self.x += 1
                    if unicodedata.east_asian_width(character) in ("W", "F"):
                        if len(line) <= self.x:
                            line.append("")
                        else:
                            line[self.x] = ""
                        self.x += 1
            index += 1

    def text(self):
        return ["".join(self.lines.get(row, [])).rstrip()
                for row in range(max(self.lines, default=-1) + 1)]


def native_time(value):
    try:
        if isinstance(value, str):
            return max(0, int(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1000))
        return max(0, int(value or 0))
    except (ValueError, TypeError, OverflowError):
        return 0


def native_interruption(row):
    # 用户要求核实中断提示：只认原生关联字段与完整控制块，真实用户引用/同名文字不能按前缀过滤。
    if row.get("type") != "user":
        return False
    message = row.get("message")
    if not isinstance(message, dict):
        return False
    content = message.get("content")
    return (isinstance(row.get("interruptedMessageId"), str)
            and bool(row["interruptedMessageId"].strip()) and isinstance(content, list)
            and len(content) == 1 and isinstance(content[0], dict) and content[0].get("type") == "text"
            and isinstance(content[0].get("text"), str) and content[0]["text"].strip() in
            ("[Request interrupted by user]", "[Request interrupted by user for tool use]"))


def real_user(row):
    if any(row.get(key) is True for key in ("isMeta", "isSidechain", "isCompactSummary", "isVisibleInTranscriptOnly")):
        return False
    if native_interruption(row):
        return False
    if row.get("type") == "attachment":
        attachment = row.get("attachment", {})
        return (attachment.get("type") == "queued_command" and attachment.get("commandMode") == "prompt"
                and attachment.get("humanTurn") is True and attachment.get("origin", {}).get("kind") == "human"
                and bool(attachment.get("prompt", "").strip()))
    if row.get("type") != "user":
        return False
    message = row.get("message")
    if not isinstance(message, dict):
        return False
    content = message.get("content", [])
    if isinstance(content, str):
        return bool(content.strip())
    if not isinstance(content, list):
        return False
    return any(
        isinstance(part, dict) and (part.get("type") == "image"
        or part.get("type") == "text" and isinstance(part.get("text"), str) and bool(part["text"].strip()))
        for part in content)


class NativeDiscovery:
    """只读原生目录；不调用 CLI、守护进程或模型，也不使用网页登记文件作为发现来源。"""
    def __init__(self, root=None):
        self.root = Path(root or os.environ.get("CLAUDE_CONFIG_DIR", Path.home() / ".claude")).resolve()
        self.logs, self.readers, self.rows, self.jobs = {}, {}, {}, {}
        self.scanned_at = None
        self.bytes_read = 0

    def scan(self, force=False):
        # 用户要求正常刷新自动发现且不高频重扫正文：目录元数据最多每两秒核对，JSONL 仅从已读取偏移继续。
        if not force and self.scanned_at is not None and time.monotonic() - self.scanned_at < 2:
            return list(self.rows.values())
        rows, jobs, alive = {}, {}, set()
        projects = (self.root / "projects").resolve()
        for path in projects.glob("*/*.jsonl"):
            try:
                native = str(uuid.UUID(path.stem))
                if not path.resolve().is_relative_to(projects):
                    continue
                stat = path.stat()
                alive.add(path)
                cached = self.logs.get(path)
                identity = (stat.st_dev, stat.st_ino)
                if (cached is None or cached["identity"] != identity or stat.st_size < cached["offset"]
                        or stat.st_size == cached["offset"] and stat.st_mtime_ns != cached["mtime"]):
                    cached = {"identity": identity, "offset": 0, "mtime": 0, "real": False,
                              "nativeId": native, "title": "", "customTitle": "", "cwd": "",
                              "createdAt": 0, "updatedAt": 0, "model": "", "effort": ""}
                    self.logs[path] = cached
                if stat.st_size > cached["offset"]:
                    with path.open("rb") as stream:
                        stream.seek(cached["offset"])
                        while True:
                            position = stream.tell()
                            line = stream.readline()
                            if not line.endswith(b"\n"):
                                cached["offset"] = position
                                break
                            cached["offset"] = stream.tell()
                            self.bytes_read += len(line)
                            try:
                                row = json.loads(line)
                            except ValueError:
                                continue
                            if not isinstance(row, dict):
                                continue
                            if row.get("type") in ("user", "assistant") and not isinstance(row.get("message"), dict):
                                continue
                            if row.get("isSidechain") or row.get("sessionId", native) != native:
                                continue
                            if row.get("type") == "custom-title" and isinstance(row.get("customTitle"), str):
                                cached["customTitle"] = " ".join(row["customTitle"].split())[:120]
                            if real_user(row):
                                cached["real"] = True
                                if not cached["title"]:
                                    content = row.get("message", {}).get("content", row.get("attachment", {}).get("prompt", ""))
                                    text = content if isinstance(content, str) else " ".join(part.get("text", "") for part in content if part.get("type") == "text")
                                    cached["title"] = " ".join(text.split())[:60] or "图片会话"
                                if not cached["cwd"] and isinstance(row.get("cwd"), str) and row["cwd"].startswith("/"):
                                    cached["cwd"] = row["cwd"]
                            # 内部摘要和旁支不贡献标题/时间；只有真实对话及其原生完成记录更新最近时间。
                            meaningful = real_user(row) or row.get("type") == "assistant" and row.get("isApiErrorMessage") is not True or row.get("subtype") == "turn_duration"
                            if meaningful:
                                at = native_time(row.get("timestamp"))
                                if at:
                                    cached["createdAt"] = min(cached["createdAt"] or at, at)
                                    cached["updatedAt"] = max(cached["updatedAt"], at)
                                model = row.get("message", {}).get("model") if row.get("type") == "assistant" else row.get("requestedModel")
                                if isinstance(model, str) and model and model != "<synthetic>":
                                    cached["model"] = model
                                if isinstance(row.get("effort"), str):
                                    cached["effort"] = row["effort"]
                cached["mtime"] = stat.st_mtime_ns
                if cached["real"]:
                    rows[native] = {key: cached[key] for key in ("nativeId", "cwd", "createdAt", "updatedAt", "model", "effort")}
                    # 用户要求忽略本程序 --name 生成的内部连接标签，真实原生自定义名称仍优先于首条问题。
                    custom = cached["customTitle"]
                    rows[native]["title"] = (custom if not re.fullmatch(r"elves-[0-9a-f]{10}", custom) else "") or cached["title"]
            except (OSError, ValueError, TypeError):
                continue
        self.logs = {path: data for path, data in self.logs.items() if path in alive}
        for path in (self.root / "jobs").glob("*/state.json"):
            try:
                if not re.fullmatch(r"[0-9a-f]{8}", path.parent.name) or not path.resolve().is_relative_to((self.root / "jobs").resolve()):
                    continue
                raw_job = json.loads(path.read_text(encoding="utf-8"))
                # 只保留会话归属/展示元数据；providerEnv、提示模板和认证信息不能进入发现缓存或返回值。
                job = {key: raw_job[key] for key in ("sessionId", "resumeSessionId", "cwd", "name", "nameSource", "createdAt", "updatedAt",
                       "tempo", "state", "needs", "detail", "outcome") if key in raw_job}
                flags = raw_job.get("respawnFlags", [])
                for index, flag in enumerate(flags[:-1]):
                    if flag in ("--model", "--effort"):
                        job[flag[2:]] = flags[index + 1]
                native = str(uuid.UUID(job.get("resumeSessionId") or job["sessionId"]))
                if native not in rows:
                    continue
                current = jobs.get(native)
                if current is None or (job.get("tempo") in ("active", "blocked") and not job.get("outcome"), native_time(job.get("updatedAt"))) > (
                        current.get("tempo") in ("active", "blocked") and not current.get("outcome"), native_time(current.get("updatedAt"))):
                    jobs[native] = {**job, "short": path.parent.name}
            except (OSError, ValueError, KeyError, TypeError):
                continue
        for native, row in rows.items():
            job = jobs.get(native, {})
            row["jobId"] = job.get("short", "")
            row["nativeStatus"] = self.status(job)
            if job.get("nameSource") == "user" and job.get("name") and not row["title"]:
                row["title"] = str(job["name"])[:120]
            for key in ("cwd", "model", "effort"):
                if not row[key] and isinstance(job.get(key), str):
                    row[key] = job[key]
            for key in ("createdAt", "updatedAt"):
                if not row[key]:
                    row[key] = native_time(job.get(key))
        rows = {native: row for native, row in rows.items() if row["cwd"].startswith("/")}
        self.rows, self.jobs, self.scanned_at = rows, jobs, time.monotonic()
        return list(rows.values())

    @staticmethod
    def status(job):
        status = {key: job.get(key) for key in ("tempo", "state", "needs", "detail", "outcome")}
        if not job or job.get("state") in ("done", "stopped", "failed", "ended") or job.get("outcome"):
            status["tempo"] = "idle"
        return status

    def control(self, request):
        subtype = request.get("subtype")
        if subtype == "initialize":
            return {}
        if subtype == "discover":
            return {"sessions": self.scan(force=True)}
        if subtype != "read_history":
            raise RuntimeError("只读发现连接不接受执行操作")
        native = str(uuid.UUID(request["native_id"]))
        self.scan()
        if native not in self.rows:
            raise RuntimeError("指定 WSL 内没有该原生会话的有效历史")
        reader = self.readers.get(native)
        if reader is None:
            reader = NativeSession("unused", native)
            reader.root = self.root
            self.readers[native] = reader
            # 只缓存最近四条打开的完整历史，发现索引本身只保存非敏感元数据和偏移。
            if len(self.readers) > 4:
                self.readers.pop(next(iter(self.readers)))
        reader.clients.update(request.get("clients", {}))
        reader.pending = request.get("pending", [])
        reader.model = request.get("model", "")
        reader.read_messages()
        row = self.rows[native]
        return {"type": "shared_snapshot", "native_id": native, "job_id": row["jobId"],
                "native_status": row["nativeStatus"], "messages": reader.messages,
                "clients": reader.clients, "pending": reader.pending, "requests": {},
                "context_configuration": reader.context_configuration(), "updated_at": row["updatedAt"]}

    def poll(self, force=False):
        pass


class NativeSession:
    def __init__(self, cli, native_id, job_id=""):
        self.cli = cli
        self.root = Path(os.environ.get("CLAUDE_CONFIG_DIR", Path.home() / ".claude")).resolve()
        digest = hashlib.sha256(str(self.root).encode()).hexdigest()[:8]
        self.address = f"/tmp/cc-daemon-{os.getuid()}/{digest}/control.sock"
        self.native_id = native_id
        # 已结束会话也要按保存的后台 ID 读取历史；不能依赖仍在线的 supervisor 才能补齐排队状态。
        self.short = job_id
        self.model = ""
        self.effort = ""
        self.messages = []
        self.image_inputs = {}
        self.clients = {}
        self.pending = []
        self.job = {}
        self.signature = None
        self.log_path = None
        self.log_offset = 0
        self.log_identity = None
        self.log_mtime = None
        self.requests = {}
        self.initialized = False
        self.seen = set()
        self.incoming = None
        self.observer = None
        self.observed_short = ""
        self.screen = Screen()
        self.preview_user = ""
        self.delivery_started = False
        self.preview = ""
        self.preview_emitted = ""
        self.preview_at = 0
        self.context_files = {}
        self.native_context = {}

    def context_configuration(self):
        # 用户要求容量与压缩设置分开：这里只传配置来源，不能把任何配置值当作原生容量。
        key = "CLAUDE_CODE_MAX_CONTEXT_TOKENS"
        keys = (key, "CLAUDE_CODE_AUTO_COMPACT_WINDOW", "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE")
        values = {name: (os.environ.get(name), "environment") for name in keys}
        path = self.root / "settings.json"
        try:
            stat = path.stat()
            signature = (stat.st_mtime_ns, stat.st_size)
            cached = self.context_files.get(path)
            if cached is None or cached[0] != signature:
                data = json.loads(path.read_text(encoding="utf-8"))
                env = data.get("env", {}) if isinstance(data, dict) else {}
                self.context_files[path] = (signature, {name: env[name] for name in keys if name in env} if isinstance(env, dict) else {})
            configured = self.context_files[path][1]
            values.update({name: (raw, "user-settings.env") for name, raw in configured.items()})
        except (OSError, ValueError):
            pass
        # 配置、原生窗口和最后实际请求绑定同一模型，不能把其他模型或会话的窗口带过来。
        model = next((row["message"].get("model", "") for row in reversed(self.messages)
                      if row.get("type") == "assistant" and row.get("isUnmetered") is not True
                      and row.get("isApiErrorMessage") is not True and row.get("message", {}).get("model") != "<synthetic>"
                      and isinstance(row.get("message", {}).get("usage", {}).get("input_tokens"), int)), self.model)
        result = {"model": model}
        for name, field in ((key, "maxContextTokens"),
                            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "windowTokens"),
                            ("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", "triggerPercent")):
            raw, scope = values[name]
            if not isinstance(raw, str) or not re.fullmatch(r"\s*\+?\d+\s*", raw):
                continue
            count = int(raw)
            if not 0 < count <= (100 if field == "triggerPercent" else 9007199254740991):
                continue
            source = {"kind": "configuration", "key": name, "scope": scope, "model": model}
            if name == key:
                result.update(maxContextTokens=count, source=source)
            else:
                result.setdefault("compaction", {})[field] = count
                result["compaction"][field + "Source"] = source
        if self.native_context.get("model") == model:
            result["native"] = self.native_context
        return result if len(result) > 1 else {}

    def observe_native_context(self, row):
        # 只接收同一原生日志/会话明确报告的 statusline 字段；不调用模型或复刻 CLI 的窗口计算。
        if row.get("session_id", row.get("sessionId", self.native_id)) != self.native_id:
            return
        context = row.get("context_window")
        message = row.get("message")
        if not isinstance(context, dict) and isinstance(message, dict):
            context = message.get("context_window")
        if not isinstance(context, dict):
            return
        model = row.get("model")
        if isinstance(model, dict):
            model = model.get("id")
        if not isinstance(model, str) and isinstance(message, dict):
            model = message.get("model")
        size = context.get("context_window_size")
        if (not isinstance(model, str) or not model or type(size) is not int
                or not 0 < size <= 9007199254740991):
            return
        report = {"model": model, "context_window_size": size}
        percentage = context.get("used_percentage")
        if type(percentage) in (int, float) and 0 <= percentage <= 100:
            report["used_percentage"] = percentage
        usage = context.get("current_usage")
        if isinstance(usage, dict):
            report["current_usage"] = {key: usage[key] for key in (
                "input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens")
                if type(usage.get(key)) is int and 0 <= usage[key] <= 9007199254740991}
        self.native_context = report

    def rpc(self, operation):
        # 2.1.322 的原生后台协议 v1；control.key 只用于同一 WSL 用户的本地连接，不回传网页。
        message = {"proto": 1, **operation}
        if message["op"] in ("reply", "attach"):
            message["auth"] = (self.root / "daemon" / "control.key").read_text().strip()
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(8)
            connection.connect(self.address)
            connection.sendall(json.dumps(message, ensure_ascii=False).encode() + b"\n")
            response = json.loads(connection.makefile("rb").readline(1024 * 1024))
        if not response.get("ok"):
            raise NativeError(response.get("code", "UNKNOWN"))
        return response

    def lookup(self):
        try:
            jobs = self.rpc({"op": "list"}).get("jobs", [])
        except (FileNotFoundError, ConnectionRefusedError):
            return None
        except (OSError, RuntimeError) as failure:
            # 查询超时或协议错误不等于原任务已结束，不能据此另起执行或重启正在工作的会话。
            raise RuntimeError("暂时无法核对原生 Claude 后台状态，未重新启动或提交消息") from failure
        return next((job for job in jobs if (job.get("resumeSessionId") or job.get("sessionId")) == self.native_id), None)

    def bind(self, job):
        self.job = job
        self.short = job["short"]
        self.native_id = job.get("resumeSessionId") or job["sessionId"]
        if self.incoming is not None and (self.observed_short != self.short
                                         or not self.observer or not self.observer.is_alive()):
            self.observed_short = self.short
            self.screen = Screen()
            self.observer = threading.Thread(target=self.observe_stream, args=(self.short,), daemon=True)
            self.observer.start()

    def observe_stream(self, short):
        try:
            with socket.socket(socket.AF_UNIX) as connection:
                connection.connect(self.address)
                connection.sendall(json.dumps({"proto": 1, "op": "subscribe",
                                               "short": short, "tail": 200}).encode() + b"\n")
                for line in connection.makefile("rb"):
                    value = json.loads(line)
                    if value.get("type") == "snapshot":
                        for chunk in value.get("streamTail", []):
                            self.incoming.put({"_stream": chunk})
                    elif value.get("type") == "stream":
                        self.incoming.put({"_stream": value.get("line", "")})
        except (OSError, ValueError):
            return

    def reply(self, text, next_turn=True):
        deadline = time.monotonic() + 20
        while True:
            try:
                return self.rpc({"op": "reply", "short": self.short,
                                 "text": text, "nextTurn": next_turn})
            except NativeError as failure:
                # 原版后台冷启动也会明确拒绝尚未就绪的提交；仅在“尚未接受”时短暂等待，不重发未知结果。
                if failure.code not in ("ERESPAWNING", "ESTARTING", "ENOREPLY") or time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)

    def check_history_resume(self):
        # 用户要求不创建副本：原生 2.1.322 在被占用/短 ID 冲突/状态不可核对时会自动分叉，首次发送前只读预检并明确拒绝这些情况。
        if str(uuid.UUID(self.native_id)) != self.native_id or (self.root / "jobs" / self.native_id[:8]).exists():
            raise NativeError("ESESSIONCONFLICT", "原生会话身份或后台登记有冲突，未创建副本或提交消息")
        try:
            for path in (self.root / "sessions").glob("*.json"):
                record = json.loads(path.read_text(encoding="utf-8"))
                if record.get("sessionId") != self.native_id or record.get("kind") == "interactive" and record.get("parkedJobId"):
                    continue
                pid = int(record.get("pid", path.stem))
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    continue
                raise NativeError("ESESSIONOPEN", "原生会话仍在其他 CLI 中打开，未接管草稿、创建副本或提交消息")
            try:
                state = self.rpc({"op": "has", "short": self.native_id[:8]})
                if not isinstance(state.get("alive"), bool) or "present" in state and not isinstance(state["present"], bool):
                    raise ValueError("无法核对原生后台占用状态")
                occupied = state.get("alive") or state.get("present")
            except (FileNotFoundError, ConnectionRefusedError):
                path = self.root / "daemon/roster.json"
                roster = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}
                worker = roster.get("workers", {}).get(self.native_id[:8])
                occupied = False
                if worker:
                    try:
                        os.kill(int(worker["pid"]), 0)
                        occupied = True
                    except ProcessLookupError:
                        pass
            if occupied:
                raise NativeError("ESESSIONBUSY", "原生后台仍持有该 UUID，未创建副本或提交消息")
        except (OSError, ValueError, KeyError, TypeError) as failure:
            raise NativeError("ERESUMECHECK", "无法只读确认原生会话未被占用，未恢复、创建副本或提交消息") from failure

    def start(self, text):
        if self.native_id:
            self.check_history_resume()
        # 网页与 CLI 必须操作所选项目的同一目录；仅关闭后台默认的 worktree 隔离，不更改权限或提示。
        arguments = [self.cli, "--bg", "--settings", '{"worktree":{"bgIsolation":"none"}}']
        if self.native_id:
            arguments += ["--resume", self.native_id]
        if self.model:
            arguments += ["--model", self.model]
        if self.effort:
            arguments += ["--effort", self.effort]
        # 恢复历史时不附加新名称，保留原生自定义标题；新会话仍沿用既有命名。
        if not self.native_id:
            arguments += ["--name", f"elves-{uuid.uuid4().hex[:10]}"]
        arguments += ["--", text]
        result = subprocess.run(arguments, stdin=subprocess.DEVNULL,
                                capture_output=True, timeout=60)
        output = (result.stdout + result.stderr).decode("utf-8", "replace")
        match = re.search(r"backgrounded\s*[·:]\s*([0-9a-f]{8})", output)
        if result.returncode or not match:
            # CLI 错误可能包含认证环境；只报告可操作说明，不转发 stderr 或设置文件。
            raise RuntimeError("无法启动原版 Claude 后台会话，请先在该 WSL 项目目录完成 CLI 安装、登录和目录信任")
        job = next((job for job in self.rpc({"op": "list"}).get("jobs", [])
                    if job.get("short") == match[1]), None)
        if not job:
            raise RuntimeError("Claude 已启动，后台会话身份尚未返回，请核对原生会话")
        if self.native_id and (job.get("resumeSessionId") or job["sessionId"]) != self.native_id:
            raise RuntimeError("原版 Claude 未返回同一 UUID，未替换原会话、历史或停止其他任务")
        self.bind(job)

    def restore_job(self):
        # 用户要求恢复同一原生会话：已有后台 ID 用原版 respawn 重新连接；
        # --bg --resume 遇到残留的运行标记会自动创建副本，不能用它代替原会话。
        expected_short = self.short
        result = subprocess.run([self.cli, "respawn", expected_short],
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=30)
        if result.returncode:
            raise RuntimeError("原版 Claude 后台恢复失败，未创建副本或提交消息，请核对原版 CLI")
        deadline = time.monotonic() + 20
        while True:
            job = self.lookup()
            if job and job.get("short") == expected_short and not job.get("outcome"):
                self.bind(job)
                # respawn 复用原生任务保存的启动参数；网页最近选择的模型/等级仍需由原生命令应用。
                for name in ("model", "effort"):
                    value = getattr(self, name)
                    if value:
                        self.reply(f"/{name} {value}", next_turn=False)
                return job
            if time.monotonic() >= deadline:
                raise RuntimeError("原生 Claude 尚未恢复同一会话，未提交消息，请稍后核对")
            time.sleep(0.2)

    def prepare_text(self, content, client_id):
        text = "\n".join(part["text"] for part in content if part.get("type") == "text")
        images = [part for part in content if part.get("type") == "image"]
        if images:
            folder = self.root / "remote-attachments" / client_id
            folder.mkdir(parents=True, exist_ok=True, mode=0o700)
            for index, image in enumerate(images):
                source = image["source"]
                extension = {"image/png": ".png", "image/jpeg": ".jpg",
                             "image/gif": ".gif", "image/webp": ".webp"}[source["media_type"]]
                path = folder / f"{index}{extension}"
                path.write_bytes(base64.b64decode(source["data"], validate=True))
                path.chmod(0o600)
                text += f"\n@{path}"
        return text

    def send(self, request):
        self.delivery_started = False
        client_id = request["client_id"]
        content = request["content"]
        self.read_messages()
        after = len(self.messages)
        active_user = next((row.get("uuid", "") for row in reversed(self.messages) if real_user(row)), "")
        live = self.lookup() if self.native_id else None
        if live:
            self.bind(live)
        # 自动续发必须以实时原生状态为准；CLI 恰好开始新任务时继续等，不能抢先把下一条塞入原生队列。
        if request.get("idle_only") and live and not live.get("outcome"):
            if live.get("tempo") == "active" or live.get("tempo") == "blocked" and live.get("needs"):
                self.poll(force=True)
                return {"deferred": True, "accepted": False}
        if request.get("steer") and live and live.get("tempo") == "blocked" and live.get("needs"):
            raise NativeError("EBUSY")
        text = self.prepare_text(content, client_id)
        if self.native_id and self.short and (not live or live.get("outcome")):
            live = self.restore_job()
        intent = {"id": client_id, "clientUserMessageId": client_id,
                  "text": "\n".join(p.get("text", "") for p in content if p.get("type") == "text"),
                  "content": content, "_nativeText": text, "_after": after, "deliveryConfirmed": False}
        # 只有选中的远程消息才到达这里；先报告派发意图，断线后可按原生记录核对，不能盲目重发。
        self.pending.append(intent)
        emit({"type": "shared_delivery_intent", "intent": intent})
        self.delivery_started = True
        try:
            if live and not live.get("outcome"):
                self.bind(live)
                self.reply(text)
            else:
                self.start(text)
        except NativeError:
            self.pending.remove(intent)
            self.delivery_started = False
            raise
        intent["deliveryConfirmed"] = True
        self.poll(force=True)
        steered = False
        if request.get("steer") and intent in self.pending and self.job.get("tempo") == "active":
            # 只投递用户选中的一条；必要时使用 Esc 提前交给原生，保留 CLI 草稿，不提交其他远程待发项。
            try:
                self.terminal_input(b"\x1b", require_active=True)
                steered = True
            except (OSError, ValueError, RuntimeError):
                # 消息已被原生受理，中断失败也不能退回待发或再次提交。
                pass
        return {"nativeId": self.native_id, "jobId": self.short, "accepted": True,
                "steered": steered, "interruptedUserId": active_user}

    def terminal_input(self, data, require_active=False):
        job = self.lookup()
        if not job or job.get("outcome"):
            raise RuntimeError("原生 Claude 会话已停止")
        if require_active and job.get("tempo") != "active":
            raise RuntimeError("原生 Claude 已进入其他状态，未发送中断按键")
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(5)
            connection.connect(self.address)
            auth = (self.root / "daemon" / "control.key").read_text().strip()
            connection.sendall(json.dumps({"proto": 1, "op": "attach", "short": self.short,
                                           "auth": auth, "cols": 120, "rows": 40,
                                           "attachId": str(uuid.uuid4())}).encode() + b"\n")
            response = bytearray()
            deadline = time.monotonic() + 5
            while b"\n" not in response:
                # 原生连接在握手时断开不能卡住整个会话；限制总等待与帧大小，确认前不发送停止/审批按键。
                remaining = deadline - time.monotonic()
                if remaining <= 0 or len(response) >= 64 * 1024:
                    raise RuntimeError("原生交互握手超时或响应过大，未发送交互按键")
                connection.settimeout(remaining)
                chunk = connection.recv(1)
                if not chunk:
                    raise RuntimeError("原生连接在握手期间断开，未发送交互按键")
                response.extend(chunk)
            if not json.loads(response).get("ok"):
                raise RuntimeError("无法连接原版 Claude 的交互界面")
            # 仅显式停止/审批使用原生按键；普通消息始终走整条入队入口。
            connection.sendall(data)
            time.sleep(0.15)

    def change_setting(self, name, value):
        old = getattr(self, name)
        # 用户选定 Max 后必须实际应用到同一原生会话；从默认空值切换也需要发送原生命令。
        if self.native_id and value != old:
            job = self.lookup()
            if job and not job.get("outcome"):
                command = "model" if name == "model" else "effort"
                # 模型设置也由原版命令入口决定立即应用还是排队，不增加网页忙碌锁。
                self.short = job["short"]
                self.reply(f"/{command} {value or 'auto'}", next_turn=False)
        setattr(self, name, value)

    def send_queue_now(self):
        self.poll()
        job = self.lookup()
        if not self.pending:
            raise RuntimeError("排队消息已经进入原生会话，请核对最新状态")
        if not job or job.get("tempo") != "active":
            raise RuntimeError("Claude 当前正在等待交互或已结束，请在原版 CLI 中核对")
        self.bind(job)
        active_user = next((row["uuid"] for row in reversed(self.messages) if real_user(row)), "")
        # 原版仅提供整队立即执行；Esc 保留 CLI 草稿，不用会连同草稿提交的 Ctrl+Enter。
        self.terminal_input(b"\x1b")
        return {"scope": "all", "nativeUserId": active_user}

    def answer(self, frame):
        response = frame["response"]
        request_id = response["request_id"]
        self.poll()
        request = self.requests.get(request_id)
        if not request:
            raise RuntimeError("该原生请求已经结束")
        answer = response["response"]
        if request["tool_name"] == "AskUserQuestion":
            values = answer.get("updatedInput", {}).get("answers", {})
            if len(request["input"].get("questions", [])) != 1:
                raise RuntimeError("多问题面板请在原版 Claude CLI 中回答")
            text = next(iter(values.values()), "")
            self.rpc({"op": "reply", "short": self.short, "text": text})
        else:
            self.terminal_input(b"1" if answer.get("behavior") == "allow" else b"\x1b")
        emit({"type": "control_cancel_request", "request_id": request_id})

    def retain_web_images(self, row, intent=None):
        # 用户要求已受理网页图片在历史/重连仍完整：优先保留原输入，其次读取该 client 专属原图，不以原生附件转录数量为准。
        client = self.clients.get(row["uuid"], "")
        content = row.get("message", {}).get("content", [])
        text = content if isinstance(content, str) else "\n".join(
            part.get("text", "") for part in content if part.get("type") == "text")
        uploads = self.root / "remote-attachments"
        folder = uploads / client if re.fullmatch(r"[A-Za-z0-9_-]{1,120}", client) else None
        paths = []
        for line in text.splitlines():
            path = Path(line[1:]) if line.startswith("@") else None
            # 用户要求空网页登记也恢复原图：仅认可原生 prompt 内本用户上传目录的直接文件，不能跨目录/符号链接读取或伪造 client ID。
            if (path is None or path.parent.parent != uploads or not re.fullmatch(r"[A-Za-z0-9_-]{1,120}", path.parent.name)
                    or not re.fullmatch(r"[0-7]\.(png|jpg|gif|webp)", path.name)
                    or path.parent.resolve().parent != uploads.resolve()
                    or path.resolve().parent != path.parent.resolve()):
                continue
            if folder is None:
                folder = path.parent
            if path.parent == folder and path not in paths:
                paths.append(path)
        row["remote_upload_paths"] = [str(path) for path in paths]
        saved = {image["filename"]: image["url"] for image in self.image_inputs.get(row["uuid"], [])}
        originals = [part for part in (intent or {}).get("content", []) if part.get("type") == "image"]
        images, total = [], 0
        for index, path in enumerate(paths):
            url = saved.get(str(path))
            if index < len(originals):
                source = originals[index]["source"]
                url = f"data:{source['media_type']};base64,{source['data']}"
            if url is None:
                try:
                    if path.stat().st_size > 5 * 1024 * 1024:
                        continue
                    data = path.read_bytes()
                except OSError:
                    continue
                mime = {".png": "image/png", ".jpg": "image/jpeg", ".gif": "image/gif", ".webp": "image/webp"}[path.suffix]
                url = f"data:{mime};base64,{base64.b64encode(data).decode()}"
            size = len(base64.b64decode(url.split(",", 1)[1], validate=True))
            # 仅张数改为 8；沿用单张 5 MiB、合计 12 MiB，历史恢复不能另设更宽松的大小限制。
            if not size or size > 5 * 1024 * 1024 or total + size > 12 * 1024 * 1024:
                continue
            total += size
            if not any(image["filename"] == str(path) for image in images):
                images.append({"filename": str(path), "url": url})
        self.image_inputs[row["uuid"]] = row["web_images"] = images

    def read_messages(self):
        if not self.log_path and self.native_id:
            paths = list((self.root / "projects").glob(f"*/{self.native_id}.jsonl"))
            if paths:
                self.log_path = paths[0]
        if not self.log_path or not self.log_path.exists():
            return
        stat = self.log_path.stat()
        identity = (stat.st_dev, stat.st_ino)
        # 用户要求重连补齐真实历史；原生日志替换或重写后不能沿用旧偏移，同一大小的新文件也须重新投影。
        if (self.log_identity is not None and identity != self.log_identity
                or stat.st_size < self.log_offset
                or self.log_mtime is not None and stat.st_size == self.log_offset
                and stat.st_mtime_ns != self.log_mtime):
            self.messages, self.log_offset, self.seen = [], 0, set()
            self.native_context = {}
            self.signature = None
        self.log_identity, self.log_mtime = identity, stat.st_mtime_ns
        with self.log_path.open("rb") as stream:
            stream.seek(self.log_offset)
            while True:
                position = stream.tell()
                line = stream.readline()
                if not line.endswith(b"\n"):
                    self.log_offset = position
                    break
                self.log_offset = stream.tell()
                try:
                    row = json.loads(line)
                except ValueError:
                    continue
                if not isinstance(row, dict):
                    continue
                if not row.get("isSidechain"):
                    self.observe_native_context(row)
                if not row.get("uuid") or row.get("isSidechain"):
                    continue
                if row.get("type") in ("user", "assistant") and not isinstance(row.get("message"), dict):
                    continue
                if row["uuid"] in self.seen:
                    continue
                if row.get("type") == "attachment":
                    attachment = row.get("attachment", {})
                    if (attachment.get("type") == "queued_command"
                            and attachment.get("commandMode") == "prompt"
                            and attachment.get("humanTurn") is True
                            and attachment.get("origin", {}).get("kind") == "human"
                            and isinstance(attachment.get("prompt"), str)):
                        # 原版执行中吸收用户排队消息时记为附件；它已实际接收，必须关联原消息并清掉排队残留。
                        row = {**row, "type": "user", "message": {
                            "role": "user", "content": attachment["prompt"]}}
                    else:
                        image = attachment.get("content", {})
                        if attachment.get("type") != "file" or image.get("type") != "image":
                            continue
                        source = image.get("file", {})
                        # 保留网页原有图片气泡；只投影原版已读取的图片，不传环境、工具目录等内部附件。
                        row = {"type": "attachment", "uuid": row["uuid"],
                               "timestamp": row.get("timestamp"),
                               "filename": attachment.get("filename", ""),
                               "remote_upload": attachment.get("filename", "").startswith(
                                   str(self.root / "remote-attachments") + "/"),
                               "message": {"content": [{"type": "image", "url":
                                   f"data:{source['type']};base64,{source['base64']}"}]}}
                if row.get("type") not in ("user", "assistant", "system", "attachment"):
                    continue
                if row.get("type") == "system" and row.get("subtype") != "turn_duration":
                    continue
                self.seen.add(row["uuid"])
                self.messages.append({key: row[key] for key in
                                      ("type", "uuid", "parentUuid", "interruptedMessageId", "timestamp", "message", "subtype", "isMeta",
                                       "isCompactSummary", "isVisibleInTranscriptOnly",
                                       "isApiErrorMessage", "isUnmetered", "apiBlockIndex", "durationMs",
                                       "requestedModel", "effort", "filename", "remote_upload")
                                      if key in row})
                if isinstance(row.get("message"), dict):
                    message = {key: row["message"][key] for key in
                               # 用户要求显示最后实际请求用量；保留原生 usage（含缓存和 iterations），不读取 job 累计 tokens。
                               ("role", "id", "model", "content", "usage") if key in row["message"]}
                    if isinstance(message.get("content"), list):
                        # 展示不需要加密签名和请求诊断；原生文件保留原文，避免把不可见数据反复传到网页。
                        message["content"] = [{key: value for key, value in part.items()
                                               if key not in ("signature", "data")}
                                              if part.get("type") == "thinking" else part
                                              for part in message["content"]
                                              if part.get("type") != "redacted_thinking"]
                    self.messages[-1]["message"] = message
                if row.get("timestamp"):
                    self.messages[-1]["timestampMs"] = native_time(row["timestamp"])
        # 原生 prompt 的 UUID 是原程序生成的；按已受理顺序关联网页 client ID，重连仍保留关联。
        mapped = set(self.clients)
        for index, row in enumerate(self.messages):
            # 用户要求内部摘要与中断控制记录不充当用户消息，也不能确认待发送消息或成为预览锚点。
            if not real_user(row):
                continue
            if row["uuid"] in mapped:
                if "web_images" not in row:
                    self.retain_web_images(row)
                continue
            content = row.get("message", {}).get("content", [])
            if isinstance(content, str):
                text = content
            else:
                text = "\n".join(part.get("text", "") for part in content
                                 if part.get("type") == "text")
            match = next((p for p in self.pending
                          if index >= p.get("_after", 0) and text == p["_nativeText"]), None)
            if match:
                self.clients[row["uuid"]] = match["clientUserMessageId"]
                self.pending.remove(match)
                mapped.add(row["uuid"])
                self.retain_web_images(row, match)
            elif "web_images" not in row:
                self.retain_web_images(row)

    def stream_preview(self, chunk):
        self.screen.feed(chunk)
        if self.job.get("tempo") != "active" or time.monotonic() - self.preview_at < 0.12:
            return
        users = [index for index, row in enumerate(self.messages) if real_user(row)]
        if not users:
            return
        start = users[-1]
        user = self.messages[start]
        if any(row.get("subtype") == "turn_duration" for row in self.messages[start + 1:]):
            return
        if self.preview_user != user["uuid"]:
            self.preview_user = user["uuid"]
            self.preview = self.preview_emitted = ""
        content = user["message"]["content"]
        prompt = content if isinstance(content, str) else "\n".join(
            part.get("text", "") for part in content if part.get("type") == "text")
        lines = self.screen.text()
        anchor = next((index for index in range(len(lines) - 1, -1, -1)
                       if lines[index].startswith("❯ ") and prompt[:20] in lines[index]), -1)
        end = next((index for index in range(anchor + 1, len(lines))
                    if lines[index].startswith("❯ ")), len(lines))
        region = lines[anchor + 1:end]
        marker = next((index for index in range(len(region) - 1, -1, -1)
                       if region[index].startswith("● ")), -1)
        if marker >= 0:
            region = [region[marker][2:]] + [line[2:] if line.startswith("  ") else line
                                          for line in region[marker + 1:]]
        elif anchor >= 0:
            return
        else:
            if not self.preview and (not region or not region[0].startswith("  ")):
                return
            region = [line[2:] if line.startswith("  ") else line for line in region]
        body = []
        for line in region:
            if re.match(r"^[✻✽✶✢·*]\s|^[─╌]{8}|^\s*⎿|^\s*ctrl\+", line):
                break
            body.append(line)
        text = "\n".join(body).strip()
        if not text or re.match(r"^[A-Z][A-Za-z0-9_]+\(", text):
            return
        known = [part.get("text", "") for row in self.messages
                 if row["type"] == "assistant"
                 for part in row.get("message", {}).get("content", [])
                 if part.get("type") == "text"]
        compact = lambda value: " ".join(value.split())
        compact_text = compact(text)
        if compact_text in compact(prompt) or any(compact_text in compact(value) for value in known):
            return
        if not self.preview or text.startswith(self.preview) or anchor >= 0:
            self.preview = text
        elif not self.preview.startswith(text):
            for size in range(min(len(self.preview), len(text)), 7, -1):
                if self.preview.endswith(text[:size]):
                    self.preview += text[size:]
                    break
        if self.preview and self.preview != self.preview_emitted:
            self.preview_emitted = self.preview
            self.preview_at = time.monotonic()
            emit({"type": "shared_preview", "native_user_id": user["uuid"], "text": self.preview})

    def poll(self, force=False):
        if not self.initialized or not self.native_id:
            return
        live = self.lookup()
        if live:
            self.bind(live)
        elif self.short:
            path = self.root / "jobs" / self.short / "state.json"
            if path.exists():
                self.job = {**self.job, **json.loads(path.read_text())}
        self.read_messages()
        job = self.job
        # 正常连接与只读发现共用完成状态判断，done 不能因旧 tempo 继续显示执行中。
        status = NativeDiscovery.status(job)
        self.requests = {}
        if status.get("tempo") == "blocked":
            uses = {}
            for row in self.messages:
                content = row.get("message", {}).get("content", [])
                if not isinstance(content, list):
                    continue
                for part in content:
                    if part.get("type") == "tool_use":
                        uses[part["id"]] = part
                    if part.get("type") == "tool_result":
                        uses.pop(part.get("tool_use_id"), None)
            for tool_id, tool in list(uses.items())[-1:]:
                request_id = f"native-{tool_id}"
                self.requests[request_id] = {"subtype": "can_use_tool", "tool_name": tool["name"],
                                             "input": tool.get("input", {}),
                                             "decision_reason": job.get("needs", "")}
        context = self.context_configuration()
        signature = (self.log_offset, json.dumps(status, sort_keys=True),
                     json.dumps(self.pending, sort_keys=True), json.dumps(context, sort_keys=True))
        if force or signature != self.signature:
            self.signature = signature
            emit({"type": "shared_snapshot", "native_id": self.native_id, "job_id": self.short,
                  "native_status": status, "messages": self.messages,
                  "clients": self.clients, "pending": self.pending, "requests": self.requests,
                  "context_configuration": context})

    def control(self, request):
        subtype = request.get("subtype")
        if subtype == "initialize":
            self.model = request.get("model", "")
            self.effort = request.get("effort", "")
            self.clients = request.get("clients", {})
            self.pending = request.get("pending", [])
            self.initialized = True
            return {**request["catalog"], "contextConfiguration": self.context_configuration()}
        if subtype == "shared_send":
            return self.send(request)
        if subtype == "shared_sync":
            self.poll(force=True)
        elif subtype == "shared_queue_now":
            return self.send_queue_now()
        elif subtype == "set_model":
            self.change_setting("model", request.get("model", ""))
        elif subtype == "apply_flag_settings":
            self.change_setting("effort", request.get("settings", {}).get("effortLevel") or "")
        elif subtype == "interrupt":
            self.terminal_input(b"\x1b")
        elif subtype == "shared_answer":
            self.answer({"response": request["response"]})
        else:
            raise RuntimeError("此原生 Claude 控制操作尚未适配")
        return {}


def main():
    arguments = argparse.ArgumentParser()
    arguments.add_argument("--cli", required=True)
    arguments.add_argument("--native-id", default="")
    arguments.add_argument("--job-id", default="")
    arguments.add_argument("--discover", action="store_true")
    options = arguments.parse_args()
    native = NativeDiscovery() if options.discover else NativeSession(options.cli, options.native_id, options.job_id)
    incoming = queue.Queue()
    native.incoming = incoming

    def read_input():
        for line in sys.stdin:
            incoming.put(line)
        incoming.put(None)

    threading.Thread(target=read_input, daemon=True).start()
    next_poll = 0
    while True:
        if time.monotonic() >= next_poll:
            try:
                native.poll()
            except (OSError, ValueError, RuntimeError):
                pass
            next_poll = time.monotonic() + 0.25
        try:
            line = incoming.get(timeout=0.25)
        except queue.Empty:
            continue
        if line is None:
            # 用户要求网页断线/服务退出不停止原版会话；这里只结束连接端。
            return
        if isinstance(line, dict):
            native.stream_preview(line["_stream"])
            continue
        frame = json.loads(line)
        if frame["type"] == "control_response":
            try:
                native.answer(frame)
            except (OSError, ValueError, RuntimeError) as failure:
                emit({"type": "shared_error", "message": str(failure)})
            continue
        request_id = frame["request_id"]
        try:
            value = native.control(frame["request"])
            response = {"subtype": "success", "request_id": request_id, "response": value}
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as failure:
            response = {"subtype": "error", "request_id": request_id, "error": str(failure),
                        "not_sent": isinstance(failure, NativeError)
                        or frame["request"].get("subtype") == "shared_send" and not native.delivery_started}
            if isinstance(failure, NativeError):
                response["code"] = failure.code
        emit({"type": "control_response", "response": response})


if __name__ == "__main__":
    main()
