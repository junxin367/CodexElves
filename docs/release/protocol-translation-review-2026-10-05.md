# 协议翻译检查记录（2026-10-05）

本轮检查 Responses → Chat Completions / Anthropic 请求转换、JSON 与 SSE 响应转换、缓存用量以及流的终止状态。共新增 9 个回归测试函数，分别先复现失败，再修复；另修正 2 个沿用旧缓存计数口径的测试。

## 已确认并修复

| 问题 | 修复前的可复现行为 | 修复结果 |
| --- | --- | --- |
| Chat 结构化输出参数丢失 | Responses 的 `text.format` 不转换，甚至被兼容字段 `response_format: text` 覆盖 | 转换 JSON Schema 的嵌套结构，保留 name、description、strict、schema；原生 Responses 参数优先。支持 text 和 json_object |
| Anthropic 结构化输出参数丢失 | `text.format` 被忽略，模型未收到 JSON Schema 约束 | 转为 `output_config.format`，保留同一对象中的思考深度；不支持的格式明确报错 |
| Chat 用量计数不符合输出协议 | 输入 100、缓存 80 时转换为输入 20；Responses 风格 usage 可能重复加缓存 | 输出统一为包含缓存的输入总量；原生 Anthropic 用量才加上缓存读写。保留缓存明细、TTL、思考 token，并覆盖 JSON/SSE |
| Anthropic 错误响应伪装成成功 | `type:error`、非空 error，甚至空对象被转换成空的 completed 响应 | 转换前拒绝上游错误，并要求 content 为数组；Chat 同样先检查显式错误对象 |
| 空错误字段误报失败 | SSE 中 `error:null` 被当成错误 | null 不触发失败；明确的 error 事件、`type:error`、非空 error 仍触发失败 |
| 截断原因误报完成 | Chat 的 content_filter、Anthropic 的 model_context_window_exceeded / refusal 被标成 completed | JSON 与 SSE 使用一致的 incomplete 状态和原因；SSE 发出 response.incomplete |
| 终止后继续修改状态 | 已发送 completed 后，尾随错误和读取失败还能产生多个终止事件 | 成功或失败后保持终态；覆盖同一输入块、分块输入、重复 fail 和 finish。保留原始失败诊断 |
| Anthropic 服务端暂停误报完成 | pause_turn 被直接映射为正常 stop | JSON 转换报错；SSE 发出 unsupported_upstream_continuation 失败，包括 message_stop 和兼容的 DONE 路径 |

实现位于 `crates/codex-elves-core/src/protocol_proxy.rs`；新增回归用例在既有 `crates/codex-elves-core/tests/protocol_proxy.rs` 的 `protocol_review_*` 测试中。

## 用量口径与兼容性

- Chat 的 prompt_tokens、Responses 的 input_tokens、Gemini 的 promptTokenCount 按输入总量处理，不减缓存。
- 没有标准输入明细的 Anthropic 风格 usage，复用 Anthropic 转换器，将未缓存输入、缓存读取、缓存写入相加。
- 输出 `input_tokens_details`，使管理器现有读取逻辑识别为已转换的 Responses 用量，避免再次加缓存。
- 混合携带标准明细和厂商扩展的 usage 优先采用标准明细；厂商扩展保留供诊断。异常厂商字段含义仍需要实际样本确认。
- 上下文窗口耗尽映射为 Responses 的 `max_output_tokens` 不完整原因。这是跨协议近似映射，保留“不完整”语义，但不能在该标准原因字段中区分输入上下文耗尽与输出上限。

## 验证结果

```text
cargo test -p codex-elves-core --lib --test protocol_proxy --test responses_websocket -- --test-threads=1 --quiet
  core lib:             489 passed, 2 ignored
  protocol_proxy:       294 passed
  responses_websocket:   30 passed
  total:                813 passed, 2 ignored

cargo fmt --check       通过
cargo check --workspace 通过
git diff --check        通过（仅既有 Windows 换行提示）
```

既有协议回归还覆盖工具名称、命名空间、自定义工具参数往返、工具结果关联、中文分片、压缩桥接，以及原生 Responses 路由。本轮未改变原生 Responses 的直接转发路径。

## 核对依据与验证边界

核对了官方 SDK 的协议定义：

- `openai/openai-python`：`types/responses/response.py`、`response_usage.py`、`response_format_text_json_schema_config_param.py`，以及 Chat 的 `response_format_json_schema.py`。
- `anthropics/anthropic-sdk-python`：`types/message.py`、`output_config_param.py`、`json_output_format_param.py`。

