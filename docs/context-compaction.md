# 上下文压缩模式与原始最近回合保留规范

## 目标

统一 CodexElves 的上下文压缩术语，并修正本地压缩对最近一次真实用户回合的处理方式：

1. 对外只区分“本地压缩”和“远程压缩”两种模式。
2. 压缩模式按最终由谁生成压缩结果判定，不按最初收到的触发类型判定。
3. 本地压缩时，LLM 只总结“指代锚点”之前的历史。
4. 从最后一条真实 `user` 之前最近的一条可见 `assistant` 消息开始，原样保留该指代锚点、
   最后一条真实 `user` 以及后续消息，不再文本化后拼入摘要。
5. 不为满足上游消息结尾规则而向持久会话写入虚拟用户内容。

## 压缩模式定义

### 本地压缩

最终压缩摘要由 CodexElves 控制的普通模型请求生成，即归类为本地压缩。

本地压缩包括：

- Codex 发送传统上下文压缩提示词，由 CodexElves 调用模型生成摘要。
- Codex 触发 Remote Compaction V2，但目标模型或协议不支持原生 V2，CodexElves 将
  `compaction_trigger` 转换为普通摘要请求并在本地兼容桥中生成压缩结果。

因此，V2 只是触发来源。只要最终经过本地兼容桥生成摘要，运行模式就是本地压缩。

本地压缩可以使用：

- CodexElves 的压缩提示词或用户配置的自定义压缩提示词。
- 最近回合原始保留策略。
- 独立压缩模型及其容量校验和失败回退。
- HTTP 与 WebSocket 共用的本地响应改写逻辑。

### 远程压缩

最终压缩结果由上游模型服务通过原生 Remote Compaction V2 生成，即归类为远程压缩。

远程压缩必须保持上游原生行为：

- 原样发送 `compaction_trigger`。
- 不替换为本地摘要提示词。
- 不使用本地最近回合保留逻辑。
- 不使用独立压缩模型。
- 不改写原生压缩项的内容结构。

### 内部诊断字段

内部仍可记录触发来源和兼容桥状态，例如：

- `legacy_prompt`
- `remote_v2_trigger`
- `local_bridge_applied`
- `native_remote_v2`

这些字段只用于日志和排障，不能再作为用户可见的第三种压缩模式。

## 改造前本地压缩实现评估

改造前的本地压缩能够完成 LLM 摘要生成、失败检测、模型回填和 HTTP/WebSocket 响应改写，
但最近回合处理仍存在结构性问题，因此不能判定为“没有问题”。

### 已正常工作的部分

- 能识别传统压缩和需要本地桥接的 V2 触发。
- 能从最后一条真实 `user` 消息定位最近回合起点。
- 能识别 user、assistant、工具调用和工具输出。
- 能按预算限制最近回合体积，并避免孤儿工具输出。
- 能把普通模型摘要改写为 Codex 可识别的压缩响应。

### 已确认的问题

1. **最近回合没有在摘要请求前摘除**

   当前先让 LLM 总结完整历史，再从同一份历史提取最近回合追加到摘要。因此最近回合可能
   同时出现在 LLM 摘要和追加文本中，形成重复信息。

2. **原始消息被文本化**

   user、assistant、工具调用和工具输出被转换成带标签的纯文本，再拼入同一条 assistant
   摘要。原始角色、内容块类型、工具调用 ID、图片和其他结构化信息无法完整保留。

3. **恢复后整体仍是一条 assistant 历史**

   最近回合虽然内容被保留，但在协议层仍属于压缩摘要这条 assistant 消息。对于禁止
   assistant prefill、要求最后一条消息必须为 user 的上游，自动续接仍可能失败。

4. **预算截断不满足“原封不动”**

   当前实现会截断超长工具记录或从最近回合开头删除条目。该行为适合文本摘要兜底，但不符合
   原始消息无损保留要求。

## 推荐的本地压缩数据流

### 1. 切分输入

压缩开始前，从压缩触发项之前的输入中找到最后一条真实 `user` 消息，再向前找到最近一条
可见 `assistant` 文本消息作为“指代锚点”：

```text
待摘要历史 = 输入开头 → 指代锚点之前
原始最近回合 = 指代锚点 → 最后一条真实 user → 压缩触发项之前
```

找不到指代锚点时，原始最近回合仍从最后一条真实 `user` 开始。

“真实 user”不包括：

