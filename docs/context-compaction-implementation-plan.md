# 本地上下文压缩原始尾部恢复实施计划

> **执行方式**：在当前会话内按任务顺序实施，每个任务完成后运行对应定向测试。项目规则
> 禁止未经用户授权提交，因此本计划不执行 `git commit`，也不使用 worktree 或红绿 TDD。

**目标：** 将本地压缩改为“较早历史摘要 + assistant 指代锚点开始的原始尾部”，并在
Claude 等禁止 assistant prefill 的模型上安全停止自动续接。

**架构：** `layered_compaction.rs` 负责输入切分、v3 载荷编解码、重复项消除和尾部角色判定；
`protocol_proxy.rs` 的 Responses、Chat Completions、Anthropic 三条请求转换路径统一调用
恢复入口；HTTP 与 Responses WebSocket 在需要真实 user 才能继续时返回无 output item 的
本地 completed 响应。

**技术栈：** Rust、serde/serde_json、Responses API JSON/SSE、tokio WebSocket。

## 全局约束

- 只影响最终执行为本地压缩的传统压缩和 Remote V2 兼容桥。
- 原生 Remote Compaction V2 保持不变。
- v1/v2 合成压缩载荷继续可读。
- 不创建虚拟 user、零宽字符或伪造工具结果。
- v3 尾部预算内保持原样；超预算时只裁剪工具结果和动态工具描述，保留 item、ID、顺序、调用参数与
  调用配对。user / assistant 原文不裁剪，配置值作为软目标。
- 保留当前未提交改动，不修改无关 UI、模型能力和中继配置逻辑。

---

### Task 1：切分摘要输入与原始尾部

**文件：**

- 修改：`crates/codex-elves-core/src/layered_compaction.rs`

**接口：**

- 产生：`LocalCompactionSplit { summary_input, retained_tail }`
- 产生：`split_local_compaction_input(input, control_kind)`

- [x] 从输入中排除压缩提示词、`compaction_trigger` 和不可跨模型复用的 reasoning item。
- [x] 定位最后一条真实 user。
- [x] 向前定位最近一条可见 assistant message 作为指代锚点。
- [x] 将锚点及其后原始 items 放入 `retained_tail`，较早 items 放入 `summary_input`。
- [x] 传统压缩和 V2 本地桥都把有效压缩提示词追加到裁剪后的 `summary_input` 末尾。
- [x] 在现有 `layered_compaction.rs` 单元测试中验证：

```rust
assert_eq!(prepared["input"], json!([
    user_message("older context"),
    compaction_prompt_item_with(DEFAULT_COMPACTION_PROMPT),
]));
assert_eq!(
    split.retained_tail,
    vec![
        assistant_message("推荐方案"),
        user_message("按推荐处理"),
        function_call_item("call-1", "shell_command"),
        function_call_output_item("call-1", "ok"),
    ]
);
```

### Task 2：写入并恢复 v3 结构化载荷

**文件：**

- 修改：`crates/codex-elves-core/src/layered_compaction.rs`

**接口：**

- 产生：`codex-elves-compaction-v3:` JSON 载荷。
- 产生：`expand_synthetic_local_compaction_request(&Value) -> Value`
- 保留：`synthetic_remote_compaction_history_text(&Value) -> Option<String>` 的 v1/v2 兼容。

- [x] 定义可序列化的 `summary` 与 `retained_tail` 结构。
- [x] V2 本地桥输出单个带 v3 载荷的 `compaction` item。
- [x] 传统压缩输出带 v3 载荷的摘要文本，允许 Codex 用标准 summary user 消息持久化。
- [x] 恢复时先移除 Codex 已保留的重复 user/developer/system，再追加 assistant summary 和
      原始尾部；如果待删除项是摘要前唯一真实 user，则保留原始副本作为 Anthropic 协议锚点，
      不生成虚拟 user。
