# DeepSeek 压缩后历史回放失败排查与修复

## RCA 结论

- 日期：2026-09-17，时间均为北京时间。
- 状态：部分确认。已确认本地发送内容、供应商拒绝行为及可恢复路径；供应商内部究竟在哪一层丢弃或查找 reasoning，无法从本地进一步定位。
- 现象：`deepseek-v4.1-flash` 在上下文压缩后返回 HTTP 400，客户端显示 `reasoning_content` 必须回传，重连 5 次仍失败。
- 直接原因：供应商的 `/v1/messages` 兼容入口拒绝历史续接请求。
- 根因边界：失败请求的 26 条 assistant 均携带非空 thinking，25 条此前已发出的 thinking 与压缩前逐项相同，另一条来自最后一次模型输出。因此本次不能归因为压缩删除了保留尾部的思考。真实对照证明，同一条兼容链路对工具调用历史的接受还依赖调用 ID；仅保留 thinking 不足以保证回放成功。原生 Chat Completions 显式携带 `reasoning_content` 可以接受相同失败历史。
- 系统性缺口：此前回归主要验证压缩和序列化是否保留 thinking；发送边界只处理思考档位不兼容，未处理“已携带真实 thinking 仍遭供应商拒绝”的情况。因此客户端重连会重复发送失败请求。
- 关键因果链：压缩完成 → 历史续接在兼容入口收到明确 400 → 原代理没有对应恢复分支 → 原请求被反复重试 → 用户看到流中断。

## 证据

| 判断 | 类型 | 证据 | 来源 |
| --- | --- | --- | --- |
| 压缩本身返回成功 | 事实 | 11:49:51.881 的请求最终为 200，`layeredCompactionTriggered=true` | `local-9364c7d8-b404-467f-a36e-d81fb3921f45` |
| 失败紧随压缩 | 事实 | 11:50:51.183 首次 400；至 11:51:20.272 共 6 次同类失败 | `local-90414795-d130-46cb-b2d2-0fe154d8597a` 等代理记录 |
| 本地保留的 thinking 未丢失 | 事实 | 失败请求有 26 条 assistant、26 个非空 thinking；已有的 25 个块与压缩前一致 | 代理请求详情及原任务 `compacted.replacement_history` |
| 不是仅由摘要文字触发 | 事实 | 只回放最后一对工具调用与结果，仍返回同样 400 | 同供应商的最小重放 |
| 工具调用 ID 会影响兼容入口结果 | 事实 | 原生生成工具调用后直接回传为 200；保留 thinking 并同时替换 call/result 的匹配 ID 后为同样 400 | 两轮原生 Anthropic 对照 |
| 对工具调用历史缓存存在依赖 | 推断 | 上述 ID 对照支持此解释，但供应商实现和内部日志不可得 | 调查边界位于供应商入口之后 |
| 显式回传 reasoning 可以恢复 | 事实 | 最后一对工具历史改用 `/v1/chat/completions` 为 200 | 同模型、同供应商、原始 reasoning |
| 修复可处理原任务历史 | 事实 | 新代码加载原任务压缩历史，26 条 assistant 均有 reasoning，实际返回 200、`ChatCompletions`、`response.completed` | 临时真实重放验证，未执行模型返回的工具调用 |

原始证据读取自本机代理日志和任务日志；仓库不保存请求正文、认证信息或完整思考文本。临时真实重放文件在验证后删除。

## 已排除解释

- 本次保留尾部在本地压缩中被删除：请求详情和逐项比对不支持。
- WS 降级或认证失败：本次是 Anthropic HTTP 400；相同认证的新请求及 Chat 回放可以成功。
- 仅由 thinking 签名或 `adaptive` 档位触发：移除签名、改为 `enabled` 后，原历史仍返回相同错误。
- 仅由历史长度、图片、摘要或工具定义复杂度触发：缩到最后一对工具历史仍可复现。

## 异常状态溯源

- 适用约束：DeepSeek 启用 thinking 且请求携带 tools 时，后续请求需要回传历史 assistant 的原始 reasoning。
- 约束依据：DeepSeek 官方 Thinking Mode 文档，`https://api-docs.deepseek.com/guides/thinking_mode`，本次调查已读取。
- 本次前提：命中。模型为 DeepSeek，`thinking.type=adaptive`，请求包含 tools。
- 实然状态：本地 Anthropic 请求使用对应的 thinking 内容块，保留尾部包含真实思考；Chat 重试显式设置 `reasoning_content`。
- 值判断：本地未观察到缺失；供应商返回的错误说明其内部 DeepSeek 请求未满足要求。
- 最后满足约束的可观察位置：本地发送前的 26 条 thinking 历史。
- 首次违反位置：供应商内部不可观察，不能把错误文本直接当作本地丢字段的证据。

## 修复与防复发

发送边界新增一次有条件的兼容重试：

1. 仍先按用户配置请求 Anthropic。
2. 只处理 DeepSeek、thinking 已开启、有工具和 assistant 历史、HTTP 400，且结构化错误明确为 `invalid_request_error` / `reasoning_content` / `thinking mode` / `must be passed back` 的组合。
3. 从原始 Responses 历史重新生成 Chat 请求，显式回传真实 reasoning、实际思考档位、工具调用和结果；不修改模型、供应商、持久化协议配置或会话文件。
4. 重试前检查所有仍作为 assistant 发送的消息均有 reasoning；不伪造缺失思考。
5. 每次代理请求最多执行一次此类兼容重试；重试结果按实际 Chat 协议转换并记录。
6. 记录 `protocol_proxy.deepseek_reasoning_replay_retry`，便于区分本地序列化缺失和供应商历史兼容拒绝。

回归覆盖压缩历史、无压缩标记历史、流式和非流式返回、默认/low/max 档位、工具配对、图片、重试失败，以及普通错误、500、关闭思考、无工具、缺少原始 reasoning、Claude 和原生 Responses 不触发兼容重试。

## 验证结果

- `cargo test -p codex-elves-core --test protocol_proxy -- --test-threads=1`：252 项通过。
- 原任务压缩历史真实重放：26 条 assistant、缺失 reasoning 为 0；最终 HTTP 200，收到 `response.completed`。
- `cargo fmt --check`、`git diff --check`：通过。
- `.\build.ps1`：退出码 0；生成 `dist/windows/CodexElves-0.4.1-windows-x64-setup.exe`，大小 36,019,420 字节。
- launcher、manager、task-board 三个程序的 `target/release` 与 `dist/windows/app` 副本 SHA256 一致。

## 未闭环项与下一步

- 供应商内部具体丢失位置仍不可见；如需修复供应商自身，需要其请求转换和历史缓存日志。本地补救的正确性不依赖确定具体内部模块。
- 真实验证使用当前供应商，其 Chat Completions 入口可用；其他供应商若不支持该入口，兼容重试仍会失败并返回实际错误，不会无限重试。
- 源码和安装包更新不会替换已运行进程；安装并重新启动 CodexElves 后，原任务的后续请求才会使用该恢复逻辑。
