# 普通发送丢回包恢复独立验收

日期：2026-11-02，Asia/Shanghai。业务材料时间为北京时间 16:43:54—16:49:23；JSON 原始时间为 UTC。

**结论：通过本次普通纯文字发送的成功回包丢失恢复子场景。** 原生只接受一个准确客户端消息，完整历史 15→16 轮且原 15 轮逐字段保持；网页在原发送响应被中止后 GET 同一原回执，没有再次发送；随后整页刷新零追加 send，最终回复、新草稿及可用发送按钮均有证据。此结论增加 R3-01 的直接真实证据，不代表整条基线、完全离线恢复或完整三轮已通过。

本次独立复核只读取归档及源码，仅新增本报告。没有操作 UI、CDP、服务或任何会话，没有重发消息、运行业务测试、构建、提交或派发代理。用户会话 `01a18af8-e152-7a30-b981-4381796dc23b` 及其队列未作为本次写测试对象。

## 1. 对象及运行身份

以下原始材料均位于 `dist/validation/remote-acceptance-20261102/send-recovery-final/`；除另行注明外，JSON Pointer 相对于对应文件根。

| 对象 | 准确值与原始定位 |
| --- | --- |
| 测试会话 | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`；`before.json`、`after.json` 的 `/threadId` |
| 客户端消息 | `e53e9cb4-cc31-44e3-adec-e98cdb14c34d`；`accepted-paused.json` 的 `/receipt/result/clientUserMessageId` |
| 原操作回执 | `7e3b33b0-8074-4be4-9d40-7e1267d69394`；`accepted-paused.json` 的 `/receipt/requestId`，也等于原请求 `idempotency-key` |
| 准确执行轮 | `01a19b6e-378e-7581-bc64-d399a902e210`；`/receipt/result/result/turnId` 与 `after.json` 的 `/history/turns/15/turnId` 相同 |
| 原生用户项 | `01a19b6e-37cc-7252-b85a-3ad214377a7e`；`after.json` 的 `/history/turns/15/items/0/id` |
| 网页对象 | Chrome tab `1283922757`；网络事件 `/events/*/source/tabId`。发送 origin 为 `http://127.0.0.1:1432` |
| 原生适配器 | 两份历史快照 `/fixture` 完全一致：源码 117、兼容标记 106、fixture `b8efa7cb-1124-44d8-8335-7981c1916f88`、epoch `193954af-d947-4ac4-8ba3-a5980df952b3`；`/adapterVersion=106` 是兼容标记 |

起始实际脚本见兄弟目录 `sync-question-final/native-final117-before-identity.json` 及 `native-final117-before-runtime.js`。重新计算实际脚本 SHA-256 为 `5e3ac1d40fc337c6246ef11e875900b7c7987eaa2c96fc0c0d70880e41c4125d`，与该 identity 的 `/actualSha256`、`/expectedSha256` 一致。源码 SHA-256 为 `4ce7129ea8d11da315b622bc2c973837edadd6b4e5a3df4ccff9b2db8e45232a`。

实际脚本除移除重复加载返回、版本改为 106、将自动侧边命名限定于测试持有对象，以及附加 `sourceURL` 外，与该 117 源码逐行相同；没有把临时运行变体写成正式安装版。兄弟目录 `browser-app.js` 与当前网页源码逐字节哈希一致，SHA-256 为 `072afb6e66f350188bf505e9520c8538b64529abca30825c6ccc3b25c2788a7f`。本报告尚未纳入发送结束后重新提取的最终脚本身份，不能宣称已证明整个后续时段没有换版。

## 2. 原始证据及判定

| 验收点 | 独立核对结果 | 原始文件与精确定位 |
| --- | --- | --- |
| 派发前后完整历史 | 两份 `isComplete=true`，15→16 轮；将原 15 个完整轮次分别与后快照相同索引比较，全部字段保持，未去除时间或权限等字段 | `before.json`、`after.json`：`/history/isComplete`、`/history/turns/0` 至 `/history/turns/14` |
| 原输入及身份只接受一次 | 原历史没有该 client；后历史只有一个对应 `userMessage`，位于新增第16轮，`params.clientUserMessageId` 与项的 `clientId` 相同。`params.input` 与用户项完整 `content` 相等，均为一个 text、空 `text_elements` | `after.json`：`/history/turns/15/params/clientUserMessageId`、`/history/turns/15/params/input`、`/history/turns/15/items/0` |
| 请求正文没有被替换 | 原请求 prompt 为 `[remote-e2e:RA-20261102-send-recovery:normal] 本轮仅验收消息回执恢复。只回复 SEND_RECOVERY_OK，不调用任何工具。`；原生接受文本准确等于该 prompt 加一个终止 LF，没有借用其他同文轮次 | `accepted-paused.json`：解析 `/event/params/request/postData`；对照 `after.json`：`/history/turns/15/items/0/content/0/text` |
| 原生完成 | 准确 turn 为 `completed`、`error=null`；本轮仅有 userMessage 和 agentMessage，最终答复准确为 `SEND_RECOVERY_OK` | `after.json`：`/history/turns/15/status`、`/history/turns/15/error`、`/history/turns/15/items/1/text` |
| 成功回包被拦在响应阶段 | 同一 send 的 Fetch pause 已有 HTTP200；将 `/raw/body` Base64 解码后，与 `/receipt` 逐字段相同：`completed/ok=true/status=sent/kind=start`，绑定上表原 ID。它证明回包丢失发生时服务已接受，不能称为未派发失败 | `accepted-paused.json`：`/event/params/networkId=74908.659`、`/event/params/responseStatusCode=200`、`/raw`、`/receipt` |
| 原发送在网页端未正常完成 | 唯一 send request `74908.659`，其后是 `net::ERR_ABORTED`；该 request 没有 `Network.responseReceived` 或 `Network.loadingFinished` | `network.json`：`/events/0`（sequence2355），`/events/84`（sequence2540） |
| 查询同一原回执恢复 | 中止后 GET 原路径 `/api/receipts/7e3b33b0-8074-4be4-9d40-7e1267d69394`，HTTP200；查询体解析结果与暂停时原成功回执逐字段相同，不是重新创建的请求 | `network.json`：`/events/85`（request `74908.689`、sequence2541）、`/events/86`（sequence2542）；`original-receipt-read.json`：解析 `/body` |
| 恢复期间零重发 | 112条网络事件，`hasMore=false/truncated=false`；按 `Network.requestWillBeSent` 计数，`POST /api/action/send` 恰好1次，其他写请求仅1次 markRead；原回执 GET1次 | `network.json`：`/events`、`/hasMore`、`/truncated`，sequence2355—2593 |
| 真刷新后零追加 send | 第二段290条事件，`hasMore=false/truncated=false`；有一次新 Document，loader 已更换；该段 `POST /api/action/send=0`，全部 `/api/action/*` 请求也为0 | `after-reload-network.json`：`/events/7`（sequence2610、新 loader `A4BF365E29E0DA5971BF1A5676FE3B0E`），全部 `/events` 覆盖 sequence2594—3179 |
| 新草稿和发送控件 | `new-draft-before-fault.txt` 全文与刷新后 `/draft` 逐字相同，包括文件原有结尾；该草稿标记在完整已接受用户历史出现0次；`sendEnabled=true`。DOM显示准确新回复及未禁用的“发送消息”按钮 | `after-reload.json`：`/draft`、`/replyVisible=true`、`/sendEnabled=true`；`after-reload-ui.txt:400`、`:406`、`:418`；`after.json` 全部用户项 |
| 队列终态 | 两份服务器快照 `/queue/data=[]`，`/queue/nextCursor=null`；没有把队列空单独当作唯一接受证明 | `before.json`、`after.json`：`/queue` |

两段网络按请求事件计数，不重复计算 Fetch pause，也不将 markRead 算作再次发送。第二段从 sequence2594 开始，恰接前段 cursor2593；它包含刷新前少量后续读取及新文档加载，但全段均没有额外业务写请求。刷新 GET 原回执为0：本例在刷新前已完成原回执查询，不能据此写成“带未决 send 锁跨刷新恢复”。

`new-draft-before-fault.txt` 本身只有文本，没有独立采集时间或 threadId 字段。因此本报告确认的是归档新稿值与刷新后实际编辑器值完全一致、没有误发；不以文件名推导故障前键入的精确时刻，也不宣称每个过渡瞬间都已留存草稿快照。

## 3. 未验界限

- 事件、历史和其他读取在发送响应暂停期间仍可工作：`network.json` 的 `/events/15`、`/events/39` 是 history 的HTTP200，且原生在发送响应中止前已完成本轮。本次不覆盖断开全部通道、完全离线、原生断连、超时永久卡住或 Android 前后台恢复。
- 原回执可查询且立即返回 completed；没有真实 failed/404、长期 pending/unknown 或跨刷新保存未决锁的材料。不能将本例扩大为所有 unknown 收敛路径通过。
- 本次只有普通纯文字，未验证图片、Skill、隐藏上下文、多端竞争、双击发送、业务明确失败保稿或换授权后的迟到回执。
- 新 Document 后没有再次采集完整原生历史；刷新没有额外 send 有完整网络证据，但“刷新后仍为16轮”不是本报告新增的原生采样结论。
- 没有以源码测试数、旧轮结果或其他问答恢复材料替代本例；未认定 R3-01 所有分支、完整三轮、正式安装交付或公开发布完成。

## 4. 本次读取材料摘要

以下摘要已同步到HTTP鉴权头脱敏后的文件；仅敏感头值发生改变，业务请求、回执正文与事件顺序保持。具体前后摘要及JSON路径见 `dist/validation/remote-acceptance-20261102/evidence-http-redaction-20261102.json`。

| 文件 | SHA-256 |
| --- | --- |
| `before.json` | `1f20defd4f2f1493e9e998984d93789f849fd7b71eff47df80b181468866ecc5` |
| `after.json` | `b0b9d7fa051ac7002478883c52c7ca1f0fe556310b75c933a8bb56f5fa39aeca` |
| `accepted-paused.json` | `a72f7c936703b0acafd811884947471ffcbe07dc0f706470e2bedfc07a9f33b0` |
| `network.json` | `bfdd7e25247652cf517fe1c73b8590a163fbd546fdb7074bed0364d45cd11c83` |
| `original-receipt-read.json` | `c266c75f6883772942f3869d2f51a1459fb149929fc02701c52227b5e09343a0` |
| `new-draft-before-fault.txt` | `f43639587dd0ab5a4be094fefc7345ee37a79b60c2d2980b451f161597c1dc85` |
| `after-reload.json` | `e37d3ec132e4c48a15b344b245311c5f81e34afc6cbe8fad7265b9352c40400f` |
| `after-reload-ui.txt` | `0010957c4e1b2b7cd4b7ac3a2bf70fd7b3c63cf89495a8ec19db721ee18b8c56` |
| `after-reload-network.json` | `cca7e5316f143c694d8d20619e1c4aefe9e45e2eaaac73938418ed71f3a75f59` |

## 5. 运行身份与收尾补充

与本例共用测试环境的 `sync-question-final/native-final117-before-identity.json`、`native-final117-after-identity.json` 已补齐：实际117业务源码/106兼容标记变体的起止SHA-256均为 `5e3ac1d40fc337c6246ef11e875900b7c7987eaa2c96fc0c0d70880e41c4125d`。当前native.js源文件、浏览器实际app.js均与验收所用源码匹配，见该目录 `final-evidence-check.json`。

全部测试结束后已恢复测试会话原完整设置和安装版106适配器，移除本轮拦截、清空本轮明确标记草稿并关闭两个测试页、停止1432服务。Windows与Android产物未变化，未重新构建、安装、提交或发布。环境与原件定位见 `remote-sync-question-20261102.md` 的收尾段；这里不追加未经本例证明的完全离线或长期unknown结论。
