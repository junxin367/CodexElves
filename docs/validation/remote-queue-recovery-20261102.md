# 原生队列保存、取消丢回包与自然 FIFO 验收（2026-11-02）

## 结论与范围

本轮原件独立复核结果为 **PASS（限定以下子场景）**：原生已保存 B、已取消 C 编辑之后，对应 HTTP 成功回包被中断；网页分别读取同一原请求回执恢复。B 的图片、Skill 和修改正文完整保留，C 的未保存修改没有进入队列或被执行。真实同步问答结束后，原生自然按 B → C 各执行一次，最终会话空闲、队列与活动编辑为空，普通草稿仍在。

这不是所有队列恢复分支通过。最早 B/C 发送至开始编辑 B 的完整 HTTP 账本未保留，不能报告全程发送或 begin 次数；本轮未断开全部网络或 SSE，不覆盖离线恢复。原生运行身份为 117 业务源码、106 兼容标记的临时适配器，不代表正式安装版 117 已验收。

复核只读取已有 JSON、文本和归档脚本，逐字段比较原生历史与队列、解码图片字节、关联原请求与回执，没有重放请求、操作 UI/CDP、改动服务、产品代码或测试。此次只新增本文件。

## 对象与运行身份

原件目录：[`dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/)。

| 对象 | 准确身份 |
| --- | --- |
| 真实测试会话 | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` |
| 保持 busy 的同步问答轮 | `01a19b79-2b01-71d0-99d2-127dc85d5654` |
| 同步请求 | `item/tool/requestUserInput`，请求 `2`，问题 `gate_continue` |
| B 逻辑 client | `3e6a42db-8def-444a-a7ca-61aa99c72028` |
| B 原物理 ID → 保存后物理 ID | `01a19b7b-eef9-7362-bb9b-3465df350ed8` → `01a19b90-c09d-7451-9039-55ed3e219c14` |
| B 编辑事务 | `d0d803e6-99ca-4ef7-9102-12f35e802c34` |
| C 逻辑 client | `9e964d43-3c53-4efc-8efd-90059844a7ed` |
| C 原物理 ID → 取消恢复物理 ID | `01a19b7c-d12d-7bb3-97b0-38ebcf990740` → `01a19b95-d125-7940-9598-bf3f3b6366ab` |
| C 编辑事务 | `81adb5b6-6137-4ce0-a1da-a62e35588895` |
| 临时适配器 fixture | `b8efa7cb-1124-44d8-8335-7981c1916f88` |
| 原生运行 epoch | `193954af-d947-4ac4-8ba3-a5980df952b3` |

本轮六份原生快照的 `/fixture` 完全相同，均声明 `sourceVersion: 117`、`compatibilityVersion: 106`；`adapterVersion` 为 `106`。运行前独立脚本原件沿用 [`sync-question-final/native-final117-before-identity.json`](../../dist/validation/remote-acceptance-20261102/sync-question-final/native-final117-before-identity.json) 与对应的 [`native-final117-before-runtime.js`](../../dist/validation/remote-acceptance-20261102/sync-question-final/native-final117-before-runtime.js)，实际文件 SHA-256 为：

```text
5e3ac1d40fc337c6246ef11e875900b7c7987eaa2c96fc0c0d70880e41c4125d
```

该身份记录中的业务源码 SHA-256 为 `4ce7129ea8d11da315b622bc2c973837edadd6b4e5a3df4ccff9b2db8e45232a`。本次复核时尚未取得运行后脚本摘要，当前只确认运行前脚本文件及各阶段相同的 fixture 声明，不能据此宣称运行前后脚本字节完全一致；运行后身份由主验收补充。

## 独立核验结果

### 1. 真实 busy 前提与历史保留：PASS

[`turn-start-request.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/turn-start-request.json) 与 [`turn-start-receipt.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/turn-start-receipt.json) 关联同一会话、client 和原生 turn；请求使用 Plan 协作模式生成真实同步问题，没有通过修改队列设置暂停调度。

从 [`queued-before-edit.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/queued-before-edit.json) 到 [`canceled-before-gate-answer.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/canceled-before-gate-answer.json)，五份快照均满足：

- `/history/isComplete` 为 `true`，`/history/turns` 共 17 轮，完整数组逐字段相同。
- 前 16 轮均已完成，后续最终快照仍逐字段保留这 16 轮。
- 第 17 轮是上述同步问答轮，状态 `inProgress`；`/metadata/threadRuntimeStatus` 为 `active` 且含 `waitingOnUserInput`。
- `/metadata/requests` 中存在同一请求 `2` 和 `gate_continue` 问题。B/C 尚未进入历史。

gate 的长时间等待包括人工操作和取证过程，不作为队列产品延迟。

### 2. B 保存成功回包丢失后的恢复：PASS

[`editing-before-save.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/editing-before-save.json) 中，B 已被原生取出，`/queue/data` 只剩原 C，`/remoteQueue/queuedEdits/0` 精确指向 B 原 ID 和编辑事务。

| 证据 | 实际结果 |
| --- | --- |
| [`save-response-paused.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/save-response-paused.json) | 原生 HTTP 200 的响应正文已是 `completed / ok:true / editState:committed`，返回 B 保存后物理 ID |
| 原 POST | 网络请求 `74908.1419`，`editQueued`，原请求／幂等 ID `aa3f4517-7aa9-4b61-8ca5-a2808e97e77a` |
| [`save-recovery-network.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/save-recovery-network.json) | 请求 `74908.1419` 记录 `net::ERR_ABORTED`；随后请求 `74908.1448` 以 GET 查询 `/api/receipts/aa3f4517-7aa9-4b61-8ca5-a2808e97e77a` |
| [`save-original-receipt.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/save-original-receipt.json) | GET 原回执正文与中断前捕获的成功响应 JSON 完全相同 |
| 有界计数 | 保存网络原件的事件序号 `6366–6688` 内：`editQueued` POST 1 次、GET 原回执 1 次，无第二次保存 |

独立解码 `save-response-paused.json` 的 `/raw/body` 后，与 `/receipt` 精确相同；GET 正文再次匹配同一请求、会话、编辑事务、保存结果、物理消息 ID 及队列快照。恢复没有依赖同文匹配另一条消息。

[`saved-before-gate-answer.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/saved-before-gate-answer.json) 的 `/queue/data` 为保存后的 B、原 C，顺序保持 B → C，活动编辑为空。

### 3. 图片、Skill 与完整输入保留：PASS（本轮输入形态）

逐项比较编辑前、保存后、取消 C 后及最终接受的 B：

- 原输入和保存后输入类型顺序均为 `text → image → skill`。
- B 的首个文本项只增加 [`edited-prompt.txt`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/edited-prompt.txt) 指定修改；原生保存末尾一个换行，`text_elements` 保持空数组。
- 保存前后所有非文本项深度相同；最终接受的用户消息 `content` 与保存后队列的 `input` 完全相同。
- `perf-blue.png` 的内嵌 PNG 解码后为 **172 字节**，编辑前、保存后和最终接受字节完全一致，`detail:null` 保留。SHA-256 为 `63db222d0886ab1a22d1a675af881b00a9aed9ea8ef69c8dbef2909aee8a06d4`，同时匹配保存请求的 `retainedImageSources` 中 `inline:` 指纹。此处比较基线是本轮编辑前的真实原生队列，不额外声称已核对原始磁盘文件。
- Skill 完整保持为 `{type:"skill",name:"root-cause-analysis",path:"C:\\Users\\junes\\.agents\\skills\\root-cause-analysis\\SKILL.md"}`，没有新增、重复或丢失。
- C 在 B 保存前后的物理 ID、逻辑 client 和完整输入均相同。

本轮没有非空 `text_elements` 或其他独立隐藏输入，不覆盖这些输入形态，也不据此宣称所有原生执行配置均逐字段不变。

### 4. C 取消成功回包丢失后的恢复：PASS

[`cancel-editing-before.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/cancel-editing-before.json) 中原生只剩保存后的 B，C 的编辑事务准确指向原 C。已输入但将被取消的修改由 [`cancel-discarded-draft.txt`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/cancel-discarded-draft.txt) 保存，含唯一标记 `QUEUE_C_CANCELED_DRAFT_MUST_NOT_RUN`。

| 证据 | 实际结果 |
| --- | --- |
| 原 begin | 网络请求 `74908.1501`，精确指定 C 原物理 ID、逻辑 client、原文及编辑事务 |
| [`cancel-response-paused.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/cancel-response-paused.json) | 原生 HTTP 200 的响应正文已是 `completed / ok:true / editState:canceled`，返回 C 取消恢复物理 ID |
| 原 POST | 网络请求 `74908.1519`，`cancelQueuedEdit`，原请求／幂等 ID `49ff3e01-fcc7-413a-a75e-8d8bba8f02d7` |
| [`cancel-recovery-network.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/cancel-recovery-network.json) | 请求 `74908.1519` 记录 `net::ERR_ABORTED`；随后请求 `74908.1525` 以 GET 查询同一原请求回执 |
| [`cancel-original-receipt.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/cancel-original-receipt.json) | GET 回执 JSON 与中断前捕获的成功响应完全相同 |
| 有界计数 | 取消网络原件的事件序号 `6772–7010` 内：begin C 1 次、cancel 1 次、GET 原回执 1 次，无第二次取消 |

[`canceled-before-gate-answer.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/canceled-before-gate-answer.json) 中 C 的物理 ID 改变、逻辑 client 保持，其完整输入与编辑前原 C 完全相同，B 完全不变。取消修改标记未进入恢复队列，也不存在于最终完整历史。

### 5. 刷新、自然 FIFO 与普通草稿：PASS

[`after-reload-before-gate.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/after-reload-before-gate.json) 记录刷新后 B、C 的准确正文与顺序，`editingControls:0`，普通草稿为：

```text
[remote-e2e:RA-20261102-queue-recovery:draft] 队列保存恢复后仍保留的普通草稿。
```

[`gate-answer-network.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/gate-answer-network.json) 中只有一次 `answer` 写请求，准确回答原请求 `2` 的 `gate_continue` 为“继续”；对应完成回执请求 ID 为 `4c5680b7-2621-4761-9141-58345af638d3`。该捕获窗口其他写操作仅为 `markRead`，没有引导、重新发送、恢复、停止或手动出队请求。

[`natural-progress.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/natural-progress.json) 的完整历史共 19 轮：

| 位置 | 对象与结果 |
| --- | --- |
| 第 1–16 轮 | 与编辑前已完成历史逐字段相同 |
| 第 17 轮 | 原 gate 轮完成，末回复 `QUEUE_GATE_DONE:继续` |
| 第 18 轮 | B client 仅一份接受记录；turn `01a19b97-cd25-7c32-b24a-5f21953f3992`；完整 `content` 等于保存后 B 的 `input`；末回复 `QUEUE_RECOVERY_B_EDITED` |
| 第 19 轮 | C client 仅一份接受记录；turn `01a19b97-f2b2-7712-a47f-2ca22ca2f936`；完整 `content` 等于编辑前原 C 的 `input`；末回复 `QUEUE_RECOVERY_C_OK` |

原生记录的 B 开始时间不早于 gate 完成，C 开始时间不早于 B 完成。最终全部 19 轮 `completed`，`/metadata/threadRuntimeStatus/type` 为 `idle`，`/metadata/requests`、`/queue/data`、`/remoteQueue/queuedMessages` 与 `/remoteQueue/queuedEdits` 均为空。按两条逻辑 client 同时检查普通用户消息与引导消息，没有第二份接受记录。

[`natural-ui.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/natural-ui.json) 和 [`natural-ui.txt`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/natural-ui.txt) 同时记录 B/C 回复和零队列行；普通草稿与刷新后记录完全相同。本轮没有发送这份草稿，不据此报告新的后续普通发送通过。

## 缺失证据与不可扩大的结论

1. **全程初始派发计数为 PARTIAL。** 最早 B/C 发出到开始编辑 B 之间取证较久，早期 HTTP 账本未完整保留。可以确认最终各一个逻辑 client 接受记录，以及上述两个有界网络窗口内没有重复保存／取消；不能虚构全程 send、begin B 或其他操作总次数。
2. **离线、跨进程恢复未测。** 本轮是成功响应正文被中断、原回执仍可只读查询并刷新页面的恢复；未关闭全部网络或事件通道，未重启原生 renderer／服务，不覆盖离线恢复、令牌跨进程持久化或服务重启后的恢复。
3. **运行后源码身份已由主验收补齐。** 相邻 `sync-question-final/native-final117-after-identity.json` 及其实际脚本与before逐字一致，SHA-256均为 `5e3ac1d40fc337c6246ef11e875900b7c7987eaa2c96fc0c0d70880e41c4125d`。这是117业务源码/106兼容标记变体的受控运行证据，仍不代表最终安装包已经安装或公开部署。
4. **菜单没有产品故障证据。** 首次 Playwright 点击后未见菜单是操作未生效的观察；[`menu-geometry.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/menu-geometry.json) 中按钮连接、未禁用、处于视口内，[`menu-mutation-trace.json`](../../dist/validation/remote-acceptance-20261102/queue-edit-recovery-final/menu-mutation-trace.json) 的 `rows` 为空，未证明菜单被队列重绘关闭。后续真实 begin/save/cancel 已完成，不能将早期自动化点击排查定性为菜单产品缺陷或产品延迟。
5. **复杂输入与并发边界。** 本轮只覆盖一张 PNG、一个真实 Skill、普通文本的保存和纯文字 C 的取消；不覆盖图片增删、非空文本引用、独立隐藏输入、与真实出队同时发生的编辑竞争或双客户端冲突。

## 收尾补充

`final.json` 再次保存完整原生状态，`final-completed.png` 是实际完成页截图；首次区域截图调用超时，随后同页完整截图成功，没有重新执行业务动作。测试会话完整设置已恢复为原值，临时适配器已恢复安装版106，所有本轮网络拦截已清除。普通草稿经过内容核对后才清空；本轮两张测试页已关闭、1432服务已停止。共享收尾证据在相邻 `sync-question-final/` 与 `client-preferences-final/`。

本轮未修改产品源码，没有重新构建、安装、提交或发布；两包摘要与15:56 Windows包及14:16 Android包一致。原始HTTP鉴权头在归档时脱敏，业务请求、回执正文和事件顺序未改，具体字段与前后摘要见相邻 `evidence-http-redaction-20261102.json`。这不改变有界窗口通过和初始派发账本不完整的区分。

## R3-06 受控上游失败的隔离与验收边界（2026-11-03）

**判定：尚未找到已验证的隔离故障驱动，真实 E2E 未执行。** `scripts/remote-access-e2e.mjs` 已声明 `queue.after_failure` 需要 `upstream` 租约，并依次调用 `environment:fail-active-upstream`、`environment:restore-upstream`；该步骤要求失败命中原活动轮，随后确认该轮失败并进入原生队列转场。当前 `scripts/remote-access-e2e-environment.mjs` 只负责校验 scope、租约编排和恢复，不连接服务或注入故障；runner 要求外部 `drivers.environment.perform/observe`。已检查的 `scripts/remote-access-e2e-receipt-driver.mjs` 是 `recovery.question_receipt` 专用驱动，只支持 `receipts` 资源，不能充当上游故障驱动。现有环境测试使用内存状态和合成 scope，只证明租约异常路径与恢复逻辑，不证明真实上游请求已被拦截。

故障须命中测试活动轮的实际模型请求，并保留请求、响应/断连与准确 turn 的关联原件。**独立远程 HTTP 服务不等于独立模型执行进程**：原生优先路径可能仍使用共享桌面的 app-server；不能因测试入口有独立 PID/端口，就宣称模型上游已被隔离。可评估独立 app-server/provider 夹具，或已证明只命中该测试 turn 的故障注入；可行性和作用范围须先取证。直接让 `turn/start` RPC 在派发前失败，不能证明一个已开始的活动轮经历了模型上游失败。现有 runner 的 `authorized-tab` 租约不允许 `upstream`，这是当前测试驱动边界，不是产品架构必须如此的结论。

执行前须由真实驱动提供原始证据，证明服务 PID、工作区、监听端口、实例 ID、该实例的模型上游地址及故障代理均属于本轮；结构字段通过 `validateRemoteAccessEnvironmentScope` 本身不等于进程和网络归属已被实测。租约应保存故障前的上游路由状态，并在 `finally` 中恢复、再次读取核对。不得修改 `45222` 用户服务、共享 `45221` 代理、用户供应商配置或全局代理/网络状态。

现有证据门禁会验证原活动轮在转场快照中为 `failed`，且存在一条 `affectedTurnId` 匹配并声明 `realUpstreamFailure: true` 的驱动记录；但该布尔值不能代替实际请求账本和轮次关联。当前 fixture 只合成上述驱动事实，不能从离线测试通过推出真实上游失败已验收。

“未知送达不自动重发”约束远程操作及用户消息的重复派发，不能擅自扩大为“Codex 内部模型 HTTP 重试次数必须为0”。模型运行器可能有原有重试策略；应记录实际尝试及最终状态，没有相应契约时不得为通过测试去禁用原生重试。

R3-06 最终通过条件：同一 `activeTurnId` 有实际模型上游失败证据且最终原生状态为 `failed`；远程原动作不重复派发，原队列项的 `clientUserMessageId` 与完整 `input` 未变；转场由原生证据判定为自动承接或完整保留，只有保留分支才允许对原 client 做一次显式引导；结果仍未知时只读核对，不补发。最终核对同一 client 的唯一接受、完整输入、最终回复和队列终态。缺少真实请求/turn关联、隔离恢复或队列闭环证据时保持 `blocked/not_run`，不得用步骤声明、离线夹具或正常停止场景替代。

2026-11-03 11:28（Asia/Shanghai）的隔离启动尝试见 [`remote-upstream-isolation-20261103.md`](remote-upstream-isolation-20261103.md)：`initialize` 成功期间，守卫发现 app-server PID 连接本机系统代理 `127.0.0.1:7897` 并终止进程。没有 `thread/start` 响应、实际模型请求或队列操作；目标连接未知，不能证明模型刷新行为或代理没有转发请求。匹配安装版的 `NO_PROXY` 行为仍缺少源码依据，因此没有重跑，也没有更改系统代理或网络策略；R3-06 保持 `blocked/not_run`。
