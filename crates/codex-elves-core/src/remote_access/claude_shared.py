"""原版 Claude 后台会话的 WSL 连接端；不运行模型、不修改 CLI、设置或权限。"""
import argparse
import base64
import contextlib
from datetime import datetime
import fcntl
import hashlib
import html
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


# 原生 rewind_conversation 明确拒绝时返回的原因；这些答复都表示记录未被修改。
# 未列出的原因统一按“不支持”报告，不猜测是否已回退。
REWIND_REFUSALS = {
    "target_not_found": ("ESTALE", "原生会话里找不到要编辑的消息（可能已被压缩或回退），请刷新后重试"),
    "stale_target": ("ESTALE", "原生会话有网页尚未显示的新内容，请刷新后重新编辑"),
    "unseen_later_turn": ("ESTALE", "原生会话有网页尚未显示的新内容，请刷新后重新编辑"),
    "state_changed": ("ESTALE", "原生会话状态已变化，请刷新后重新编辑"),
    "turn_running": ("EBUSY", "Claude 仍在执行，请等待结束后再编辑"),
    "commands_queued": ("EBUSY", "Claude 仍有排队中的输入，请等待处理后再编辑"),
    "prompt_pending": ("EBUSY", "Claude 仍有待处理的输入，请等待处理后再编辑"),
    "usage_limited": ("EBUSY", "当前用量受限，原版暂不允许回退"),
    "poll_tool_result_target": ("EUNSUPPORTED", "该消息位于工具调用过程中，原版不支持从这里编辑"),
    "target_splits_tool_call": ("EUNSUPPORTED", "该消息位于工具调用过程中，原版不支持从这里编辑"),
    "delivered_poll_events_in_range": ("EUNSUPPORTED", "该消息之后有已送达的后台事件，原版不支持从这里编辑"),
    "persist_failed": ("ESESSION", "原版未能保存回退记录，未修改历史"),
}


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
                if kind in ("(", ")", "*", "+", "-", ".", "/"):
                    # 原生 attach 会发送 ESC(B 等字符集切换；跨包保留完整序列，不能把 B 当成输入内容。
                    if index + 2 >= len(text):
                        self.carry = text[start:]
                        break
                    index += 3
                    continue
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
                            # 清屏必须同时处理光标所在行，否则短文本重绘会把上一帧的尾巴带进正文。
                            # ED 3 仅清滚动历史；此缓冲只保存可见画面，不能因此清掉当前回复。
                            if mode == 2:
                                self.lines.clear()
                            elif mode == 0:
                                self.lines = {row: line for row, line in self.lines.items() if row <= self.y}
                                self.lines[self.y] = self.lines.get(self.y, [])[:self.x]
                            elif mode == 1:
                                self.lines = {row: line for row, line in self.lines.items() if row >= self.y}
                                line = self.lines.get(self.y, [])
                                self.lines[self.y] = [" "] * min(self.x + 1, len(line)) + line[self.x + 1:]
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


def native_setting_source(row):
    origin = row.get("origin")
    return (row.get("type") == "user" and row.get("entrypoint") == "cli"
            and (origin is None or isinstance(origin, dict) and origin.get("kind") != "human")
            and row.get("promptSource") != "typed" and row.get("turnOrigin") != "human")


def native_setting_command(row):
    # 用户要求模型/等级命令使用变更提示行；仅识别 CLI 控制记录，保留用户输入或引用的同样标签。
    if not native_setting_source(row) or not isinstance(row.get("message"), dict):
        return None
    content = row.get("message", {}).get("content")
    if not isinstance(content, str):
        return None
    match = re.fullmatch(
        r"\s*<command-name>/(model|effort)</command-name>\s*"
        r"<command-message>\1</command-message>\s*"
        r"<command-args>([^<>\r\n]*)</command-args>\s*", content)
    return {"name": match[1], "value": match[2].strip()} if match else None


def native_skill_command(row):
    # 原生 Skill 调用用 command-message/name/args 包装；这里只识别候选，
    # 必须再关联网页原输入或同 promptId 的原生技能展开记录，不能按 XML 文本改写用户引用。
    if row.get("type") != "user" or row.get("isMeta") is True \
            or row.get("entrypoint") not in ("cli", "sdk-cli") or not row.get("promptId"):
        return None
    content = row.get("message", {}).get("content")
    if not isinstance(content, str):
        return None
    match = re.fullmatch(
        r"\s*<command-message>([\w.:-]+)</command-message>\s*"
        r"<command-name>/([\w.:-]+)</command-name>"
        r"(?:\s*<command-args>([\s\S]*)</command-args>)?\s*", content)
    if not match or match[2] in ("model", "effort", "compact", "goal"):
        return None
    arguments = (match[3] or "").strip()
    return {"name": match[2], "text": "/" + match[2] + (" " + arguments if arguments else "")}


def skill_input_key(text):
    # 原生会规整命令与参数之间的空白；只用于 Skill 回执关联，参数内部的空白和其他正文保持原样。
    parts = text.strip().split(maxsplit=1) if isinstance(text, str) else []
    return parts[0] + (" " + parts[1].strip() if len(parts) > 1 else "") if parts else ""


def after_delivery_intent(row, index, intent):
    # 新意图用追加偏移跨越原生回退分支；旧存档继续兼容原有顺序索引。
    return row.get("nativeOffset", -1) >= intent["_afterOffset"] if "_afterOffset" in intent \
        else index >= intent.get("_after", 0)


def normalize_skill_records(messages, pending):
    candidates = [(index, row) for index, row in enumerate(messages)
                  if row.get("nativeSkillCommand") and not row["nativeSkillCommand"].get("accepted")]
    if not candidates:
        return False
    companions = {(row.get("parentUuid"), row.get("promptId")) for row in messages
                  if row.get("type") == "user" and row.get("isMeta") is True
                  and row.get("turnCompanion") is True and row.get("promptId")}
    changed = False
    for index, row in candidates:
        command = row.get("nativeSkillCommand")
        raw = row.get("message", {}).get("content")
        intents = [intent for intent in pending if after_delivery_intent(row, index, intent)]
        if any(intent.get("_nativeText") == raw for intent in intents):
            # 网页明确发送的是 XML 引用时保持原文，即使另有内部伴随记录也不能误当命令。
            row.pop("nativeSkillCommand", None)
            continue
        intent = next((intent for intent in intents if skill_input_key(intent.get("_nativeText")) == command["text"]), None)
        if intent is None and (row.get("uuid"), row.get("promptId")) not in companions:
            continue
        row["message"] = {**row["message"], "content": command["text"]}
        row["nativeSkillCommand"] = {"name": command["name"], "accepted": True}
        changed = True
    return changed


def native_setting_dialog(text):
    """仅识别原版完整的编号确认框；无编号、未知 Hook 面板等保留本机确认。"""
    lines = [line.strip().strip("│").strip() for line in (text.splitlines() if isinstance(text, str) else text)]
    for index in range(len(lines) - 1, -1, -1):
        if lines[index] not in ("Switch model?", "Change effort level?"):
            continue
        body = lines[index:]
        yes = next((i for i, line in enumerate(body)
                    if re.fullmatch(r"(?:❯\s*)?1\.\s+Yes, switch to .+", line)), None)
        if yes is None or yes + 1 >= len(body) or not re.fullmatch(
                r"(?:❯\s*)?2\.\s+No, go back", body[yes + 1]):
            return None
        description = "\n".join(line for line in body[1:yes] if line)
        if not any(hint in description for hint in (
                "Your next response will be slower and use more tokens",
                "A PreModelSwitch hook asked you to confirm")):
            return None
        target = body[yes].split("Yes, switch to ", 1)[1].strip()
        return {"kind": "model" if lines[index] == "Switch model?" else "effort",
                "target": target, "description": description}
    return None


PLAN_FEEDBACK_OPTION = "Tell Claude what to change"


def native_plan_dialog(text, root=None):
    """原版计划模式的 “Would you like to proceed?” 审批框（2.1.323 实测）。

    用户要求网页能处理计划审批，不能再卡在“等待本机确认”：以 Yes 开头的批准项按编号即提交；
    “Tell Claude what to change” 按编号后输入修改意见再回车。其余未知选项只读展示，不猜按键。
    对话框出现时 ExitPlanMode 的 tool_use 可能尚未写入记录，所以只能依据画面识别。
    """
    lines = [line.strip().strip("│").strip() for line in (text.splitlines() if isinstance(text, str) else text)]
    index = next((i for i in range(len(lines) - 1, -1, -1) if "Would you like to proceed?" in lines[i]
                  and "written up a plan" in " ".join(lines[max(0, i - 1):i + 1])), None)
    if index is None:
        return None
    options, footer = [], ""
    for line in lines[index + 1:]:
        match = re.fullmatch(r"(?:❯\s*)?(\d)\.\s+(.+)", line)
        if match:
            label = match[2].strip()
            action = "approve" if label.startswith("Yes") else "feedback" if label == PLAN_FEEDBACK_OPTION else "unknown"
            options.append({"index": int(match[1]), "label": label, "action": action})
        elif "ctrl+g" in line or re.match(r"[─╌▔]{3}", line):
            footer = line
            break
    if ([option["index"] for option in options] != list(range(1, len(options) + 1))
            or not any(option["action"] == "approve" for option in options)):
        return None
    plan, path = "", ""
    match = re.search(r"·\s*(~?/\S+/plans/[^/\s]+\.md)\s*$", footer)
    if match and root:
        try:
            # 计划正文以原生计划文件为准（画面可能只显示一部分）；只读取当前配置目录 plans/ 下的文件。
            resolved = Path(os.path.expanduser(match[1])).resolve()
            if (resolved.is_relative_to((Path(root) / "plans").resolve()) and resolved.is_file()
                    and resolved.stat().st_size <= 64 * 1024):
                plan, path = resolved.read_text(encoding="utf-8", errors="replace").strip(), str(resolved)
        except OSError:
            plan, path = "", ""
    if not plan:
        start = next((i for i, line in enumerate(lines[:index]) if line == "Here is Claude's plan:"), None)
        if start is not None:
            plan = "\n".join(line for line in lines[start + 1:index]
                             if line and not re.fullmatch(r"[╌─▔]+", line)).strip()
    return {"kind": "plan", "target": "", "description": plan, "planPath": path, "options": options}


REWIND_CANCEL_OPTION = "Never mind"


def native_rewind_dialog(text):
    """原版终端的回退菜单（/rewind，2.1.323 实测）。

    用户要求网页能看出终端打开了回退菜单，并可从网页关闭（只取消，见 answer_rewind）。菜单打开期间原生状态为
    waiting / dialog open，网页消息只进入原生队列，菜单关闭后才执行；终端确认回退时，排队消息接在回退点之后执行。
    菜单分两页：选消息页标题下为 “Restore the code and/or conversation to the point before…”，底栏
    “Enter to continue · Esc to cancel”；确认页为 “Confirm you want to restore to the point before you sent
    this message:”，以 “│” 引用所选消息，下列编号选项（含 “Never mind”）。两页都必须完整匹配才识别。
    """
    raw = text.splitlines() if isinstance(text, str) else list(text)
    lines = [line.strip().strip("│").strip() for line in raw]
    index = next((i for i in range(len(lines) - 1, -1, -1) if lines[i] == "Rewind"), None)
    # 原生菜单的标题、说明与列表之间隔有空行（实测），按非空行依次匹配。
    rest = [line for line in lines[index + 1:] if line] if index is not None else []
    if not rest:
        return None
    heading, body = rest[0], rest[1:]
    if heading.startswith("Restore the code and/or conversation"):
        if not any("Esc to cancel" in line for line in body):
            return None
        return {"kind": "rewind", "target": "", "description": "", "step": "select", "options": []}
    if not heading.startswith("Confirm you want to restore"):
        return None
    quoted = [line.strip()[1:].strip() for line in raw[index + 1:] if line.strip().startswith("│")]
    message = "\n".join(line for line in quoted
                        if line and not re.fullmatch(r"\((?:[^()]*\bago|just now)\)", line))
    options = []
    for line in body:
        match = re.fullmatch(r"(?:❯\s*)?(\d)\.\s+(.+)", line)
        if match:
            options.append({"index": int(match[1]), "label": match[2].strip()})
    if ([option["index"] for option in options] != list(range(1, len(options) + 1))
            or not any(option["label"] == REWIND_CANCEL_OPTION for option in options)):
        return None
    return {"kind": "rewind", "target": "", "description": message[:2000], "step": "confirm", "options": options}


def native_dialog(job, text, native_id, root=None):
    if job.get("sessionStatus") != "waiting":
        return None
    # 计划审批框的原生 needs 是 “permission prompt”，与工具审批相同，只能按画面区分。
    plan = native_plan_dialog(text, root)
    if plan is None and job.get("needs") != "dialog open":
        return None
    parsed = plan or native_setting_dialog(text) or native_rewind_dialog(text)
    # 用户要求未知弹窗也可见，但不得猜按键；没有原生等待代次时只提供本机处理提示。
    stamp = job.get("sessionStatusAtMs")
    value = parsed or {"kind": "unknown", "target": "", "description": ""}
    # attach 可能改变终端列宽；仅排版换行不构成新审批，实际说明或原生等待代次变化仍更换身份。
    identity = [native_id, job.get("short"), stamp,
                {key: " ".join(raw.split()) if isinstance(raw, str) else raw for key, raw in value.items()}]
    return {**value, "id": "native-dialog-" + hashlib.sha256(
        json.dumps(identity, sort_keys=True).encode()).hexdigest()[:32],
        "statusAtMs": stamp, "remoteSupported": bool(parsed and stamp),
        "state": "waiting"}


def native_questions(block):
    raw = block.get("questions") if isinstance(block, dict) else None
    if not isinstance(raw, list) or not raw:
        return None
    questions, names = [], set()
    for row in raw:
        if not isinstance(row, dict) or not isinstance(row.get("question"), str) or not row["question"].strip():
            return None
        if row["question"] in names or not isinstance(row.get("options"), list):
            return None
        names.add(row["question"])
        options = [{"label": option["label"], "description": option.get("description")
                    if isinstance(option.get("description"), str) else ""}
                   for option in row["options"] if isinstance(option, dict) and isinstance(option.get("label"), str)]
        questions.append({"question": row["question"], "options": options,
                          **{key: row[key] for key in ("header", "multiSelect") if key in row}})
    return questions


def native_question_request(job, native_id):
    # 原生问答在回答前可能只存在于实时 block，不能等待 JSONL 出现 AskUserQuestion 才展示。
    if job.get("sessionStatus") != "waiting" or job.get("needs") != "input needed":
        return None
    questions = native_questions(job.get("block"))
    if not questions:
        # 原生没有提供完整问题结构时也必须显示等待原因，不能退回“仍在执行”或猜测审批按键。
        identity = [native_id, job.get("short"), job.get("sessionStatusAtMs"), "input needed"]
        return {"subtype": "native_dialog", "dialog": {
            "id": "native-question-" + hashlib.sha256(json.dumps(identity).encode()).hexdigest()[:32],
            "kind": "unknown", "target": "", "state": "waiting", "remoteSupported": False,
            "description": "Claude 正在等待回答，但原生未提供完整问题数据，请在同一 CLI 会话中处理。"}}
    stamp = job.get("sessionStatusAtMs")
    if not stamp:
        return None
    identity = [native_id, job.get("short"), stamp, questions]
    request_id = "native-question-" + hashlib.sha256(
        json.dumps(identity, ensure_ascii=False, sort_keys=True).encode()).hexdigest()[:32]
    return {"subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": {"questions": questions},
            "nativeQuestion": {"id": request_id, "statusAtMs": stamp, "state": "waiting"}}


def question_screen_lines(lines):
    # 只解析问答左栏；原版右侧 diff 面板可能占同一终端行，连续空白分栏不能混入答案。
    # 多选的焦点行是“❯    Submit”；先折叠光标后的缩进，不能把这个原生提交按钮当右栏截掉。
    return [re.split(r"\s{3,}", re.sub(r"^❯\s+", "❯ ", line.strip().strip("│").strip()), maxsplit=1)[0]
            for line in lines]


def question_text_key(text):
    return re.sub(r"[\s`*_│]", "", text)


def native_question_page(lines, questions):
    lines = question_screen_lines(lines)
    starts = [index for index, line in enumerate(lines) if "✔ Submit" in line and ("←" in line or "→" in line)]
    # 单问单选的原版面板没有 Submit 标签，选择后直接提交；仍须从当前问题和编号选项核对界面。
    body = lines[starts[-1] + 1:] if starts else lines if len(questions) == 1 else []
    if "Review your answers" in body and "Ready to submit your answers?" in body:
        return {"index": len(questions), "review": body}
    option_start = next((i for i in range(len(body) - 1, -1, -1)
                         if re.match(r"^(?:❯\s*)?1\.\s", body[i])), None)
    if option_start is None:
        return None
    heading = question_text_key("".join(body[:option_start]))
    matches = [i for i, question in enumerate(questions) if question_text_key(question["question"]) in heading]
    if len(matches) != 1:
        return None
    choices, multi = {}, False
    for line in body[option_start:]:
        match = re.match(r"^(?:❯\s*)?([1-9])\.\s+(.*)$", line)
        if match:
            checkbox = re.match(r"^\[[^\]]\]\s+(.*)$", match[2])
            multi = multi or checkbox is not None
            choices[checkbox[1] if checkbox else match[2]] = match[1]
    return {"index": matches[0], "choices": choices, "lines": body,
            "multi": multi}


def native_question_review_matches(lines, questions, answers):
    entries, question, answer = [], [], None
    for line in lines[lines.index("Review your answers") + 1:]:
        if line == "Ready to submit your answers?":
            break
        if line.startswith("● "):
            if answer is not None:
                entries.append(("".join(question), "".join(answer)))
            question, answer = [line[2:]], None
        elif line.startswith("→ "):
            answer = [line[2:]]
        elif answer is not None:
            answer.append(line)
        elif question:
            question.append(line)
    if answer is not None:
        entries.append(("".join(question), "".join(answer)))
    return len(entries) == len(questions) and all(
        question_text_key(label) == question_text_key(row["question"])
        and re.sub(r"\s", "", value) == re.sub(r"\s", "", answers[row["question"]])
        for (label, value), row in zip(entries, questions))