当前桥接仍没有 Anthropic `pause_turn` 自动续传能力；本轮解决的是错误地宣告完成，并未实现服务端工具恢复。Anthropic 的 json_object 请求会明确报不支持，JSON Schema 能否执行仍取决于上游模型和中转实现。

本轮验证为源码测试与本地模拟上游，不包含真实中转账号、已安装客户端或安装包端到端验收，不能据此保证所有第三方协议扩展都兼容。

## 后续检查：工具调用与思考块回放

继续检查时，新增的 6 个 `protocol_continuation_review` 测试分别复现了以下问题，修复后均通过：

| 问题 | 修复结果 |
| --- | --- |
| Anthropic 非流式思考块丢失签名，多个块被合并，redacted_thinking 被丢弃 | 保留每个块及其签名；被隐藏的思考块只作为不透明数据保存并回放 |
| Anthropic 流式复用同一思考状态，拼接了不同块的签名 | 在每个思考块关闭时结束该项，后续块使用独立状态与 ID；覆盖 1、7、512 字节分片及中文 |
| 引用标签过滤改变了签名对应的思考原文 | 展示内容仍过滤引用标签；必要时在 encrypted_content 中使用带版本的本地封装保存原始块，回放恢复原文。签名及 redacted data 不做解析或解密 |
| 截断响应仍发布工具参数完成事件和 completed 工具项 | Chat 与 Anthropic 的流式、非流式均不将本轮截断工具调用发布为可执行输出；文本标记为 incomplete，保留已收到的正文和思考 |
| Anthropic 工具块没有关闭，但 message_stop 被当作成功 | 对未关闭的已知文本、思考、工具块报 stream_error，不发布工具完成事件；保持既有未知扩展块的兼容行为 |
| Chat 旧式 function_call 仅在非流式下有效 | 流式增量同样进入工具调用状态，拼接参数并与非流式输出保持一致 |
| 文本、拒绝内容交替时多个消息复用相同 ID | 每个新消息项使用独立 ID，保留已有首项 ID 形式 |

新增测试包含 JSON/SSE 响应转换后再转回 Anthropic 请求的往返断言，检查原始思考文本、签名、隐藏块、工具关联和输出项 ID；不是只检查转换后字段是否存在。流式完成事件中的输出项也与终止响应逐项比对。

本地封装只是协议桥接数据格式，不是加密机制。普通未改写的思考块继续使用已有原始签名形式。原始思考及签名的流式缓冲直接追加字符串，避免每个分片复制已有全文；块完成后释放原始块状态。

截断处理采用保守策略：当前响应的工具调用不执行，也不自动补全或续传半段参数。真实上游签名校验和安装客户端行为仍需端到端验证。

后续检查的完整验证结果：

```text
core lib:             489 passed, 2 ignored
protocol_proxy:       300 passed
responses_websocket:   30 passed
total:                819 passed, 2 ignored

cargo fmt --check       通过
cargo check --workspace 通过
git diff --check        通过（仅既有 Windows 换行提示）
```

补充核对官方 SDK 类型：Anthropic 的 `thinking_block.py`、`redacted_thinking_block.py`，以及 OpenAI 的 `response_function_tool_call.py`、`response_custom_tool_call.py`。

## 再次检查：交错输出顺序与客户端历史

本轮聚焦同一条响应中正文、思考、工具交错时的顺序。新增 5 个 `protocol_order_review` 测试，覆盖非流式历史回放、流式索引、Chat 分段、忽略索引的客户端接收方式，以及截断终态。

确认并修复的顺序问题：

- 非流式 Anthropic 转换原先分别收集思考、正文、工具，然后按类型重排。现在按源内容遍历顺序输出；只合并连续正文，遇到思考或工具时结束当前正文项。
- 流式转换原先跨越思考、工具调用追加到同一个正文项。现在在这些边界结束旧正文，后续正文使用新的消息项及 ID。
- 没有参数分片的工具原先在流结束时才获得输出位置，可能落到后续正文之后。现在首次出现时保留位置，仍等待足够的元数据再发布工具项。
- 仅排序最终 output 数组无法保证 Codex 的历史顺序。官方 `codex-rs/codex-api/src/sse/responses.rs` 将完成事件转换为只携带 item 的 `ResponseEvent::OutputItemDone`；因此完成事件的到达顺序也必须正确。现在较晚的完成项等待较早项完成，按输出顺序发送；正文 delta 不进入这个等待队列。

验证包括一份混合正文、带签名思考、空参数工具和隐藏思考块的响应。非流式和流式都转回 Anthropic 历史，逐项比较原始块。流式覆盖 1、23、4096 字节输入分片，并同时检查：

