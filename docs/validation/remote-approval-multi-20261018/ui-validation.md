# 远程授权与多问答 UI 验证

验证日期：2026-10-18。

## 已完成

- 本机原生 `QB`、`C5t`、`r5t`、`Hz` 与 `mcp-request-card` 的识别和响应合同已读取核对。
- `mcp_tool_call` 批准卡按 `_meta` 中的明确字段识别，不再要求填写 JSON。只有请求声明 `always` 或 `session` 时才显示对应选项。
- 原生浏览器批准的精确分支要求 `mode: form`、`serverName: browser | browser-use`、`requestedSchema.properties` 为空、合法且存在的 `persist`、非空 `_meta.origin`。普通表单继续保留 JSON 输入能力。
- 较高风险来自 `_meta.riskLevel: high`；批准响应不加入其他网站或额外范围字段。
- 高风险卡保留风险、原始 message、可选 subtitle 和按钮，移除重复的 connector/tool 身份与网站行。有效 `tool_params_display: []` 隐藏参数详情；有效非空条目仅显示其声明的值和名称；显式无效声明保守隐藏，只有缺省声明时才回退原始参数。
- 多请求按设备、会话、来源版本、请求 ID、方法、参数分别维护卡片；一项提交只锁定自身。卡片复用采用现有 `reconcileChildren`，保持其他卡片、滚动和正在填写的内容。
- 阻塞问答和普通 MCP JSON 表单草稿在卡片重建后恢复；异步问题回执状态参与重绘签名。单项包含多个问题时全部显示、全部提交。
- 多请求显示数量提示并提供最多 `55dvh`、`560px` 且受实际可用高度限制的内部滚动区域。

## 检查结果

- `node --check assets/remote-access/app.js`：通过。
- PostCSS 解析 `assets/remote-access/style.css`：通过。
- `git diff --check -- assets/remote-access/app.js assets/remote-access/style.css`：通过；仅输出既有 LF/CRLF 提示。
- 当前源码函数在 Node VM 和最小 DOM 模型中的 20 项检查全部通过。详细结果见 `ui-source-checks.json`。
- 最终显示修正的 5 项定向检查、JavaScript 语法和差异检查通过，见 `ui-display-checks.json`。此轮仅修改显示模板和参数显示选择，复用此前 20 项检查及主代理的 7 项请求链路、拒绝按钮浏览器证据。
- 旧代码对比复现四项缺陷：原生工具批准显示 JSON 输入、异步卡片被整体重建、断线时阻塞控件仍可点击、权限变化后阻塞问答草稿丢失。
- 所有验证使用合成请求，未批准真实工具调用，未启动浏览器或服务。

## 集成验证

`ui-fixture-contract.json` 提供已核对合同的合成请求。真实浏览器建议同时挂载两个不同 ID 的 `toolApproval`、`blockingQuestion`、`asyncQuestion`，验证：

1. 桌面和移动端均显示四项数量，内部滚动可访问每张卡片；阻塞问答和异步问答都显示两个问题。
2. 在后两张卡片输入内容，提交首项工具批准；确认第二项仍可处理，后两项草稿、焦点和滚动保持。
3. 仅从快照移除已完成的首项，其他三项仍显示；完成一次问答仅移除对应请求。
4. 分别点击“允许一次”“始终允许”“拒绝”，响应严格等于 fixture 的 `expectedResponses`，重复点击只提交一次；无 `persist` 时不出现持久授权按钮。
5. 断线、只读、无审批权限、回执结果未知时保留门禁；恢复连接后草稿保留。
6. generic browser 精确分支显示批准按钮；带真实表单字段的普通请求仍显示原有 JSON 输入。

这些源码检查不能替代实际浏览器、原生请求聚合和原生授权往返验证，集成结果由主代理统一记录。

## 明确边界

`codex_apps` 的数据库执行绑定专用 connector（`connector_20205bf7d4e99a89d7154bb849718324`）需要独立的执行绑定验证。本次保守提示在 Codex 桌面处理，不实现该合同，也不授予持久权限。不完整或非法的 `mcp_tool_call` 元数据同样保守提示桌面处理。
