# 原生高风险授权与并发请求调查

调查日期：2026-10-18（Asia/Shanghai）。

状态：原生请求与响应合同核对完成；同一会话多请求枚举和逐项响应验证完成；用户当时只看到一个请求的历史运行场景未完整复放。

## 结论与范围

- 当前安装包能把截图中的 Chrome Developer Tools 请求明确识别为 MCP 工具授权。它通过 `_meta.codex_approval_kind="mcp_tool_call"` 和 `_meta.riskLevel="high"` 表示授权类型与风险，不是需要用户填写 JSON 的普通表单。
- `assets/remote-access/native.js` 原样保留这些参数，并通过 `replyWithMcpServerElicitationResponse` 回复。对本次问题，无须改变原生请求协议。
- `blockingRequestsSnapshot` 完整映射当前会话的 `conversation.requests`；`asyncQuestionRequests` 完整枚举最新进行中轮次的全部问题消息和每条消息的全部问题。隔离源码验证没有复现“仅保留一项”。
- 不把其他会话或已结束轮次混入当前会话。不添加自动授权，不改变 Chrome 权限或平台策略。

## 当前安装与运行证据

| 对象 | 结果 |
| --- | --- |
| Windows Appx | `OpenAI.Codex 26.1007.9446.0` |
| App 内部 package.json | `26.1007.71326` |
| 当前本机 CLI | `codex-cli 0.162.0-alpha.17.2` |
| 原生桥接运行版本 | `40` |
| 已缓存会话数 | `23` |
| 所有已缓存会话的待处理 requests | `0` |
| 所有已缓存会话的未完成 externalRequests | `0` |
| 最新进行中轮次包含的问题 | `0` |

本次重新读取当前安装的 `app.asar`。`app-initial-102e45562729.js`、`app-primary-cd52df2361eb.js`、`app-shared-30671c57ec82.js` 的 SHA-256 均与此前提取副本相同，随后才使用该副本定位函数。另从当前安装包重新提取了 MCP 请求卡片等相关模块。

证据保存在同目录 `native-evidence.json`，包括安装包路径、文件哈希、函数源位置与当前运行态只读摘要。没有操作真实待授权项。

## 高风险请求的生产与识别

当前 Chrome 插件 `26.1007.71326/scripts/browser-service.mjs` 的 `JR()` 直接产生与用户截图相同的文案：

```text
Allow Browser use to use full Chrome Developer Tools access on <origin>
```

它提供的元数据为：

```js
{
  codex_approval_kind: "mcp_tool_call",
  connector_id: "browser-use",
  connector_name: "<当前浏览器显示名>",
  persist: "<仅平台允许持久授权时为 always，否则省略>",
  riskLevel: "high",
  tool_name: "access_browser_origin_with_raw_cdp",
  tool_title: "Use raw CDP on browser origin",
  tool_params: { origin: "<本次请求的 HTTP(S) origin>" },
  tool_params_display: [],
  full_cdp_access: true,
  origin: "<本次请求的 HTTP(S) origin>"
}
```

原生 `app-shared` 的 `QB()` 在 `mode="form"` 时识别 `codex_approval_kind="mcp_tool_call"`。此元数据分支要求 `connector_id` 为字符串、`tool_params` 为对象；`persist` 支持 `session`、`always` 或相应数组。原生卡片 `ye()` 根据 `riskLevel==="high"` 显示“较高风险”，且只有请求提供的 `persist` 包含 `always` 时才显示“始终允许”。

普通有字段的 elicitation 表单、浏览器安全登录、邮箱验证码和工具建议有各自原生分支，不能只因方法名为 `mcpServer/elicitation/request` 就全部显示成批准按钮。

## 准确响应合同

调用仍为远程 `answer`，`method` 保持 `mcpServer/elicitation/request`，`requestId` 使用原始请求 ID。

| 用户操作 | response |
| --- | --- |
| 允许一次 | `{"action":"accept","content":{},"_meta":null}` |
| 始终允许 | `{"action":"accept","content":{},"_meta":{"persist":"always"}}` |
| 拒绝 | `{"action":"decline","content":null,"_meta":null}` |

