"""Local, tool-using Codex prompt comparison. Credentials are read only in memory."""

from __future__ import annotations

import argparse
import copy
import hashlib
import http.client
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
SCRATCH = REPO / "temp" / "prompt-ab-2026-10-10"
USER_CODEX = Path.home() / ".codex"
CLI = Path.home() / "AppData/Local/OpenAI/Codex/bin/9691020b546a15b2/codex.exe"
MODEL = "gpt-6-astra"
EFFORT = "high"
PROTOCOL_VERSION = 4
UPSTREAM = "http://127.0.0.1:45221"
TRIALS: dict[str, dict] = {}
LOCK = threading.Lock()


def digest(value):
    if not isinstance(value, str):
        value = json.dumps(value, ensure_ascii=False, sort_keys=True)
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def save_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def strip_system(value):
    if isinstance(value, list):
        return [strip_system(x) for x in value
                if not (isinstance(x, dict) and x.get("role") in ("system", "developer"))]
    if isinstance(value, dict):
        if value.get("role") in ("system", "developer"):
            return {}
        return {k: strip_system(v) for k, v in value.items()}
    return value


class Relay(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def do_POST(self):
        trial_id, route = self.path.lstrip("/").split("/", 1)
        trial = TRIALS[trial_id]
        original = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        # Equal harness control for both arms: exclude host skill catalogs and delegation.
        original["tools"] = [t for t in original.get("tools", []) if t.get("name") != "collaboration"]
        common_input = []
        for item in original.get("input", []):
            if item.get("role") == "developer" and isinstance(item.get("content"), list):
                item["content"] = [c for c in item["content"]
                                   if not c.get("text", "").startswith(
                                       ("<skills_instructions>", "<multi_agent_role>", "<multi_agent_mode>"))]
                if not item["content"]:
                    continue
            common_input.append(item)
        original["input"] = common_input
        if not trial["requests"]:
            save_json(HERE / "runs" / trial_id / "initial_runtime_messages.json",
                      [x for x in original.get("input", []) if x.get("role") in ("system", "developer")])
            save_json(HERE / "runs" / trial_id / "initial_user_messages.json",
                      [x for x in original.get("input", []) if x.get("role") == "user"])
        request = copy.deepcopy(original)
        if trial["arm"] in ("draft", "draft_keep_runtime"):
            request["instructions"] = (HERE / "draft.snapshot.md").read_text(encoding="utf-8").strip()
            if trial["arm"] == "draft":
                request["input"] = strip_system(request.get("input", []))
        roles = [x.get("role", x.get("type")) for x in request.get("input", []) if isinstance(x, dict)]
        record = {
            "model": request.get("model"), "reasoning": request.get("reasoning"),
            "instructions_sha256": digest(request.get("instructions", "")),
            "instructions_chars": len(request.get("instructions", "")),
            "original_instructions_sha256": digest(original.get("instructions", "")),
            "tools_sha256": digest(request.get("tools", [])),
            "tool_names": [t.get("name", t.get("type")) for t in request.get("tools", [])],
            "roles": roles, "input_sha256": digest(request.get("input", [])),
            "original_input_sha256": digest(original.get("input", [])),
            "runtime_messages_removed": sum(
                x.get("role") in ("system", "developer") for x in original.get("input", [])
                if isinstance(x, dict)
            ) if trial["arm"] == "draft" else 0,
            "requested_service_tier": request.get("service_tier"),
        }
        with LOCK:
            trial["requests"].append(record)
        body = json.dumps(request, ensure_ascii=False).encode("utf-8")
        upstream = urlsplit(UPSTREAM)
        headers = {k: v for k, v in self.headers.items()
                   if k.lower() not in ("host", "content-length", "connection", "accept-encoding")}
        headers["Content-Length"] = str(len(body))
        headers["Accept-Encoding"] = "identity"
        connection = http.client.HTTPConnection(upstream.hostname, upstream.port, timeout=600)
        started = time.monotonic()
        response_buffer = b""
        try:
            connection.request("POST", "/" + route, body, headers)
            response = connection.getresponse()
            record["http_status"] = response.status
            self.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() in ("content-type", "x-request-id"):
                    self.send_header(key, value)
            self.send_header("Connection", "close")
            self.end_headers()
            while True:
                block = response.read1(65536)
                if not block:
                    break
                self.wfile.write(block)
                self.wfile.flush()
                response_buffer += block
                while b"\n" in response_buffer:
                    line, response_buffer = response_buffer.split(b"\n", 1)
                    if line.startswith(b"data: "):
                        try:
                            event = json.loads(line[6:])
                        except (ValueError, UnicodeDecodeError):
                            continue
                        if event.get("type") in ("response.completed", "response.failed", "response.incomplete"):
                            result = event.get("response", {})
                            record["response_model"] = result.get("model")
                            record["usage"] = result.get("usage")
                            record["response_status"] = result.get("status")
                            if result.get("error"):
                                record["response_error"] = result["error"]
        except Exception as exc:
            record["transport_error"] = type(exc).__name__
        finally:
            record["duration_seconds"] = round(time.monotonic() - started, 3)
            connection.close()
            self.close_connection = True
            save_json(HERE / "runs" / trial_id / "requests.json", trial["requests"])


def snapshot_inputs():
    SCRATCH.mkdir(parents=True, exist_ok=True)
    catalog = json.loads((USER_CODEX / "codex-elves-model-catalog.json").read_text(encoding="utf-8"))
    model = next(x for x in catalog["models"] if x["slug"] == MODEL)
    (HERE / "default.snapshot.md").write_text(model["base_instructions"], encoding="utf-8")
    draft = (REPO / "docs/prompts/claude-opus-5.5-codex-system-prompt.draft.md").read_text(encoding="utf-8")
    (HERE / "draft.snapshot.md").write_text(draft, encoding="utf-8")
    selected = copy.deepcopy(model)
    selected["prefer_websockets"] = False
    catalog_path = SCRATCH / "catalog.json"
    save_json(catalog_path, {"models": [selected]})
    metadata = {
        "date": "2026-10-10", "protocol_version": PROTOCOL_VERSION, "model": MODEL, "effort": EFFORT,
        "cli_version": subprocess.check_output([str(CLI), "--version"], text=True).strip(),
        "default_sha256": digest(model["base_instructions"]),
        "draft_sha256": digest(draft.strip()),
        "default_chars": len(model["base_instructions"]),
        "draft_chars": len(draft.strip()),
        "draft_mode": "Replicate CodexElves override: replace instructions; recursively remove system/developer input.",
        "source_catalog_sha256": digest((USER_CODEX / "codex-elves-model-catalog.json").read_text(encoding="utf-8")),
        "default_matches_model_template": model["base_instructions"] == model["model_messages"]["instructions_template"],
        "common_controls": "Remove skill catalog, multi-agent role/mode text and collaboration tool identically in both arms; no AGENTS, plugins, MCP or memories.",
        "provider": "Existing local CodexElves relay; no production config modifications",
    }
    save_json(HERE / "metadata.json", metadata)
    return catalog_path


def run_trial(case, arm, repeat, port, catalog_path, api_key, timeout=600):
    trial_id = f"v{PROTOCOL_VERSION}-{case['id']}-{arm}-r{repeat}"
    work = SCRATCH / trial_id / "workspace"
    home = SCRATCH / "isolated-codex-home"
    output = HERE / "runs" / trial_id
    for path in (work, home, output):
        path.mkdir(parents=True, exist_ok=True)
    for name, text in case["files"].items():
        target = work / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(work)], check=True, capture_output=True)
    # An isolated Git root prevents unrelated parent project instructions entering either arm.
    before = {name: digest(text) for name, text in case["files"].items()}
    TRIALS[trial_id] = {"arm": arm, "requests": []}
    provider = (
        '{name="Prompt AB",wire_api="responses",'
        f'base_url="http://127.0.0.1:{port}/{trial_id}/v1",'
        'env_key="PROMPT_AB_API_KEY"}'
    )
    skill_paths = list((Path.home() / ".agents/skills").rglob("SKILL.md"))
    skills_config = "[" + ",".join(
        '{path=' + json.dumps(str(path.parent)) + ',enabled=false}' for path in skill_paths
    ) + "]"
    command = [
        str(CLI), "exec", "--ignore-user-config", "--ephemeral", "--json",
        "--color", "never", "--skip-git-repo-check",
        "-C", str(work), "-m", MODEL, "-s", "workspace-write",
        "-c", 'approval_policy="never"', "-c", 'windows.sandbox="unelevated"',
        "-c", f'model_reasoning_effort="{EFFORT}"',
        "-c", "model_provider=\"ab\"", "-c", f"model_providers.ab={provider}",
        "-c", f"model_catalog_json={json.dumps(str(catalog_path))}",
        "-c", "project_doc_max_bytes=0", "-c", "features.multi_agent=false",
        "-c", "features.multi_agent_v2=false", "-c", "features.goals=false",
        "-c", "features.plugins=false", "-c", "features.apps=false",
        "-c", "features.skip_host_skill_discovery=true", "-c", "features.shell_snapshot=false",
        "-c", "features.memories=false", "-c", "features.hooks=false",
        "-c", f"skills.config={skills_config}",
        "-c", "shell_environment_policy.ignore_default_excludes=false",
        "-c", "web_search=\"disabled\"", "-o", str(output / "final.txt"), "-",
    ]
    environment = dict(os.environ)
    environment["CODEX_HOME"] = str(home)
    environment["PROMPT_AB_API_KEY"] = api_key
    prompt = case["prompt"]
    (output / "prompt.txt").write_text(prompt, encoding="utf-8")
    started = time.monotonic()
    timed_out = False
    with (output / "events.jsonl").open("w", encoding="utf-8") as stdout, \
         (output / "stderr.txt").open("w", encoding="utf-8") as stderr:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=stdout, stderr=stderr,
                                   env=environment, text=True, encoding="utf-8",
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            process.communicate(prompt, timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            # Kill only this benchmark's own child tree.
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           capture_output=True, check=False)
            process.wait(timeout=30)
    duration = round(time.monotonic() - started, 3)
    events = []
    for line in (output / "events.jsonl").read_text(encoding="utf-8").splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            pass
    completed = [e["item"] for e in events if e.get("type") == "item.completed"]
    final = (output / "final.txt").read_text(encoding="utf-8") if (output / "final.txt").exists() else ""
    changed = []
    for path in work.rglob("*"):
        if path.is_file() and ".git" not in path.relative_to(work).parts and "__pycache__" not in path.parts:
            name = path.relative_to(work).as_posix()
            if before.get(name) != digest(path.read_text(encoding="utf-8", errors="replace")):
                changed.append(name)
                target = output / "files" / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(path.read_bytes())
    changed += [name for name in before if not (work / name).exists()]
    result = {
        "trial_id": trial_id, "protocol_version": PROTOCOL_VERSION, "case": case["id"], "arm": arm, "repeat": repeat,
        "exit_code": process.returncode, "timed_out": timed_out, "seconds": duration,
        "turn_completed": any(e.get("type") == "turn.completed" for e in events),
        "usage": next((e.get("usage") for e in reversed(events) if e.get("type") == "turn.completed"), None),
        "tool_count": sum(x.get("type") in ("command_execution", "file_change", "mcp_tool_call", "web_search") for x in completed),
        "command_count": sum(x.get("type") == "command_execution" for x in completed),
        "changed_files": sorted(set(changed)), "final_chars": len(final),
        "request_count": len(TRIALS[trial_id]["requests"]),
        "response_models": sorted({r.get("response_model") for r in TRIALS[trial_id]["requests"] if r.get("response_model")}),
    }
    if "evaluate" in case:
        result["grading"] = case["evaluate"](work, final, completed)
    save_json(output / "result.json", result)
    print(json.dumps(result, ensure_ascii=False), flush=True)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--smoke", action="store_true")
    parser.add_argument("--cases", nargs="*")
    parser.add_argument("--arms", nargs="+", default=["default", "draft"])
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--workers", type=int, default=2)
    options = parser.parse_args()
    catalog_path = snapshot_inputs()
    api_key = json.loads((USER_CODEX / "auth.json").read_text(encoding="utf-8"))["OPENAI_API_KEY"]
    server = ThreadingHTTPServer(("127.0.0.1", 0), Relay)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        if options.smoke:
            smoke = {"id": "smoke", "files": {"value.txt": "19\n"},
                     "prompt": "读取当前目录 value.txt，报告其中整数乘以 7 的结果。不要修改文件。"}
            for arm in options.arms:
                run_trial(smoke, arm, 1, server.server_port, catalog_path, api_key, timeout=180)
        else:
            from cases import CASES
            from concurrent.futures import ThreadPoolExecutor
            selected = [case for case in CASES if not options.cases or case["id"] in options.cases]
            jobs = []
            for repeat in range(1, options.repeats + 1):
                for index, case in enumerate(selected):
                    arms = options.arms if (repeat + index) % 2 else list(reversed(options.arms))
                    for arm in arms:
                        if not (HERE / "runs" / f"v{PROTOCOL_VERSION}-{case['id']}-{arm}-r{repeat}" / "result.json").exists():
                            jobs.append((case, arm, repeat))
            with ThreadPoolExecutor(max_workers=options.workers) as pool:
                futures = [pool.submit(run_trial, case, arm, repeat, server.server_port,
                                       catalog_path, api_key) for case, arm, repeat in jobs]
                for future in futures:
                    future.result()
    finally:
        server.shutdown()


if __name__ == "__main__":
    main()