- developer、system 和内部 reminder。
- 工具输出在协议转换过程中形成的 user 容器。
- 代理生成的兼容标记或控制消息。
- 压缩提示词和 `compaction_trigger`。

指代锚点用于处理以下短回复：

- `按推荐处理`
- `继续`
- `就按这个`
- `同意`

这些回复的具体含义通常位于上一条 assistant 消息中，不能依赖 LLM 摘要一定保留。

### 2. 生成摘要

只把“待摘要历史”和有效压缩提示词发送给摘要模型。最近回合不进入摘要模型输入，避免摘要
再次转述该回合。

### 3. 保存压缩结果

本地压缩结果保存两部分：

```text
summary
  较早历史的 LLM 摘要

retained_tail
  最近回合的原始 Responses items
```

Remote Compaction V2 collector 仍只接收一个 `compaction` item，因此本地兼容载荷需要使用
新的版本化结构保存 `summary` 和 `retained_tail`，不能直接在压缩响应中并列返回多个 output
item。

新的本地结构化载荷使用 `codex-elves-compaction-v3:` 前缀，正文为 JSON：

```text
{
  "summary": "较早历史摘要",
  "retained_tail": [原始 Responses items]
}
```

V2 本地兼容桥把该载荷写入单个 `compaction.encrypted_content`。传统本地压缩把同一载荷作为
摘要文本返回；Codex 将其包装成压缩摘要 user 消息后，代理在下一次请求中识别 v3 前缀并恢复。

### 4. 恢复历史

后续请求遇到本地合成压缩项时：

1. 把 `summary` 恢复为历史 assistant context。
2. 按原始顺序追加 `retained_tail` 中的消息和工具记录。
3. 不改变原始 role、item ID、call ID、工具名称、顺序和工具调用配对关系。尾部未超预算时
   内容块保持原样；触发容量裁剪时只替换过大的工具结果或动态工具描述。
4. 删除 Codex 压缩器已经原样保留、同时又出现在 `retained_tail` 中的 user/developer/system
   重复项；优先按 item ID 匹配，其次按 turn metadata，最后按 role 与文本匹配。若待删除项是
   摘要前唯一一条真实 user，删除后会导致 Anthropic 请求以 assistant 开头，则保留这条原始
   user 作为协议锚点；该极小历史场景允许原文重复一次，但不生成虚拟 user 内容。
5. 不把兼容层生成的传输内容写回持久会话。

### 5. 容量处理

原始最近回合必须在 item 和调用配对层面保持原子性：

- user / assistant 消息原文不裁剪。
- 函数调用参数不裁剪。
- 不删除工具调用或工具输出的一半，不改变 `id`、`call_id`、工具名称和顺序。
- 尾部不超过 `retain_tokens` 时，所有 Responses items 原样保留。

尾部超过 `retain_tokens` 时，将该配置视为裁剪目标：

1. 一旦确认超目标，先扫描全部可裁剪工具结果和动态工具描述，无条件移除其中的图片、音频和视频
   Data URL；该扫描不因中途达到目标而提前结束，避免较小或靠后的媒体载荷原样残留。同一个
   `tool_search_output` 的 `output` 与 `tools` 独立处理。
2. 再优先处理占用最大的工具结果。文本结果保留头尾各一部分并裁掉中间，插入仅含类型和
   估算 Token 数的短标记；短标记格式为 `<truncated:<kind>;~<tokens>t>`。结构化媒体载荷替换
   为 `<truncated:media;~<tokens>t>`。
3. 多个工具结果保留预览后仍超目标时，将最大的结果继续缩成仅保留标记。
4. legacy `tool_result` 只裁剪 `content.content`，保留 `content.tool_use_id`；legacy
   `tool_call` 的 `tool_use.input` 原样保留。
5. `tool_search_output` 只沿嵌套 `tools` 列表裁剪 namespace / function 自身的描述，不进入
   `parameters` 或 `input_schema`；媒体清理同样不进入 schema。工具名称和参数 schema 保持
   不变，保证恢复后仍能注册动态工具。
6. 如果剩余超量来自不可裁剪的 user / assistant 原文或调用参数，则允许软超配置目标，不返回
   `local_compaction_retained_tail_too_large`。

2 MiB 合成载荷上限仍是物理硬限制；超过该上限时明确失败，避免生成无法解码的压缩记录。

## 自动续接规则

恢复原始最近回合后，按目标上游最终收到的有效尾部角色处理：