- [x] 重复项匹配顺序固定为 item ID、turn metadata、role + 文本。
- [x] v3 尾部超过 `retain_tokens` 时，先完整移除全部工具结果和动态工具描述中的媒体 Data URL，
      再按“工具结果预览 → 仅保留结果标记”的顺序自适应裁剪；调用参数保持原样，配置值不再触发
      `response.failed`。
- [x] legacy `tool_result` / `tool_call` 使用嵌套详情路径裁剪，保留 `tool_use_id`、调用 ID
      和工具名称；其中 `tool_call.tool_use.input` 保持原样；`tool_search_output` 只裁剪工具对象自身描述，不进入 `parameters` /
      `input_schema`，且分别处理同时存在的 `output` 与 `tools`，保留动态工具 schema。
- [x] user / assistant 原文不可裁剪且仍超目标时允许软超；2 MiB 载荷上限仍明确失败。
- [x] 在现有单元测试中验证预算内图片内容块逐项相等，以及超预算图片、文本结果裁剪后仍保留
      call ID、tool call/output 顺序与合法结构；函数参数即使超过目标也原样保留；补充 25-item/45,754 Token
      脱敏回放、第二轮 marker-only、legacy 配对、动态工具注册和 2 MiB 边界测试。

### Task 3：接入三种协议转换

**文件：**

- 修改：`crates/codex-elves-core/src/protocol_proxy.rs`
- 修改：`crates/codex-elves-core/tests/protocol_proxy.rs`

**接口：**

- 消费：`expand_synthetic_local_compaction_request`
- 产生：三种上游协议一致的恢复历史。

- [x] `normalize_native_responses_request` 在规范化 message ID 前展开 v3。
- [x] `responses_to_chat_completions` 在转换消息前展开 v3。
- [x] `responses_to_anthropic_messages` 在转换消息前展开 v3。
- [x] 保证 Anthropic tool_use/tool_result 和 Chat tool_calls/tool 消息仍使用原 call ID。
- [x] 在现有单元与协议测试中验证最终有效尾部为 user、tool result、assistant 三种情况。

### Task 4：安全结束不支持 assistant prefill 的自动续接

**文件：**

- 修改：`crates/codex-elves-core/src/layered_compaction.rs`
- 修改：`crates/codex-elves-core/src/protocol_proxy.rs`
- 修改：`crates/codex-elves-core/src/responses_websocket_bridge.rs`

**接口：**

- 产生：`local_compaction_requires_real_user(request, model) -> bool`
- 产生：Responses JSON/SSE/WebSocket 的空 output `completed` 响应。

- [x] 仅在请求包含本地合成压缩历史、最终有效尾部属于 assistant 侧、目标模型为 Claude
      家族时触发暂停。
- [x] HTTP 请求不调用上游，直接返回 `status=completed`、`output=[]`。
- [x] WebSocket 请求不转发上游，直接向下游发送对应 created/completed 事件。
- [x] user 或合法 tool result 尾部继续正常发送上游。
- [x] v1/v2 旧载荷在 Claude assistant 尾部场景下也采用暂停策略。

### Task 5：回归验证与文档一致性

**文件：**

- 修改：`docs/context-compaction.md`

- [x] 运行：

```powershell
rustfmt --check --edition 2024 <本次修改的 Rust 文件>
cargo test -p codex-elves-core --lib layered_compaction
cargo test -p codex-elves-core --test protocol_proxy
cargo test -p codex-elves-core --lib responses_websocket_bridge
cargo check --workspace
git diff --check
```

- [x] 核对原生 GPT Responses Remote Compaction V2 测试仍保留原始 trigger 和 opaque
      compaction item。
- [x] 核对工作区没有新增临时测试文件、没有执行 commit、没有覆盖用户原有改动。

> 全局 `cargo fmt --check` 仍会命中本次任务开始前已存在的
> `crates/codex-elves-core/tests/cdp_bridge.rs` 格式差异；本次涉及的四个 Rust 文件已通过
> 独立 `rustfmt --check`，未修改该无关文件。