def native_control_command(row):
    # 只识别原生 CLI 控制记录；用户引用相同 XML 不能被解释为目标或压缩操作。
    if not native_setting_source(row):
        return None
    content = row.get("message", {}).get("content")
    if not isinstance(content, str):
        return None
    match = re.fullmatch(r"\s*<command-name>/(goal|compact)</command-name>\s*"
                        r"<command-message>\1</command-message>\s*"
                        r"<command-args>([\s\S]*)</command-args>\s*", content)
    return {"name": match[1], "value": match[2].strip()} if match else None


def native_agent_notification(row):
    # 子代理完成通知是原生控制消息；只认原生来源和完整结构，用户引用相同 XML 仍按普通正文显示。
    origin = row.get("origin") or {}
    if (row.get("type") != "user" or not isinstance(origin, dict) or origin.get("kind") != "task-notification"
            or origin.get("producer") != "session-task"):
        return None
    content = row.get("message", {}).get("content")
    if (isinstance(content, list) and len(content) == 1 and isinstance(content[0], dict)
            and content[0].get("type") == "text"):
        content = content[0].get("text")
    if not isinstance(content, str) or not re.fullmatch(r"\s*<task-notification>[\s\S]*</task-notification>\s*", content):
        return None
    def field(name):
        match = re.search(rf"<{name}>([^<]*)</{name}>", content)
        return html.unescape(match[1]).strip() if match else ""
    identity, status = field("task-id"), field("status")
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", identity) or status not in ("completed", "failed", "stopped"):
        return None
    value = {"agentId": identity, "status": status, "toolUseId": field("tool-use-id"),
             "completedAtMs": native_time(row.get("timestamp"))}
    for tag, key in (("subagent_tokens", "totalTokens"), ("tool_uses", "totalToolUseCount"),
                     ("duration_ms", "totalDurationMs")):
        text = field(tag)
        if text.isdigit():
            value[key] = int(text)
    return value


def native_task_notification(row):
    # 用户要求后台命令、监控事件、后台子代理触发的自动续跑显示为“自动继续”，不当成用户消息、不可编辑。
    # 只认原生来源（origin.kind=task-notification）且正文完整为 <task-notification> 的记录；用户引用相同 XML 仍是正文。
    # 监控事件没有 status，不能套用子代理通知的状态校验（2.1.323 实测）。
    origin = row.get("origin") or {}
    message = row.get("message")
    if row.get("type") != "user" or not isinstance(origin, dict) or origin.get("kind") != "task-notification" \
            or not isinstance(message, dict):
        return None
    content = message.get("content")
    if (isinstance(content, list) and len(content) == 1 and isinstance(content[0], dict)
            and content[0].get("type") == "text"):
        content = content[0].get("text")
    if not isinstance(content, str) or not re.fullmatch(r"\s*<task-notification>[\s\S]*</task-notification>\s*", content):
        return None

    def field(name, limit=400):
        match = re.search(rf"<{name}>([\s\S]*?)</{name}>", content)
        return html.unescape(match[1]).strip()[:limit] if match else ""

    identity = field("task-id")
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", identity):
        return None
    value = {"taskId": identity, "summary": field("summary")}
    status, tool, event = field("status"), field("tool-use-id"), field("event", 2000)
    if re.fullmatch(r"[a-z_]{1,32}", status):
        value["status"] = status
    if re.fullmatch(r"[A-Za-z0-9_-]{1,128}", tool):
        value["toolUseId"] = tool
    if event:
        value["event"] = event
    return value


_LOCAL_COMMAND = re.compile(r"\s*<command-name>/([\w.:-]+)</command-name>\s*<command-message>[^<]*</command-message>\s*"
                            r"(?:<command-args>([\s\S]*)</command-args>)?\s*")
_LOCAL_OUTPUT = re.compile(r"\s*<local-command-(stdout|stderr)>([\s\S]*)</local-command-\1>\s*")
_ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")


def native_local_command(row, parent=None):
    # 用户要求终端执行的本地斜杠命令（如 /resume、/skills）显示为“终端执行”事件，不显示成可编辑的用户消息。
    # 旧格式：CLI 写入的 user 行，父节点是同一 promptId 的 <local-command-caveat> 元数据（2.1.323 实测）；
    # 新格式：system/local_command。模型/等级、目标、压缩命令另有专门展示，这里排除。
    if row.get("type") == "system":
        content = row.get("content") if row.get("subtype") == "local_command" else None
    elif native_setting_source(row) and isinstance(row.get("message"), dict):
        caveat = parent.get("message", {}).get("content") if isinstance(parent, dict) else None
        if not (isinstance(caveat, str) and caveat.lstrip().startswith("<local-command-caveat>")
                and parent.get("isMeta") is True and parent.get("promptId") == row.get("promptId") is not None):
            return None
        content = row["message"].get("content")
    else:
        return None
    match = _LOCAL_COMMAND.fullmatch(content) if isinstance(content, str) else None
    if not match or match[1] in ("model", "effort", "goal", "compact"):
        return None
    return {"name": "/" + match[1], "args": (match[2] or "").strip()[:400]}


def native_local_output(row, parent=None):
    # 本地命令输出只归属到已识别的命令记录；去掉终端控制序列，保留原文供网页信息提示查看。
    if not isinstance(parent, dict) or not parent.get("nativeLocalCommand"):
        return None
    content = row.get("content") if row.get("type") == "system" else row.get("message", {}).get("content") \
        if isinstance(row.get("message"), dict) else None
    match = _LOCAL_OUTPUT.fullmatch(content) if isinstance(content, str) else None
    if not match:
        return None
    return {"commandId": parent["uuid"], "stream": match[1], "text": _ANSI.sub("", html.unescape(match[2])).strip()[:4000]}


def native_system_notice(row):
    # 用户要求不再静默丢弃原生系统记录：离开期间回顾、原生提示、阻止继续或出错的 Stop Hook 以事件行展示；
    # 无输出、未阻止的 Stop Hook 汇总只是例行记录，不展示。
    if row.get("type") != "system":
        return None
    subtype, content = row.get("subtype"), row.get("content")
    if subtype == "away_summary" and isinstance(content, str) and content.strip():
        return {"kind": "awaySummary", "text": re.sub(r"\s*\(disable recaps in /config\)\s*$", "", content.strip())[:2000]}
    if subtype == "informational" and isinstance(content, str) and content.strip():
        return {"kind": "informational", "text": content.strip()[:2000]}
    if subtype == "stop_hook_summary":
        errors = [error if isinstance(error, str) else json.dumps(error, ensure_ascii=False)
                  for error in row.get("hookErrors") or [] if error][:5] if isinstance(row.get("hookErrors"), list) else []
        reason = row.get("stopReason") if isinstance(row.get("stopReason"), str) else ""
        if errors or row.get("preventedContinuation") is True:
            return {"kind": "stopHook", "prevented": row.get("preventedContinuation") is True,
                    "reason": reason.strip()[:1000], "errors": [error[:400] for error in errors]}
    return None


def native_turn_start(row):
    # 原生自动续跑以任务通知开启新的一轮；判断轮次是否结束时与真实用户输入同等对待。
    return real_user(row) or bool(row.get("nativeTaskNotification") or native_task_notification(row))


# 与网页 app.js 的 editedFileDiffLimit（1 MiB）保持一致；超过时只转发“覆盖修改”，网页显示未提供差异。
NATIVE_PATCH_LIMIT = 1024 * 1024