- 尾部是原始 user 或合法 tool result：允许自动续接。
- 尾部是 assistant，且上游支持 assistant prefill：允许自动续接。
- 尾部是 assistant，且上游禁止 assistant prefill：停止自动续接，等待用户发送真实消息。

默认不生成虚拟 user 消息。严格上游遇到 assistant 尾部时，代理返回一个没有 output item
的本地 `completed` 响应，结束本次自动续接并等待真实用户消息；不得向持久历史写入空 user、
零宽字符或说明性占位内容。

## 兼容范围

本设计适用于所有最终执行为本地压缩的请求：

- 传统压缩触发。
- Remote Compaction V2 触发后的本地兼容桥。
- HTTP。
- Responses WebSocket。
- Responses、Chat Completions 和 Anthropic 上游协议。

本设计不修改最终执行为远程压缩的原生 Remote Compaction V2 请求。

旧版 `codex-elves-compaction-v1` 和 `codex-elves-compaction-v2` 载荷继续按现有文本摘要方式
读取；新的 `codex-elves-compaction-v3` 结构化载荷使用独立前缀，避免旧会话失效。旧版合成
摘要在禁止 assistant prefill 的模型上无法安全自动续接时，同样停止并等待真实用户消息。

## 真实模型验证

2026 年 8 月 3 日通过本地协议代理调用真实 `claude-sonnet-5`，使用 `store=false` 对
当前结构和规划结构进行最小 A/B/C/D 对照。测试未写入 Codex 会话历史。

### A：当前 assistant 尾部结构

输入结构：

```text
user：历史恢复说明
assistant：LLM 摘要 + 被文本化的最近回合
```

结果：

- HTTP 400。
- 上游错误为 `This model does not support assistant message prefill. The conversation must end with a user message.`
- 与故障会话中的错误一致。

### B：摘要后恢复最后一条真实 user

输入结构：

```text
user：历史恢复说明
assistant：较早历史摘要
user：按推荐处理
```

结果：

- 非流式请求 HTTP 200，状态 `completed`。
- 流式请求 HTTP 200，产生 `response.completed`，未产生 `response.failed`。
- 当摘要明确包含“推荐方案”和“下一步”时，模型能正确继续到运行市场基准探针。
- 当摘要只描述目标、没有记录推荐方案时，模型会要求补充上下文。

该结果证明：`assistant summary + user` 可以通过真实 Claude Sonnet 的协议校验，但如果只
从最后一条 user 开始保留，任务连续性仍依赖摘要质量。因此最终方案增加上一条 assistant
指代锚点，不再要求摘要可靠保存短回复的指代对象。

### C：原始最近回合最终仍是 assistant

输入结构：

```text
user：历史恢复说明
assistant：较早历史摘要
user：按推荐处理
assistant：已经开始检查市场基准采集链路
```

结果：

- HTTP 400。
- 仍返回 assistant prefill 错误。

因此，原始最近回合无损恢复不能单独保证自动续接；恢复后的有效尾部角色仍必须参与能力
判断。

### D：原始最近回合以 tool result 结束

输入结构：

```text
user：历史恢复说明
assistant：较早历史摘要
user：按推荐处理
assistant/tool_call：执行市场基准探针
user/tool_result：Gamma 和价格历史探针结果
```

结果：

- HTTP 200，状态 `completed`。
- 保留工具定义、工具调用与工具输出配对后，模型可以继续生成。
- 未保留 assistant 指代锚点且摘要信息不足时，模型会要求补充推荐方案。
- 保留 assistant 指代锚点后，模型可直接从原始消息读取已批准方案、决策条件和后续步骤，
  并继续到检查日期窗口、重新训练和验证 `market_brier`。

该结果证明：tool result 尾部可以安全续接，但必须完整保留工具调用配对，并确保摘要保存
最近回合依赖的决策背景。

### 验证结论

- 规划中的“较早历史摘要 + 原始最近回合”结构可以执行。
- 最后一条有效消息为真实 user 或合法 tool result 时，Claude Sonnet 可以自动续接。
- 最后一条有效消息为 assistant 时，Claude Sonnet 仍拒绝请求，必须暂停自动续接并等待
  真实用户消息。
- 原始最近回合解决角色、工具结构和短回复指代信息丢失问题；摘要只负责指代锚点之前的较早
  历史，不承担最近回合语义完整性的可靠性责任。

### 真实故障会话复测

2026 年 8 月 3 日使用会话 `019fb0f7-a19f-7251-9af3-46dd732cbec5` 的真实压缩前历史复测：

