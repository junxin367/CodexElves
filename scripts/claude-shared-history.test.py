"""原生执行中吸收队列消息的回归样例；只读取临时日志，不连接或启动 Claude。"""
import json
import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch


source = Path(__file__).resolve().parents[1] / "crates/codex-elves-core/src/remote_access/claude_shared.py"
namespace = {"__name__": "claude_shared_history_test"}
exec(compile(source.read_text(encoding="utf-8"), str(source), "exec"), namespace)
NativeSession = namespace["NativeSession"]
NativeDiscovery = namespace["NativeDiscovery"]


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
        with contextlib.redirect_stdout(io.StringIO()):
            native.stream_preview("正在处理\n新回复")
        self.assertEqual(native.preview_user, "real-user")

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
                            "message": {"role": "assistant", "id": "reply", "model": "native-model",
                                        "content": [{"type": "text", "text": "回复"}], "usage": usage}}])
        self.assertEqual(native.messages[0]["message"]["usage"], usage)
        self.assertFalse(native.messages[0]["isUnmetered"])

    def test_explicit_context_window_is_model_bound_and_excludes_compaction_settings(self):
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
                self.assertEqual(native.context_configuration(), {}, f"非法容量不能用于显示：{invalid}")
            del data["env"][key]
            settings.write_text(json.dumps(data))
            self.assertEqual(native.context_configuration()["maxContextTokens"], 400000)
            namespace["os"].environ.pop(key)
            self.assertEqual(native.context_configuration(), {}, "只有压缩窗口和95%时容量仍未知")

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


if __name__ == "__main__":
    unittest.main()