def native_tool_result(row):
    # 用户要求按 Claude 原生语义渲染工具：只转发展示需要的结构化补丁、后台任务 ID 与任务清单 ID，
    # 不复制 originalFile、stdout 等大字段。由 Rust claude_tools.rs 消费，字段改名需同步核对。
    result = row.get("toolUseResult")
    message = row.get("message")
    content = message.get("content") if isinstance(message, dict) else None
    if row.get("type") != "user" or not isinstance(result, dict) or not isinstance(content, list):
        return None
    calls = [part.get("tool_use_id") for part in content
             if isinstance(part, dict) and part.get("type") == "tool_result"]
    # 原生每个工具结果单独成行；一行多个结果时元数据归属不明，不转发。
    if len(calls) != 1 or not isinstance(calls[0], str):
        return None
    value = {}
    if isinstance(result.get("filePath"), str) and isinstance(result.get("structuredPatch"), list):
        value["filePath"] = result["filePath"]
        value["fileChange"] = "create" if result.get("type") == "create" else "update"
        hunks, size = [], 0
        for hunk in result["structuredPatch"]:
            numbers = [hunk.get(key) if isinstance(hunk, dict) else None
                       for key in ("oldStart", "oldLines", "newStart", "newLines")]
            lines = hunk.get("lines") if isinstance(hunk, dict) else None
            if (not all(type(number) is int and number >= 0 for number in numbers)
                    or not isinstance(lines, list) or not all(isinstance(line, str) for line in lines)):
                hunks = None
                break
            size += sum(len(line) + 1 for line in lines)
            hunks.append({**dict(zip(("oldStart", "oldLines", "newStart", "newLines"), numbers)), "lines": lines})
        if hunks is not None and size <= NATIVE_PATCH_LIMIT:
            value["structuredPatch"] = hunks
    task = result.get("backgroundTaskId")
    if isinstance(task, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", task):
        value["backgroundTaskId"] = task
    if result.get("interrupted") is True:
        value["interrupted"] = True
    created = result.get("task")
    if isinstance(created, dict) and type(created.get("id")) in (str, int):
        value["createdTaskId"] = str(created["id"])
    if result.get("success") is False:
        value["success"] = False
    return {"toolUseId": calls[0], **value} if value else None


def real_user(row):
    if any(row.get(key) is True for key in ("isMeta", "isSidechain", "isCompactSummary", "isVisibleInTranscriptOnly")):
        return False
    command = row.get("nativeControlCommand") or native_control_command(row)
    if command:
        return command["name"] == "goal" and command["value"].lower() not in ("", "clear", "stop", "off", "reset", "none", "cancel")
    if native_interruption(row) or native_agent_notification(row) or row.get("nativeAgentNotification") \
            or row.get("nativeTaskNotification") or native_task_notification(row) \
            or row.get("nativeLocalCommand") or row.get("nativeLocalOutput") \
            or native_setting_command(row) or row.get("nativeControlOutput") \
            or row.get("nativeSettingCommand") or row.get("nativeSettingOutput"):
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


class NativeHistory:
    """保留原生父子链；回退只改变展示分支，不改写 JSONL 或删除旧分支。"""
    def __init__(self):
        self.parents, self.head = {}, None
        self.cut_before = None

    def observe(self, row):
        if row.get("type") == "last-prompt":
            # 用户要求网页编辑采用结构化回退（B 方案）：原生 rewind_conversation 成功后只追加
            # {"type":"last-prompt","explicit":true,"rewound":true,"cutUuid":目标}，不写新的消息节点。
            # 按该标记把展示分支切到目标之前，与重启后的原生 CLI 一致；终端 /rewind 不写此标记（2.1.323 实测）。
            cut = row.get("cutUuid")
            if row.get("explicit") is True and row.get("rewound") is True and cut in self.parents:
                self.cut_before = cut
            return
        identity = row.get("uuid")
        if not identity or row.get("isSidechain") or identity in self.parents:
            return
        parent = row.get("parentUuid", self.head)
        if parent and parent not in self.parents:
            # 旧存档/不完整控制记录的父节点可能缺失；没有完整回退证据时保留原有顺序，不能误删已确认消息。
            parent = self.head
        if row.get("subtype") == "compact_boundary":
            # 压缩边界是上下文变换，历史仍连续；不能把压缩误判为用户回退。
            logical = row.get("logicalParentUuid")
            parent = logical if logical in self.parents else self.head
        self.parents[identity] = parent
        if row.get("type") in ("user", "assistant", "system", "attachment"):
            self.head = identity
            if self.cut_before and real_user(row) and identity != self.cut_before:
                self.cut_before = None

    def select(self, rows):
        head = self.parents.get(self.cut_before) if self.cut_before else self.head
        if not self.parents:
            return rows
        active = set()
        while head and head not in active:
            active.add(head)
            head = self.parents.get(head)
        # 并行工具调用（2.1.323 实测）：每条结果的父节点是各自的 tool_use 行，只有最后一条在当前链上。
        # 用户要求工具状态如实显示：答复的调用都在当前分支、且父节点也在当前分支的结果行一并保留，
        # 否则其余工具会一直显示进行中或状态未知、任务清单也会丢失编号；被回退分支上的调用不在当前链上，不受影响。
        uses = {part.get("id") for row in rows if row.get("uuid") in active and row.get("type") == "assistant"
                for part in (row.get("message", {}).get("content") or []) if isinstance(part, dict)
                and part.get("type") == "tool_use"}

        def parallel_result(row):
            content = row.get("message", {}).get("content") if row.get("type") == "user" else None
            if not isinstance(content, list) or not content or self.parents.get(row.get("uuid")) not in active:
                return False
            return all(isinstance(part, dict) and part.get("type") == "tool_result"
                       and part.get("tool_use_id") in uses for part in content)

        return [row for row in rows if row.get("uuid") in active or parallel_result(row)]

    def current_head(self):
        return self.parents.get(self.cut_before) if self.cut_before else self.head


def native_turn_finished(messages):
    # 用户要求只有执行中才排队；原生完成、中断后仍可能用 blocked/needs 保存等待用户答复的说明。
    # 只认当前真实用户轮次的结束记录，不按回复文本猜测；后续新输入或工具调用使旧结束记录失效。
    for row in reversed(messages):
        if native_interruption(row) or row.get("isApiErrorMessage") is True:
            return True
        if native_turn_start(row):
            return False
        message = row.get("message")
        content = message.get("content", []) if isinstance(message, dict) else []
        if row.get("type") == "assistant" and isinstance(content, list) \
                and any(part.get("type") == "tool_use" for part in content if isinstance(part, dict)):
            return False
        if row.get("type") == "system" and row.get("subtype") == "turn_duration":
            return True
    return False


def preview_text_key(value):
    # 只用于排除终端预览重复，正式 Markdown 原文不改写；终端已去掉代码、强调、链接等显示标记。
    value = html.unescape(value)
    value = re.sub(r"!\[([^\]]*)\]\([^\n)]*\)|\[([^\]]*)\]\([^\n)]*\)",
                   lambda match: match.group(1) if match.group(1) is not None else match.group(2), value)
    value = re.sub(r"(?m)^\s{0,3}(?:#{1,6}\s+|>+\s*)", "", value)
    value = re.sub(r"[`*_~<>]", "", value)
    return "".join(value.split())


def preview_prompt_key(content):
    # 用户输入只折叠终端换行与缩进，不去掉 Markdown/标点，也不能用截取前缀或空文字认领回复。
    if isinstance(content, str):
        text = content
    elif isinstance(content, list):
        text = "\n".join(part["text"] for part in content if isinstance(part, dict)
                         and part.get("type") == "text" and isinstance(part.get("text"), str))
    else:
        return ""
    return "".join(text.split())


_PROGRESS_ACTIONS = ("reading", "writing", "editing", "creating", "deleting", "searching", "running", "executing")
_PROGRESS_TARGETS = ("file", "files", "folder", "folders", "directory", "directories", "pattern", "patterns",
                     "search", "searches", "shell command", "shell commands", "command", "commands", "tool", "tools")
_PROGRESS_ACTION = "(?:" + "|".join(_PROGRESS_ACTIONS) + ")"
_PROGRESS_TARGET = "(?:" + "|".join(sorted(_PROGRESS_TARGETS, key=len, reverse=True)) + ")"
_PROGRESS_CLAUSE = rf"{_PROGRESS_ACTION}\s+\d+\s+{_PROGRESS_TARGET}"
_PROGRESS_SEPARATOR = r"(?:\s*,\s*|\s+and\s+)"
_PROGRESS_LINE = re.compile(
    rf"\s*{_PROGRESS_CLAUSE}(?:{_PROGRESS_SEPARATOR}{_PROGRESS_CLAUSE})*"
    r"(?:\s*[·•]\s*(?:\d+(?:\.\d+)?(?:ms|[hms])\s*)+)?(?:\.{3}|…)?\s*", re.IGNORECASE)
# 只在尚未结束的最后一行延迟不完整的进度前缀，避免 Reading… 分包时先污染正文；
# 一旦后续文字表明它是普通句子就正常展示，不能全局隐藏以这些单词开头的回复。
_PROGRESS_ACTION_PREFIX = "(?:" + "|".join(sorted(
    {word[:end] for word in _PROGRESS_ACTIONS for end in range(1, len(word) + 1)}, key=len, reverse=True)) + ")"
_PROGRESS_TARGET_PREFIX = "(?:" + "|".join(sorted(
    {word[:end] for word in _PROGRESS_TARGETS for end in range(1, len(word) + 1)}, key=len, reverse=True)) + ")"
_PROGRESS_PARTIAL_CLAUSE = rf"(?:{_PROGRESS_ACTION_PREFIX}|{_PROGRESS_ACTION}\s+\d*|{_PROGRESS_ACTION}\s+\d+\s+{_PROGRESS_TARGET_PREFIX}?)?"
_PROGRESS_PARTIAL = re.compile(
    rf"\s*(?:{_PROGRESS_CLAUSE}{_PROGRESS_SEPARATOR})*{_PROGRESS_PARTIAL_CLAUSE}\s*|"
    rf"\s*{_PROGRESS_CLAUSE}(?:{_PROGRESS_SEPARATOR}{_PROGRESS_CLAUSE})*"
    r"(?:\s+a(?:n(?:d\s*)?)?|\s*,\s*|\s*[·•]\s*[\d.hms\s…]*|\.{1,3}|…)\s*", re.IGNORECASE)


def terminal_progress_line(line, partial=False):
    # 工具进度可组合多个动作；只匹配完整进度语法，正式原生正文始终保持原文。
    # 识别前合并空白，避免终端长缩进在不完整语法分支中反复回溯；不改写实际展示文本。
    line = " ".join(line.split())
    return _PROGRESS_LINE.fullmatch(line) is not None or (
        partial and bool(line) and _PROGRESS_PARTIAL.fullmatch(line) is not None)


def terminal_reply_region(lines, prompt, occurrences):
    if not prompt:
        # 纯图片或被终端折叠的输入没有可核对文字时，等待原生历史；不能借用上一条输入框。
        return []
    anchors = [index for index, line in enumerate(lines) if line.startswith("❯ ")]
    matches = []
    for position, anchor in enumerate(anchors):
        end = anchors[position + 1] if position + 1 < len(anchors) else len(lines)
        prompt_end = next((index for index in range(anchor + 1, end)
                           if re.match(r"^● |^[✻✽✶✢·*]\s|^[─╌]{8}|^\s*ctrl\+", lines[index])), end)
        visible = "\n".join([lines[anchor][2:]] + lines[anchor + 1:prompt_end])
        if preview_prompt_key(visible) == prompt:
            matches.append((prompt_end, end))
    # 重复输入必须在当前画面中出现到同一原生序次，不能把旧画面回复绑定到新 UUID。
    # 终端尾部被截断、序次无法确认时仅停用临时预览，不影响发送、工具或正式历史。
    if len(matches) < occurrences:
        return []
    start, end = matches[-1]
    marker = next((index for index in range(end - 1, start - 1, -1)
                   if lines[index].startswith("● ")), None)
    if marker is None:
        return []
    return [lines[marker][2:]] + [line[2:] if line.startswith("  ") else line for line in lines[marker + 1:end]]


class NativeSubagents:
    """只在列表/详情读取时观察原生子代理文件，不进入主会话的发送、250ms 轮询或权限处理。"""
    def __init__(self, root, native_id):
        self.projects = (root / "projects").resolve()
        self.native_id = str(uuid.UUID(native_id))
        self.logs = {}

    @staticmethod
    def valid_id(value):
        return isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", value) is not None

    def path(self):
        path = next(self.projects.glob(f"*/{self.native_id}.jsonl"), None)
        if path is None or not path.resolve().is_relative_to(self.projects):
            raise RuntimeError("原生主会话记录暂不可读取")
        return path

    def rows(self, path, agent_id=None):
        if not path.exists():
            return []
        if not path.resolve().is_relative_to(self.projects):
            raise RuntimeError("原生子代理记录不在所属会话目录内")
        stat = path.stat()
        cached = self.logs.setdefault(str(path), {"offset": 0, "rows": [], "seen": set(),
                                                 "signature": None, "history": NativeHistory()})
        identity = (stat.st_dev, stat.st_ino)
        signature = (identity, stat.st_size, stat.st_mtime_ns)
        previous = cached["signature"]
        if previous and (previous[0] != identity or stat.st_size < cached["offset"]
                         or previous[1] == stat.st_size and previous[2] != stat.st_mtime_ns):
            cached.update(offset=0, rows=[], seen=set(), history=NativeHistory())
        if signature == previous:
            return cached["rows"] if agent_id else cached["history"].select(cached["rows"])
        with path.open("rb") as stream:
            stream.seek(cached["offset"])
            while True:
                position = stream.tell()
                line = stream.readline()
                if not line.endswith(b"\n"):
                    cached["offset"] = position
                    break
                cached["offset"] = stream.tell()
                try:
                    row = json.loads(line)
                except ValueError:
                    continue
                if isinstance(row, dict) and not agent_id:
                    cached["history"].observe(row)
                if (not isinstance(row, dict) or not row.get("uuid")
                        or row.get("type") not in ("user", "assistant", "system")
                        or row.get("sessionId", self.native_id) != self.native_id
                        or agent_id and row.get("agentId", agent_id) != agent_id
                        or not agent_id and row.get("isSidechain")
                        or row["uuid"] in cached["seen"]):
                    continue
                if row["type"] in ("user", "assistant") and not isinstance(row.get("message"), dict):
                    continue
                value = {key: row[key] for key in ("type", "uuid", "parentUuid", "timestamp", "subtype",
                         "agentId", "parentAgentId", "isMeta", "isApiErrorMessage", "isCompactSummary",
                         "isVisibleInTranscriptOnly", "apiBlockIndex", "requestedModel", "effort",
                         "interruptedMessageId") if key in row}
                value["timestampMs"] = native_time(row.get("timestamp"))
                if (agent_id and row["type"] == "user"
                        and isinstance(row.get("origin"), dict) and row["origin"].get("kind") == "coordinator"):
                    # 已写入子代理原生记录的主代理消息是该子视图的真实输入；只调整只读投影，不参与主会话准入。
                    value["isMeta"] = False
                    value["nativeSubagentInput"] = True
                if "message" in row:
                    message = row["message"]
                    value["message"] = {key: message[key] for key in
                                        ("role", "id", "model", "content", "usage", "stop_reason") if key in message}
                    content = value["message"].get("content")
                    if isinstance(content, list):
                        value["message"]["content"] = [{key: item for key, item in part.items()
                            if key not in ("signature", "data")} if part.get("type") == "thinking" else part
                            for part in content if isinstance(part, dict) and part.get("type") != "redacted_thinking"]
                result = row.get("toolUseResult")
                if isinstance(result, dict) and self.valid_id(result.get("agentId") or result.get("resumedAgentId")):
                    value["toolUseResult"] = {key: result[key] for key in
                        ("agentId", "resumedAgentId", "status", "description", "resolvedModel", "totalDurationMs",
                         "totalTokens", "totalToolUseCount") if key in result}
                tool_result = native_tool_result(row)
                if tool_result:
                    # 子代理只读视图与主会话共用工具渲染，同样只带展示所需的精简元数据。
                    value["nativeToolResult"] = tool_result
                notification = native_agent_notification(row)
                if notification:
                    value["nativeAgentNotification"] = notification
                cached["seen"].add(row["uuid"])
                cached["rows"].append(value)
        cached["signature"] = signature
        return cached["rows"] if agent_id else cached["history"].select(cached["rows"])

    def collect(self, requested=None, include_messages=False):
        if requested is not None and not self.valid_id(requested):
            raise RuntimeError("原生子代理 ID 无效")
        parent = self.path()
        directory = parent.with_suffix("") / "subagents"
        if directory.exists() and directory.resolve() != parent.parent.resolve() / self.native_id / "subagents":
            raise RuntimeError("原生子代理目录不属于当前会话")
        calls, agents, notifications, resumes = {}, {}, {}, {}
        for row in self.rows(parent):
            content = row.get("message", {}).get("content", [])
            if isinstance(content, list):
                for part in content:
                    if part.get("type") == "tool_use" and part.get("name") in ("Agent", "Task"):
                        calls[part["id"]] = {"input": part.get("input", {}), "at": row["timestampMs"]}
                    if part.get("type") == "tool_result" and part.get("tool_use_id") in calls:
                        result = row.get("toolUseResult", {})
                        if self.valid_id(result.get("agentId")):
                            agents[result["agentId"]] = {**result, "toolUseId": part["tool_use_id"]}
            notification = row.get("nativeAgentNotification")
            if notification:
                notifications[notification["agentId"]] = notification
            resumed_id = row.get("toolUseResult", {}).get("resumedAgentId")
            if self.valid_id(resumed_id):
                resumes[resumed_id] = row["timestampMs"]
        # 原生日志保留被回退的分支及其文件；仅排除明确属于失效调用的子代理，不删除磁盘记录。
        all_rows = self.logs[str(parent)]["rows"]
        discarded_calls = {
            part["id"] for row in all_rows if isinstance(row.get("message", {}).get("content"), list)
            for part in row["message"]["content"]
            if isinstance(part, dict) and part.get("type") == "tool_use"
            and part.get("name") in ("Agent", "Task") and part.get("id") not in calls
        }
        discarded_agents = {
            row.get("toolUseResult", {}).get("agentId")
            for row in all_rows if isinstance(row.get("message", {}).get("content"), list)
            and any(part.get("type") == "tool_result" and part.get("tool_use_id") in discarded_calls
                    for part in row["message"]["content"] if isinstance(part, dict))
        }
        identities = set(agents)
        for path in directory.glob("agent-*"):
            match = re.fullmatch(r"agent-([A-Za-z0-9_-]{1,64})\.(?:jsonl|meta\.json)", path.name)
            if match:
                identities.add(match[1])
        if requested is not None and requested not in identities:
            raise NativeError("ENOENT", "当前主会话没有该原生子代理记录")
        result = []
        for identity in sorted(identities):
            if requested is not None and requested != identity:
                continue
            metadata_path = directory / f"agent-{identity}.meta.json"
            metadata = {}
            if metadata_path.exists():
                if not metadata_path.resolve().is_relative_to(directory.resolve()):
                    raise RuntimeError("原生子代理元数据不属于当前会话")
                try:
                    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
                except ValueError:
                    pass
                if not isinstance(metadata, dict):
                    metadata = {}
            native = agents.get(identity, {})
            tool_id = metadata.get("toolUseId") or native.get("toolUseId", "")
            if tool_id in discarded_calls or identity in discarded_agents:
                discarded_agents.add(identity)
                continue
            call = calls.get(tool_id, {})
            task = call.get("input", {})
            path = directory / f"agent-{identity}.jsonl"
            if include_messages and not path.is_file():
                # 记录暂不可读时沿用现有读取失败处理，不能用空历史撤掉网页已经确认并显示的消息。
                raise RuntimeError("原生子代理记录暂不可读取")
            if path.exists() and not path.resolve().is_relative_to(directory.resolve()):
                raise RuntimeError("原生子代理记录不属于当前会话")
            rows = self.rows(path, identity)
            if not call and metadata.get("isFork"):
                # 嵌套 fork 或父会话压缩后，启动调用仍保存在该 fork 文件中；只按原生 toolUseId 取回任务。
                for row in rows:
                    content = row.get("message", {}).get("content", [])
                    if not isinstance(content, list):
                        continue
                    block = next((part for part in content if part.get("type") == "tool_use"
                                  and part.get("id") == tool_id and part.get("name") in ("Agent", "Task")), None)
                    if block:
                        call = {"input": block.get("input", {}), "at": row["timestampMs"]}
                        task = call["input"]
                        break
            # fork 文件开头是启动调用及继承说明，不属于子代理再次执行 Agent；用原始任务替换该原生启动说明。
            if metadata.get("isFork"):
                boundary = next((index for index, row in enumerate(rows)
                    if row.get("type") == "user" and isinstance(row.get("message", {}).get("content"), list)
                    and any(part.get("type") == "tool_result" and part.get("tool_use_id") == tool_id
                            for part in row["message"]["content"])), None)
                rows = rows[boundary + 1:] if boundary is not None else []
                if task.get("prompt"):
                    rows = [{"type": "user", "uuid": f"task-{identity}", "timestampMs": call.get("at", 0),
                             "message": {"role": "user", "content": task["prompt"]}}, *rows]
            notification = notifications.get(identity, {})
            model, effort, updated, started = native.get("resolvedModel", ""), "", 0, call.get("at", 0)
            latest_prompt, latest_final = 0, 0
            for row in rows:
                updated = max(updated, row["timestampMs"])
                started = started or row["timestampMs"]
                if real_user(row):
                    latest_prompt = max(latest_prompt, row["timestampMs"])
                message = row.get("message", {})
                if row.get("type") == "assistant" and message.get("model") not in (None, "", "<synthetic>"):
                    model = message["model"]
                    if message.get("stop_reason") in ("end_turn", "stop_sequence"):
                        latest_final = max(latest_final, row["timestampMs"])
                if row.get("effort"):
                    effort = row["effort"]
            # 同 ID 续跑会移除 finished 而非写 false；新的原生输入使上次完成通知失效，不能继续归入已完成列表。
            current_input = max(latest_prompt, resumes.get(identity, 0))
            resumed = bool(notification.get("completedAtMs") and current_input > notification["completedAtMs"])
            status = notification.get("status") or native.get("status")
            if metadata.get("finished") is False or resumed and latest_final < current_input:
                status = "active"
            elif metadata.get("finished") is True:
                status = "completed" if resumed or status not in ("failed", "stopped") else status
            elif resumed:
                status = "active"
            elif status not in ("completed", "failed", "stopped"):
                status = "active" if status in ("active", "async_launched", "running") else "unknown"
            if resumed and status == "active":
                started = current_input
            outcome = status if status in ("completed", "failed", "stopped") else None
            entry = {"nativeId": identity, "toolUseId": tool_id,
                     "name": task.get("description") or metadata.get("description") or identity,
                     "agentType": metadata.get("agentType") or task.get("subagent_type", ""),
                     "model": model, "reasoningEffort": effort, "status": status, "outcome": outcome,
                     "startedAtMs": started or None, "updatedAtMs": updated or None,
                     "parentAgentId": next((row["parentAgentId"] for row in rows if row.get("parentAgentId")), None)}
            for key in ("totalDurationMs", "totalTokens", "totalToolUseCount"):
                if not resumed and isinstance(notification.get(key, native.get(key)), (int, float)):
                    entry[key] = notification.get(key, native.get(key))
            if outcome:
                entry["completedAtMs"] = (None if resumed else notification.get("completedAtMs")) or updated or None
            if include_messages:
                entry["messages"] = rows
            result.append(entry)
        while True:
            removed = {row["nativeId"] for row in result if row.get("parentAgentId") in discarded_agents}
            if removed.issubset(discarded_agents):
                break
            discarded_agents.update(removed)
        return [row for row in result if row["nativeId"] not in discarded_agents]


def native_json(path):
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
        return value if isinstance(value, dict) else {}
    except (OSError, ValueError):
        return {}


def native_activity(root, job, native_id, *, stored=None, workers=None):
    """后台列表负责定位进程；执行状态来自同一原生任务和存活会话的记录。"""
    short = job.get("short", "")
    if not isinstance(short, str) or not re.fullmatch(r"[0-9a-f]{8}", short):
        return job
    fields = ("tempo", "state", "needs", "detail", "outcome", "block")
    result = {**job, **dict.fromkeys(fields)}
    stored = native_json(root / "jobs" / short / "state.json") if stored is None else stored
    if (stored.get("resumeSessionId") or stored.get("sessionId")) == native_id:
        result.update({key: stored.get(key) for key in fields})
        if stored.get("state") in ("done", "stopped", "failed", "ended"):
            # updatedAt 也会被重命名/设置更新，不能当作本轮完成时间。
            result["endedAtMs"] = native_time(stored.get("lastTerminalAt") or stored.get("firstTerminalAt"))
    if job.get("outcome"):
        result["outcome"] = job["outcome"]
    # 用户要求仅明确执行中才排队。daemon list 的 running/resuming 是宿主记录，
    # 不能覆盖已结束轮次；只定向读取此 job 的 REPL，不扫描其他会话或启动 CLI。
    if workers is None:
        workers = native_json(root / "daemon" / "roster.json").get("workers", {})
    worker = workers.get(short, {}) if isinstance(workers, dict) else {}
    if not isinstance(worker, dict):
        return result
    pid = worker.get("replPid")
    if worker.get("sessionId") not in (native_id, job.get("sessionId")) or type(pid) is not int or pid <= 1:
        return result
    record = native_json(root / "sessions" / f"{pid}.json")
    if (record.get("pid") != pid or record.get("sessionId") != native_id
            or record.get("jobId") != short or record.get("procStart") is None):
        return result
    try:
        # /proc 的第 22 列为进程启动标识；先越过可能含空格的 comm，防止复用 PID 的旧登记误报执行中。
        started = (Path("/proc") / str(pid) / "stat").read_text().rsplit(") ", 1)[1].split()[19]
        if str(record["procStart"]) != started:
            return result
    except (OSError, IndexError):
        return result
    state = record.get("status")
    if state not in ("busy", "idle", "waiting"):
        return result
    result["sessionStatus"] = state
    result["sessionStatusAtMs"] = native_time(record.get("statusUpdatedAt"))
    if state == "busy":
        result.update(tempo="active", state="working", needs=None, outcome=None)
        result.pop("endedAtMs", None)
    elif state == "waiting":
        result.update(tempo="blocked", state="blocked",
                      needs=record.get("waitingFor") or result.get("needs"), outcome=None)
        result.pop("endedAtMs", None)
    else:
        result.update(tempo="idle", needs=None)
        if result.get("state") not in ("done", "stopped", "failed", "ended"):
            result["state"] = "idle"
    return result


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
                                if "parentUuid" in row and row["parentUuid"] is None:
                                    cached["title"] = ""
                                cached["real"] = True
                                if not cached["title"]:
                                    content = row.get("message", {}).get("content", row.get("attachment", {}).get("prompt", ""))
                                    control = native_control_command(row)
                                    text = control["value"] if control else content if isinstance(content, str) else " ".join(part.get("text", "") for part in content if part.get("type") == "text")
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
                job = {key: raw_job[key] for key in ("sessionId", "resumeSessionId", "cwd", "name", "nameSource", "createdAt", "updatedAt", "firstTerminalAt", "lastTerminalAt",
                       "tempo", "state", "needs", "detail", "outcome") if key in raw_job}
                # 已有会话首次打开也要恢复实时问答；只保留问题与选项，不带入原始 job 的其他私有字段。
                questions = native_questions(raw_job.get("block"))
                if questions:
                    job["block"] = {"questions": questions}
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
        workers = native_json(self.root / "daemon" / "roster.json").get("workers", {})
        for native, row in rows.items():
            job = jobs.get(native, {})
            row["jobId"] = job.get("short", "")
            row["nativeStatus"] = self.status(native_activity(self.root, job, native, stored=job, workers=workers))
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
    def status(job, messages=None):
        status = {key: job.get(key) for key in ("tempo", "state", "needs", "detail", "outcome",
                                              "endedAtMs", "sessionStatus", "sessionStatusAtMs")}
        if job.get("sessionStatus") == "busy":
            status.update(tempo="active", state="working", endedAtMs=None)
        elif job.get("sessionStatus") == "waiting":
            # 新的原生确认框优先于旧轮次 done；等待确认不能变成空闲，也不能伪装执行中触发网页排队。
            status.update(tempo="blocked", state="blocked", endedAtMs=None)
        elif not job or job.get("state") in ("done", "stopped", "failed", "ended") or job.get("outcome"):
            status["tempo"] = "idle"
        elif status.get("tempo") in ("active", "blocked") and messages is not None and native_turn_finished(messages):
            # 原生完成证据也须纠正残留 active；新的 busy 会话状态仍优先，保留目标续跑等真实执行。
            status["tempo"] = "idle"
        if status.get("tempo") == "idle":
            status["needs"] = None
        return status

    def control(self, request):
        subtype = request.get("subtype")
        if subtype == "initialize":
            return {}
        if subtype == "discover":
            return {"sessions": self.scan(force=True)}
        if subtype not in ("read_history", "read_subagents", "read_subagent"):
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
        if subtype in ("read_subagents", "read_subagent"):
            if reader.subagents is None:
                reader.subagents = NativeSubagents(self.root, native)
            if subtype == "read_subagents":
                return {"agents": reader.subagents.collect()}
            identity = request.get("agent_id")
            if not NativeSubagents.valid_id(identity):
                raise RuntimeError("原生子代理 ID 无效")
            return {"agent": reader.subagents.collect(identity, request.get("include_messages") is True)[0]}
        reader.clients.update(request.get("clients", {}))
        reader.pending = request.get("pending", [])
        reader.native_commands = (request.get("native_control") or {}).get("commands", [])
        previous_compaction = (request.get("native_control") or {}).get("compaction") or {}
        if previous_compaction.get("state") in ("pending", "running", "unknown"):
            reader.compaction = {**previous_compaction, "state": "unknown"}
        reader.restore_compaction_events(request.get("native_control") or {})
        reader.model = request.get("model", "")
        reader.restore_settings(request.get("native_control") or {})
        reader.restore_question_reply(request.get("native_control") or {})
        reader.read_messages()
        row = self.rows[native]
        reader.short = row["jobId"]
        reader.job = self.jobs.get(native, row["nativeStatus"])
        reader.dialog = native_dialog(row["nativeStatus"], "", native)
        question = native_question_request(
            {**self.jobs.get(native, {}), **row["nativeStatus"], "short": row["jobId"]}, native)
        requests = reader.question_requests(question) if question else (
            {reader.dialog["id"]: {"subtype": "native_dialog", "dialog": reader.dialog}} if reader.dialog else {})
        return {"type": "shared_snapshot", "native_id": native, "job_id": row["jobId"],
                "native_status": self.status(row["nativeStatus"], reader.messages), "messages": reader.messages,
                "history_head": reader.history.current_head(),
                "retired_usage": reader.retired_usage,
                "clients": reader.clients, "pending": reader.pending,
                "requests": requests,
                "context_configuration": reader.context_configuration(), "updated_at": row["updatedAt"],
                "native_control": reader.control_projection()}

    def poll(self, force=False):
        pass


def permission_footer_mode(lines):
    # 只识别输入框下方的原生状态栏，不能把助手正文或用户引用当成权限已切换的证据。
    separators = [index for index, line in enumerate(lines) if re.match(r"^\s*[─╌━]{6,}", line)]
    if not separators:
        return None
    footer = " ".join(lines[separators[-1] + 1:]).strip().lower()
    for phrase, mode in (
        ("bypass permissions", "bypassPermissions"), ("permissions bypassed", "bypassPermissions"),
        ("accept edits", "acceptEdits"), ("plan mode", "plan"), ("auto mode", "auto"),
        ("don't ask", "dontAsk"), ("dontask", "dontAsk"),
        ("manual mode", "default"), ("default mode", "default"),
    ):
        if phrase in footer:
            return mode
    if "for shortcuts" in footer and "mode" not in footer and "permissions" not in footer:
        return "default"
    return None


def empty_permission_prompt(lines):
    # 改权限不能清除或提交另一个终端正在编辑的草稿，也不能把快捷键送入确认框。
    positions = [index for index, line in enumerate(lines) if re.match(r"^\s*❯", line)]
    if not positions:
        return False
    position = positions[-1]
    if re.sub(r"^\s*❯", "", lines[position]).strip():
        return False
    trailing = lines[position + 1:]
    end = next((index for index, line in enumerate(trailing) if re.match(r"^\s*[─╌━]{6,}", line)), None)
    return end is not None and not any(line.strip() for line in trailing[:end])


def permission_live_shortcut(lines):
    # 只有当前实例的原生状态栏明确提示此快捷键时，才确认执行中可用；其它版本保留空闲后切换。
    separators = [index for index, line in enumerate(lines) if re.match(r"^\s*[─╌━]{6,}", line)]
    footer = " ".join(lines[separators[-1] + 1:]) if separators else ""
    return bool(re.search(r"shift\s*\+\s*tab\s+to\s+cycle", footer, re.I))


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
        self.all_messages = []
        self.history = NativeHistory()
        self.retired_usage = []
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
        # 已确认被原生撤回（出字前停止）的输入 UUID，见 withdrawn_prompt。
        self.withdrawn = set()
        # 定时任务触发记录 UUID → 原生说明，见 scheduled_prompt。
        self.scheduled_fires = {}
        self.preview_user = ""
        self.preview_basis = ""
        self.preview_context_cache = None
        self.preview_prompt_cache = None
        self.preview_dirty = False
        self.delivery_started = False
        self.preview = ""
        self.preview_emitted = ""
        self.preview_at = 0
        self.context_files = {}
        self.native_context = {}
        self.goal = None
        self.goal_revision = ""
        self.goal_notice = ""
        self.compaction = {"state": "idle"}
        self.native_commands = []
        self.native_capabilities = {}
        self.control_screen_before = ""
        self.compaction_screen_active = False
        self.compaction_events = {}
        self.compaction_event_id = ""
        self.subagents = None
        self.setting_changes = {}
        self.actual_settings = {}
        self.setting_evidence_at = {}
        self.setting_catalog = []
        self.dialog = None
        self.answered_dialog = ""
        self.question_reply = None
        self.question_layout = {"id": "", "multi": set()}
        self.permission_mode = None
        self.permission_catalog = []
        self.permission_launch_key = None
        self.permission_launch_flags = []
        self.permission_retry_at = 0
        self.session_cwd = ""
        self.lock_folder = Path(f"/tmp/codex-elves-claude-{os.getuid()}")

    def control_projection(self):
        current = self.compaction_events.get(self.compaction_event_id)
        if current and current.get("afterUuid") is None and not current.get("_anchorResolved") and self.messages:
            current["afterUuid"] = self.compaction_anchor(current.get("startedAtMs", current.get("completedAtMs")))
            current["_anchorResolved"] = True
        return {"goal": self.goal, "goalRevision": self.goal_revision, "goalNotice": self.goal_notice,
                "compaction": self.compaction, "commands": self.native_commands,
                "capabilities": {**self.native_capabilities, "permissionModes": self.permission_modes()},
                "compactionEvents": list(self.compaction_events.values()),
                "compactionEventId": self.compaction_event_id,
                "settings": self.settings_projection(), "dialog": self.dialog,
                "questionReply": self.question_reply}

    def restore_question_reply(self, control):
        reply = control.get("questionReply")
        if isinstance(reply, dict) and isinstance(reply.get("id"), str) and reply.get("state") == "submitted":
            self.question_reply = dict(reply)

    def question_requests(self, request):
        if not request.get("nativeQuestion"):
            return {request["dialog"]["id"]: request}
        identity = request["nativeQuestion"]["id"]
        if self.question_layout["id"] != identity:
            self.question_layout = {"id": identity, "multi": set()}
        # 当前 CLI 的 block 摘要没有多选字段；仅按同一原生问题实际显示的复选框补齐，不按问题文字猜测。
        questions = request["input"]["questions"]
        page = native_question_page(self.screen.text(), questions)
        if page and page["index"] < len(questions) and page.get("multi"):
            self.question_layout["multi"].add(page["index"])
        for index in self.question_layout["multi"]:
            questions[index]["multiSelect"] = True
        if self.question_reply and self.question_reply.get("id") == identity:
            request["nativeQuestion"]["state"] = "submitted"
        return {identity: request}

    def settings_projection(self):
        return {**self.actual_settings, "changes": self.setting_changes}

    def restore_settings(self, control):
        settings = control.get("settings") or {}
        for name in ("model", "effort"):
            if isinstance(settings.get(name), str):
                self.actual_settings[name] = settings[name]
            change = (settings.get("changes") or {}).get(name)
            if isinstance(change, dict) and change.get("id"):
                self.setting_changes[name] = dict(change)
        if settings.get("permissionMode") in ("default", "manual", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions"):
            self.actual_settings["permissionMode"] = settings["permissionMode"]
        permission = (settings.get("changes") or {}).get("permissionMode")
        if (isinstance(permission, dict) and permission.get("id")
                and permission.get("requested") == self.permission_mode):
            self.setting_changes["permissionMode"] = dict(permission)

    def launch_permission_flags(self):
        key = (self.short, self.job.get("pid"), self.job.get("cliVersion"))
        if key != self.permission_launch_key:
            self.permission_launch_key = key
            self.permission_launch_flags = []
            if re.fullmatch(r"[0-9a-f]{8}", self.short):
                try:
                    record = json.loads((self.root / "jobs" / self.short / "state.json").read_text())
                    flags = record.get("respawnFlags", [])
                    if isinstance(flags, list):
                        # 只保留权限参数，不能把原始启动参数中的凭据或项目内容带入快照。
                        for index, flag in enumerate(flags):
                            if flag in ("--allow-dangerously-skip-permissions", "--dangerously-skip-permissions"):
                                self.permission_launch_flags.append(flag)
                            elif flag == "--permission-mode" and index + 1 < len(flags):
                                self.permission_launch_flags += [flag, flags[index + 1]]
                except (OSError, ValueError):
                    pass
        return self.permission_launch_flags

    def permission_modes(self):
        modes = self.permission_catalog or ["default", "manual", "acceptEdits", "plan",
                                            "auto", "bypassPermissions", "dontAsk"]
        if not self.short:
            return list(modes)
        flags = self.launch_permission_flags()
        # 用户要求实例全局明确设置 bypassPermissions 时，已有原生会话也应显示并允许该模式；
        # 显式启动参数仍然优先作为能力证据，配置读取失败时保持原有收敛行为，不扩大权限。
        configured_bypass = False
        try:
            configured_bypass = self.configured_permission_mode() == "bypassPermissions"
        except NativeError:
            pass
        bypass = configured_bypass or any(flag in flags for flag in (
            "--allow-dangerously-skip-permissions", "--dangerously-skip-permissions", "bypassPermissions"))
        # 原生共享会话只能选择已提供的运行模式；启动参数和实例配置都未开放时不能由网页补开。
        return [mode for mode in modes if (mode != "bypassPermissions" or bypass)
                and (mode != "dontAsk" or mode in flags)]

    def configured_permission_mode(self):
        path = self.root / "settings.json"
        try:
            if path.exists() and path.stat().st_size > 2 * 1024 * 1024:
                raise ValueError("config too large")
            settings = json.loads(path.read_text(encoding="utf-8-sig")) if path.exists() else {}
            mode = settings.get("permissions", {}).get("defaultMode", "default")
        except (OSError, ValueError, AttributeError):
            raise NativeError("ECONFIG", "无法读取 Claude 实例的全局权限，已暂缓切换权限") from None
        if mode not in ("default", "manual", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions"):
            raise NativeError("ECONFIG", "Claude 实例的默认权限模式无效，已暂缓切换权限")
        return mode

    def change_permission_mode(self, requested):
        # 用户要求切换权限不能阻断普通消息：目标选择与原生实际权限分开保存。
        # 审批、草稿或画面暂不可读时只登记待生效，不发送取消/提交按键，也不把正常发送报成失败。
        if requested not in ("configured", "default", "manual", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions"):
            raise NativeError("EINVAL", "Claude 权限模式无效")
        previous = self.setting_changes.get("permissionMode") or {}
        change = {**previous} if previous.get("requested") == requested else {
            "id": str(uuid.uuid4()), "requested": requested, "issuedAt": int(time.time() * 1000)}
        self.permission_retry_at = time.monotonic() + 1
        try:
            result = self._change_permission_mode(requested)
            pending = result.get("permissionPending") is True
            change.update(state="pending" if pending else "applied")
            change.pop("reason", None)
            if not pending:
                change["actual"] = self.actual_settings.get("permissionMode")
        except (OSError, ValueError, RuntimeError) as failure:
            # 有效选择的原生切换失败只影响权限生效状态；连接端稍后在安全画面重试，消息仍走原生投递入口。
            self.permission_mode = requested
            change.update(state="pending", reason=str(failure))
            result = {"permissionPending": True}
        if change != previous:
            self.setting_changes["permissionMode"] = change
            self.signature = None
        return {**result, "nativeSettings": self.settings_projection()}

    def retry_permission_mode(self, job):
        change = self.setting_changes.get("permissionMode") or {}
        if (change.get("state") != "pending" or time.monotonic() < self.permission_retry_at
                or not job or job.get("outcome") or job.get("needs")
                or job.get("sessionStatus") not in ("idle", "busy")):
            return
        self.change_permission_mode(change["requested"])

    def _change_permission_mode(self, requested):
        modes = ("default", "manual", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions")
        mode = self.configured_permission_mode() if requested == "configured" else requested
        if mode not in modes:
            raise NativeError("EINVAL", "Claude 权限模式无效")
        target = "default" if mode == "manual" else mode
        live = self.lookup() if self.native_id else None
        if not live or live.get("outcome"):
            # 尚未启动的轮次由原生 --permission-mode 实施；不为设置权限提前启动模型或复制会话。
            self.permission_mode = requested
            return {"nativeSettings": self.settings_projection(), "permissionPending": True}
        self.short = live["short"]
        self.job = live
        expected_short = self.short
        allow_busy = False

        # 用户要求普通发送不被重复应用同一权限阻断：原生状态栏已是目标模式时只确认，不发送快捷键。
        # 这里允许保留原生草稿、执行和审批等待；只有确实需要切换模式时，才要求下方的空闲、空输入框检查。
        observed = permission_footer_mode(self.read_terminal_screen())
        current_job = self.lookup()
        if (observed == target and current_job and current_job["short"] == expected_short
                and not current_job.get("outcome")):
            self.permission_mode = requested
            self.actual_settings["permissionMode"] = observed
            self.signature = None
            return {"nativeSettings": self.settings_projection()}

        def read_current():
            nonlocal allow_busy
            job = self.lookup()
            if (not job or job["short"] != expected_short or job.get("outcome")
                    or job.get("sessionStatus") not in ("idle", "busy") or job.get("needs")):
                raise NativeError("EPERMISSIONBUSY", "原生会话正在等待交互，权限将在可安全切换时生效")
            lines = self.read_terminal_screen()
            if not empty_permission_prompt(lines):
                raise NativeError("EPERMISSIONDRAFT", "原生 CLI 正在编辑或显示交互面板，未发送权限快捷键")
            if job.get("sessionStatus") == "busy":
                allow_busy = allow_busy or permission_live_shortcut(lines)
                if not allow_busy:
                    raise NativeError("EPERMISSIONBUSY", "当前实例未确认支持执行中切换，权限将在空闲后生效")
            value = permission_footer_mode(lines)
            if value is None:
                raise NativeError("EPERMISSIONUNKNOWN", "无法识别原生权限状态栏，已暂缓切换权限")
            self.actual_settings["permissionMode"] = value
            return value

        def switch(current):
            self.terminal_input(b"\x1b[Z", permission={"short": expected_short, "mode": current,
                                                    "allowBusy": allow_busy})
            # 原生按键接收与终端重绘异步完成；短暂等待状态栏变化，避免将旧一帧误判成模式循环结束。
            deadline = time.monotonic() + 0.6
            while True:
                value = read_current()
                if value != current or time.monotonic() >= deadline:
                    return value
                time.sleep(0.04)

        original = current = read_current()
        if current != target:
            supported = self.permission_modes()
            if mode not in supported and not (target == "default" and any(
                    value in supported for value in ("default", "manual"))):
                raise NativeError("EPERMISSIONMODE",
                    "该原生共享会话未开放所选权限模式，选择暂未生效；可在新建会话时选择")
        visited = set()
        for _ in range(8):
            if current == target:
                # 实例全局配置已让 CLI 处于目标模式时直接保留；缺少启动开关仍禁止网页另行开放该能力。
                self.permission_mode = requested
                self.actual_settings["permissionMode"] = current
                self.signature = None
                return {"nativeSettings": self.settings_projection()}
            if current in visited:
                break
            visited.add(current)
            # Claude Code 2.1.325 实测支持执行中 Shift+Tab；仅在同一会话、无交互面板且空输入框时操作，
            # 每一步重新核对原生状态栏，不能取消任务、覆盖草稿或替用户处理审批。
            current = switch(current)
        for _ in range(8):
            if current == original:
                self.actual_settings["permissionMode"] = original
                raise NativeError("EPERMISSIONMODE",
                    "原生会话未提供所选权限模式，已恢复原权限，选择暂未生效；可在新建会话时选择")
            current = switch(current)
        self.actual_settings.pop("permissionMode", None)
        raise NativeError("EPERMISSIONUNKNOWN", "原生权限状态待核对，已暂停切换权限，请在 CLI 确认当前模式")

    def resolve_setting_label(self, name, label):
        if name == "effort":
            value = label.lower().removesuffix(" effort")
            return "" if value == "auto" else value if value in ("low", "medium", "high", "xhigh", "max") else None
        for model in self.setting_catalog:
            if label in (model.get("value"), model.get("resolvedModel"), model.get("displayName")):
                return model["value"]
        return None

    def observe_used_setting(self, name, actual, row):
        at = native_time(row.get("timestamp"))
        if at < self.setting_evidence_at.get(name, 0):
            return
        self.actual_settings[name] = actual
        self.setting_evidence_at[name] = at
        change = self.setting_changes.get(name) or {}
        if change.get("state") not in ("pending", "confirmation", "confirming") or at < change.get("issuedAt", 0):
            return
        def normalize(value):
            resolved = self.resolve_setting_label(name, value)
            return value if resolved is None else resolved
        if normalize(actual) == normalize(change["requested"]):
            state = "applied"
        elif (change.get("dialogId") and at >= change.get("dialogAtMs", 0)
              and self.job.get("sessionStatus") != "waiting"
              and normalize(actual) == normalize(change.get("previous", ""))):
            # CLI 的拒绝结果可能不写 JSONL；确认框之后的新请求实际使用原模型，也是保留原设置的证据。
            state = "rejected"
        else:
            return
        change.update(state=state, actual=actual, resultId=row["uuid"])
        setattr(self, name, actual)

    def observe_setting_record(self, row):
        output = row.get("nativeSettingOutput")
        at = native_time(row.get("timestamp"))
        if output:
            name, requested = output["name"], output["value"]
            if name == "effort" and requested == "auto":
                requested = ""
            text = re.sub(r"\x1b\[[0-9;]*m", "", row["message"]["content"])
            pattern = (r"<local-command-stdout>(Set model to|Kept model as|Current model:)\s+(?:`([^`]+)`|([^<\n]+))"
                       if name == "model" else
                       r"<local-command-stdout>(Set effort level to|Kept effort level as|Current effort level:)\s+(?:`([^`]+)`|([^<\n]+))")
            match = re.search(pattern, text)
            if not match:
                return
            state = "rejected" if match[1].startswith("Kept ") else "applied"
            change = self.setting_changes.get(name) or {}
            if at < max(self.setting_evidence_at.get(name, 0), change.get("issuedAt", 0)):
                return
            label = (match[2] or match[3]).strip()
            actual = requested if state == "applied" and requested else self.resolve_setting_label(name, label)
            if actual is None and state == "rejected":
                actual = change.get("previous")
            if actual is None:
                return
            matching = change.get("requested") == requested or change.get("source") == "native"
            if not matching and change.get("state") in ("pending", "confirmation", "confirming"):
                # 先前的命令回显不能覆盖后来明确选择的设置；仍更新实际值，后一个命令继续由 CLI 排队。
                change["previous"] = actual
            else:
                self.setting_changes[name] = {
                    "id": change["id"] if matching else output["commandId"], "requested": requested,
                    "previous": change.get("previous", self.actual_settings.get(name, "")),
                    "state": state, "actual": actual, "resultId": row["uuid"], "issuedAt": at,
                    "dialogId": change.get("dialogId") if matching else None}
        elif row.get("type") == "assistant" and row.get("isApiErrorMessage") is not True:
            model = row.get("requestedModel") or row.get("message", {}).get("model")
            if model and model != "<synthetic>":
                self.observe_used_setting("model", model, row)
            effort = row.get("effort")
            if isinstance(effort, str):
                self.observe_used_setting("effort", effort, row)
            return
        else:
            return
        self.actual_settings[name] = actual
        self.setting_evidence_at[name] = at
        setattr(self, name, actual)

    def observe_dialog(self, job):
        dialog = native_dialog(job, self.screen.text(), self.native_id, self.root)
        if dialog and (dialog["id"] == self.answered_dialog or any(
                change.get("dialogId") == dialog["id"] and change.get("state") == "confirming"
                for change in self.setting_changes.values())):
            dialog["state"] = "submitted"
        self.dialog = dialog
        if job.get("sessionStatus") in ("busy", "idle"):
            for name, change in self.setting_changes.items():
                if change.get("state") == "confirming" and change.get("decision") == "decline":
                    # 原生取消不一定落盘：显式拒绝且确认框确已关闭后恢复原值，不能只凭按键写入宣告完成。
                    actual = self.actual_settings.get(name, change.get("previous", ""))
                    change.update(state="rejected", actual=actual)
                    setattr(self, name, actual)
        if not dialog or dialog["kind"] not in ("model", "effort"):
            return
        name = dialog["kind"]
        change = self.setting_changes.get(name)
        target = self.resolve_setting_label(name, dialog["target"])
        if change and change.get("dialogId") == dialog["id"] and change["state"] in ("applied", "rejected"):
            self.dialog = None
            return
        if change and target and change["requested"] != target and change["state"] in ("pending", "confirmation", "confirming"):
            return
        if not change or change["state"] in ("applied", "rejected"):
            change = {"id": dialog["id"], "requested": target or dialog["target"],
                      "previous": self.actual_settings.get(name, getattr(self, name)),
                      "issuedAt": dialog["statusAtMs"] or 0, "source": "native"}
            self.setting_changes[name] = change
        change.update(state="confirming" if dialog["state"] == "submitted" else "confirmation",
                      targetLabel=dialog["target"], dialogId=dialog["id"],
                      dialogAtMs=dialog["statusAtMs"] or 0)

    def restore_compaction_events(self, control):
        # 正文状态行属于已观察到的原生活动；重连和下一条命令不能删除已确认的中断、失败记录。
        for event in control.get("compactionEvents", []):
            if isinstance(event, dict) and isinstance(event.get("id"), str):
                self.compaction_events.setdefault(event["id"], dict(event))
        self.compaction_event_id = control.get("compactionEventId") or self.compaction_event_id
        event = self.compaction_events.get(self.compaction_event_id)
        if event and event.get("status") == "inProgress" and self.compaction.get("state") == "unknown":
            event["status"] = "unknown"

    def compaction_anchor(self, before=None):
        return next((row["uuid"] for row in reversed(self.messages)
                     if row.get("uuid") and (before is None or native_time(row.get("timestamp")) <= before)
                     and (real_user(row) or row.get("type") == "assistant"
                                            or row.get("subtype") == "turn_duration")), None)

    def record_compaction_event(self, identity, status, at=None, **fields):
        event = self.compaction_events.setdefault(identity, {
            "id": identity, "afterUuid": self.compaction_anchor(), "source": "manual"})
        event.update(status=status, **fields)
        observed_at = int(time.time() * 1000) if at is None else at
        if status == "inProgress":
            event.setdefault("startedAtMs", observed_at)
        elif status in ("completed", "failed", "interrupted"):
            event["completedAtMs"] = observed_at
            event.pop("phase", None)
        return event

    def observe_control_record(self, row):
        version = row.get("version", "")
        if not self.native_capabilities and isinstance(version, str) and re.fullmatch(r"\d+\.\d+\.\d+", version):
            # 只读历史不启动 CLI；2.1.322 已实测，执行前仍用 initialize.commands 核对当前实例。
            supported = tuple(map(int, version.split("."))) >= (2, 1, 322)
            self.native_capabilities = {"goal": supported, "compact": supported}
        attachment = row.get("attachment")
        if not isinstance(attachment, dict):
            attachment = {}
        at = native_time(row.get("timestamp"))
        if row.get("type") == "attachment" and attachment.get("type") == "goal_status":
            condition, met = attachment.get("condition"), attachment.get("met")
            if not isinstance(condition, str) or not condition.strip() or type(met) is not bool:
                return
            self.goal_revision = row["uuid"]
            if attachment.get("sentinel") is True:
                if not met:
                    self.goal_notice = ""
                    self.goal = {"objective": condition, "status": "active", "createdAt": at // 1000,
                                 "updatedAt": at // 1000, "nativeId": row["uuid"], "source": "claude"}
                elif not self.goal or self.goal.get("status") != "completed":
                    # 原生 clear 也写 met=true 的 sentinel；它仅表示结束，不能冒充目标已达成。
                    self.goal = None
                    self.goal_notice = ""
            elif self.goal and self.goal["objective"] == condition:
                self.goal["status"] = "completed" if met else "active"
                if met:
                    self.goal_notice = ""
                self.goal["updatedAt"] = at // 1000
                if isinstance(attachment.get("reason"), str):
                    self.goal["reason"] = attachment["reason"][:4000]
                # 只映射原生显式统计；不估算 Token、轮次或等待中的耗时，新目标从空统计开始。
                for source, target in (("tokens", "tokensUsed"), ("iterations", "iterations"), ("durationMs", "timeUsedSeconds")):
                    value = attachment.get(source)
                    if type(value) is int and 0 <= value <= 9007199254740991:
                        self.goal[target] = value / 1000 if source == "durationMs" else value
            for command in self.native_commands:
                if (command["name"] == "goal" and command["state"] in ("pending", "unknown")
                        and at >= command["issuedAt"] and row["uuid"] != command.get("baseline")
                        and (command["argument"] == condition and not met
                             or command["argument"] == "clear" and met and attachment.get("sentinel") is True)):
                    command["state"], command["nativeId"] = "completed", row["uuid"]
        elif row.get("type") == "system" and row.get("subtype") == "informational":
            text = row.get("content")
            if isinstance(text, str) and text.startswith(("Goal paused", "Goal still active", "Goal cleared after an unrecoverable error")):
                # 只投影原生目标通知，不返回 hook stderr、认证信息或请求诊断。
                self.goal_notice = text[:2000]
                if self.goal and text.startswith("Goal paused"):
                    self.goal["status"] = "blocked"
                elif self.goal and text.startswith("Goal still active"):
                    self.goal["status"] = "active"
        elif row.get("type") == "system" and row.get("subtype") == "compact_boundary":
            active = next((command for command in self.native_commands
                           if command["name"] == "compact" and command["state"] in ("pending", "running", "stopping", "unknown")), None)
            current = self.compaction_events.get(self.compaction_event_id)
            stale = active and (at < active["issuedAt"] or row["uuid"] == active.get("baseline"))
            if not active and current and current.get("status") in ("inProgress", "unknown"):
                stale = at < current.get("startedAtMs", 0)
            known = next((event for event in self.compaction_events.values() if event.get("nativeId") == row["uuid"]), None)
            identity = known["id"] if known else active["id"] if active and not stale \
                else current["id"] if current and not stale and current.get("status") in ("inProgress", "unknown") else row["uuid"]
            metadata = row.get("compactMetadata") or {}
            fields = {key: metadata[key] for key in ("preTokens", "postTokens", "durationMs")
                      if type(metadata.get(key)) is int and 0 <= metadata[key] <= 9007199254740991}
            if "durationMs" in fields and at >= fields["durationMs"]:
                fields["startedAtMs"] = at - fields["durationMs"]
            self.record_compaction_event(identity, "completed", at, nativeId=row["uuid"],
                                         source="automatic" if metadata.get("trigger") in ("auto", "automatic") else "manual",
                                         **fields)
            if stale:
                return
            self.compaction_event_id = identity
            self.compaction = {"state": "idle", "itemId": row["uuid"]}
            for command in self.native_commands:
                if (command["name"] == "compact" and command["state"] in ("pending", "running", "stopping", "unknown")
                        and at >= command["issuedAt"] and row["uuid"] != command.get("baseline")):
                    command["state"], command["nativeId"] = "completed", row["uuid"]

    def native_command(self, request):
        name, argument = request.get("name"), request.get("argument", "").strip()
        self.delivery_started = False
        if name not in ("goal", "compact") or not self.native_capabilities.get(name):
            raise NativeError("EUNSUPPORTED", "当前 Claude CLI 未提供此原生命令")
        if name == "goal" and (not argument or len(argument.encode("utf-16-le")) // 2 > 4000):
            raise NativeError("EINVAL", "Claude 目标需要 1–4000 个字符")
        if name == "compact" and not self.native_id:
            raise NativeError("EINVAL", "请先创建会话并产生对话，再压缩上下文")
        self.read_messages()
        expected = request.get("expectedGoal")
        if expected is not None and (not self.goal or any(
                self.goal.get(key) != expected.get(key) for key in ("nativeId", "objective", "status"))):
            raise NativeError("EBUSY", "原生目标已变化，请重新读取后操作")
        if any(command["name"] == name and command["state"] in ("pending", "running", "stopping", "unknown")
               for command in self.native_commands):
            raise NativeError("EBUSY", "上一条同类原生命令尚未确认，请先核对状态")
        live = self.lookup() if self.native_id else None
        if live:
            self.bind(live)
        if self.native_id and self.short and (not live or live.get("outcome")):
            live = self.restore_job()
        command = {"id": request["id"], "name": name, "argument": argument,
                   "state": "pending", "issuedAt": int(time.time() * 1000),
                   "baseline": self.goal_revision if name == "goal" else self.compaction.get("itemId"),
                   "beforeOutput": hashlib.sha256(self.compact_output().encode()).hexdigest()}
        # 只保留每类最近一次操作；已接收操作重连后仅核对，绝不重新发送。
        self.native_commands = [value for value in self.native_commands if value["name"] != name] + [command]
        self.control_screen_before = "\n".join(self.screen.text())
        if name == "compact":
            self.compaction_event_id = ""
            self.compaction = {"state": "pending", "itemId": command["id"]}
        emit({"type": "shared_native_command_intent", "native_control": self.control_projection()})
        self.delivery_started = True
        try:
            text = f"/{name}" + (f" {argument}" if argument else "")
            if live and not live.get("outcome"):
                self.reply(text, next_turn=False)
            else:
                self.start(text)
        except NativeError:
            self.delivery_started = False
            command["state"] = "failed"
            if name == "compact":
                self.compaction = {"state": "idle", "itemId": command["id"]}
            self.poll(force=True)
            raise
        except (OSError, RuntimeError, subprocess.SubprocessError):
            command["state"] = "unknown"
            if name == "compact":
                self.compaction["state"] = "unknown"
                event = self.compaction_events.get(self.compaction_event_id)
                if event and event.get("status") == "inProgress":
                    event["status"] = "unknown"
            self.poll(force=True)
            raise
        self.poll(force=True)
        return {"accepted": True, "nativeId": self.native_id, "jobId": self.short,
                "nativeCommand": {key: command[key] for key in ("id", "name", "state")},
                "nativeControl": self.control_projection()}

    def compact_output(self):
        lines = self.screen.text()
        starts = [index for index, line in enumerate(lines) if re.match(r"^\s*❯\s*/compact\s*$", line)]
        return " ".join(lines[starts[-1] + 1:starts[-1] + 6]) if starts else ""

    def observe_control_screen(self):
        lines = self.screen.text()
        text = "\n".join(lines)
        compacting = bool(re.search(r"(?m)^[✶✻✽✢·*]\s+Compacting conversation", text))
        ended_display = self.compaction_screen_active and not compacting
        if compacting and not self.compaction_screen_active:
            self.compaction = {**self.compaction, "state": "running"}
            command = next((command for command in self.native_commands if command["name"] == "compact"
                            and command["state"] in ("pending", "running", "unknown")), None)
            previous = self.compaction_events.get(self.compaction_event_id)
            identity = command["id"] if command else previous["id"] if previous and previous.get("status") == "unknown" else str(uuid.uuid4())
            self.compaction_event_id = identity
            self.record_compaction_event(identity, "inProgress")
            for command in self.native_commands:
                if command["name"] == "compact" and command["state"] in ("pending", "unknown"):
                    command["state"] = "running"
        if self.compaction_screen_active and not compacting:
            event = self.compaction_events.get(self.compaction_event_id)
            if event and event.get("phase") == "stopping":
                self.record_compaction_event(event["id"], "interrupted")
                self.compaction = {"state": "idle", "itemId": event["id"]}
            for command in self.native_commands:
                if command["name"] == "compact" and command["state"] == "stopping":
                    command.update(state="completed", message="压缩已停止")
                    self.record_compaction_event(command["id"], "interrupted")
                    self.compaction = {"state": "idle", "itemId": command["id"]}
        self.compaction_screen_active = compacting
        # 无历史/失败可能只有原生终端回显。仅关联本次 /compact 行后的回显，不按旧错误或用户引用判断。
        if text != self.control_screen_before:
            for command in self.native_commands:
                if command["name"] != "compact" or command["state"] not in ("pending", "running"):
                    continue
                output = self.compact_output()
                if command["state"] == "pending" and hashlib.sha256(output.encode()).hexdigest() == command.get("beforeOutput"):
                    continue
                if re.search(r"Nothing to compact|Not enough messages|Conversation too short", output, re.I):
                    command.update(state="completed", message="当前历史无需压缩")
                    self.record_compaction_event(command["id"], "completed", outcome="noop")
                    self.compaction_event_id = command["id"]
                    self.compaction = {"state": "idle", "itemId": command["id"]}
                elif re.search(r"Error:|Failed to compact|Compaction failed", output, re.I):
                    command.update(state="failed", message="原生压缩未完成，请在 CLI 中核对错误")
                    self.record_compaction_event(command["id"], "failed")
                    self.compaction_event_id = command["id"]
                    self.compaction = {"state": "idle", "itemId": command["id"]}
        event = self.compaction_events.get(self.compaction_event_id)
        if ended_display and event and event.get("status") == "inProgress":
            # 原生画面已结束但还没有终态证据时保留同一行待核对，不无限显示“压缩中”或猜测成功。
            event["status"] = "unknown"
            self.compaction["state"] = "unknown"
            for command in self.native_commands:
                if command["name"] == "compact" and command["state"] == "running":
                    command["state"] = "unknown"

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
        job = next((job for job in jobs if (job.get("resumeSessionId") or job.get("sessionId")) == self.native_id), None)
        return native_activity(self.root, job, self.native_id) if job else None

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
                        # 一次性读取完整终端快照，避免历史压缩画面的中间帧被识别成新的压缩开始。
                        self.incoming.put({"_stream": "".join(value.get("streamTail", []))})
                    elif value.get("type") == "stream":
                        self.incoming.put({"_stream": value.get("line", "")})
        except (OSError, ValueError):
            return

    def reply(self, text, next_turn=True):
        # 用户要求原生引导：nextTurn=True 在 CLI 内对应 priority="next"，
        # 当前工具调用结束后吸收消息；不是 Esc 中断，也不是等整轮结束才投递。
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

    def start(self, text=None):
        if self.native_id:
            self.check_history_resume()
        # 网页与 CLI 必须操作所选项目的同一目录；仅关闭默认 worktree 隔离，保留原生提示和工具。
        arguments = [self.cli, "--bg", "--settings", '{"worktree":{"bgIsolation":"none"}}']
        if self.permission_mode and self.permission_mode != "configured":
            mode = self.permission_mode
            if mode == "default" and "manual" in self.permission_catalog and "default" not in self.permission_catalog:
                mode = "manual"
            elif mode == "manual" and "default" in self.permission_catalog and "manual" not in self.permission_catalog:
                mode = "default"
            arguments += ["--permission-mode", mode]
            if mode == "bypassPermissions":
                arguments += ["--allow-dangerously-skip-permissions"]
        if self.native_id:
            arguments += ["--resume", self.native_id]
        if self.model:
            arguments += ["--model", self.model]
        if self.effort:
            arguments += ["--effort", self.effort]
        # 恢复历史时不附加新名称，保留原生自定义标题；新会话仍沿用既有命名。
        if not self.native_id:
            arguments += ["--name", f"elves-{uuid.uuid4().hex[:10]}"]
        if text is not None:
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
        live = self.lookup() if self.native_id else None
        if live:
            self.bind(live)
        # 查询期间 CLI 可能写入完成或新输入；准入和引导都使用这次查询后的原生历史。
        self.read_messages()
        status = NativeDiscovery.status(live or {}, self.messages)
        if self.compaction.get("state") == "running" and status.get("sessionStatus") != "waiting":
            status.update(tempo="active", state="compacting")
        active_at_admission = status.get("tempo") == "active"
        # 自动续发必须以实时原生状态为准；CLI 恰好开始新任务时继续等，不能抢先把下一条塞入原生队列。
        if request.get("idle_only") and live:
            if active_at_admission or status.get("tempo") == "blocked" and status.get("needs") \
                    and status.get("needs") != "dialog open":
                self.poll(force=True)
                return {"deferred": True, "accepted": False}
        # 原生弹窗等待不等于执行中；普通消息仍直接送入 CLI，由原生保留到确认完成，不退回网页队列。
        # 引导也只进入原生 next 队列；已有审批继续由 CLI 处理，不借此替用户批准、拒绝或取消工具。
        text = self.prepare_text(content, client_id)
        if self.native_id and self.short and (not live or live.get("outcome")):
            live = self.restore_job()
        after = len(self.messages)
        intent = {"id": client_id, "clientUserMessageId": client_id,
                  "text": "\n".join(p.get("text", "") for p in content if p.get("type") == "text"),
                  "content": content, "_nativeText": text, "_after": after,
                  "_afterOffset": self.log_offset, "deliveryConfirmed": False}
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
        # 引导只投递选中的一条；由原生 next 队列决定吸收时机，前后均不发送中断按键或等待旧轮停止。
        # accepted 仅代表原生入口接收，正文仍由随后关联到的原生用户消息确认，不能伪造执行完成。
        return {"nativeId": self.native_id, "jobId": self.short, "accepted": True}

    @contextlib.contextmanager
    def edit_lock(self):
        # 用户要求只禁止同时编辑历史：同一 WSL 用户下连接同一原生会话的多个连接端（如正式服务与验收服务）
        # 串行执行编辑，第二个编辑直接拒绝；不阻止原版终端，终端打开回退面板时由 waiting 状态拒绝。
        folder = self.lock_folder
        folder.mkdir(mode=0o700, exist_ok=True)
        if folder.is_symlink() or folder.stat().st_uid != os.getuid():
            raise NativeError("ESESSION", "编辑锁目录归属异常，未修改历史")
        handle = os.open(folder / f"edit-{uuid.UUID(self.native_id)}.lock",
                         os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise NativeError("EBUSY", "该会话正在由另一个网页编辑历史，请稍后重试") from None
            yield
        finally:
            os.close(handle)

    def session_holders(self):
        # 两个进程不能同时写同一份原生记录：交接前后确认没有任何 CLI 进程（后台或终端）打开该会话。
        holders = []
        for path in (self.root / "sessions").glob("*.json"):
            record = native_json(path)
            if record.get("sessionId") != self.native_id:
                continue
            try:
                pid = int(record.get("pid", path.stem))
                if pid <= 1:
                    continue
                os.kill(pid, 0)
            except (TypeError, ValueError, ProcessLookupError):
                continue
            except PermissionError:
                pass
            holders.append(pid)
        return holders

    def job_alive(self):
        try:
            return self.rpc({"op": "has", "short": self.short}).get("alive") is True
        except (FileNotFoundError, ConnectionRefusedError):
            return False

    def stop_job(self):
        # 用户要求（B 方案）：编辑前用原版 claude stop 停止同一后台会话，让 print 进程独占记录完成回退；
        # 已连接的原版终端会提示会话已退出，需重新 claude attach，这是用户已确认接受的代价。
        result = subprocess.run([self.cli, "stop", self.short], stdin=subprocess.DEVNULL,
                                capture_output=True, timeout=30)
        if result.returncode:
            raise NativeError("ESESSION", "原生后台会话未能停止，未修改历史")
        deadline = time.monotonic() + 15
        while True:
            try:
                alive = self.job_alive()
            except (OSError, NativeError):
                alive = True
            if not alive and not self.session_holders():
                return
            if time.monotonic() >= deadline:
                raise NativeError("ESESSION", "原生后台会话仍未退出，未修改历史")
            time.sleep(0.2)

    @staticmethod
    def background_work(lines):
        # 用户要求交接只在完全空闲时进行：停止进程会结束后台命令、监控和后台子代理。
        # 2.1.323 实测后台命令运行时，原生底栏显示“1 shell · ↓ to manage”，结束后提示消失。
        # 订阅流只重绘变化区域，分隔线未必在画面里；因此只看画面最底部三行（输入框与底栏），
        # 并要求带箭头的“↓ to manage”，正文里的同样文字不算。画面为空返回 None 表示无法确认。
        bottom = [line.strip() for line in lines if line.strip()][-3:]
        if not bottom:
            return None
        return "↓ to manage" in " ".join(bottom)

    def scheduled_prompt(self, row):
        fire = self.scheduled_fires.get(row.get("parentUuid")) if row.get("type") == "user" else None
        content = row.get("message", {}).get("content") if fire is not None else None
        text = content if isinstance(content, str) else "\n".join(
            part.get("text", "") for part in content if isinstance(part, dict) and part.get("type") == "text") \
            if isinstance(content, list) else ""
        if fire is None or not text.strip():
            return None
        return {"taskId": "scheduled", "kind": "schedule", "summary": text.strip()[:400], "event": fire}

    def withdrawn_prompt(self, status):
        # 出字前停止（2.1.323 实测）：原生撤回本条输入并把原文放回终端输入框，记录不写中断标记，
        # 下一条输入也不再以它为父节点。用户要求网页如实显示“已撤回”而不是一直等待开始：
        # 仅当会话在线空闲、本条之后没有回复或中断记录、且终端输入框正显示同一原文时认定；同一条只需观察到一次。
        start = next((index for index in range(len(self.messages) - 1, -1, -1)
                      if native_turn_start(self.messages[index])), None)
        if start is None or not real_user(self.messages[start]):
            return None
        row = self.messages[start]
        if row["uuid"] in self.withdrawn:
            return row["uuid"]
        if (status.get("tempo") != "idle" or not status.get("sessionAlive")
                or any(later.get("type") == "assistant" or native_interruption(later)
                       for later in self.messages[start + 1:])):
            return None
        content = row.get("message", {}).get("content")
        text = content if isinstance(content, str) else "\n".join(
            part.get("text", "") for part in content if isinstance(part, dict) and part.get("type") == "text") \
            if isinstance(content, list) else ""
        expected = "".join(text.split())
        bottom = [line.strip() for line in self.screen.text() if line.strip()][-6:]
        shown = next(("".join(line[1:].split()) for line in reversed(bottom) if line.startswith("❯")), "")
        size = min(len(shown), len(expected), 24)
        if not expected or size < min(len(expected), 6) or shown[:size] != expected[:size]:
            return None
        self.withdrawn.add(row["uuid"])
        return row["uuid"]

    @staticmethod
    def background_summary(lines):
        # 用户要求网页看到原版仍在运行的后台任务：与 background_work 同样只认底部三行的原生底栏。
        # 2.1.323 实测底栏如 “auto mode on · ← 2 agents · 1 shell · esc to interrupt · ↓ to manage”，
        # 只有带 “↓ to manage” 时才有后台任务；取其中“数字 + 名词”段（1 shell / 1 monitor），
        # 跳过 “← 2 agents”（会话一开始就有，与后台任务无关）和 “esc to interrupt”。由网页翻译；画面不可读时返回 None。
        bottom = [line.strip() for line in lines if line.strip()][-3:]
        if not bottom:
            return None
        for line in reversed(bottom):
            if "↓ to manage" in line:
                segments = [segment.strip() for segment in line.split("·")]
                # 多类任务同时运行时的写法未实测（可能分段，也可能 “1 shell, 1 monitor” 同段），两种都按逗号再拆。
                counts = [piece.strip() for segment in segments[:next(index for index, segment in enumerate(segments)
                                                                      if "↓ to manage" in segment)]
                          for piece in segment.split(",") if re.fullmatch(r"\d+ [a-z][a-z ]{0,30}", piece.strip())]
                return ", ".join(counts)[:80] or "background"
        return ""

    def current_screen(self):
        # 后台会话画面由订阅流持续投影到 self.screen；编辑在主循环内同步执行，
        # 先补齐已到达但未处理的画面帧，其他输入（网页控制请求）按原顺序放回。
        deferred = []
        while self.incoming is not None:
            try:
                frame = self.incoming.get_nowait()
            except queue.Empty:
                break
            if isinstance(frame, dict) and "_stream" in frame:
                self.stream_preview(frame["_stream"])
            else:
                deferred.append(frame)
        for frame in deferred:
            self.incoming.put(frame)
        return self.screen.text()

    def run_rewind(self, target, last_seen, cwd):
        # 用户要求（B 方案）用结构化接口回退，不再模拟按键操作 /rewind 菜单。原生 print 进程按 stream-json
        # 控制协议先 initialize，再发送 rewind_conversation；last_seen 为网页当前分支的最后一条 user 记录，
        # 原生发现网页尚未显示的新轮次会拒绝。该进程不发送用户消息、不调用模型；--strict-mcp-config 与
        # disableAllHooks 只作用于这个短进程，避免为一次回退启动 MCP 或触发用户 Hook。
        # 成功返回原生答复；明确拒绝抛 NativeError（记录未修改）；发出回退请求后失去答复抛 RuntimeError（结果未知）。
        process = subprocess.Popen(
            [self.cli, "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
             "--strict-mcp-config", "--settings", '{"disableAllHooks":true}', "--resume", self.native_id],
            cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        lines = queue.Queue()

        def drain():
            for line in process.stdout:
                lines.put(line)
            lines.put(None)

        reader = threading.Thread(target=drain, daemon=True)
        reader.start()

        def request(subtype, timeout, **fields):
            identity = uuid.uuid4().hex
            process.stdin.write(json.dumps({"type": "control_request", "request_id": identity,
                                            "request": {"subtype": subtype, **fields}}).encode() + b"\n")
            process.stdin.flush()
            deadline = time.monotonic() + timeout
            while True:
                line = lines.get(timeout=max(0.01, deadline - time.monotonic()))
                if line is None:
                    raise EOFError("原生回退进程已退出")
                try:
                    frame = json.loads(line)
                except ValueError:
                    continue
                response = frame.get("response") if isinstance(frame, dict) and frame.get("type") == "control_response" else None
                if isinstance(response, dict) and response.get("request_id") == identity:
                    return response
                if time.monotonic() >= deadline:
                    raise TimeoutError("原生回退进程未答复")

        lost = (OSError, ValueError, EOFError, TimeoutError, queue.Empty)
        try:
            try:
                ready = request("initialize", 60)
            except lost:
                raise NativeError("ESESSION", "无法启动原生回退进程，未修改历史") from None
            if ready.get("subtype") != "success":
                raise NativeError("ESESSION", "原生回退进程初始化失败，未修改历史")
            # 从发出回退请求起记录可能已被修改；此后失去答复只能按未知处理，不能当作未修改。
            self.delivery_started = True
            try:
                response = request("rewind_conversation", 30, target_message_uuid=target,
                                   last_seen_user_message_uuid=last_seen)
            except lost:
                raise RuntimeError("原生回退结果未知，修改后的消息未发送，请刷新核对历史") from None
        finally:
            with contextlib.suppress(OSError):
                process.stdin.close()
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            reader.join(timeout=5)
            with contextlib.suppress(OSError):
                process.stdout.close()
        value = response.get("response") if response.get("subtype") == "success" else None
        if isinstance(value, dict) and value.get("rewound") is True:
            return value
        self.delivery_started = False
        reason = value.get("reason") if isinstance(value, dict) else None
        if not isinstance(reason, str) or not reason:
            raise NativeError("EUNSUPPORTED", "当前原版 Claude 未接受结构化回退，未修改历史")
        code, message = REWIND_REFUSALS.get(reason, ("EUNSUPPORTED", f"原版 Claude 拒绝回退（{reason}），未修改历史"))
        raise NativeError(code, message)

    def edit_message(self, request):
        # 用户要求（2026-11-13 确认 B 方案）：网页编辑历史改为交接。停止同一后台会话 → print 进程用
        # rewind_conversation 结构化回退 → claude respawn 恢复同一会话 → 发送修改后的消息。
        # 必须保持原 UUID 与后台短 ID；已连接的原版终端会提示会话退出，需重新 claude attach。
        # 仅在完全空闲时允许：无执行中轮次、无后台任务、无待确认弹窗、无压缩、无其他网页编辑。
        # 不模拟按键操作 /rewind 菜单，失败时恢复会话并如实区分“未修改”和“结果未知”。
        self.delivery_started = False
        self.read_messages()
        target = next((row for row in self.messages if row.get("uuid") == request.get("native_user_id")
                       and row.get("nativeEditable") is True), None)
        if target is None or request.get("history_head") != self.history.current_head():
            raise NativeError("ESTALE", "原生历史已变化，请重新打开要编辑的消息")
        prompt = request.get("prompt")
        if not isinstance(prompt, str) or not prompt.strip() or len(prompt.encode()) > 128 * 1024:
            raise NativeError("EINVAL", "修改后的消息不能为空或超过 128 KiB")
        content = self.edit_content(target, prompt)
        with self.edit_lock():
            live = self.lookup()
            if live:
                self.bind(live)
            self.read_messages()
            if request.get("history_head") != self.history.current_head():
                raise NativeError("ESTALE", "原生历史已变化，未提交编辑")
            status = NativeDiscovery.status(live or {}, self.messages)
            if (status.get("tempo") != "idle" or status.get("needs")
                    or live and live.get("sessionStatus") not in (None, "idle")
                    or self.compaction.get("state") == "running"):
                raise NativeError("EBUSY", "请等待 Claude 结束当前执行后再编辑历史消息")
            was_live = bool(live and not live.get("outcome"))
            if was_live:
                background = self.background_work(self.current_screen())
                if background is None:
                    # 连接端尚未收到画面时才单独订阅一次快照。
                    try:
                        background = self.background_work(self.read_terminal_screen())
                    except (OSError, ValueError, RuntimeError):
                        background = None
                if background is None:
                    raise NativeError("EBUSY", "暂时无法确认 Claude 后台任务状态，未修改历史")
                if background:
                    raise NativeError("EBUSY", "Claude 仍有后台任务在运行；编辑会停止会话并结束这些任务，请等任务结束后再编辑")
            elif self.session_holders():
                raise NativeError("ESESSIONOPEN", "原生会话仍在其他 CLI 中打开，未修改历史")
            last_seen = next((row["uuid"] for row in reversed(self.messages)
                              if row.get("type") == "user" and not row.get("isMeta")), None)
            cwd = self.session_cwd or self.job.get("cwd") or ""
            if not isinstance(cwd, str) or not cwd.startswith("/") or not Path(cwd).is_dir():
                raise NativeError("ESESSION", "找不到原生会话的项目目录，未修改历史")
            if was_live:
                self.stop_job()
            try:
                self.run_rewind(target["uuid"], last_seen, cwd)
            except Exception:
                # 回退未完成：恢复原本在运行的同一后台会话；结果未知时也须恢复，再由网页按原生记录核对。
                if was_live:
                    with contextlib.suppress(OSError, RuntimeError, subprocess.SubprocessError):
                        self.restore_job()
                raise
            # 回退已被原生确认；修改后的消息尚未发送，后续失败按“未发送”报告并把文字退回输入框。
            self.delivery_started = False
            self.read_messages()
            if any(row.get("uuid") == target["uuid"] for row in self.messages):
                self.history.cut_before = target["uuid"]
                self.messages = self.history.select(self.all_messages)
            self.preview = self.preview_emitted = self.preview_user = self.preview_basis = ""
            self.preview_context_cache = self.preview_prompt_cache = None
            try:
                reopened = bool(self.short) and self.job_alive()
            except (OSError, NativeError):
                reopened = True
            if reopened or self.session_holders():
                # 交接期间若终端抢先 claude attach，新进程可能载入了回退前的状态；不能再发消息覆盖。
                raise NativeError("ESESSION", "编辑期间原生会话被重新打开，回退可能未生效；修改后的消息未发送，请刷新核对")
            try:
                if self.short:
                    self.restore_job()
                else:
                    self.start()
            except (OSError, RuntimeError, subprocess.SubprocessError) as failure:
                raise NativeError("ESESSION", "历史已回退，但原生会话未能重新启动；修改后的消息未发送，可直接重新发送") from failure
            if request.get("model") != self.model:
                self.change_setting("model", request.get("model", self.model))
            if request.get("effort") != self.effort:
                self.change_setting("effort", request.get("effort", self.effort))
            result = self.send({"client_id": request["client_id"], "content": content, "idle_only": True})
            if result.get("deferred"):
                raise RuntimeError("历史已回退，但原生会话已开始新的执行；修改后的消息未发送，请稍后重新发送")
            return {**result, "reverted": True}

    def edit_content(self, target, prompt):
        # 编辑文字保留原图；图片从原生消息/已读取附件还原，继续经现有上传入口进入同一 CLI。
        content, total = [{"type": "text", "text": prompt}], 0
        originals = target.get("web_images", [])
        if originals:
            # 网页原图清单是完整来源；不再叠加 CLI 转录的同一批图片，用户主动重复添加的原图仍保留。
            blocks = [{"type": "image", "url": image["url"]} for image in originals]
        else:
            blocks = list(target.get("message", {}).get("content", [])) \
                if isinstance(target.get("message", {}).get("content"), list) else []
            start = self.messages.index(target) + 1
            for row in self.messages[start:]:
                if row.get("type") != "attachment":
                    break
                blocks.extend(row.get("message", {}).get("content", []))
        for block in blocks:
            if not isinstance(block, dict) or block.get("type") != "image":
                continue
            image = block.get("source", {})
            url = block.get("url", "")
            match = re.fullmatch(r"data:(image/(?:png|jpeg|gif|webp));base64,([A-Za-z0-9+/=\r\n]+)", url)
            if match:
                image = {"type": "base64", "media_type": match[1], "data": match[2]}
            if (not isinstance(image, dict) or image.get("type") != "base64"
                    or image.get("media_type") not in ("image/png", "image/jpeg", "image/gif", "image/webp")):
                raise NativeError("EIMAGE", "原消息图片暂不可恢复，未回退历史")
            try:
                size = len(base64.b64decode(image.get("data"), validate=True))
            except (ValueError, TypeError):
                raise NativeError("EIMAGE", "原消息图片内容暂不可恢复，未回退历史") from None
            total += size
            # 与 remote_access.rs 的 8 张、单图 5 MiB、合计 12 MiB 同步；附件无法完整保留时不回退。
            if len(content) > 8 or size > 5 * 1024 * 1024 or total > 12 * 1024 * 1024:
                raise NativeError("EIMAGE", "原消息图片超过远程输入限制，请在原版 CLI 编辑；未回退历史")
            content.append({"type": "image", "source": image})
        return content

    def read_terminal_screen(self):
        # 仅在用户回答时订阅一次新画面；每次读取等候最多 2 秒，最多 4 帧，不增加发送轮询。
        # 单帧上限 16 MiB：长会话的 200 段终端快照可超过 1 MiB（2026-11-13 编辑交接验收实测），
        # 原 1 MiB 上限会让问答、确认框直接报“无法读取原生交互界面”；与 Rust MAX_FRAME_BYTES 同为 16 MiB。
        screen = Screen()
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(2)
            connection.connect(self.address)
            connection.sendall(json.dumps({"proto": 1, "op": "subscribe", "short": self.short,
                                           "tail": 200}).encode() + b"\n")
            with connection.makefile("rb") as stream:
                for _ in range(4):
                    line = stream.readline(16 * 1024 * 1024)
                    if not line.endswith(b"\n"):
                        break
                    frame = json.loads(line)
                    if frame.get("type") == "snapshot":
                        screen.feed("".join(frame.get("streamTail", [])))
                        return screen.text()
        raise NativeError("EQUESTIONCHANGED", "暂时无法读取原生交互界面，未发送交互按键")

    def verify_dialog(self, expected):
        # 回答前读取同一后台的新终端快照，再核对原生等待代次；迟到点击不能落入新弹窗或聊天输入框。
        screen = self.read_terminal_screen()
        current = native_dialog(self.lookup() or {}, screen, self.native_id, self.root)
        if current and current["id"] == expected["id"] and current["remoteSupported"]:
            return
        raise NativeError("EDIALOGCHANGED", "原生确认界面已变化，请重新读取；未发送确认按键")

    def check_question(self, expected):
        current = native_question_request(self.lookup() or {}, self.native_id)
        identity = expected["nativeQuestion"]["id"]
        if not current or current.get("nativeQuestion", {}).get("id") != identity:
            raise NativeError("EQUESTIONCHANGED", "原生问题已经结束或变化，请重新读取；未提交回答")
        if self.question_reply and self.question_reply.get("id") == identity:
            raise NativeError("EQUESTIONCHANGED", "回答已经提交，请等待原生结果；不会重复提交")

    def read_question_page(self, request, index=None):
        # 原生面板重绘沿用同一问题身份；只等最多 2 秒，不把旧页面上的按键盲目发送到下一问。
        deadline = time.monotonic() + 2
        while True:
            self.check_question(request)
            page = native_question_page(self.read_terminal_screen(), request["input"]["questions"])
            if page and (index is None or page["index"] == index):
                return page
            if time.monotonic() >= deadline:
                raise NativeError("EQUESTIONCHANGED", "原生问答界面未按预期切换，回答尚未提交，请在 CLI 核对后重试")
            time.sleep(0.1)

    def submit_multi_question(self, request, index, selected):
        page = self.read_question_page(request, index)
        # 原版多选勾选时已保存答案。先回到首个普通选项以退出文字输入，数字键才不会污染答案或触发 Submit。
        # 最多跨过全部选项、Other、Submit 和底部 Chat 入口；每一步都重新核对同一问题。
        for _ in range(len(request["input"]["questions"][index]["options"]) + 3):
            if any(re.match(r"^❯\s*1\.\s+\[", line) for line in page["lines"]):
                break
            self.terminal_input(b"\x1b[A", question=request)
            page = self.read_question_page(request, index)
        else:
            raise NativeError("EQUESTIONCHANGED", "未找到原生多选位置，未提交回答")
        checkboxes = [(match[1], match[2] != " ") for line in page["lines"]
                      if (match := re.match(r"^(?:❯\s*)?([1-9])\.\s+\[([^\]])\]", line))]
        if not set(selected).issubset(number for number, _ in checkboxes):
            raise NativeError("EQUESTIONCHANGED", "原生多选选项已变化，未提交回答")
        # 网页提交的是完整答案集合；清掉原生面板尚未提交的旧勾选，再按选项顺序恢复本次选择。
        for number, checked in checkboxes:
            if checked:
                self.terminal_input(number.encode(), question=request)
        for number in selected:
            self.terminal_input(number.encode(), question=request)
        # 在普通选项焦点上用原生 Tab 切到下一问/汇总；不能在 Submit 焦点上发送数字选择。
        self.terminal_input(b"\t", question=request)

    def answer_questions(self, request, values):
        questions = request["input"]["questions"]
        if not isinstance(values, dict) or any(
                not isinstance(values.get(q["question"]), str) or not values[q["question"]].strip()
                for q in questions):
            raise NativeError("EINVAL", "请回答所有问题")
        answers = {q["question"]: values[q["question"]].strip() for q in questions}
        # 自由回答经原版输入框粘贴；拒绝终端控制字符，答案不能变成快捷键或退出粘贴后的命令。
        if any(re.search(r"[\x00-\x1f\x7f]", value) for value in answers.values()):
            raise NativeError("EINVAL", "回答请使用单行文字，不能包含终端控制字符")
        identity = request["nativeQuestion"]["id"]
        try:
            for index, question in enumerate(questions):
                page = self.read_question_page(request)
                for _ in range(len(questions) + 1):
                    if page["index"] == index:
                        break
                    direction = b"\x1b[Z" if page["index"] > index else b"\t"
                    destination = page["index"] + (-1 if page["index"] > index else 1)
                    self.terminal_input(direction, question=request)
                    page = self.read_question_page(request, destination)
                if page["index"] != index:
                    raise NativeError("EQUESTIONCHANGED", "无法定位原生问题，未提交回答")
                answer = answers[question["question"]]
                choices = page["choices"]
                if "Type something." not in choices and "Type something" not in choices:
                    raise NativeError("EQUESTIONCHANGED", "原生正在编辑回答或使用不同面板，请在 CLI 核对后重试")
                single_final = len(questions) == 1 and not page["multi"]
                parts = answer.split(", ")
                labels = [option["label"] for option in question["options"]]
                predefined = page["multi"] and len(set(parts)) == len(parts) and all(part in labels for part in parts)
                if predefined:
                    selected = [label for label in labels if label in parts]
                    if any(label not in choices for label in selected):
                        raise NativeError("EQUESTIONCHANGED", "原生多选选项未完整显示，请在 CLI 核对")
                    self.submit_multi_question(request, index, [choices[label] for label in selected])
                    # 与原版多选编码一致：含逗号分隔符或引号的单项使用 JSON 引号，其余按选项顺序连接。
                    answers[question["question"]] = ", ".join(
                        json.dumps(label, ensure_ascii=False) if ', ' in label or '"' in label else label
                        for label in selected)
                elif answer in choices and not page["multi"]:
                    self.terminal_input(choices[answer].encode(), question=request, final_question=single_final)
                else:
                    other = choices.get("Type something.") or choices.get("Type something")
                    if not other:
                        raise NativeError("EQUESTIONCHANGED", "此原生面板暂不支持网页填写，请在 CLI 回答")
                    if not page["multi"]:
                        self.terminal_input(other.encode(), question=request)
                    # 只有当前问题的 Other 输入获得焦点后才粘贴；不向普通聊天输入框发送回答。
                    focused = self.read_question_page(request, index)
                    if page["multi"]:
                        # 原版多选的数字键只勾选、不移动焦点；先移到 Other 输入，离开输入后再核对勾选状态。
                        for _ in range(int(other) + 1):
                            selected = next((int(match[1]) for line in focused["lines"]
                                             if (match := re.match(r"^❯\s*([1-9])\.", line))), None)
                            if selected == int(other):
                                break
                            if selected is None:
                                raise NativeError("EQUESTIONCHANGED", "原生多选焦点已变化，未粘贴回答")
                            self.terminal_input(b"\x1b[B" if selected < int(other) else b"\x1b[A",
                                                question=request)
                            focused = self.read_question_page(request, index)
                    if not any(re.match(rf"^❯\s*{other}\.", line) for line in focused["lines"]):
                        raise NativeError("EQUESTIONCHANGED", "原生回答输入框没有获得焦点，未粘贴答案")
                    self.terminal_input(b"\x15\x1b[200~" + answer.encode() + b"\x1b[201~", question=request)
                    if not page["multi"]:
                        self.terminal_input(b"\r", question=request, final_question=single_final)
                    else:
                        self.submit_multi_question(request, index, [other])
                        answers[question["question"]] = (
                            json.dumps(answer, ensure_ascii=False) if ', ' in answer or '"' in answer else answer)
                if single_final:
                    self.question_reply = {"id": identity, "state": "submitted"}
                    return
                self.read_question_page(request, index + 1)
            page = self.read_question_page(request, len(questions))
            if not native_question_review_matches(page["review"], questions, answers):
                raise NativeError("EQUESTIONCHANGED", "原生汇总与本次回答不一致，未提交，请在 CLI 核对")
            # 多问必须核对完整汇总后一次提交；不用 reply(text)，该入口只回答第一问。
            self.terminal_input(b"1", question=request, final_question=True)
            self.question_reply = {"id": identity, "state": "submitted"}
        finally:
            if self.question_reply and self.question_reply.get("id") == identity:
                self.poll(force=True)

    def terminal_input(self, data, dialog=None, question=None, final_question=False, permission=None):
        job = self.lookup()
        if not job or job.get("outcome"):
            raise RuntimeError("原生 Claude 会话已停止")
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
            if dialog:
                self.verify_dialog(dialog)
                # 按键一旦开始写入就不自动重试；同一确认即使回执丢失，也不能再次执行。
                self.answered_dialog = dialog["id"]
                change = self.setting_changes.get(dialog["kind"])
                if change and change.get("dialogId") == dialog["id"]:
                    change["state"] = "confirming"
            if question:
                self.check_question(question)
                if final_question:
                    # 最后提交按键开始写入后即保留回执，断线或多端迟到点击不能再提交同一问题。
                    self.question_reply = {"id": question["nativeQuestion"]["id"], "state": "submitted"}
            if permission:
                current = self.lookup() or {}
                lines = self.read_terminal_screen()
                if (current.get("short") != permission["short"] or current.get("outcome")
                        or current.get("sessionStatus") not in ("idle", "busy") or current.get("needs")
                        or current.get("sessionStatus") == "busy" and not permission.get("allowBusy")
                        or not empty_permission_prompt(lines)
                        or permission_footer_mode(lines) != permission["mode"]):
                    raise NativeError("EPERMISSIONCHANGED", "原生输入或权限状态已变化，未发送切换按键")
            # 仅停止、审批和经逐步核对的空闲权限切换使用按键；普通消息和引导仍走原生消息入口。
            # 多段按键（如计划审批“编号 → 修改意见 → 回车”）在同一连接内依次发送，段间留出原生重绘时间。
            for index, chunk in enumerate(data if isinstance(data, list) else [data]):
                if index:
                    time.sleep(0.3)
                connection.sendall(chunk)
            time.sleep(0.15)
            return True

    def change_setting(self, name, value):
        old = getattr(self, name)
        pending = self.setting_changes.get(name) or {}
        if value == pending.get("requested") and pending.get("state") in ("pending", "confirmation", "confirming"):
            return
        # 用户选定 Max 后必须实际应用到同一原生会话；从默认空值切换也需要发送原生命令。
        if self.native_id and value != old:
            job = self.lookup()
            if job and not job.get("outcome"):
                command = "model" if name == "model" else "effort"
                # 模型设置也由原版命令入口决定立即应用还是排队，不增加网页忙碌锁。
                self.short = job["short"]
                self.read_messages()
                previous = self.actual_settings.get(name, old)
                issued_at = int(time.time() * 1000)
                self.reply(f"/{command} {value or 'auto'}", next_turn=False)
                # 命令受理不是设置生效；仅原生关联回显更新实际值，确认期间保留已收到的消息。
                self.actual_settings.setdefault(name, previous)
                self.setting_changes[name] = {"id": str(uuid.uuid4()), "requested": value,
                    "previous": previous, "state": "pending", "issuedAt": issued_at}
            else:
                self.actual_settings[name] = value
                self.setting_changes.pop(name, None)
        setattr(self, name, value)

    def answer(self, frame):
        response = frame["response"]
        request_id = response["request_id"]
        self.poll()
        request = self.requests.get(request_id)
        if not request:
            raise RuntimeError("该原生请求已经结束")
        answer = response["response"]
        if request.get("nativeQuestion"):
            if request["nativeQuestion"]["state"] != "waiting":
                raise NativeError("EQUESTIONCHANGED", "回答已提交，正在等待原生结果")
            self.answer_questions(request, answer.get("updatedInput", {}).get("answers", {}))
            return
        if request.get("subtype") == "native_dialog":
            dialog = request["dialog"]
            if not dialog["remoteSupported"] or dialog["state"] != "waiting" or request_id == self.answered_dialog:
                raise NativeError("EDIALOGCHANGED", "该确认已提交或需要在原版 Claude CLI 中处理")
            if dialog["kind"] == "plan":
                self.answer_plan(request_id, dialog, answer)
                return
            if dialog["kind"] == "rewind":
                self.answer_rewind(request_id, dialog, answer)
                return
            if answer.get("behavior") not in ("allow", "deny"):
                raise NativeError("EINVAL", "请选择确认切换或保留原设置")
            change = self.setting_changes.get(dialog["kind"])
            if change and change.get("dialogId") == dialog["id"]:
                change["decision"] = "accept" if answer["behavior"] == "allow" else "decline"
            try:
                self.terminal_input(b"1" if answer["behavior"] == "allow" else b"2", dialog=dialog)
                # 确认只操作原弹窗，不再发送原消息，也不伪造模型已经切换成功。
                self.answered_dialog = request_id
            finally:
                if self.answered_dialog == request_id:
                    self.poll(force=True)
            return
        if request["tool_name"] == "AskUserQuestion":
            values = answer.get("updatedInput", {}).get("answers", {})
            if len(request["input"].get("questions", [])) != 1:
                raise RuntimeError("多问题面板请在原版 Claude CLI 中回答")
            text = next(iter(values.values()), "")
            self.rpc({"op": "reply", "short": self.short, "text": text})
        else:
            self.terminal_input(b"1" if answer.get("behavior") == "allow" else b"\x1b")
        emit({"type": "control_cancel_request", "request_id": request_id})

    def answer_plan(self, request_id, dialog, answer):
        # 用户要求网页处理计划审批（交互见 native_plan_dialog）：只使用原生对话框列出的编号，不发送 Esc、不猜未知选项。
        # 修改意见是单行输入框，换行会提前提交，故合并为单行后再输入。
        options = {option["index"]: option for option in dialog.get("options", [])}
        if answer.get("behavior") == "allow":
            option = options.get(answer.get("option"))
            if not option or option["action"] != "approve":
                raise NativeError("EINVAL", "请选择原生审批框中的批准方式")
            keys = [str(option["index"]).encode()]
        elif answer.get("behavior") == "deny":
            option = next((item for item in options.values() if item["action"] == "feedback"), None)
            feedback = " ".join(str(answer.get("feedback") or "").split())
            if option is None:
                raise NativeError("EUNSUPPORTED", "当前原生审批框不支持从网页提交修改意见，请在 Claude CLI 中处理")
            if not feedback or len(feedback) > 4000:
                raise NativeError("EINVAL", "请填写修改意见（不超过 4000 字）")
            keys = [str(option["index"]).encode(), feedback.encode(), b"\r"]
        else:
            raise NativeError("EINVAL", "请选择批准方式或填写修改意见")
        try:
            self.terminal_input(keys, dialog=dialog)
            self.answered_dialog = request_id
        finally:
            if self.answered_dialog == request_id:
                self.poll(force=True)

    def answer_rewind(self, request_id, dialog, answer):
        # 用户要求网页只能关闭原版终端的回退菜单：等同在终端取消，不回退任何内容；确认回退只能在终端操作。
        # 2.1.323 实测：选消息页按 Esc 关闭整个菜单；确认页选 “Never mind” 或按 Esc 都只退回选消息页。
        # 因此确认页先选 “Never mind”，重新读取画面确认已回到选消息页后再按 Esc；每次发送前 verify_dialog 都核对画面。
        if answer.get("behavior") != "deny":
            raise NativeError("EUNSUPPORTED", "网页只能关闭终端回退菜单，确认回退请在原版 Claude CLI 中操作")
        sent = False
        try:
            if dialog.get("step") == "confirm":
                option = next((item for item in dialog.get("options", [])
                               if item["label"] == REWIND_CANCEL_OPTION), None)
                if option is None:
                    raise NativeError("EUNSUPPORTED", "回退确认页没有取消选项，请在原版 Claude CLI 中处理")
                self.terminal_input(str(option["index"]).encode(), dialog=dialog)
                self.answered_dialog, sent = request_id, True
                listing = self.wait_rewind_list()
                if listing is None:
                    return
                try:
                    self.terminal_input(b"\x1b", dialog=listing)
                    self.answered_dialog = listing["id"]
                except NativeError:
                    # 已退回选消息页；画面再次变化（如终端有人在操作）时不追加按键，网页按最新画面显示。
                    pass
                return
            self.terminal_input(b"\x1b", dialog=dialog)
            self.answered_dialog, sent = request_id, True
        finally:
            if sent:
                self.poll(force=True)

    def wait_rewind_list(self):
        # 只等 2 秒让回退菜单回到选消息页；等不到（菜单已关闭、画面变化或读取失败）就不再发送按键。
        deadline = time.monotonic() + 2
        while True:
            try:
                screen = self.read_terminal_screen()
            except NativeError:
                screen = []
            current = native_dialog(self.lookup() or {}, screen, self.native_id, self.root)
            if current and current["kind"] == "rewind" and current.get("step") == "select" and current["remoteSupported"]:
                return current
            if time.monotonic() >= deadline:
                return None
            time.sleep(0.2)

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
        previous_offset = self.log_offset
        stat = self.log_path.stat()
        identity = (stat.st_dev, stat.st_ino)
        # 用户要求重连补齐真实历史；原生日志替换或重写后不能沿用旧偏移，同一大小的新文件也须重新投影。
        if (self.log_identity is not None and identity != self.log_identity
                or stat.st_size < self.log_offset
                or self.log_mtime is not None and stat.st_size == self.log_offset
                and stat.st_mtime_ns != self.log_mtime):
            self.messages, self.all_messages, self.log_offset, self.seen = [], [], 0, set()
            self.history = NativeHistory()
            self.scheduled_fires = {}
            self.retired_usage = []
            previous_offset = -1
            self.preview_prompt_cache = None
            self.native_context = {}
            self.goal, self.goal_revision = None, ""
            self.goal_notice = ""
            active_compact = next((command for command in self.native_commands
                                   if command["name"] == "compact" and command["state"] in ("pending", "running", "stopping", "unknown")), None)
            self.compaction = {"state": active_compact["state"] if active_compact["state"] in ("pending", "unknown")
                               else "running" if self.compaction_screen_active else "unknown",
                               "itemId": active_compact["id"]} if active_compact else {"state": "idle"}
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
                self.history.observe(row)
                if (not self.session_cwd and not row.get("isSidechain") and isinstance(row.get("cwd"), str)
                        and row["cwd"].startswith("/")):
                    # 原生按会话启动目录保存记录；交接回退的 print 进程须在同一目录 --resume 才能找到原记录。
                    self.session_cwd = row["cwd"]
                if not row.get("isSidechain"):
                    self.observe_native_context(row)
                if not row.get("uuid") or row.get("isSidechain"):
                    continue
                if row.get("type") in ("user", "assistant") and not isinstance(row.get("message"), dict):
                    continue
                if row["uuid"] in self.seen:
                    continue
                self.observe_control_record(row)
                if row.get("type") == "user" and row.get("permissionMode") in (
                        "default", "manual", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions"):
                    self.actual_settings["permissionMode"] = row["permissionMode"]
                content = row.get("content") if row.get("type") == "system" else row["message"].get("content") \
                    if isinstance(row.get("message"), dict) else None
                if isinstance(content, str) and ("<command-name>" in content or "<local-command-" in content):
                    # 本地命令与输出须先标注再判断可编辑；父节点只在最近几条中查找，避免长历史逐行全量扫描。
                    parent = next((message for message in reversed(self.all_messages[-8:])
                                   if message.get("uuid") == row.get("parentUuid")), None)
                    local_command = native_local_command(row, parent)
                    if local_command:
                        row["nativeLocalCommand"] = local_command
                    local_output = native_local_output(row, parent)
                    if local_output:
                        row["nativeLocalOutput"] = local_output
                system_notice = native_system_notice(row)
                if system_notice:
                    row["nativeSystemNotice"] = system_notice
                if row.get("type") == "system" and row.get("subtype") == "scheduled_task_fire":
                    # 定时任务触发（2.1.323 实测）：system/scheduled_task_fire 后紧跟一条以它为父节点、
                    # 没有 origin 标记的 user 提示。用户要求按“定时任务触发”的自动续跑展示，不当作用户消息、不可编辑。
                    self.scheduled_fires[row["uuid"]] = str(row.get("content") or "").strip()[:200]
                notification = native_agent_notification(row)
                if notification:
                    row["nativeAgentNotification"] = notification
                # 须在判断可编辑之前标注，real_user 才能排除通知与定时任务提示。
                task_notification = native_task_notification(row) or self.scheduled_prompt(row)
                if task_notification:
                    row["nativeTaskNotification"] = task_notification
                native_editable = row.get("type") == "user" and real_user(row)
                if row.get("type") == "attachment":
                    attachment = row.get("attachment", {})
                    if attachment.get("type") == "goal_status":
                        row = {**row, "attachment": {key: attachment[key] for key in
                               ("type", "condition", "met", "sentinel", "reason") if key in attachment}}
                    elif (attachment.get("type") == "queued_command"
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
                if row.get("type") == "system" and row.get("subtype") not in ("turn_duration", "compact_boundary") \
                        and not (row.get("nativeLocalCommand") or row.get("nativeLocalOutput")
                                 or row.get("nativeSystemNotice")):
                    continue
                control = native_control_command(row)
                if control:
                    row["nativeControlCommand"] = control
                elif row.get("type") == "user" and isinstance(row.get("message", {}).get("content"), str):
                    output = re.fullmatch(r"\s*<local-command-stdout>([\s\S]*)</local-command-stdout>\s*",
                                          row["message"]["content"])
                    parent = next((message for message in reversed(self.messages)
                                   if message.get("uuid") == row.get("parentUuid")
                                   and message.get("nativeControlCommand")), None)
                    if output and parent:
                        row["nativeControlOutput"] = parent["nativeControlCommand"]
                setting = native_setting_command(row)
                if setting:
                    row["nativeSettingCommand"] = setting
                elif (native_setting_source(row)
                      and isinstance(row.get("message", {}).get("content"), str)
                      and re.fullmatch(r"\s*<local-command-stdout>[\s\S]*</local-command-stdout>\s*",
                                       row["message"]["content"])):
                    command = next((message for message in reversed(self.messages)
                                    if message.get("uuid") == row.get("parentUuid")
                                    and message.get("nativeSettingCommand")), None)
                    if command:
                        # 必须能关联到同一原生命令；孤立回显或用户引用不能按文本前缀误删。
                        row["nativeSettingOutput"] = {"commandId": command["uuid"],
                                                      **command["nativeSettingCommand"]}
                self.observe_setting_record(row)
                skill = native_skill_command(row)
                if skill:
                    row["nativeSkillCommand"] = skill
                self.seen.add(row["uuid"])
                # 原生回退菜单定位真实 user 节点；吸收在工具轮次中的 queued_command 仍保留正文，不伪造独立回退点。
                row["nativeEditable"] = native_editable and not row.get("nativeControlCommand")
                row["nativeOffset"] = position
                self.all_messages.append({key: row[key] for key in
                                      ("type", "uuid", "parentUuid", "interruptedMessageId", "timestamp", "message", "subtype", "isMeta",
                                       "isCompactSummary", "isVisibleInTranscriptOnly",
                                       "isApiErrorMessage", "isUnmetered", "apiBlockIndex", "durationMs",
                                       "requestedModel", "effort", "filename", "remote_upload",
                                       "nativeSettingCommand", "nativeSettingOutput", "nativeControlCommand",
                                       "nativeControlOutput", "attachment", "compactMetadata", "nativeEditable", "nativeOffset",
                                       "nativeSkillCommand", "promptId", "turnCompanion",
                                       "nativeLocalCommand", "nativeLocalOutput", "nativeSystemNotice")
                                      if key in row})
                self.messages = self.all_messages
                if notification:
                    self.messages[-1]["nativeAgentNotification"] = notification
                if task_notification:
                    self.messages[-1]["nativeTaskNotification"] = task_notification
                result = row.get("toolUseResult")
                if isinstance(result, dict) and NativeSubagents.valid_id(result.get("agentId") or result.get("resumedAgentId")):
                    # 原生 Agent 的受理回执足以关联卡片；不为更新主会话卡片扫描子代理文件或复制工具输出。
                    self.messages[-1]["nativeAgentResult"] = {key: result[key] for key in
                        ("agentId", "status", "resolvedModel", "totalDurationMs", "totalTokens",
                         "totalToolUseCount") if key in result}
                    if result.get("resumedAgentId"):
                        self.messages[-1]["nativeAgentResult"].update(
                            agentId=result["resumedAgentId"], status="async_launched", resumed=True)
                tool_result = native_tool_result(row)
                if tool_result:
                    self.messages[-1]["nativeToolResult"] = tool_result
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
        self.messages = self.history.select(self.all_messages)
        # 只在原生记录变化时核对 Skill 展开，不给每次 250ms 状态轮询增加历史扫描。
        if self.log_offset != previous_offset and normalize_skill_records(self.messages, self.pending):
            self.preview_context_cache = self.preview_prompt_cache = None
        if self.log_offset != previous_offset:
            # 只在原生日志新增/重写时更新失效分支的数字账本；正文、图片不重复传输，不增加发送前磁盘工作。
            active = {row["uuid"] for row in self.messages}
            self.retired_usage = [
                {"type": "assistant", "uuid": row["uuid"], "timestampMs": row.get("timestampMs"),
                 "isApiErrorMessage": row.get("isApiErrorMessage"),
                 "message": {key: row["message"][key] for key in ("id", "model", "usage") if key in row["message"]}}
                for row in self.all_messages if row["uuid"] not in active
                and row.get("type") == "assistant" and row.get("message", {}).get("usage")
            ]
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
            # 回退后当前分支的序号会变小；新增意图用原生追加偏移核对，不能因旧序号漏认新消息或认领旧同文消息。
            match = next((p for p in self.pending
                          if after_delivery_intent(row, index, p) and (text == p["_nativeText"]
                          or row.get("nativeSkillCommand", {}).get("accepted")
                          and text == skill_input_key(p["_nativeText"]))), None)
            if match:
                self.clients[row["uuid"]] = match["clientUserMessageId"]
                self.pending.remove(match)
                mapped.add(row["uuid"])
                self.retain_web_images(row, match)
            elif "web_images" not in row:
                self.retain_web_images(row)

    def preview_context(self):
        # 绑定当前原生用户和已落盘正文；工具状态、用量及 pending 变化不更换预览身份。
        # 按日志身份、修改时间及偏移缓存，避免每个终端片段重复扫描长历史。
        key = (self.native_id, self.log_identity, self.log_mtime, self.log_offset, len(self.messages))
        if self.preview_context_cache and self.preview_context_cache[0] == key:
            return self.preview_context_cache[1]
        start = next((index for index in range(len(self.messages) - 1, -1, -1)
                      if native_turn_start(self.messages[index])), None)
        result = None
        # 自动续跑轮次在终端不显示原始通知文本，无法定位回复区域；不预览，避免把新回复挂到上一轮。
        if start is not None and not real_user(self.messages[start]):
            start = None
        if start is not None:
            user, records, known = self.messages[start], [], []
            prompt = preview_prompt_key(user["message"]["content"])
            prompt_identity = (self.native_id, self.log_identity, user["uuid"], prompt)
            if self.preview_prompt_cache is None or self.preview_prompt_cache[0] != prompt_identity:
                # 历史序次只在切换用户输入或重读日志后计算一次，不在每个流式片段重复扫描长历史。
                occurrences = sum(real_user(row) and preview_prompt_key(row.get("message", {}).get("content")) == prompt
                                  for row in self.messages[:start + 1]) if prompt else 0
                self.preview_prompt_cache = (prompt_identity, occurrences)
            for row in self.messages[start + 1:]:
                message = row.get("message")
                if row.get("type") != "assistant" or not isinstance(message, dict):
                    continue
                content = message.get("content", [])
                if not isinstance(content, list):
                    continue
                texts = [part["text"] for part in content
                         if isinstance(part, dict) and part.get("type") == "text"
                         and isinstance(part.get("text"), str) and part["text"]]
                # 与正式投影一致：明确的原生合成占位不是正文，不能让它退役仍有效的预览。
                if message.get("model") == "<synthetic>" and row.get("isApiErrorMessage") is False \
                        and len(content) == 1 and len(texts) == 1 and texts[0].strip() == "No response requested.":
                    continue
                if texts:
                    records.append([row.get("uuid"), message.get("id"), row.get("apiBlockIndex"), texts])
                    known.extend(preview_text_key(text) for text in texts)
            basis = hashlib.sha256(json.dumps([user["uuid"], records], ensure_ascii=False,
                                               separators=(",", ":")).encode("utf-8")).hexdigest()
            result = (user, basis, known, prompt, self.preview_prompt_cache[1])
        self.preview_context_cache = (key, result)
        return result

    def stream_preview(self, chunk):
        self.screen.feed(chunk)
        self.preview_dirty = True
        self.flush_preview()

    def flush_preview(self):
        if not self.preview_dirty or self.job.get("tempo") != "active" or time.monotonic() - self.preview_at < 0.12:
            return
        context = self.preview_context()
        if context is None:
            return
        self.preview_dirty = False
        user, basis, known, prompt, occurrences = context
        if self.preview_user != user["uuid"] or self.preview_basis != basis:
            self.preview_user = user["uuid"]
            self.preview_basis = basis
            self.preview = self.preview_emitted = ""
        if native_turn_finished(self.messages):
            return
        lines = self.screen.text()
        region = terminal_reply_region(lines, prompt, occurrences)
        body = []
        last_line = max((index for index, line in enumerate(region) if line.strip()), default=-1)
        for index, line in enumerate(region):
            # 用户要求执行进度不能混入回复正文；CLI 的耗时后缀同属状态行，
            # 必须在追加预览前截断，否则后续真实回复会被“仅接受前缀增长”的保护拒绝。
            if re.match(r"^[✻✽✶✢·*]\s|^[─╌]{8}|^\s*⎿|^\s*ctrl\+", line) \
                    or terminal_progress_line(line, partial=index == last_line):
                break
            body.append(line)
        text = "\n".join(body).strip()
        # 64 KiB 与服务端临时预览限额一致，过大的终端片段由正式原生历史完整展示。
        if not text or len(text.encode("utf-8")) > 64 * 1024 or re.match(r"^[A-Z][A-Za-z0-9_]+\(", text):
            return
        compact_text = preview_text_key(text)
        if not compact_text or compact_text in prompt \
                or any(value and (compact_text in value or compact_text.startswith(value)) for value in known):
            return
        previous = preview_text_key(self.preview)
        if previous and (not compact_text.startswith(previous) or compact_text == previous):
            return
        self.preview = text
        if self.preview and self.preview != self.preview_emitted:
            self.preview_emitted = self.preview
            self.preview_at = time.monotonic()
            emit({"type": "shared_preview", "native_user_id": user["uuid"],
                  "preview_basis": basis, "text": self.preview})

    def poll(self, force=False):
        if not self.initialized or not self.native_id:
            return
        live = self.lookup()
        if live:
            self.bind(live)
        elif self.short:
            path = self.root / "jobs" / self.short / "state.json"
            if path.exists():
                self.job = native_activity(self.root, {**self.job, "short": self.short}, self.native_id)
        self.observe_control_screen()
        self.read_messages()
        job = self.job
        self.retry_permission_mode(live)
        # 正常连接与只读发现共用完成状态判断，done 不能因旧 tempo 继续显示执行中。
        status = NativeDiscovery.status(job, self.messages)
        # /compact 是本地原生命令，daemon 可能仍报 idle；只在原生画面明确显示压缩时补充执行状态。
        if self.compaction.get("state") == "running" and status.get("sessionStatus") != "waiting":
            status.update(tempo="active", state="compacting")
        # 后台任务属于会话进程：会话不在线时一定已结束；在线时按原生底栏汇报，画面不可读则不报告。
        status["sessionAlive"] = bool(live and not live.get("outcome"))
        if status["sessionAlive"]:
            background = self.background_summary(self.screen.text())
            if background is not None:
                status["background"] = background
        withdrawn = self.withdrawn_prompt(status)
        if withdrawn:
            status["withdrawnPrompt"] = withdrawn
        self.observe_dialog(job)
        self.requests = {}
        question = native_question_request(job, self.native_id)
        if question:
            self.requests = self.question_requests(question)
        elif self.dialog:
            self.requests[self.dialog["id"]] = {"subtype": "native_dialog", "dialog": self.dialog}
        elif status.get("tempo") == "blocked" and job.get("needs") != "dialog open":
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
        preview = self.preview_context()
        signature = (self.log_identity, self.log_mtime, self.log_offset, json.dumps(status, sort_keys=True),
                     json.dumps(self.pending, sort_keys=True), json.dumps(context, sort_keys=True),
                     json.dumps(self.requests, sort_keys=True),
                     json.dumps(self.control_projection(), sort_keys=True))
        if force or signature != self.signature:
            self.signature = signature
            emit({"type": "shared_snapshot", "native_id": self.native_id, "job_id": self.short,
                  "native_status": status, "messages": self.messages,
                  "history_head": self.history.current_head(),
                  "retired_usage": self.retired_usage,
                  "clients": self.clients, "pending": self.pending, "requests": self.requests,
                  "preview_basis": preview[1] if preview else None,
                  "context_configuration": context, "native_control": self.control_projection()})
        # 120ms 节流内到达的最后一段必须补发；复用现有 250ms 轮询，不新增线程或读取。
        # 先发布正式快照，让接收端建立同一轮次/正文基线，再补充仍有效的临时预览。
        self.flush_preview()

    def control(self, request):
        subtype = request.get("subtype")
        if subtype == "initialize":
            self.model = request.get("model", "")
            self.effort = request.get("effort", "")
            self.permission_mode = request.get("permission_mode")
            self.permission_catalog = request["catalog"].get("permissionModes", [])
            self.setting_catalog = request["catalog"].get("models", [])
            self.restore_settings(request.get("native_control") or {})
            self.restore_question_reply(request.get("native_control") or {})
            self.clients = request.get("clients", {})
            self.pending = request.get("pending", [])
            self.native_commands = (request.get("native_control") or {}).get("commands", [])
            previous_compaction = (request.get("native_control") or {}).get("compaction") or {}
            if previous_compaction.get("state") in ("pending", "running", "unknown"):
                self.compaction = {**previous_compaction, "state": "unknown"}
            self.restore_compaction_events(request.get("native_control") or {})
            commands = {command.get("name") for command in request["catalog"].get("commands", [])}
            self.native_capabilities = {name: name in commands for name in ("goal", "compact")}
            self.initialized = True
            return {**request["catalog"], "contextConfiguration": self.context_configuration()}
        if subtype == "shared_send":
            return self.send(request)
        if subtype == "shared_edit_message":
            return self.edit_message(request)
        if subtype == "shared_native_command":
            return self.native_command(request)
        if subtype == "shared_sync":
            self.poll(force=True)
        elif subtype == "set_model":
            self.change_setting("model", request.get("model", ""))
        elif subtype == "set_permission_mode":
            return self.change_permission_mode(request.get("mode"))
        elif subtype == "apply_flag_settings":
            self.change_setting("effort", request.get("settings", {}).get("effortLevel") or "")
        elif subtype == "interrupt":
            self.terminal_input(b"\x1b")
            if self.compaction.get("state") == "running":
                event = self.compaction_events.get(self.compaction_event_id)
                if event and event.get("status") == "inProgress":
                    event["phase"] = "stopping"
                for command in self.native_commands:
                    if command["name"] == "compact" and command["state"] == "running":
                        command["state"] = "stopping"
        elif subtype == "shared_answer":
            self.answer({"response": request["response"]})
        else:
            raise RuntimeError("此原生 Claude 控制操作尚未适配")
        return {"nativeSettings": self.settings_projection()}


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
            # 用户要求接收确认不能倒退为未发送；shared_send 里的后续状态查询也可能返回 NativeError。
            # 只有发送函数明确保留“尚未准入”标记才可判为未投递，已开始或已接收后的错误只能待核对。
            not_sent = not native.delivery_started if frame["request"].get("subtype") in ("shared_send", "shared_native_command", "shared_edit_message") \
                else isinstance(failure, NativeError)
            response = {"subtype": "error", "request_id": request_id, "error": str(failure),
                        "not_sent": not_sent}
            if isinstance(failure, NativeError):
                response["code"] = failure.code
        emit({"type": "control_response", "response": response})


if __name__ == "__main__":
    main()
