# 协议代理：web_search 行为与差异

本文档记录本地协议代理在不同上游协议下对 `web_search` 工具的处理方式，以及 Codex 体验上的差异。供后续维护和对齐参考。

## 背景

Codex 内置的 `web_search` 是 OpenAI 平台的 hosted 工具：使用 GPT 模型时由 OpenAI 服务端执行搜索，客户端不参与。当请求经由本地协议代理转发到非 OpenAI 上游（如 Anthropic Claude）时，链路里没有 OpenAI 服务端来执行这个 hosted 工具。

## 历史问题（已修复）

代理早期把 `web_search` 当作普通客户端工具声明给 Anthropic：

1. Claude 发起普通 `tool_use(web_search)`，等待客户端返回 `tool_result`。
2. Codex 客户端没有真实的本地搜索执行器，只回传一个空壳 `web_search_call`（仅含 query，无结果）。
3. 代理把空结果转回 Claude，Claude 拿不到内容，换 query 反复重搜。
4. 历史无限膨胀，永远不产出最终回答，表现为“使用网页搜索就停止反应”。

## 当前处理（Anthropic 路径）

- 请求阶段：当会话没有可执行的 MCP 搜索 fallback（如 tavily/exa）时，`web_search` 声明为 Anthropic 原生 server-side 工具 `{"type":"web_search_20250305","name":"web_search"}`，由 Claude 服务端自己执行搜索。
- 若会话配置了 MCP 搜索 fallback，则保留原有客户端可执行路径，不切换为 server-side。
- `tool_choice` 强制 `web_search` 时降级为 `auto`（Anthropic 不允许用 `tool_choice:tool` 强制 server-side 工具）。
- 响应阶段：Claude 的 `server_tool_use(web_search)` 转为 Codex `web_search_call` 进度项；`web_search_tool_result` 不透传给客户端（结果已被 Claude 消化进最终文本）。
- 历史阶段：客户端 echo 回的空壳 `web_search_call` 被丢弃，不会作为非法工具历史发回上游。
- `pause_turn`（server-side 工具耗时较长时的中间停止）映射为正常结束，避免非法 `finish_reason` 泄漏给 Codex。

## GPT 原生 vs Claude（经代理）的差异

在隔离环境用同一问题（“联网搜索 Rust 最新稳定版本”）实测对比：

| 维度 | GPT 原生 | Claude（经代理 server-side） |
| --- | --- | --- |
| 联网搜索 | 支持 | 支持 |
| 答案正确性 | 正确 | 正确 |
| 死循环 | 无 | 无 |
| 上游 400 / `messages.N` 报错 | 无 | 无 |
| 上游请求轮数 | 1 轮（OpenAI 服务端内部完成） | 多轮（Claude server-side 多轮搜索 + 续轮） |
| 来源引用 | 答案带可点击来源链接（`url_citation`） | 当前为纯文本，暂无可点击来源 |

两点关键差异：

1. 多轮：GPT 由 OpenAI 服务端 1 轮内部完成；Claude 的 server-side web_search 是多轮（可能含 `pause_turn` 续轮），属正常协议行为，能正常收敛出答案。
2. 来源引用：GPT 答案携带 `url_citation` 注解，Codex 渲染为可点击来源；Claude 返回的 `web_search_result_location` citation（含 url/title/cited_text）目前未翻译为 OpenAI `url_citation` 注解，因此 Codex 暂时只显示纯文本答案。

## 后续可选增强

把 Anthropic 的 `web_search_result_location` citation 翻译为 OpenAI `url_citation` 注解，使 Claude 答案也能在 Codex 里显示可点击来源，对齐 GPT 体验。要点：

- Anthropic citation 不携带进入输出文本的字符偏移，`cited_text` 是源文档片段而非输出文本子串；`url_citation` 需要的 `start_index`/`end_index` 需按对应 text block 在整个 `output_text` 中的区间合成，偏移单位为 UTF-16 code unit。
- 流式下 citation 通过 `content_block_delta` 的 `citations_delta` 到达，挂在对应 text block 上；建议不发增量 annotation 事件，统一在最终 message item 一次性带上 annotations。
