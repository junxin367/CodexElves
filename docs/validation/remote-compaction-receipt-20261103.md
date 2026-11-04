# 上下文压缩成功回包丢失与刷新恢复：独立复核

日期：2026-11-03，Asia/Shanghai。本文只复核已归档的两次压缩，不重新执行压缩、发送、排队、查回执或其他现场动作，不修改产品、配置及总验收表。

执行方收尾补充：`runtime-capture-after-side-close-compaction.json` 已捕获结束原生摘要与安装起点一致，`app-runtime-after.js` 与起点网页正文逐字相同；`runtime-restored.json` 验证原119全文恢复，新epoch `a28d96f2-154f-437c-8746-fe505e23c0c2`。浏览器拦截已清除，原STOP草稿保留在1283922838；原标签1283922800在最终列表中缺失，原因未观测，不声称检查了该旧标签。详见 `browser-restored.json`。

**结论：两次真实压缩均有同一 turn/item 的完成证据；第二次命中“客户端 unknown 尚未完成即刷新，随后依据原生状态恢复，零重复压缩”的子场景。** 第一次丢回包前压缩已完成、本地待确认记录为空，不能计为未决跨刷新恢复。第二次恢复采样前的原回执 GET 数为 0，不能写成“刷新后自动 GET 原回执解锁”。末尾两次 GET 是执行方在终态之后的额外诊断。

R1-24 与 R3-12 仍为 `partial`：本次补充正常完成、成功回包丢失及未决跨刷新原生状态收敛；真实失败/拒绝、原回执驱动的未决恢复、非空队列、后续实际发送和 Android 真机均未覆盖。

## 1. 原件、身份与独立校验方法

证据根目录为 `dist/validation/remote-acceptance-20261103/side-close-compaction/`，下文路径均相对此目录。只使用 `compaction-` 前缀、`compaction-r2/` 和运行身份材料，不使用同目录的侧边创建、关闭原件补足压缩结论。

透明校验结果：[compaction-independent-verification.json](../../dist/validation/remote-acceptance-20261103/side-close-compaction/compaction-independent-verification.json)。独立脚本解码原 Fetch body 后核对原响应、关联网络身份、比较完整持久历史及普通草稿，**41 项断言全部通过**。这些是对已有原件的校验，不是新执行的端到端用例或产品测试。

