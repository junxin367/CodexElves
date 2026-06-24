# 协议代理真实隔离测试手册

本文档用于验证“当前开发代码里的本地协议代理”是否真的能被 Codex CLI 使用，避免误连安装版代理或误用系统 `CODEX_HOME`。

## 固定原则

- 禁止把真实测试指向安装版本地代理。脚本默认把 `45221` 视为安装版保留端口；如果安装版端口变化，用 `-ReservedProxyPorts` 显式传入当前保留端口。
- 必须单独启动 dev helper，默认端口使用 `51555`。
- 必须复制配置到临时 `CODEX_HOME`，再把临时 `config.toml` 的 provider `base_url` 改成 dev helper 地址。
- 真实上游配置来自复制后的 `settings.json`，不要手工粘贴或打印 API Key。
- 所有日志、会话和复制出的配置只允许落到 `temp/`。
- `temp/` 不参与提交；脚本会校验运行目录在仓库内时必须被 Git ignore。里面可能含真实配置和真实工具调用证据。

## 一键脚本

默认从当前用户环境复制：

- Codex 配置：`$env:CODEX_HOME`，不存在时使用 `%USERPROFILE%\.codex`
- CodexElves 设置：`%APPDATA%\CodexElves\settings.json`

```powershell
.\scripts\dev-codex-real-smoke.ps1
```

完整回归建议在修改协议代理后执行：

```powershell
.\scripts\dev-codex-real-smoke.ps1 -IncludeClaude -IncludeGptControl
```

常用参数：

- `-Port 51556`：换一个隔离 dev helper 端口。
- `-ReservedProxyPorts 45221,57321`：设置安装版/生产代理保留端口。脚本会拒绝 dev helper 使用这些端口，也会检查临时 `CODEX_HOME` 是否仍引用这些端口。
- `-Model deepseek-v4-pro`：指定 Chat Completions 协议模型。
- `-IncludeClaude`：额外跑 Anthropic 协议 `claude-sonnet-4-6` 的 `web_search` 闭环。
- `-IncludeGptControl`：额外跑 GPT Responses 协议对照。
- `-ScenariosPath <json>`：加载自定义测试场景。
- `-Scenario name1,name2`：只执行指定场景。
- `-ExtraScenarioJson '<json>'`：从命令行临时追加测试场景。
- `-ListScenarios`：列出最终可用场景，不启动 helper，也不发起真实调用。
- `-SourceCodexHome <path>`：显式指定要复制的 Codex 配置目录。
- `-SourceSettingsPath <path>`：显式指定要复制的 CodexElves `settings.json`。

脚本会拒绝使用 `-ReservedProxyPorts` 中的端口，并在临时 `config.toml` 或复制出的 `settings.json` 中发现 `127.0.0.1:<reserved>`、`localhost:<reserved>` 或 `[::1]:<reserved>` 时直接失败。

## 标准覆盖场景

脚本默认覆盖：

- 普通回答：确认基础流式响应没有断流。
- 普通 MCP 工具：调用 `mcp__pal/version`，确认 namespace function 映射正常。
- `tool_search`：确认 Codex 原生 `tool_search_call/tool_search_output` 路径可用。
- `web_search`：确认非 GPT 模型能通过搜索 MCP fallback 执行真实搜索并继续第二轮回答。

可选覆盖：

- `-IncludeClaude`：验证 Anthropic 协议上游返回 `web_search` 后，Codex 能执行 `mcp_tool_call`，再继续生成最终回答。
- `-IncludeGptControl`：验证 GPT Responses 原生路径仍可用，没有被 fallback 改坏。

## 自定义场景

如果要测试新的提示词或回归场景，不需要改脚本。可以写一个 JSON 文件：

```json
[
  {
    "name": "deepseek-tool-search-pal",
    "model": "default",
    "prompt": "Use tool_search to find pal mcp, then answer one Chinese sentence with the namespace you found.",
    "expectedJsonlPatterns": ["tool_search"],
    "expectedLastPatterns": ["mcp__pal"]
  },
  {
    "name": "claude-web-search-pal",
    "model": "claude",
    "prompt": "You must call web_search to search pal mcp GitHub, then answer one Chinese sentence summarizing the result.",
    "enabled": false,
    "expectedJsonlPatterns": ["\"type\":\"mcp_tool_call\"", "tavily_search|web_search_exa|exa_search"],
    "expectedDiagnosticPatterns": ["finishReason.*stop|finish_reason.*stop"]
  }
]
```