这是当前原生 `mcp-request-card` 的 `ye()` 与 `app-shared` 的 `Hz()` 共同构造的实际字段。`persist` 位于响应 `_meta`，不能放在 `content` 中。

Chrome 的 `handleFullCdpPromptResult()` 把该结果交给 `{kind:"fullCdp", origin:<本次 origin>}` 资源处理。这里“始终允许”的范围仍为原请求资源与 origin。远程端不得附加 `browser_use_persistent_approval_scope:"all-sites"`；该字段属于原生另一种浏览器站点授权交互，当前 raw CDP 请求不使用它。

## 并发集合的实际路径

`blockingRequestsSnapshot()`：

1. 从 `manager.getConversation(id)` 取得所选会话。
2. 过滤合成的 `remote/requestUserInputAsync` 类型。
3. 对其余所有请求映射 `{id, method, params}`。
4. 以整个集合的 JSON 签名维护 `requestsEpoch`、`requestsRevision`。

这里没有取第一项、最后一项、按类型仅保留一项或跨会话聚合的分支。

`syncRequestNotification()` 将回调处理放到 microtask，目的是在原生移除已响应请求后读取完成变化后的集合。多个回调读取的是该会话的完整集合，而不是各自单独请求。对数字 `0` 请求 ID 的类型检查也会保留该 ID。

`asyncQuestionRequests()` 在最新 `inProgress` 轮次中对所有 `agentMessage` 使用 `flatMap`，再对每条消息的全部 `questions` 使用 `flatMap`。合成请求 ID 包含轮次和消息 ID，问题 ID 包含消息 ID 与问题序号。原生 `Q5t()` 同样选择最新进行中轮次；没有依据把已结束轮次重新纳入远程问题。

原生 `qH()`、`Bmn()`、`Vmn()` 也按全部消息与问题序号生成相同问题身份。`acceptedAsyncQuestionReplies()` 将已接受的回复映射回具体问题，不以“已有一条回复”删除同轮其余问题。

## externalRequests 与子会话

对当前 `app.asar` 中 14805 个非依赖 JavaScript 文件做了定向标记扫描。`externalRequests` 只出现在主进程和共享前端的同一套会话拥有权清理及状态读取代码中，没有找到生产这些问答、授权请求的路径；运行态未完成数量也为 0。因此不能据此宣称桥接漏收，也不增加第二个请求来源。

原生确实提供子会话拓扑接口，且不同会话各自持有 `requests`。这不证明当前问题来自子会话，也不证明应把所有子会话授权合入所选会话。未变更会话归属或公开请求 shape。

## 验证

`native-contract-results.json` 保存了 11 项通过结果。验证执行的是当前 `native.js` 源码，运行在 Node VM 中，`manager` 由纯内存替身提供。没有调用真实原生响应接口。

- 同一会话三个阻塞请求同时保留。
- 高风险原始参数完整透传。
- 两条异步消息的三个问题同时保留且身份不同。
- 接受一个问题的回复后，只移除对应问题。
- canonical 与 legacy 历史枚举一致。
- 已结束轮次和其他会话不会混入当前问题。
- 同一批通知在完成原生变化后读取完整集合。
- 通知保留所有请求及数字 `0` 请求 ID。
- 允许一次、始终允许、拒绝的响应逐字段保留，且只消除所选请求。

本轮没有修改 `assets/remote-access/native.js`，没有安装、重启正式服务、提交或推送。临时验证脚本在完成后删除，仅保留报告和 JSON 证据。

## 未验证边界

用户截图对应的真实待授权请求当前已不在缓存集合中，未取得其当时的完整实时快照。协议识别和响应字段由当前生产者、原生解析器、原生卡片与响应构造函数交叉核对；用户当时“同时触发但远程只看到一个”的全部实际事件顺序尚未完整回放。前端多卡交互和草稿稳定性应由对应浏览器对照验证单独说明。