| 对象 | 本次读取的事实 | 边界 |
| --- | --- | --- |
| 主会话 M | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` | 两次压缩均准确归属 M |
| 控制会话 C | `01a1996d-0b7a-71d0-b9ad-9c521d224ad9` | 核对其持久历史与队列，不把 loaded 空集合称为已完整加载 |
| 归档网页文本 | `app-runtime.js`，SHA-256 `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387` | 本次重算归档文件字节；未重新连接网页 |
| 原生业务源码 | 121，源码 SHA-256 `54b79c2095b7486ce21a580fe61a571a90c6f2cf95d88f0de9350ec722e54c73` | 来源为快照中的夹具登记 |
| 实际原生运行文本 | `runtime-installed.json /after/actualRuntimeSha256` 为 `4543c4728aa5123a142e771722ca4fcaf1dd16d74eef7084805e3b3ae10eeab2`；重算 `native-test-121.js` 得到相同值 | 121 业务源码使用 106 兼容标记；不将兼容数字 106 当成业务源码版本 |
| 运行 epoch | `1fe806bb-39a4-45f9-9ba2-f11bb54286bd` | 五份压缩原生快照均与安装核对记录相同 |
| 原普通稿 | `STOP_LIVE_2_UNSENT_DRAFT_保留` | 基线、第一轮终态、第二轮 unknown、刷新后及最终采样逐字相同 |

夹具对象另含 `runtimeSourceSha256=97830d105abe4712cf5d6220bbf67247139bebe8f123dd9811cc34afb95f106e`，它是未附本轮 `sourceURL` 尾注的兼容正文摘要；完整执行文本摘要为4543c472…。两者摘要范围不同，不将正文摘要冒充完整执行摘要。结束捕获身份与恢复结果由后补收尾记录独立证明，不仅依据相同epoch推断。

复核方法：

1. 按 `body.base64Encoded` 解码两份原 Fetch 响应，再解析 JSON，与同文件 `receipt` 精确比较；不把人工整理的 receipt 字段单独当成原 HTTP body。
2. 六份压缩网络事件按 `sequence` 去重；用精确 URL 路径、POST 正文、幂等键、Network ID 和 Fetch ID 关联原动作，分别统计 compact、markRead 和诊断 GET。不比较所有请求头，也不把认证信息复制到校验结果。
3. 原生完成以准确 T/I 的运行、完成字段及时间戳为准；HTTP `receipt.state=completed` 仅表示调用返回。`compaction.state=idle` 本身也不足以区分成功、失败和中断。
4. 读取 `threads[id].persisted.thread.turns` 比较原轮次前缀，并按原顺序投影全部用户消息的 `turnId` 和完整 item，核对身份、内容、数量及顺序。散列序列化口径见透明校验 JSON。

## 2. 两个独立原请求

两次写动作均由现有压缩入口提交：

```text
POST /api/action/compact
Idempotency-Key: A
{"threadId":"01a19a01-ebb4-7ee1-a8e1-90d180656ac1"}
```

| 身份 | 第一次 | 第二次 |
| --- | --- | --- |
| A：原回执/幂等键 | `51cf625b-b1e8-4a8b-9d9c-2a597fc33459` | `ba7bf331-c09b-4ead-a0ed-3aabcacc0603` |
| T：原生 turn | `01a19edf-a930-7db1-a978-61e5466b0708` | `01a19ee4-2b31-77a3-b45c-f292fdf72d2e` |
| I：原生压缩 item | `01a19edf-a953-7f21-8717-ea9aa5b6ff80` | `01a19ee4-2b58-7343-959a-f4708d0cc411` |
| Network ID | `26108.313` | `26108.390` |
| Fetch ID | `interception-job-58578.0` | `interception-job-58759.0` |
| 原 POST sequence | 1554 | 2024 |
| Response 暂停 sequence | 1559 | 2033 |
| 原网络失败 sequence | 1701 | 2034 |

两份 `accepted-response.json` 均证明原 HTTP 为 200，body 中 `requestId=A`、`state=completed`、`result.ok=true`，但 `result.compaction.state=running`，T/I 与上表一致。两份 `response-dropped.json` 及相同 Network ID 的 `Network.loadingFailed` 共同证明原响应被 `Aborted`，浏览器终态为 `net::ERR_ABORTED`、`canceled=true`。这不是伪造失败 body，也不是请求派发前取消。

六份原网络事件均登记 `truncated=false`、`hasMore=false`。去重后的动作统计为：

| 动作 | 数量 | 解释 |
| --- | --- | --- |
| `/api/action/compact` | 2 | 每个原 A 各 1 次；没有同 A 或新 A 的自动重压 |
| `/api/action/markRead` | 2 | 原生压缩结束后的自动已读，各有独立幂等键；不能把本次描述为所有 action 只有 2 次 |
| send/create/queue/目标等其他业务动作 | 0 | 限六份压缩事件账本的捕获窗口 |
| `/api/receipts/A` | 2 | 均发生于恢复终态之后，详见第 5 节 |

## 3. 第一次：正常完成且成功回包被丢弃，未命中未决刷新

| 时间（Asia/Shanghai） | 原件事实 |
| --- | --- |
| 08:47:00.395 | T1 开始 |
| 08:47:00.435 | I1 开始，`type=contextCompaction`、`source=manual` |
| 08:47:13.259 | `compaction-running.json` 的 loaded T1 为 `inProgress`，I1 为 `completed=false`、完成时间为空 |
| 08:47:28.694 | 同一 I1 的实际 `completedAtMs`；持续 28,259 ms |
| 08:47:52.334 | `compaction-before-drop-state.json` 已记录普通稿保持、`record=null` |
| 08:47:52.390 | `compaction-response-dropped.json` 记录中止原 Network `26108.313` |
| 08:50:22.615 | `compaction-first-final.json` 独立读到 T1=`completed`、error=null，I1=`completed=true` |

第一次的开始/终态使用相同 T1/I1，并非初始 idle、旧压缩记录或 HTTP 受理成功推定。`compaction-first-final-ui.json` 显示一条“上下文已压缩”、原普通稿和可用发送控件。

但 I1 在原响应被中止之前已经完成；`before-drop-state` 的本地待确认记录已为空。因此第一次只能支持“正常完成＋原成功回包丢失未造成重压或丢稿”，**不计作客户端 unknown 跨刷新恢复**。

第一份运行快照还有一个投影差异：持久 T1 已存在但 items 仍为空；loaded T1 已有准确 I1、`completed=false`。运行态判据使用后者；后续持久终态也保存了同一 I1 的开始和完成时间，不能因运行时持久 items 尚空就认定未执行。

## 4. 第二次：unknown 跨新文档，原生状态恢复且没有重压

第二轮紧接第一轮，基线 T/I 为已完成的 T1/I1。原 POST、暂停成功 body、Aborted 及 unknown 状态采样归属同一 A2，没有另发替代请求。

刷新前的真实本地记录见 `compaction-r2/before-refresh-state.json /state`：

```json
{
  "state": "unknown",
  "threadId": "01a19a01-ebb4-7ee1-a8e1-90d180656ac1",
  "receiptId": "ba7bf331-c09b-4ead-a0ed-3aabcacc0603",
  "acknowledged": false,
  "turnId": null,
  "itemId": null,
  "baselineTurnId": "01a19edf-a930-7db1-a978-61e5466b0708",
  "baselineItemId": "01a19edf-a953-7f21-8717-ea9aa5b6ff80"
}
```

此处省略 deviceId，原件及透明校验 JSON 保留准确授权身份。记录中未确认的 T/I 为 null、baseline 为上一轮 T1/I1；不能拿页面上第一条“上下文已压缩”作为第二次成功证据。当时 UI 同时显示“压缩结果仍待确认”“查回执”“重新读取”，发送按钮 disabled，原普通稿仍在。

| 时间（Asia/Shanghai） | 事件与身份 |
| --- | --- |
| 08:51:55.764 | 原 POST A2，Network `26108.390`，旧 loader `E13B146BD0A0535BF00403D871700272` |
| 08:51:55.820 / 55.864 | T2 / I2 开始 |
| sequence 2033 → 2034 | 原成功 running 响应暂停，随后同 Network ID `net::ERR_ABORTED` |
| 08:51:56.290 | 保存 `unknown` 原 A2、旧 baseline T1/I1、未变普通稿 |
| 08:51:56.346 | `refresh-events.json` 新 Document 请求开始；loader 为 `B3EB14570503C87931146A0E6AEC2AE9`，与旧页不同 |
| 08:51:57.614 | 新 loader 发起 `26108.440` 的 M thread 读取；保存的真实响应为 T2/I2 running、I2 completed=false |
| 08:51:58.024 | 新 loader 的 `26108.446` 同样返回准确 T2/I2 running |
| 08:52:08.193 | 原生 I2 实际完成，持续 12,329 ms |
| 08:52:09.334 | 新 loader 的 `26108.452` 发起 M thread 读取；真实响应为同 T2/I2 idle、I2 completed=true |
| 08:52:41.223 | `after-refresh-state.json` 记录 pending 为空、原普通稿保持；UI 无 unknown 提示、发送控件可用 |

表中三个 thread 时间均为对应 `Network.requestWillBeSent.wallTime`，不是冒充响应完成时刻。原响应 body 与各 Network ID 的配对保存在 `after-refresh-thread-responses.json`；三次请求正文都准确为 M，loader 都属于新文档。

unknown 状态采样比新 Document 请求早约 56 ms，且远早于 I2 的完成时间，确实命中“尚未完成时刷新”。之后新文档独立读到 running 与 completed，不是只看到旧条目或刷新后的初始 idle。

`compaction-r2-running.json` 于 08:52:12.745 采样，实际已是 idle、T2 completed、I2 completed=true。**文件名不能改变字段事实，本报告未将该文件作为运行中证据。** 第二轮运行证据来自原成功受理 body 及新文档前两份 thread 响应。

恢复采样前没有 `/api/receipts/A2`，刷新后也没有任何 compact POST。可确认客户端沿原生状态读取/生命周期机制收敛；现有原件没有逐次客户端 `setCompactionState` 写入日志，不能进一步唯一归因于哪一条 thread 返回或 SSE 事件最先解除锁定。该边界不影响“新文档恢复且未重压”的结论。

## 5. 末尾 GET 的性质：诊断原受理回执，不是压缩终态或恢复来源

`compaction-final-receipts.json` 明确登记 `purpose=post-terminal-diagnostic-read`：

| 时间（Asia/Shanghai） | GET / 原回执 | 网络身份 |
| --- | --- | --- |
| 08:53:53.944 | A1 | `26108.477`，sequence 2480 |
| 08:53:53.979 | A2 | `26108.478`，sequence 2486 |

两次 GET 均晚于 08:52:41.223 的 pending 清空/原稿恢复采样。返回内容仍与最初受理回执一致：`state=completed`、`result.compaction.state=running`、各自原 T/I。这是原动作的固定受理快照，不会因为压缩后来完成而自动改写。

因此：

- 不能把其中的 `running` 写成最终仍在压缩。
- 不能把这两次 GET 写成刷新后自动查询、通过查回执解除 unknown，或用户点击了查回执按钮。
- 这两次查询能证明准确原 A 仍可读、没有改用新 A；真正完成由第 3、4 节的原生同 T/I 证据证明。

## 6. 原用户消息、控制会话、草稿与最终可用性

独立比较 `compaction-baseline.json` 与 `compaction-final.json`：

| 对象 | 前后结果 | 校验 |
| --- | --- | --- |
| M 持久轮次 | 44 → 46 | 原 44 轮前缀逐字段完全相同；仅新增 T1、T2 |
| M 用户消息 | 46 → 46 | 按原顺序的 turnId、完整 userMessage item 精确相同，身份无重复；压缩没有新增或误删用户消息 |
| C 持久轮次 | 8 → 8 | 完整 8 轮精确相同 |
| C 用户消息 | 7 → 7 | 完整身份、内容及顺序精确相同 |
| M/C 队列 | 均为空 → 均为空 | queue 对象精确相同；没有原非空队列可据此验收 |
| M/C 原配置 | `metadata.latestThreadSettings` 相同 | 不将 token/时间等压缩预期变化当成配置变更 |
| M loaded 轮次 | 45 → 47 | 与持久轮次分开计数 |
| C loaded 轮次 | 0 → 0，`isComplete=false` | C 持久 8 轮可核；不能据 loaded 空集合称其完整加载 |

M 原用户消息投影 SHA-256 前后均为：

```text
95005b062222ebd580543c4a9d7a2a7e56a5d995da63be77ba16cb24e04cca8a
```

C 用户消息投影 SHA-256 前后均为：

```text
fe0f4aba6ce87fa24431b78f29d9b4c28fc2c4db5d85f0aef67ecaf70889b648
```

上述散列基于透明校验 JSON 记载的规范化 JSON 投影，不是原始大文件散列；原件各自的字节散列另列在 `sourceHashes`。

压缩改变当前上下文、用量和新增压缩记录属于测试预期。本次没有要求整个 thread JSON 不变；实际额外确认了原持久 44 轮前缀不变。

普通稿在五个检查点均为 `STOP_LIVE_2_UNSENT_DRAFT_保留`：基线、第一轮终态、第二轮刷新前 unknown、刷新后、最终。`compaction-r2/final-ui.json` 和已独立查看的 `final-ui.png` 一致显示两条“上下文已压缩”、原普通稿及可用发送按钮，没有 unknown 提示。**本次没有点击发送**，只证明普通输入/发送控件恢复可用，不扩为后续实际发送成功。

最终 `compaction-final.json` 为 M 持久 46 轮、运行状态 idle、队列空、待答请求空；C 持久 8 轮、队列空，其当前原生状态仍为 notLoaded，不将其称为已加载 idle。

## 7. 验收增量、未覆盖分支与现场收尾边界

| 基线 | 本次新增可证范围 | 仍未覆盖 |
| --- | --- | --- |
| R1-24 手动压缩 | 两次同 T/I 真正完成；原用户消息及普通稿保持；完成后输入和发送控件可用 | 压缩拒绝/失败、并发旧完成、完成后的实际发送及全部版本范围 |
| R3-12 压缩未知/断线/失败 | 第二次原成功回包 Aborted 后真实 unknown，未决状态跨新文档，独立原生读到 running→completed；0 重压、原稿保持 | 全断线、长期 unknown、GET 原 A 驱动恢复、原生真实失败显示、回执 404、人工核对解锁、非空队列及自然续跑 |
| 交付/平台 | 归档网页及临时 121/106 原生夹具身份明确 | 新包构建/安装、Android 桥接和真机均不因此通过 |

两份中止记录均登记 `interceptionCleared=true`，只能说明各自准确拦截已清除。本次独立复核没有连接浏览器、原生或服务，也没有执行恢复。原生准确恢复 119、服务/测试页收尾由主代理另行记录；本报告不提前宣告整个现场已恢复。

本次仅新增本文与对应透明校验 JSON，不修改产品、配置、其他报告或 73 条总状态。该子场景通过不等于 R3-12 整条、全部三轮或整体目标完成。