1. 每个 added/done 事件的 ID 对应最终 output 中相同索引的项。
2. 不使用 output_index、只按 done 到达顺序收集的历史，与最终 output 一致。
3. 截断时仍终止为 incomplete，保留文本，不发送工具调用完成结果。

完整回归曾发现本次修改引入的一个回退：同一 Chat 分片中的工具调用会先结束正文项，而 finish_reason 在分片末尾才读取，导致截断正文被标成 completed。已将结束原因读取提前到内容处理前；既有截断回归恢复通过，未放宽断言。

最终验证：

```text
core lib:             489 passed, 2 ignored
protocol_proxy:       305 passed
responses_websocket:   30 passed
total:                824 passed, 2 ignored

cargo fmt --check       通过
cargo check --workspace 通过
git diff --check        通过（仅既有 Windows 换行提示）
```

协议顺序依据为 Anthropic 官方 Streaming 文档的内容索引约定，以及本次核对的官方 Codex 事件解析源码。本轮仍是本地转换器和模拟输入验证；官方源码核对不等同于当前安装客户端的端到端运行证据。

## 继续检查：工具历史的思考归属与请求异常

本轮新增 3 个 `protocol_request_review` 测试。修复前，两个缺陷测试失败，合法选项兼容测试通过；修复后全部通过。

- 工具历史包含“助手正文 → 思考 → 工具调用 → 工具结果”时，`flush_tool_calls` 将调用合并进已有助手消息，却未消费待回放的思考。思考随后被放到工具结果之后，可能额外生成助手消息，或错误归入后续助手回复。现在合并调用时同步追加并清空待回放思考，保留原有正文和思考。
- 流式 Chat 转换直接向 `stream_options` 写入 `include_usage`。传入布尔值时测试捕获到 JSON 索引 panic。现在仅接受对象、null 或缺省值，其余类型返回包含字段名的错误。对象中的扩展字段继续保留，`include_usage` 仍强制开启。

历史回归覆盖 function、custom、tool_search 和旧式 tool_call 四种格式；每种均包含两个并行调用、逆序返回的结果，并分别验证历史结束于工具结果和继续包含助手回复的情况。断言思考只属于发起调用的助手消息、调用 ID 与结果保持对应，后续回复不携带前一轮思考。

请求异常回归覆盖布尔、数字、字符串、数组，要求转换返回错误而不是 panic；兼容回归覆盖缺省、null、空对象，以及包含扩展字段和 `include_usage: false` 的对象。

最终验证：

```text
core lib:             489 passed, 2 ignored
protocol_proxy:       308 passed
responses_websocket:   30 passed
total:                827 passed, 2 ignored

cargo fmt --check       通过
cargo check --workspace 通过
git diff --check        通过（仅既有 Windows 换行提示）
```

本轮验证范围为本地转换器、核心回归与模拟上游；未进行真实上游或已安装客户端端到端验证。

## 新增五轮检查与修复

用户要求“继续检查和修复 5 轮”后，独立完成以下五轮。测试均加入已有的 `tests/protocol_proxy.rs`，位于 `protocol_five_round_review` 模块。每轮先运行新增测试确认失败，再修改实现并确认通过；没有把此前已完成的检查计入这五轮。

| 轮次 | 检查范围与复现结果 | 修复与回归覆盖 |
| --- | --- | --- |
| 1 | SSE 分帧：单独 CR 或混合换行导致 `invalid_sse_json`；开头 BOM 使首行被忽略、正文或响应标识丢失 | 在新增解码文本上归一化 CR/CRLF/LF，跨分片保留跳过 LF 的状态，仅移除流开头的一个 BOM。2 个测试覆盖 Chat/Anthropic、1/2/7/4096 字节切分和正文内部 BOM |
| 2 | UTF-8：同一分片先含非法字节，末尾再截断合法中文时，原有整段有损解码将中文和 emoji 变成多个替换字符 | 逐段跳过确定非法的字节，并继续检查后续文本；末尾尚未完整的字符仍留待下一分片。1 个测试覆盖两种协议、非法字节紧邻中文、多种切分及完整输入 |
| 3 | 多候选隔离：JSON 数组顺序变化时选错候选；SSE 每次取第一项，混入其他候选的正文、思考、工具参数和结束原因 | JSON/SSE 共用按 `index: 0` 选择的逻辑；保留无 index 的旧式上游兼容。2 个测试覆盖乱序数组、候选独立分片、相同工具索引、另一候选的 length 结束原因、空 choices 用量分片，以及非流式缺少第零候选时返回错误 |
| 4 | 用量明细：null、空对象或仅有音频明细时，输出缺少整数 reasoning_tokens；null 别名挡住 thinking 计数 | 只采用对象形态明细，优先保留有效 reasoning 计数，否则使用有效 thinking 计数或 0；保留其他明细字段。2 个测试覆盖两种协议的 JSON/SSE、nullable 明细、扩展字段与别名回退 |
| 5 | 地址拼接：完整 Responses/Anthropic 端点生成 `/responses/models` 或 `/messages/models`；去重 `/v1/v1` 时误改 `/v1/v10` 等合法路径 | 模型发现统一识别三种完整协议端点；重复版本匹配要求完整路径段。2 个测试覆盖自定义前缀、大小写、尾斜杠、`#` 约定、v10/v11/v1beta/v1-custom，以及重复 v1 的原有兼容行为 |

