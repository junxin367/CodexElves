"""原生执行中吸收队列消息的回归样例；只读取临时日志，不连接或启动 Claude。"""
import json
import contextlib
import io
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch


source = Path(__file__).resolve().parents[1] / "crates/codex-elves-core/src/remote_access/claude_shared.py"
namespace = {"__name__": "claude_shared_history_test"}
exec(compile(source.read_text(encoding="utf-8"), str(source), "exec"), namespace)
NativeSession = namespace["NativeSession"]
NativeDiscovery = namespace["NativeDiscovery"]
trust_source = source.with_name("claude_project_trust.py")
trust_namespace = {"__name__": "claude_project_trust_test"}
exec(compile(trust_source.read_text(encoding="utf-8"), str(trust_source), "exec"), trust_namespace)


class SharedQueueHistoryTests(unittest.TestCase):
    def test_completed_native_job_clears_stale_working_and_permission_states(self):
        native = NativeSession("unused", "test-session", "saved-job")
        native.initialized = True
        native.read_messages = lambda: None
        native.context_configuration = lambda: {}
        native.messages = [{"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "old-tool", "name": "Write", "input": {}}]}}]
        for tempo in ["active", "blocked"]:
            native.lookup = lambda: {"short": "saved-job", "sessionId": "test-session",
                                     "state": "done", "tempo": tempo, "needs": "permission"}
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                native.poll(force=True)
            frame = json.loads(output.getvalue())
            self.assertEqual(frame["native_status"]["tempo"], "idle")
            self.assertEqual(frame["requests"], {})

    def test_terminal_handshake_disconnect_and_oversize_do_not_send_keys_or_loop(self):
        native = NativeSession("unused", "test-session", "saved-job")
        native.lookup = lambda: {"tempo": "active"}
        for reply, pattern in [(b"", "握手期间断开"), (b"x", "响应过大")]:
            connection = MagicMock()
            connection.__enter__.return_value = connection
            connection.recv.return_value = reply
            with self.subTest(reply=reply), patch.object(namespace["socket"], "socket", return_value=connection), \
                    patch.object(Path, "read_text", return_value="fixture-auth"):
                with self.assertRaisesRegex(RuntimeError, pattern):
                    native.terminal_input(b"\x1b", require_active=True)
            self.assertEqual(connection.sendall.call_count, 1, "只能发送握手，不能发送交互按键")
            self.assertLessEqual(connection.recv.call_count, 64 * 1024)

    def test_terminal_handshake_success_sends_only_requested_key(self):
        native = NativeSession("unused", "test-session", "saved-job")
        native.lookup = lambda: {"tempo": "active"}
        connection = MagicMock()
        connection.__enter__.return_value = connection
        connection.recv.side_effect = [bytes([byte]) for byte in b'{"ok":true}\n']
        with patch.object(namespace["socket"], "socket", return_value=connection), \
                patch.object(Path, "read_text", return_value="fixture-auth"), \
                patch.object(namespace["time"], "sleep"):
            native.terminal_input(b"\x1b", require_active=True)
        self.assertEqual(connection.sendall.call_args_list[-1].args[0], b"\x1b")
        self.assertEqual(connection.sendall.call_count, 2)

    def test_history_replacement_with_same_length_reloads_native_records(self):
        native = self.read([{"type": "user", "uuid": "user-one", "message": {"content": "first"}}])
        original_size = native.log_path.stat().st_size
        replacement = native.log_path.with_suffix(".replacement")
        replacement.write_text(json.dumps(
            {"type": "user", "uuid": "user-two", "message": {"content": "other"}}) + "\n", encoding="utf-8")
        self.assertEqual(replacement.stat().st_size, original_size)
        replacement.replace(native.log_path)
        native.read_messages()
        self.assertEqual([row["uuid"] for row in native.messages], ["user-two"])
        with native.log_path.open("a", encoding="utf-8") as stream:
            stream.write(json.dumps({"type": "assistant", "uuid": "reply",
                                      "message": {"content": [{"type": "text", "text": "new"}]}}) + "\n")
        native.read_messages()
        self.assertEqual([row["uuid"] for row in native.messages], ["user-two", "reply"])
        native.read_messages()
        self.assertEqual(len(native.messages), 2)

    def test_invalid_metadata_does_not_prevent_later_history_from_loading(self):
        native = self.read([
            None, ["auxiliary"], {"type": "user", "uuid": "diagnostic", "message": None},
            {"type": "user", "uuid": "valid", "timestamp": "invalid-time", "message": {"content": "问题"}},
            {"type": "assistant", "uuid": "answer", "timestamp": 1794416642798,
             "message": {"content": [{"type": "text", "text": "回复"}]}},
        ])
        self.assertEqual([row["uuid"] for row in native.messages], ["valid", "answer"])
        self.assertEqual(native.messages[0]["timestampMs"], 0)
        self.assertEqual(native.messages[1]["timestampMs"], 1794416642798)

    def native_active(self):
        native = NativeSession("unused", "test-session", "saved-job")
        native.messages = [{"type": "user", "uuid": "active-user", "message": {"content": "正在处理"}}]
        native.read_messages = lambda: None
        native.poll = lambda **kwargs: None
        native.lookup = lambda: {"short": "saved-job", "sessionId": "test-session",
                                 "tempo": "active"}
        return native

    def test_idle_only_delivery_defers_when_cli_started_work_without_touching_input(self):
        native = self.native_active()
        replies = []
        native.reply = lambda *args, **kwargs: replies.append(args)
        result = native.send({"client_id": "C", "idle_only": True,
                              "content": [{"type": "text", "text": "只投递 C"}]})
        self.assertTrue(result["deferred"])
        self.assertFalse(result["accepted"])
        self.assertEqual(replies, [])
        self.assertEqual(native.pending, [])
        self.assertFalse(native.delivery_started)

    def test_completed_turn_waiting_for_user_reply_sends_directly(self):
        native = self.native_active()
        native.messages.append({"type": "system", "subtype": "turn_duration"})
        job = {"short": "saved-job", "sessionId": "test-session",
               "tempo": "blocked", "state": "blocked", "needs": "Waiting for the user's next answer"}
        native.lookup = lambda: job
        replies = []
        native.reply = lambda text, **kwargs: replies.append(text)
        with contextlib.redirect_stdout(io.StringIO()):
            result = native.send({"client_id": "after-completion", "idle_only": True,
                                  "content": [{"type": "text", "text": "continue"}]})
        self.assertTrue(result.get("accepted"), "完成后的澄清等待不能被误判为执行中")
        self.assertEqual(replies, ["continue"])
        self.assertEqual(NativeDiscovery.status(job, native.messages)["tempo"], "idle")

    def test_new_user_after_completion_still_keeps_real_permission_wait_blocked(self):
        native = self.native_active()
        native.messages.extend([
            {"type": "system", "subtype": "turn_duration"},
            {"type": "user", "uuid": "new-native", "message": {"content": "new task"}},
            {"type": "assistant", "message": {"content": [
                {"type": "tool_use", "id": "permission-tool", "name": "Bash", "input": {}}]}},
        ])
        job = {"short": "saved-job", "sessionId": "test-session",
               "tempo": "blocked", "needs": "permission"}
        native.lookup = lambda: job
        native.reply = MagicMock()
        result = native.send({"client_id": "waiting", "idle_only": True,
                              "content": [{"type": "text", "text": "next"}]})
        self.assertTrue(result["deferred"])
        native.reply.assert_not_called()

    def test_steering_an_idle_queue_does_not_interrupt_the_new_turn(self):
        native = self.native_active()
        native.lookup = lambda: {"short": "saved-job", "sessionId": "test-session", "tempo": "idle"}
        replies, keys = [], []
        def reply(text, **kwargs):
            replies.append(text)
            native.job["tempo"] = "active"
        native.reply = reply
        native.terminal_input = lambda data, **kwargs: keys.append(data)
        with contextlib.redirect_stdout(io.StringIO()):
            result = native.send({"client_id": "idle-steer", "steer": True,
                                  "content": [{"type": "text", "text": "start new work"}]})
        self.assertTrue(result["accepted"])
        self.assertFalse(result["steered"])
        self.assertEqual(replies, ["start new work"])
        self.assertEqual(keys, [], "不能把本次发送启动的轮次当成需要中断的旧轮次")

    def test_steer_does_not_interrupt_a_different_turn_started_during_delivery(self):
        native = self.native_active()
        def reply(text, **kwargs):
            native.messages.append({"type": "user", "uuid": "other-turn", "message": {"content": "another input"}})
        native.reply = reply
        native.terminal_input = MagicMock()
        with contextlib.redirect_stdout(io.StringIO()):
            result = native.send({"client_id": "selected", "steer": True,
                                  "content": [{"type": "text", "text": "selected input"}]})
        self.assertTrue(result["accepted"])
        self.assertFalse(result["steered"])
        native.terminal_input.assert_not_called()

    def test_interrupted_turn_waiting_for_user_can_accept_a_new_message(self):
        native = self.native_active()
        native.messages.append({"type": "user", "uuid": "interrupt", "interruptedMessageId": "response",
                                "message": {"content": [{"type": "text", "text": "[Request interrupted by user]"}]}})
        native.lookup = lambda: {"short": "saved-job", "sessionId": "test-session",
                                 "tempo": "blocked", "needs": "Waiting for a new instruction"}
        native.reply = MagicMock()
        with contextlib.redirect_stdout(io.StringIO()):
            result = native.send({"client_id": "after-stop", "idle_only": True,
                                  "content": [{"type": "text", "text": "new instruction"}]})
        self.assertTrue(result.get("accepted"))
        native.reply.assert_called_once()

    def test_status_error_after_native_acceptance_cannot_claim_not_sent(self):
        native = self.native_active()
        native.reply = MagicMock()
        native.poll = lambda **kwargs: (_ for _ in ()).throw(namespace["NativeError"]("ESTATUS"))
        incoming = namespace["queue"].Queue()
        incoming.put(json.dumps({"type": "control_request", "request_id": "fixture",
                                 "request": {"subtype": "shared_send", "client_id": "accepted-before-status",
                                             "content": [{"type": "text", "text": "one input"}]}}))
        incoming.put(None)
        output = io.StringIO()
        with patch.dict(namespace, {"NativeSession": lambda *args: native}), \
                patch.object(namespace["argparse"].ArgumentParser, "parse_args",
                             return_value=SimpleNamespace(cli="unused", native_id="", job_id="", discover=False)), \
                patch.object(namespace["queue"], "Queue", return_value=incoming), \
                patch.object(namespace["threading"], "Thread"), contextlib.redirect_stdout(output):
            namespace["main"]()
        response = [json.loads(line) for line in output.getvalue().splitlines()
                    if json.loads(line).get("type") == "control_response"][-1]["response"]
        self.assertFalse(response["not_sent"], "已经被原生接收后，状态查询失败只能待核对，不能允许作为未发送重试")
        native.reply.assert_called_once()
        self.assertTrue(native.pending[0]["deliveryConfirmed"])

    def preview_frames(self, native, screen):
        native.preview_at = 0
        native.job.setdefault("tempo", "active")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            native.stream_preview("\x1b[2J\x1b[H" + screen.replace("\n", "\r\n"))
        return [json.loads(line) for line in output.getvalue().splitlines()
                if json.loads(line).get("type") == "shared_preview"]

    def test_terminal_preview_does_not_repeat_markdown_or_render_tool_status(self):
        native = self.native_active()
        native.messages[0]["message"]["content"] = "fixture prompt"
        native.messages.append({"type": "assistant", "uuid": "reply-row", "message": {
            "id": "reply", "content": [{"type": "text", "text": "Use `Example <dev@example.test>` now."}]}})
        self.assertEqual(self.preview_frames(native,
            "❯ fixture prompt\n● Use Example dev@example.test now.\n  Running 3 shell commands...\n"), [])
        self.assertEqual(self.preview_frames(native,
            "❯ fixture prompt\n● Running 3 shell commands...\n"), [])
        self.assertEqual(self.preview_frames(native, "  An unrelated terminal fragment\n"), [])

    def test_terminal_preview_never_shrinks_on_redraw_and_retires_after_native_text_arrives(self):
        native = self.native_active()
        native.messages[0]["message"]["content"] = "fixture prompt"
        first = self.preview_frames(native, "❯ fixture prompt\n● A partial response\n")
        self.assertEqual(first[-1]["text"], "A partial response")
        self.assertEqual(self.preview_frames(native, "❯ fixture prompt\n● A partial\n"), [])
        grown = self.preview_frames(native, "❯ fixture prompt\n● A partial response continues\n")
        self.assertEqual(grown[-1]["text"], "A partial response continues")
        self.assertEqual(first[-1]["preview_basis"], grown[-1]["preview_basis"])
        native.messages.append({"type": "assistant", "uuid": "reply-row", "message": {
            "id": "reply", "content": [{"type": "text", "text": "A **partial** response continues"}]}})
        self.assertEqual(self.preview_frames(native,
            "❯ fixture prompt\n● A partial response continues\n"), [])

    def test_preview_identity_is_scoped_to_the_current_native_user(self):
        native = self.native_active()
        native.messages.extend([
            {"type": "assistant", "message": {"id": "old-reply", "content": [
                {"type": "text", "text": "Repeated answer"}]}},
            {"type": "system", "subtype": "turn_duration"},
            {"type": "user", "uuid": "next-user", "message": {"content": "next question"}},
        ])
        frames = self.preview_frames(native, "❯ next question\n● Repeated answer\n")
        self.assertEqual(frames[-1]["native_user_id"], "next-user")
        self.assertEqual(frames[-1]["text"], "Repeated answer")

    def test_preview_basis_changes_for_real_text_not_tool_metadata_or_synthetic_placeholders(self):
        native = self.native_active()
        basis = native.preview_context()[1]
        native.messages.append({"type": "assistant", "uuid": "tool-row", "message": {
            "id": "tool-call", "content": [{"type": "tool_use", "id": "tool", "name": "Read", "input": {}}]}})
        self.assertEqual(native.preview_context()[1], basis)
        native.messages.append({"type": "assistant", "uuid": "placeholder", "isApiErrorMessage": False,
                                "message": {"id": "placeholder", "model": "<synthetic>",
                                            "content": [{"type": "text", "text": "No response requested."}]}})
        self.assertEqual(native.preview_context()[1], basis)
        native.messages.append({"type": "assistant", "uuid": "real-text", "message": {
            "id": "real-text", "model": "real-model",
            "content": [{"type": "text", "text": "No response requested."}]}})
        self.assertNotEqual(native.preview_context()[1], basis, "真实正文不能按同名文本隐藏")

    def test_single_steer_delivers_only_selected_message_and_preserves_draft(self):
        native = self.native_active()
        replies, keys = [], []
        native.reply = lambda text, **kwargs: replies.append(text)
        native.terminal_input = lambda data, **kwargs: keys.append((data, kwargs))
        with contextlib.redirect_stdout(io.StringIO()):
            result = native.send({"client_id": "C", "steer": True,
                                  "content": [{"type": "text", "text": "只投递 C"}]})
        self.assertEqual(replies, ["只投递 C"])
        self.assertEqual(keys, [(b"\x1b", {"require_active": True})])
        self.assertTrue(result["accepted"])
        self.assertEqual(result["interruptedUserId"], "active-user")
        self.assertTrue(native.pending[0]["deliveryConfirmed"])

    def test_interrupt_failure_after_acceptance_never_reports_message_as_unsent(self):
        native = self.native_active()
        native.reply = lambda *args, **kwargs: None
        native.terminal_input = lambda *args, **kwargs: (_ for _ in ()).throw(RuntimeError("已进入问答"))
        with contextlib.redirect_stdout(io.StringIO()):
            result = native.send({"client_id": "C", "steer": True,
                                  "content": [{"type": "text", "text": "只投递 C"}]})
        self.assertTrue(result["accepted"])
        self.assertFalse(result["steered"])
        self.assertTrue(native.pending[0]["deliveryConfirmed"])

    def test_explicit_rejection_and_unknown_transport_keep_different_delivery_states(self):
        for failure, started, pending in [(namespace["NativeError"]("ENOREPLY"), False, 0),
                                          (TimeoutError(), True, 1)]:
            native = self.native_active()
            native.reply = lambda *args, **kwargs: (_ for _ in ()).throw(failure)
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(type(failure)):
                native.send({"client_id": "C", "content": [{"type": "text", "text": "只投递 C"}]})
            self.assertEqual(native.delivery_started, started)
            self.assertEqual(len(native.pending), pending)
            if pending:
                self.assertFalse(native.pending[0]["deliveryConfirmed"])

    def test_steer_never_uses_escape_to_answer_a_permission_panel(self):
        native = self.native_active()
        native.lookup = lambda: {"short": "saved-job", "sessionId": "test-session",
                                 "tempo": "blocked", "needs": "permission"}
        replies = []
        native.reply = lambda *args, **kwargs: replies.append(args)
        with self.assertRaises(namespace["NativeError"]):
            native.send({"client_id": "C", "steer": True,
                         "content": [{"type": "text", "text": "只投递 C"}]})
        self.assertEqual(replies, [])
        self.assertFalse(native.delivery_started)

    def test_recovery_uses_original_job_and_sends_only_the_new_message(self):
        native = NativeSession("native-cli", "test-session", "saved-job")
        native.model = "gpt-6.1-sol"
        native.effort = "max"
        native.messages = [{"type": "user", "message": {"content": "历史消息"}}]
        native.read_messages = lambda: None
        native.poll = lambda **kwargs: None
        job = {"short": "saved-job", "sessionId": "test-session", "tempo": "idle"}
        lookups = iter([None, job])
        native.lookup = lambda: next(lookups)
        replies = []
        native.reply = lambda text, next_turn=True: replies.append((text, next_turn))
        with patch.object(namespace["subprocess"], "run") as run:
            run.return_value.returncode = 0
            result = native.send({"client_id": "new-client",
                                  "content": [{"type": "text", "text": "新的补充"}]})
        self.assertEqual(run.call_args.args[0], ["native-cli", "respawn", "saved-job"])
        self.assertEqual(replies, [("/model gpt-6.1-sol", False),
                                  ("/effort max", False), ("新的补充", True)])
        self.assertEqual(result["nativeId"], "test-session")
        self.assertEqual(result["jobId"], "saved-job")
        self.assertEqual(len(native.pending), 1)
        self.assertEqual(native.pending[0]["_after"], 1)

    def test_unknown_native_lookup_does_not_restart_or_resubmit(self):
        native = NativeSession("native-cli", "test-session", "saved-job")
        native.read_messages = lambda: None
        native.rpc = lambda operation: (_ for _ in ()).throw(TimeoutError())
        with patch.object(namespace["subprocess"], "run") as run:
            with self.assertRaisesRegex(RuntimeError, "未重新启动或提交消息"):
                native.send({"client_id": "new-client",
                             "content": [{"type": "text", "text": "新的补充"}]})
        run.assert_not_called()
        self.assertEqual(native.pending, [])

    def test_failed_native_recovery_does_not_fork_or_submit(self):
        native = NativeSession("native-cli", "test-session", "saved-job")
        native.read_messages = lambda: None
        native.lookup = lambda: None
        replies = []
        native.reply = lambda text, next_turn=True: replies.append(text)
        with patch.object(namespace["subprocess"], "run") as run:
            run.return_value.returncode = 1
            with self.assertRaisesRegex(RuntimeError, "未创建副本或提交消息"):
                native.send({"client_id": "new-client",
                             "content": [{"type": "text", "text": "新的补充"}]})
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.args[0], ["native-cli", "respawn", "saved-job"])
        self.assertEqual(replies, [])
        self.assertEqual(native.pending, [])

    def test_interactive_session_rejects_before_launch_and_stale_pid_does_not_block(self):
        with tempfile.TemporaryDirectory() as folder:
            native_id = "00000000-0000-4000-8000-000000000001"
            native = NativeSession("unused", native_id)
            native.root = Path(folder)
            sessions = native.root / "sessions"
            sessions.mkdir()
            (sessions / "123.json").write_text(json.dumps({
                "sessionId": native_id, "pid": 123, "kind": "interactive"
            }), encoding="utf-8")
            native.read_messages = lambda: None
            native.lookup = lambda: None
            native.rpc = lambda operation: {"ok": True, "alive": False, "present": False}
            with patch.object(namespace["os"], "kill", return_value=None), \
                    patch.object(namespace["subprocess"], "run") as run, \
                    contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(namespace["NativeError"]) as failure:
                    native.send({"client_id": "interactive-rejected",
                                 "content": [{"type": "text", "text": "未投递的输入"}]})
            self.assertEqual(failure.exception.code, "ESESSIONOPEN")
            self.assertEqual(native.pending, [])
            self.assertFalse(native.delivery_started)
            run.assert_not_called()
            # 区分真实交互式进程占用和已退出的登记，不能据此启动第二个会话或误称模型请求失败。
            with patch.object(namespace["os"], "kill", side_effect=ProcessLookupError), \
                    patch.object(namespace["subprocess"], "run") as run:
                native.check_history_resume()
            run.assert_not_called()

    def test_live_effort_change_from_default_uses_native_command_without_restarting(self):
        native = NativeSession("unused", "test-session", "saved-job")
        native.lookup = lambda: {"short": "saved-job", "tempo": "active"}
        replies = []
        native.reply = lambda text, next_turn=True: replies.append((text, next_turn))
        native.change_setting("effort", "max")
        native.change_setting("effort", "max")
        self.assertEqual(replies, [("/effort max", False)])
        self.assertEqual(native.effort, "max")
        native.lookup = lambda: None
        native.change_setting("effort", "high")
        self.assertEqual(len(replies), 1)
        self.assertEqual(native.effort, "high")

    def read(self, rows, after=0, job_id="", pending_text="待接收消息"):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        log = Path(folder.name) / "native.jsonl"
        log.write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows),
                       encoding="utf-8")
        native = NativeSession("unused", "test-session", job_id)
        native.log_path = log
        native.pending = [{"clientUserMessageId": "web-client", "_nativeText": pending_text,
                           "_after": after}]
        native.read_messages()
        return native

    def test_native_interruption_keeps_metadata_without_consuming_a_literal_pending_message(self):
        marker = "[Request interrupted by user]"
        native = self.read([
            {"type": "user", "uuid": "real-user", "message": {"content": "正在处理"}},
            {"type": "user", "uuid": "interruption", "parentUuid": "assistant-row",
             "interruptedMessageId": "response-id",
             "message": {"role": "user", "content": [{"type": "text", "text": marker}]}},
        ], pending_text=marker)
        self.assertEqual(native.messages[1]["interruptedMessageId"], "response-id")
        self.assertEqual(native.messages[1]["parentUuid"], "assistant-row")
        self.assertFalse(namespace["real_user"](native.messages[1]))
        self.assertEqual(native.clients, {})
        self.assertEqual(len(native.pending), 1)
        native.job = {"tempo": "active"}
        preview_output = io.StringIO()
        with contextlib.redirect_stdout(preview_output):
            native.stream_preview("正在处理\n新回复")
        self.assertEqual(native.preview_user, "real-user")
        self.assertEqual(preview_output.getvalue(), "", "真实中断之后不再显示终端残留预览")

    def test_native_interruption_does_not_hide_human_quotes_or_unmarked_identical_text(self):
        marker = "[Request interrupted by user]"
        for content, metadata, text in [
            (marker, {}, marker),
            ([{"type": "text", "text": marker}], {}, marker),
            ([{"type": "text", "text": "请解释 " + marker}], {"interruptedMessageId": "response-id"}, "请解释 " + marker),
        ]:
            with self.subTest(content=content):
                native = self.read([{"type": "user", "uuid": "literal", **metadata,
                                     "message": {"content": content}}], pending_text=text)
                self.assertTrue(namespace["real_user"](native.messages[0]))
                self.assertEqual(native.clients, {"literal": "web-client"})
                self.assertEqual(native.pending, [])

    def test_native_interruption_requires_a_real_response_reference_and_single_text_control(self):
        marker = {"type": "text", "text": "[Request interrupted by user]"}
        for reference, content in [("", [marker]), ("response", [marker, {"type": "image"}]),
                                   ("response", "引用 [Request interrupted by user]")]:
            self.assertFalse(namespace["native_interruption"](
                {"type": "user", "interruptedMessageId": reference, "message": {"content": content}}))
        for message in (None, "API diagnostics"):
            self.assertFalse(namespace["native_interruption"]({"type": "api-request-blob", "message": message}))

    def queued(self, **changes):
        return {"type": "attachment", "uuid": "absorbed-user",
                "timestamp": "2026-11-11T07:03:46Z", "renderedRole": "system",
                "attachment": {"type": "queued_command", "commandMode": "prompt",
                               "prompt": "待接收消息", "humanTurn": True,
                               "origin": {"kind": "human"}, **changes}}

    def test_absorbed_prompt_clears_queue_and_keeps_identity_without_matching_older_identical_text(self):
        native = self.read([
            {"type": "user", "uuid": "older-user", "message": {"content": "待接收消息"}},
            self.queued(),
        ], after=1)
        self.assertEqual(native.pending, [])
        self.assertEqual(native.clients, {"absorbed-user": "web-client"})
        self.assertEqual(native.messages[1]["type"], "user")
        self.assertEqual(native.messages[1]["message"]["content"], "待接收消息")
        native.read_messages()
        self.assertEqual(len(native.messages), 2)
        self.assertEqual(native.clients, {"absorbed-user": "web-client"})

    def test_internal_or_shell_attachments_do_not_confirm_a_user_message(self):
        for change in [{"origin": {"kind": "agent"}}, {"humanTurn": False},
                       {"commandMode": "bash"}]:
            with self.subTest(change=change):
                native = self.read([self.queued(**change)])
                self.assertEqual(len(native.pending), 1)
                self.assertEqual(native.messages, [])

    def test_compact_summary_metadata_survives_without_confirming_a_real_prompt(self):
        text = "This session is being continued from a previous conversation."
        native = self.read([
            {"type": "user", "uuid": "compact", "isCompactSummary": True,
             "isVisibleInTranscriptOnly": True, "message": {"content": "待接收消息"}},
            {"type": "user", "uuid": "quoted", "message": {"content": text}},
        ])
        self.assertTrue(native.messages[0]["isCompactSummary"])
        self.assertTrue(native.messages[0]["isVisibleInTranscriptOnly"])
        self.assertEqual(native.clients, {})
        self.assertEqual(len(native.pending), 1)
        self.assertEqual(native.messages[1]["message"]["content"], text)
        native.job = {"tempo": "active"}
        with contextlib.redirect_stdout(io.StringIO()):
            native.stream_preview("  新回复\n")
        self.assertEqual(native.preview_user, "quoted")

    def test_setting_commands_and_paired_outputs_are_control_records_but_typed_quotes_survive(self):
        command = ("<command-name>/effort</command-name>\n"
                   "<command-message>effort</command-message>\n"
                   "<command-args>xhigh</command-args>")
        output = "<local-command-stdout>Set effort level to xhigh</local-command-stdout>"
        native = self.read([
            {"type": "user", "uuid": "command", "entrypoint": "cli",
             "message": {"role": "user", "content": command}},
            {"type": "user", "uuid": "output", "parentUuid": "command", "entrypoint": "cli",
             "message": {"role": "user", "content": output}},
            {"type": "user", "uuid": "orphan-output", "parentUuid": "unrelated", "entrypoint": "cli",
             "message": {"role": "user", "content": output}},
            {"type": "user", "uuid": "quote", "entrypoint": "cli", "origin": {"kind": "human"},
             "promptSource": "typed", "turnOrigin": "human",
             "message": {"role": "user", "content": command}},
        ], pending_text=command)
        self.assertEqual(native.messages[0]["nativeSettingCommand"], {"name": "effort", "value": "xhigh"})
        self.assertEqual(native.messages[1]["nativeSettingOutput"]["commandId"], "command")
        self.assertFalse(namespace["real_user"](native.messages[0]))
        self.assertFalse(namespace["real_user"](native.messages[1]))
        self.assertNotIn("nativeSettingOutput", native.messages[2], "孤立回显不按标签文本猜测来源")
        self.assertNotIn("nativeSettingCommand", native.messages[3], "保留真实用户的同文引用")
        self.assertEqual(native.clients, {"quote": "web-client"}, "控制命令不能确认用户的待发送消息")
        self.assertEqual(native.pending, [])
        literal = self.read([{"type": "user", "uuid": "quoted-code", "entrypoint": "cli",
                             "message": {"content": "```xml\n" + command + "\n```"}}],
                            pending_text="```xml\n" + command + "\n```")
        self.assertEqual(literal.clients, {"quoted-code": "web-client"})

    def test_image_attachments_still_use_existing_image_projection(self):
        native = self.read([{"type": "attachment", "uuid": "image",
                            "attachment": {"type": "file", "filename": "/test/0.png",
                                           "content": {"type": "image", "file": {
                                               "type": "image/png", "base64": "AQID"}}}}])
        self.assertEqual(native.messages[0]["type"], "attachment")
        self.assertEqual(native.messages[0]["message"]["content"],
                         [{"type": "image", "url": "data:image/png;base64,AQID"}])
        self.assertEqual(len(native.pending), 1)

    def test_eight_accepted_web_images_survive_partial_native_attachments_and_reconnect(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        root = Path(folder.name)
        uploads = root / "remote-attachments" / "web-client"
        uploads.mkdir(parents=True)
        images = []
        for index in range(8):
            data = bytes([index + 1, 2, 3])
            (uploads / f"{index}.png").write_bytes(data)
            images.append({"type": "image", "source": {"type": "base64", "media_type": "image/png",
                "data": namespace["base64"].b64encode(data).decode()}})
        prompt = "八张图片" + "".join(f"\n@{uploads / f'{index}.png'}" for index in range(8))
        rows = [{"type": "user", "uuid": "native-user", "message": {"content": prompt}},
                {"type": "attachment", "uuid": "only-last-image", "attachment": {
                    "type": "file", "filename": str(uploads / "7.png"), "content": {
                        "type": "image", "file": {"type": "image/png", "base64": images[-1]["source"]["data"]}}}}]
        log = root / "native.jsonl"
        log.write_text("".join(json.dumps(row) + "\n" for row in rows))
        native = NativeSession("unused", "test-session")
        native.root, native.log_path = root, log
        native.pending = [{"clientUserMessageId": "web-client", "_nativeText": prompt, "_after": 0,
                           "content": [{"type": "text", "text": "八张图片"}, *images]}]
        native.read_messages()
        self.assertEqual(len(native.messages[0]["web_images"]), 8)
        self.assertEqual(len(native.messages[0]["remote_upload_paths"]), 8)
        self.assertEqual(native.pending, [])
        restored = NativeSession("unused", "test-session")
        restored.root, restored.log_path = root, log
        restored.control({"subtype": "initialize", "catalog": {}, "clients": native.clients})
        restored.read_messages()
        self.assertEqual(restored.messages[0]["web_images"], native.messages[0]["web_images"])
        restored.read_messages()
        self.assertEqual(len(restored.messages), 2)

    def test_native_usage_and_cache_iterations_are_not_dropped_by_the_bridge(self):
        usage = {"input_tokens": 100, "cache_read_input_tokens": 1000,
                 "cache_creation_input_tokens": 200, "output_tokens": 50,
                 "iterations": [{"type": "message", "input_tokens": 30}]}
        native = self.read([{"type": "assistant", "uuid": "reply-uuid", "isUnmetered": False,
                            "requestedModel": "native-model", "effort": "xhigh",
                            "message": {"role": "assistant", "id": "reply", "model": "native-model",
                                        "content": [{"type": "text", "text": "回复"}], "usage": usage}}])
        self.assertEqual(native.messages[0]["message"]["usage"], usage)
        self.assertFalse(native.messages[0]["isUnmetered"])
        self.assertEqual(native.messages[0]["requestedModel"], "native-model")
        self.assertEqual(native.messages[0]["effort"], "xhigh")

    def test_context_configuration_keeps_compaction_separate_and_bound_to_the_usage_model(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        native = NativeSession("unused", "test-session")
        native.root = Path(folder.name)
        settings = native.root / "settings.json"
        key = "CLAUDE_CODE_MAX_CONTEXT_TOKENS"
        data = {"env": {key: "600000", "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "700000",
                        "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "95", "ANTHROPIC_AUTH_TOKEN": "not-for-export"},
                "autoCompactWindow": 800000, "modelSettings": {"gpt-6.1-sol": {"autoCompactWindow": 900000}}}
        native.model = "gpt-6.1-sol"
        native.read_messages = lambda: None
        native.lookup = lambda: None
        with patch.dict(namespace["os"].environ, {key: "400000"}, clear=True):
            settings.write_text(json.dumps(data))
            catalog = native.control({"subtype": "initialize", "model": native.model, "catalog": {}})
            context = catalog["contextConfiguration"]
            self.assertEqual(context["maxContextTokens"], 600000)
            self.assertEqual(context["compaction"]["windowTokens"], 700000)
            self.assertEqual(context["compaction"]["triggerPercent"], 95)
            self.assertEqual(context["source"], {"kind": "configuration", "key": key,
                                               "scope": "user-settings.env", "model": "gpt-6.1-sol"})
            self.assertNotIn("not-for-export", json.dumps(context))
            with contextlib.redirect_stdout(io.StringIO()) as output:
                native.poll(force=True)
            self.assertEqual(json.loads(output.getvalue())["context_configuration"], context)
            native.messages.append({"type": "assistant", "message": {"model": "other-model", "usage": {"input_tokens": 1}}})
            self.assertEqual(native.context_configuration()["model"], "other-model")
            native.messages.append({"type": "assistant", "isApiErrorMessage": True,
                                    "message": {"model": "error-model", "usage": {"input_tokens": 0}}})
            self.assertEqual(native.context_configuration()["model"], "other-model")
            for invalid in ("0", "-1", "600k", "bad", "9007199254740992", True, 600000):
                data["env"][key] = invalid
                settings.write_text(json.dumps(data))
                self.assertNotIn("maxContextTokens", native.context_configuration(),
                                 f"非法容量不能用于显示：{invalid}")
            del data["env"][key]
            settings.write_text(json.dumps(data))
            self.assertEqual(native.context_configuration()["maxContextTokens"], 400000)
            namespace["os"].environ.pop(key)
            context = native.context_configuration()
            self.assertNotIn("maxContextTokens", context)
            self.assertNotIn("native", context, "压缩配置不能冒充原生容量")
            self.assertEqual(context["compaction"]["windowTokens"], 700000)

    def test_native_statusline_context_is_sanitized_model_bound_and_cleared_on_log_replacement(self):
        usage = {"input_tokens": 100, "cache_read_input_tokens": 1000,
                 "cache_creation_input_tokens": 200, "output_tokens": 50}
        native = self.read([
            {"type": "assistant", "uuid": "reply", "message": {"model": "native-model", "usage": usage}},
            {"session_id": "test-session", "model": {"id": "native-model"},
             "context_window": {"context_window_size": 1000000, "used_percentage": 0.13,
                                "current_usage": usage, "private": "must-not-export"}}
        ])
        context = native.context_configuration()["native"]
        self.assertEqual(context["context_window_size"], 1000000)
        self.assertEqual(context["used_percentage"], 0.13)
        self.assertNotIn("private", json.dumps(context))
        self.assertNotIn("output_tokens", context["current_usage"])
        native.observe_native_context({"session_id": "another-session", "model": "native-model",
                                      "context_window": {"context_window_size": 600000}})
        self.assertEqual(native.native_context, context)
        native.messages.append({"type": "assistant", "message": {"model": "new-model", "usage": usage}})
        self.assertNotIn("native", native.context_configuration())
        native.log_path.write_text("", encoding="utf-8")
        native.read_messages()
        self.assertEqual(native.native_context, {})

    def test_native_discovery_uses_logs_jobs_and_incremental_offsets_without_registry_or_execution(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        root = Path(folder.name)
        project = root / "projects" / "native-project"
        project.mkdir(parents=True)
        primary = "38515c78-cbe6-4961-b5b0-6ee76e81876c"
        history = "00000000-0000-4000-8000-000000000001"
        def write(native, rows):
            path = project / f"{native}.jsonl"
            path.write_text("".join(json.dumps(row) + "\n" for row in rows))
            return path
        message = lambda native: {"type": "user", "uuid": f"user-{native}", "sessionId": native,
            "cwd": "/mnt/e/shared", "timestamp": "2026-11-11T01:00:00Z", "message": {"content": "原生真实问题"}}
        path = write(primary, [message(primary), {"type": "custom-title", "sessionId": primary, "customTitle": "原生自定义标题"}])
        write(history, [message(history), {"type": "custom-title", "sessionId": history, "customTitle": "elves-b831d35da2"}])
        write("00000000-0000-4000-8000-000000000002", [{"type": "assistant", "message": {"content": []}}])
        branch = "00000000-0000-4000-8000-000000000003"
        compact = "00000000-0000-4000-8000-000000000004"
        internal = "00000000-0000-4000-8000-000000000005"
        write(branch, [{**message(branch), "isSidechain": True}])
        write(compact, [{**message(compact), "isCompactSummary": True}])
        write(internal, [{"type": "attachment", "sessionId": internal, "cwd": "/mnt/e/shared", "isMeta": True,
            "attachment": {"type": "queued_command", "commandMode": "prompt", "humanTurn": True,
                           "origin": {"kind": "human"}, "prompt": "内部记录"}}])
        job = root / "jobs" / primary[:8]
        job.mkdir(parents=True)
        (job / "state.json").write_text(json.dumps({"sessionId": primary, "cwd": "/mnt/e/shared", "tempo": "active"}))
        discovery = NativeDiscovery(root)
        self.assertFalse((root / "remote-claude-sessions.json").exists())
        with patch.object(namespace["subprocess"], "run", side_effect=AssertionError("发现不能启动程序")), \
             patch.object(NativeSession, "rpc", side_effect=AssertionError("发现不操作守护进程")):
            rows = discovery.control({"subtype": "discover"})["sessions"]
            self.assertEqual({row["nativeId"] for row in rows}, {primary, history})
            found = next(row for row in rows if row["nativeId"] == primary)
            self.assertEqual(found["title"], "原生自定义标题")
            self.assertEqual(found["jobId"], primary[:8])
            self.assertEqual(next(row for row in rows if row["nativeId"] == history)["jobId"], "")
            self.assertEqual(next(row for row in rows if row["nativeId"] == history)["title"], "原生真实问题")
            count = discovery.bytes_read
            self.assertEqual(discovery.control({"subtype": "discover"})["sessions"], rows)
            self.assertEqual(discovery.bytes_read, count, "重复刷新不能重读既有正文")
            extra = json.dumps({"type": "assistant", "uuid": "new-reply", "sessionId": primary,
                "timestamp": "2026-11-11T02:00:00Z", "message": {"model": "gpt-6.1-sol", "content": [{"type": "text", "text": "新回复"}]}}) + "\n"
            with path.open("a", encoding="utf-8", newline="") as stream:
                stream.write(extra)
            updated = discovery.control({"subtype": "discover"})["sessions"]
            self.assertEqual(discovery.bytes_read - count, len(extra.encode()))
            self.assertGreater(next(row for row in updated if row["nativeId"] == primary)["updatedAt"], found["updatedAt"])

    def test_unregistered_history_reads_29_turns_eight_and_two_original_images_and_usage_without_a_job(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        root = Path(folder.name)
        native = "00000000-0000-4000-8000-000000000029"
        project = root / "projects" / "only-cli"
        project.mkdir(parents=True)
        (root / "settings.json").write_text(json.dumps({"env": {"CLAUDE_CODE_MAX_CONTEXT_TOKENS": "600000"}}))
        rows = []
        for index in range(29):
            prompt = f"原生第{index}轮"
            if index < 2:
                uploads = root / "remote-attachments" / f"accepted-{index}"
                uploads.mkdir(parents=True)
                for image in range(8 if index == 0 else 2):
                    path = uploads / f"{image}.png"
                    path.write_bytes(bytes([image + 1, 2, 3]))
                    prompt += f"\n@{path}"
            rows.append({"type": "user", "uuid": f"user-{index}", "sessionId": native, "cwd": "/mnt/e/shared",
                         "timestamp": "2026-11-11T01:00:00Z", "message": {"content": prompt}})
            if index == 0:
                rows.append({"type": "attachment", "uuid": "partial-native-image", "attachment": {
                    "type": "file", "filename": str(uploads / "7.png"), "content": {"type": "image", "file": {
                        "type": "image/png", "base64": "CAID"}}}})
            rows.append({"type": "assistant", "uuid": f"assistant-{index}", "sessionId": native,
                         "timestamp": "2026-11-11T02:00:00Z", "message": {"id": f"reply-{index}", "model": "gpt-6.1-sol",
                         "content": [{"type": "text", "text": "原生回复"}], "usage": {"input_tokens": 181,
                         "cache_read_input_tokens": 108032, "cache_creation_input_tokens": 0, "output_tokens": 545}}})
        path = project / f"{native}.jsonl"
        path.write_text("".join(json.dumps(row) + "\n" for row in rows))
        original = path.read_bytes()
        with patch.object(namespace["subprocess"], "run", side_effect=AssertionError("读历史不得启动模型")), \
             patch.object(NativeSession, "rpc", side_effect=AssertionError("读历史不得调用原生执行协议")):
            discovery = NativeDiscovery(root)
            for reader in (discovery, discovery, NativeDiscovery(root)):
                frame = reader.control({"subtype": "read_history", "native_id": native})
                users = [row for row in frame["messages"] if row["type"] == "user"]
                self.assertEqual(len(users), 29)
                self.assertEqual(len(users[0]["web_images"]), 8)
                self.assertEqual(len(users[1]["web_images"]), 2)
                self.assertEqual(frame["clients"], {})
                self.assertEqual(frame["native_id"], native)
                self.assertEqual(frame["job_id"], "")
                self.assertEqual(frame["context_configuration"]["maxContextTokens"], 600000)
                self.assertEqual(frame["messages"][-1]["message"]["usage"]["cache_read_input_tokens"], 108032)
            with self.assertRaisesRegex(RuntimeError, "不接受执行"):
                discovery.control({"subtype": "shared_send"})
        self.assertEqual(path.read_bytes(), original)

    def test_history_first_user_send_preserves_uuid_and_refuses_occupied_sessions_without_forking(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        native_id = "00000000-0000-4000-8000-000000000029"
        native = NativeSession("native-cli", native_id)
        native.root = Path(folder.name)
        native.read_messages = lambda: None
        native.poll = lambda **kwargs: None
        native.lookup = lambda: None
        job = {"short": native_id[:8], "sessionId": native_id, "tempo": "idle"}
        native.rpc = lambda operation: {"ok": True, "alive": False, "present": False} if operation["op"] == "has" else {"jobs": [job]}
        with patch.object(namespace["subprocess"], "run") as run, contextlib.redirect_stdout(io.StringIO()):
            run.return_value.returncode = 0
            run.return_value.stdout = f"backgrounded · {job['short']}".encode()
            run.return_value.stderr = b""
            result = native.send({"client_id": "accepted", "content": [{"type": "text", "text": "真实续聊"}]})
        arguments = run.call_args.args[0]
        self.assertEqual(arguments[arguments.index("--resume") + 1], native_id)
        self.assertNotIn("--fork-session", arguments)
        self.assertNotIn("--name", arguments)
        self.assertEqual(result["nativeId"], native_id)
        self.assertEqual(native.native_id, native_id)
        occupied = NativeSession("native-cli", native_id)
        occupied.root = Path(folder.name)
        occupied.read_messages = lambda: None
        occupied.lookup = lambda: None
        sessions = occupied.root / "sessions"
        sessions.mkdir()
        (sessions / f"{namespace['os'].getpid()}.json").write_text(json.dumps({"sessionId": native_id,
            "kind": "interactive", "pid": namespace["os"].getpid()}))
        with patch.object(namespace["subprocess"], "run") as run, contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(namespace["NativeError"], "未接管草稿"):
                occupied.send({"client_id": "must-not-send", "content": [{"type": "text", "text": "不能分叉"}]})
            run.assert_not_called()
        self.assertEqual(occupied.pending, [])
        self.assertFalse(occupied.delivery_started)

    def test_finished_session_can_reconcile_history_without_a_running_supervisor(self):
        native = self.read([self.queued()], job_id="saved-job")
        native.root = native.log_path.parent
        native.initialized = True
        native.lookup = lambda: None
        job = native.root / "jobs" / native.short
        job.mkdir(parents=True)
        (job / "state.json").write_text(json.dumps({"tempo": "idle", "state": "done"}))
        with contextlib.redirect_stdout(io.StringIO()) as output:
            native.poll(force=True)
        frame = json.loads(output.getvalue())
        self.assertEqual(frame["job_id"], "saved-job")
        self.assertEqual(frame["native_id"], "test-session")
        self.assertEqual(frame["pending"], [])
        self.assertEqual(frame["clients"]["absorbed-user"], "web-client")


class ProjectTrustTests(unittest.TestCase):
    def fixture(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        root = Path(folder.name)
        project = root / "project"
        project.mkdir()
        config = root / ".claude.json"
        config.write_text(json.dumps({"oauthAccount": {"accountUuid": "preserved-account"},
                                      "projects": {"other-project": {"hasTrustDialogAccepted": True}},
                                      "nativeSetting": {"keep": True}}), encoding="utf-8")
        return project, config

    def test_check_is_read_only_and_confirmation_preserves_other_native_configuration(self):
        project, config = self.fixture()
        original = config.read_bytes()
        with patch.object(trust_namespace["subprocess"], "run") as run:
            run.return_value.returncode = 128
            checked = trust_namespace["inspect"](str(project), file=config)
            self.assertFalse(checked["trusted"])
            self.assertEqual(config.read_bytes(), original, "检查不修改原生配置")
            result = trust_namespace["confirm"](str(project), str(project.resolve()), file=config)
            self.assertTrue(result["trusted"])
            saved = json.loads(config.read_text())
            old = json.loads(original)
            self.assertEqual(saved.pop("projects")[str(project.resolve())], {"hasTrustDialogAccepted": True})
            self.assertEqual(saved, {k: v for k, v in old.items() if k != "projects"})
            self.assertEqual(json.loads(config.read_text())["projects"]["other-project"],
                             old["projects"]["other-project"])
            before_repeat = config.read_bytes()
            self.assertTrue(trust_namespace["confirm"](str(project), str(project.resolve()), file=config)["trusted"])
            self.assertEqual(config.read_bytes(), before_repeat, "相同目录确认幂等，不重复改写配置")
            self.assertFalse(Path(str(config) + ".lock").exists())

    def test_changed_path_and_broad_directory_are_rejected_before_writing(self):
        project, config = self.fixture()
        original = config.read_bytes()
        with patch.object(trust_namespace["subprocess"], "run") as run:
            run.return_value.returncode = 128
            with self.assertRaisesRegex(ValueError, "目录已变化"):
                trust_namespace["confirm"](str(project), str(project / "different"), file=config)
            with self.assertRaisesRegex(ValueError, "具体项目目录"):
                trust_namespace["confirm"]("/", "/", file=config)
        self.assertEqual(config.read_bytes(), original)
        self.assertFalse(Path(str(config) + ".lock").exists())
        with patch.dict(trust_namespace["os"].environ, {"CLAUDE_CONFIG_DIR": "relative-config"}):
            with self.assertRaisesRegex(ValueError, "绝对路径"):
                trust_namespace["config_path"]()

    def test_trusted_symlink_configuration_can_be_read_but_is_not_replaced(self):
        project, config = self.fixture()
        value = json.loads(config.read_text())
        value["projects"][str(project.resolve())] = {"hasTrustDialogAccepted": True}
        config.write_text(json.dumps(value))
        link = config.parent / "linked-config"
        link.symlink_to(config)
        with patch.object(trust_namespace["subprocess"], "run") as run:
            run.return_value.returncode = 128
            self.assertTrue(trust_namespace["inspect"](str(project), file=link)["trusted"])
            with self.assertRaisesRegex(RuntimeError, "符号链接"):
                trust_namespace["confirm"](str(project), str(project.resolve()), file=link)
        self.assertTrue(link.is_symlink())
        self.assertEqual(json.loads(config.read_text()), value)

    def test_foreign_native_lock_and_invalid_json_are_not_overwritten(self):
        project, config = self.fixture()
        original = config.read_bytes()
        lock = Path(str(config) + ".lock")
        lock.mkdir()
        with patch.object(trust_namespace["subprocess"], "run") as run, \
                patch.object(trust_namespace["time"], "monotonic", side_effect=[0, 6]):
            run.return_value.returncode = 128
            with self.assertRaisesRegex(RuntimeError, "正在更新配置"):
                trust_namespace["confirm"](str(project), str(project.resolve()), file=config)
        self.assertTrue(lock.exists(), "不能删除原生 CLI 的锁")
        self.assertEqual(config.read_bytes(), original)
        lock.rmdir()
        config.write_text("{broken", encoding="utf-8")
        with patch.object(trust_namespace["subprocess"], "run") as run:
            run.return_value.returncode = 128
            with self.assertRaises(ValueError):
                trust_namespace["confirm"](str(project), str(project.resolve()), file=config)
        self.assertEqual(config.read_text(), "{broken")
        self.assertFalse(lock.exists())

    def test_git_boundary_and_managed_workspace_require_the_exact_confirmed_path(self):
        project, config = self.fixture()
        child = project / "child"
        child.mkdir()
        with patch.object(trust_namespace["subprocess"], "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = (str(project.resolve()) + "\n").encode()
            checked = trust_namespace["inspect"](str(child), file=config)
            self.assertEqual(checked["trustPath"], str(project.resolve()))
            with self.assertRaises(ValueError):
                trust_namespace["confirm"](str(child), str(child.resolve()), file=config)
            self.assertTrue(trust_namespace["confirm"](str(child), checked["trustPath"], file=config)["trusted"])
            managed = project.parent / "managed"
            run.return_value.returncode = 128
            checked = trust_namespace["inspect"](str(managed), managed=True, file=config)
            self.assertFalse(managed.exists(), "未确认的无项目目录不提前创建")
            self.assertTrue(trust_namespace["confirm"](str(managed), checked["trustPath"],
                                                       managed=True, file=config)["trusted"])
            self.assertTrue(managed.is_dir())


if __name__ == "__main__":
    unittest.main()
