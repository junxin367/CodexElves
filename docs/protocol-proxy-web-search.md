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

## 当前处理

使用 Anthropic 协议不代表上游具有 Claude 的服务端搜索能力。必须区分 `server_tool_use`（服务端执行）和普通 `tool_use`（等待客户端执行）。

- 已加载搜索 MCP 工具时，普通 `web_search` 调用转换为该工具的 `function_call`。Codex 执行真实搜索、回传结果，再继续生成回答。
- 搜索 MCP 尚未加载但已声明客户端 `tool_search` 时，普通 `web_search` 调用先转换为 `tool_search_call`，查询搜索工具目录。下一轮从 `tool_search_output.tools` 恢复工具声明，并走真实搜索路径；原始用户问题保留在会话中。
- 没有执行器、没有客户端工具发现，或发现后仍没有可执行搜索工具时，显式返回 `web_search_unavailable`，不能生成假的搜索完成记录，也不能反复发现工具。
- SWE 系列模型（如 `swe-2`、`devin/swe-2`）使用包含 `input_schema.query` 的普通搜索工具声明，不套用 Claude 的 `web_search_20250305` 类型。其他 Anthropic 请求（包括 `devin/claude-*`）保留原生服务端搜索声明；只有实际声明服务端工具时，强制工具选择才降级为 `auto`。
- 真正的 `server_tool_use` 和服务端结果保留在不可执行的历史封装中，供后续上游回放；不会改成客户端工具调用。
- 原生 Responses 请求保持直通，不改变其服务端搜索行为。

`web_search_call` 是 Responses 的搜索记录，不是客户端执行器。给它添加 `execution: "client"` 不能让 Codex 执行搜索。普通搜索调用必须转换为可执行的 `function_call` 或工具发现调用。

## 当前边界

- 代理不会凭空提供搜索服务。客户端路径需要配置搜索 MCP（如 Tavily/Exa）并允许 Codex 加载、执行该工具。
- `pause_turn` 表示上游需要续传；当前桥接器显式报告不支持续传，不伪装成正常结束。
- 修改搜索转换后，需验证流式与非流式、工具懒加载、搜索结果回传后的最终回答，以及缺少执行器时的错误行为。