执行全部启用场景：

```powershell
.\scripts\dev-codex-real-smoke.ps1 -ScenariosPath .\temp\proxy-smoke-scenarios.json
```

只执行指定场景，即使它在 JSON 中 `enabled` 为 `false` 也会执行：

```powershell
.\scripts\dev-codex-real-smoke.ps1 -ScenariosPath .\temp\proxy-smoke-scenarios.json -Scenario claude-web-search-pal
```

命令行临时追加一个场景：

```powershell
$scenario = @'
{
  "name": "custom-deepseek-normal",
  "model": "deepseek-v4-pro",
  "prompt": "Only answer OK. Do not call tools."
}
'@
.\scripts\dev-codex-real-smoke.ps1 -ExtraScenarioJson $scenario -Scenario custom-deepseek-normal
```

`model` 支持以下别名：

- `default` / `model` / `$Model`：使用 `-Model` 参数。
- `claude` / `claudemodel` / `$ClaudeModel`：使用 `-ClaudeModel` 参数。
- `gpt` / `gptmodel` / `$GptModel`：使用 `-GptModel` 参数。
- 其它值按真实模型名传给 `codex exec -m`。

场景可选期望字段：

- `expectedJsonlPatterns` / `expected_jsonl_patterns`：在 `codex-*.jsonl` 中必须匹配的正则。
- `expectedLastPatterns` / `expected_last_patterns`：在 `codex-*.last.txt` 中必须匹配的正则。
- `expectedDiagnosticPatterns` / `expected_diagnostic_patterns`：在 `helper-diagnostic.log` 中必须匹配的正则。

默认场景已内置基础正向断言：

- `pal-version`：要求 JSONL 出现 `mcp_tool_call`、`server=pal`、`tool=version`。
- `tool-search`：要求 JSONL 出现 `tool_search`，最终回答出现 `mcp__pal`。
- `web-search`：要求 JSONL 出现 `mcp_tool_call` 和搜索 MCP 工具名。
- `normal`：要求最终回答为 `OK`。

## 期望证据

每次运行会创建一个目录：

```text
temp/dev-codex-smoke-run/<timestamp>/
```

关键文件：

- `codex-*.jsonl`：Codex CLI 事件流。
- `codex-*.last.txt`：最终回答。
- `helper-diagnostic.log`：协议代理诊断日志。
- `codex-home/config.toml`：临时 Codex 配置，应该指向 `http://127.0.0.1:<port>/v1`。
- `settings/settings.json`：复制出来的真实 CodexElves 配置。

判断通过的关键点：

- 命令 exit code 为 `0`。
- 日志里没有 `unsupported`、`stream disconnected`、`response.failed`、`"type":"error"`。
- 默认工具场景的正向证据断言全部通过，例如 `web_search` 场景的 JSONL 中出现 `mcp_tool_call` 和 `tavily_search` 或 `web_search_exa`。
- Anthropic 场景的 `helper-diagnostic.log` 可看到第二轮最终 `finishReason` 为 `stop`。

## 修改协议代理后的推荐顺序

先跑自动化测试：

```powershell
cargo fmt
cargo test -p codex-elves-core --test protocol_proxy -- --test-threads=1
cargo test -p codex-elves-core --test launcher
git diff --check
```

再跑真实隔离 smoke：

```powershell
.\scripts\dev-codex-real-smoke.ps1 -IncludeClaude -IncludeGptControl
```

## 常见错误方向

- 直接请求安装版代理端口，例如默认 `http://127.0.0.1:45221/v1`：安装版代理不包含当前工作区改动。安装版端口变化时，要同步更新 `-ReservedProxyPorts`。
- 直接复用系统 `CODEX_HOME`：会污染真实会话和真实配置。
- 只测 `curl /v1/responses`：不能证明 Codex runtime 能执行工具 call。
- 只测 GPT：GPT Responses 原生服务端工具能工作，不代表 Chat/Anthropic 翻译路径能工作。
- 只看上游返回：必须检查 Codex JSONL，确认工具被 runtime 执行，并且第二轮回答完成。