局部验证命令为 `cargo test -p codex-elves-core --test protocol_proxy round_N_ -- --test-threads=1`。格式化后的汇总验证 `protocol_five_round_review` 为 **9 passed, 0 failed**。

实现检查确认：SSE 的常见 LF 输入仍直接追加，只在需要时处理新文本；非法 UTF-8 保持原有替换字符策略；候选隔离仍读取响应级用量；用量归一化保留音频等扩展明细；地址修复保持自定义路径和既有 `#` 约定。

协议依据核对了 WHATWG HTML Standard 的 SSE 解析规则，以及官方 `openai/openai-python` 中的 `chat_completion_chunk.py`、`completion_usage.py`、`responses/response_usage.py`。这次只修改协议转换实现、已有协议测试文件及本检查记录。

五轮修复后的完整回归：

```text
cargo test -p codex-elves-core --lib --test protocol_proxy --test responses_websocket -- --test-threads=1 --quiet
core lib:             489 passed, 2 ignored
protocol_proxy:       317 passed
responses_websocket:   30 passed
total:                836 passed, 2 ignored

cargo fmt --check       通过
cargo check --workspace 通过
git diff --check        通过（仅既有 Windows 换行提示）
```

完成核对：五轮均有源码定位、新增测试的修复前失败证据及修复后通过证据，全部新增测试参加最终协议回归；完整核心和 WebSocket 回归通过。本次没有执行安装包构建、真实上游请求或安装客户端端到端验证。

## 重点复查：翻译前后的内容与参数一致性

本次继续聚焦协议翻译，新增 `protocol_translation_fidelity_review` 的 4 个测试。修复前，3 个缺陷测试失败，1 个普通 JSON 结果兼容测试通过；修复后全部通过。

1. **图片精度字段丢失。** Responses 将 `detail` 放在 input_image 外层，Chat 将其放在 image_url 对象内，原转换只复制 URL。现在将显式外层 detail 移入对应位置；未提供外层字段时保留已有嵌套字段，不凭空添加默认值。测试覆盖 low/high/auto/original、字符串和兼容对象形式的 URL，以及用户消息和工具结果中的图片。
2. **非流式工具参数被改写。** 输出转换误用历史参数归一化函数，将不完整 JSON、字符串或数组等参数包装为 `{"input": ...}`；流式路径则保留原始字符串。现在在现代 tool_calls 和旧式 function_call 的输出方向保留参数字符串，保持 JSON/SSE 输出与参数完成事件一致。测试覆盖不完整 JSON、JSON 字符串/数组/布尔/null、普通文本、中文、前后空白和空参数。既有空参数转 `{}` 的兼容行为继续保留；历史请求修复逻辑仍由原函数处理。
3. **纯文本块工具结果被包装成 JSON 正文。** 原实现仅在混合图片时提取内容块的文本，纯文本数组则直接序列化。现在将可识别的文本块数组按顺序提取正文；含未知类型或无效文本字段的数组仍作为完整 JSON 数据保留。测试覆盖 function/custom 工具、正常及孤立结果、Chat/Anthropic 两条路径，以及正文中的中文、换行和 JSON 字面文本。

兼容测试确认：对象结果、普通对象数组、混合未知块和非字符串 text 字段没有被误当成文本块，反序列化后仍与原始数据一致。

核对的官方类型为 `openai/openai-python` 的 `responses/response_input_image_param.py`、`chat/chat_completion_content_part_image_param.py`、`responses/response_input_param.py` 和 `chat/chat_completion_message_function_tool_call.py`。本次修复处理字段映射与参数保真，不新增工具参数校验或自动修复能力。

最终验证：

```text
core lib:             489 passed, 2 ignored
protocol_proxy:       321 passed
responses_websocket:   30 passed
total:                840 passed, 2 ignored

cargo fmt --check       通过
cargo check --workspace 通过
git diff --check        通过（仅既有 Windows 换行提示）
```

验证范围仍是本地源码测试与模拟上游，不包括真实中转账号或安装客户端端到端验证。