- 摘要输入共 2,308 个 Responses items，请求体约 7.23 MB，总上下文约 89 万 token。
- 真实 `claude-sonnet-5` 成功生成摘要，但摘要中 `推荐方案`、`方案 1`、`按推荐处理` 和
  `probe_market_benchmark.py` 均为 0 次。
- 将真实原始尾部接回后，模型返回 HTTP 200，并准确生成
  `uv run python ..\..\temp\probe_market_benchmark.py` 的下一步工具调用。
- 工具调用只用于观察模型续接结果，没有实际执行。

该结果确认：摘要不能承担短回复指代解析的可靠性责任；必须原样保留 assistant 指代锚点和
后续原始尾部。

2026 年 8 月 5 日使用会话 `019fc6cf-cc9b-7472-8b6a-ae57e9e268d3` 的失败请求回放容量
裁剪：

- 从会话 JSONL 精确恢复 25 个保留尾部 item，旧逻辑估算为 45,754 Token，与原错误一致。
- 其中一条 `view_image` 的 `function_call_output` 含 136,782 字符的 PNG data URL，单项
  约占 39,835 Token，是直接超限来源。
- 新逻辑保留全部 25 个 item 及调用配对，把媒体结果替换为裁剪标记。
- 回放后保留尾部为 14,699 字符，保守复算约 6,040 Token；Remote Compaction V2 返回
  completed，不再产生超限失败。

## 测试设计

需要长期保留以下回归测试：

- 模式分类只产生本地压缩和远程压缩两种结果。
- V2 触发经过本地兼容桥后分类为本地压缩。
- 原生 Remote Compaction V2 分类为远程压缩。
- 最近回合从最后一条真实 user 之前最近的可见 assistant 指代锚点开始切分。
- 最近回合不进入摘要模型请求。
- 最近回合以原始 Responses items 保存和恢复。
- Codex 原生保留的 user/developer/system 与 v3 尾部重复时通常只保留一份；若它是摘要前唯一
  的真实 user，则保留原始副本以满足 Anthropic 首条消息约束。
- user、assistant、工具调用和工具输出顺序保持不变。
- 工具调用与输出 ID 配对保持不变。
- 预算内图片和结构化内容块保持原样。
- 超预算文本工具结果保留头尾和裁剪标记，调用配对不变。
- 超预算图片工具结果移除 data URL，保留原工具结果 item 和明确文本标记。
- 超预算函数调用参数仍原样保留，且 `id`、`call_id` 和工具名称不变。
- legacy `tool_call/tool_result` 的嵌套 ID 外壳在裁剪后仍能通过 Chat 与 Anthropic 配对。
- `tool_search_output` 裁剪描述后仍保留工具名称和参数 schema，并能在 Chat 与 Anthropic
  注册动态工具。
- 多个大工具结果会进入第二轮 marker-only 裁剪，直到达到目标。
- 脱敏 25-item 夹具稳定复现裁剪前 45,754 Token，并在裁剪后保留全部 item。
- 不可裁剪的 user / assistant 原文把配置值视为软目标，不因配置值失败。
- 仅超过 2 MiB 物理载荷上限时明确失败。
- 旧版文本压缩载荷仍可恢复。
- 禁止 assistant prefill 的上游不会收到 assistant 结尾的自动续接请求。
- 为避免虚拟用户内容，无法安全续接时停止并等待真实用户消息。
- HTTP 与 WebSocket 得到一致的压缩模式和恢复结果。

## 成功标准

- 用户界面、日志和文档只展示本地压缩或远程压缩。
- V2 触发但最终由本地兼容桥生成摘要时，统一显示为本地压缩。
- LLM 摘要不再重复最近一次真实用户回合。
- `按推荐处理`、`继续` 等短回复保留其上一条 assistant 指代对象。
- 最近回合不再被压成 assistant 摘要中的纯文本。
- 本地压缩恢复后保留原始消息和工具协议结构。
- 不再因本地压缩把整个历史收束为 assistant prefill 而触发上游协议错误。
- 不向持久会话写入代理生成的虚拟用户内容。

## 非目标

- 不修改上游原生 Remote Compaction V2 的压缩算法或载荷。
- 不把本地兼容桥作为第三种用户可见压缩模式。
- 不在本次设计中修改会话 UI 或代理日志展示。
- 不通过重标角色、空 user、零宽字符或伪造工具调用绕过上游协议校验。
