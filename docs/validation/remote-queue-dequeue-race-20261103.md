# 自然出队竞争：迟到移除与编辑第二轮通过，首轮超时保留

日期：2026-11-03，Asia/Shanghai。下文时间均为北京时间；原件使用 UTC。

**迟到移除和编辑第二轮的限定业务子场景均经独立复核、事实函数核验通过：各自原 Q 在旧 POST 暂缓期间自然唯一完成，放行同一请求后原 A 明确返回 failed/409，历史及空队列保持，普通未发送草稿保留。** 编辑首轮仍因浏览器取消、拦截 ID 失效而未通过；完整 runner 报告及第二轮启动前的自动重连尚未通过验收，R3-04 不因此整体结项。

本次只离线读取原始 JSON、脚本及截图，执行逐字段比较和摘要复算，仅新增本报告；未操作浏览器、服务或真实会话。原件目录为 `dist/validation/remote-acceptance-20261103/queue-dequeue-race/`，下文文件名均相对此目录。

## 运行身份与适用范围

- 原生 target：`DC2A82A8EFAA78FEC87FAE0B59E1E229`；fixture：`2e6ccd4e-0c44-4d0c-a965-21d1a94c03fd`；epoch：`371c41e3-4a6c-4fa8-856b-a6da173e702e`。三份 remove 快照中的 fixture、epoch 完整相同。
- `runtime-native-engine-before.json/.js`（03:37:48.010）与 `runtime-native-engine-before-goal.json/.js`（04:54:09.003）实际脚本 `scriptId=104878`，全文 SHA-256 均实算为 `5b97e84abf7933129193a3bb0230bc9fc3feb39fd0705fef65ff1c7af930cf78`，与原件 `actualRuntimeSha256`、fixture 的 `expectedRuntimeSha256` 一致。
- 这是 **121 业务来源／106 兼容运行标记的测试夹具**。`runtime-plan.json` 明列版本标记替换、安装早退移除及已登记测试会话的自动命名保护；不能表述为未改造的 121 安装版。来源摘要为 `54b79c2095b7486ce21a580fe61a571a90c6f2cf95d88f0de9350ec722e54c73`；兼容脚本正文摘要与含调试尾注的全文摘要是两个范围。
- remove 网页 origin 为 `http://127.0.0.1:1435`，操作页／回答页 tab 为 `1283922820`／`1283922823`。`draft-fixed-empty-baseline.json` 于04:33:53记录双页 `appSha256A/B` 均为 `a4d9c02b2b222608cd73df59ccea6d99b6e2c99d7130fb035df9f074f3a37c55`；归档 `app-draft-fixed.js` 全文实算一致。执行方记录此后同两页完成草稿验证及 remove，期间未换源码；本次未重新连接网页取源。
- 较早 `page-source-identity.json` 的 `641b08…` 与 `page-after-layout-identity.json` 的 `f1a832…` 分属早期 gate／编辑阶段，不能替代 remove 的网页身份，也不能把编辑和 remove 试跑合称同一网页版本的通过结果。

## 原 Q、真实自然消费与迟到移除

主会话 M：`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`。原活动 T：`01a19dfe-d4f6-73f1-a4c0-74dd60ea0d4d`；原 Q：`01a19dff-382c-78c1-a58e-7de37c4fbed0`；原 client：`8bd7151b-b442-4ad8-8a83-c2fa1dd30327`。

`remove-before-held.json` 的 `/threads/0/serverQueue/data/0/input` 仅有一项 text，`text_elements=[]`，正文如下且末尾保留一个换行：

```text
[remote-e2e:RA-20261103-queue-race:remove-Q] 只回复 QUEUE_REMOVE_Q_DONE，不调用工具、不读写文件、不访问网络。
```

| 时间 | 原件与核对结果 |
| --- | --- |
| 04:41:50.684 | `remove-network.json`：唯一 Q 入队 POST，network ID `24688.1125`；`remove-action-responses.json` 为 completed／queued，准确绑定原 Q、client |
| 04:42:02.462 | `remove-before-held.json`：主会话完整37轮，T 为 inProgress，runtime 为 active／waitingOnUserInput；同步问题 ID `queue_remove_continue`、原生 request ID 数字7；server 队列仅原 Q，历史中原 client 尚未正式接受 |
| 04:42:48.219／.341 | 原 remove POST 派发；`remove-held.json` 留存真实 `Fetch.requestPaused`，network ID `24688.1137`，Fetch ID `interception-job-51462.0`，没有 `responseStatusCode` 或 `responseErrorReason`，属于 Request 阶段暂停 |
| 04:42:50.238 | 回答页唯一 `POST /api/action/answer`，准确回答数字7、`queue_remove_continue=继续`；没有调用停止、重新发送或引导来代替自然出队 |
| 04:42:56.046 → .070 | 原 T 自然 completed，回复 `QUEUE_REMOVE_GATE_DONE`；原 Q 的新轮于24ms后开始，04:43:01.992 completed，回复 `QUEUE_REMOVE_Q_DONE` |
| 04:43:09.271 | `remove-consumed-before-release.json`：完整38轮、idle、requests 空；原 client 在原生历史和持久历史中各仅有同一份正式 userMessage，完整 content 与原 server input 逐字段相同；server 队列与 native 组合队列均空 |
| 04:43:21.051–.130 | `remove-release-command.json`：`Fetch.continueRequest` 成功，params 只有上述原 Fetch ID，无 URL、方法、headers 或正文改写；明确晚于消费快照 |
| 04:43:21.198 | `remove-network.json`：同 network ID 的真实 HTTP200 响应；`remove-original-receipt.json` 与 `remove-response-command.json` 正文逐字段相同，原 A failed，result.ok=false／code=409，准确绑定原 M／Q，提示“此队列消息已不存在，请刷新会话” |
| 04:44:19.617 | `remove-after-release.json`：完整38轮历史及持久读取对象与消费快照逐字段相同，仍 idle、requests 空、server／native 组合队列空 |

原 remove A 为 `2821a296-604e-40d6-b130-a5fe2272c3e6`。Network 与 Fetch 原件的 URL、POST 方法、`postData` 原串、大小写不敏感读取的唯一幂等键一致，`Fetch.networkId` 等于 `Network.requestId`；Network 特有的 `isSameSite`／`mixedContentType` 差异不参与业务身份判断。原请求还记录 `Network.loadingFinished`，没有对应 `loadingFailed`。这是原请求的明确拒绝，HTTP200 与业务 code=409 分开记录。

正式接受的新 T 为 `01a19e00-34a5-7221-b75e-924c1074f337`，正式 userMessage 为 `01a19e00-34d6-72d3-b1fa-bfc6bd48c60b`。新 T 的 `params.input=[]`，本次以持久 userMessage.clientId／content 验证接受，未用空 `params.input` 否定自动消费，也未依赖 `starting/consuming` 状态。

## 动作次数、历史及草稿保护

`remove-network.json` 保存151条事件（sequence5447–5774），`remove-answer-network.json` 保存147条（sequence1971–2288），二者均 `truncated=false/hasMore=false`。只按 `Network.requestWillBeSent` 的 network ID 计数，窗口内动作如下；`/api/read/*` 的 POST 属于读取，单独排除：

| 动作 | 次数 | network ID／原回执 A |
| --- | ---: | --- |
| Q 入队 send | 1 | `24688.1125`／`22ef9d4d-bf49-441e-9fb4-ee16ce72fa17`，completed／queued |
| 同步回答 answer | 1 | `90484.378`／`1b14610d-f5e6-4863-81a2-7c1bd515b8a3`，completed |
| 迟到 removeQueued | 1 | `24688.1137`／上述原 A，failed／409 |
| markRead | 4 | `24688.1145`、`24688.1152`、`90484.387`、`90484.394`，各有 completed／ok 原回执 |

七项的请求幂等键均与 `remove-action-responses.json` 对应响应的 requestId 相同，并绑定 M；窗口内没有第二次 remove、编辑、stop、steerQueued 或补偿 send。计数只覆盖这两个归档窗口，建立活动 T 的更早 send 不在其中，不能据此声称全场只有一次 send。

- 主会话原生完整历史和持久历史的轮数均为37→38→38；前36轮分别逐字段保持，放行后未新增、删除或改写任何轮次。
- 控制会话 `01a1996d-0b7a-71d0-b9ad-9c521d224ad9` 的原生投影是0轮、`isComplete=false/runtime=notLoaded`，不能宣称已加载完整历史；正式持久8轮、server 队列及 pendingRequests 在三个截点均逐字段保持。
- 两会话的完整 `/settings` 对象在三个截点分别一致，包含模型、思考深度、协作模式、权限及目标字段。M 在本子场景期间为 plan，`mode-restored.json` 另于04:49:52记录其恢复 default／idle，goal=null；这不代表后续目标测试或整个环境已完成恢复。
- 已读取 `remove-after-response-ui.json` 并目视核对 `remove-after-response.jpg`：可见原 Q 完成回复，输入区仍为 `QUEUE_REMOVE_UNSENT_DRAFT_保留`。本次未把草稿作为新消息补发。

## 编辑首轮试跑保留为未通过

`edit-held.json` 原 begin POST 的 Q 为 `01a19dce-595d-7453-a881-8b25559523ff`，client 为 `46bbee27-6c83-4d30-a08b-ee4704a20d7d`，ED 为 `8fde973d-e298-481d-9040-e61ed7b2aef1`，network ID `24688.611`，原 A `3208a5b9-fcb1-4ad9-aceb-f8292fa1b2ee`。它于04:08:06.138派发并暂缓，04:08:51.999快照已显示原 Q 自然完成；但04:09:37.158出现 `Network.loadingFailed/canceled=true/net::ERR_ABORTED`，由同一请求的单调时间戳复算间隔91,019.252ms。04:09:49.714尝试放行原 Fetch ID `interception-job-50435.0`，得到 `Invalid InterceptionId`（`edit-release-expired.json`）。

因此没有证据证明旧 begin POST 已成功放行并收到服务端409，不能计为编辑竞争通过，不能归为服务端真实失败或 R3-06 上游失败。`edit-timeout-cleanup-responses.json` 随后记录准确 ED 的 queueEdit 为 missing／operationPending=false／canDismiss=true，以及 UI 显式 dismiss 的 completed／dismissed 回执。`edit-timeout-ended.json` 的完整36轮历史和持久读取与自然消费快照相同；`edit-timeout-ended-ui.json` 记录恢复后的普通稿 `QUEUE_EDIT_UNSENT_AFTER_REFRESH_保留`。这些只证明本次超时后的清理收口，不补足编辑竞争验收。

## 编辑第二轮：新事务的迟到 begin 明确过期

第二轮使用全新的 gate client `3dec1aa9-cbac-473b-aafc-606fd9ac13d7`，原始 `turn/start` 请求及回执保存在 `edit-r2/edit-turn-start-request.json`、`edit-r2/edit-turn-start-receipt.json`，05:25:23.414派发并绑定新 T `01a19e27-1359-7353-8d98-fd4ea4a1e78c`。这是同 owner 的原生调用，不是 HTTP send，也不是重发首轮事务。三份第二轮快照记录与前述相同 fixture／epoch 和121／106标记；网页沿用已归档的 `a4d9c02b…`。

第二轮原 Q 为 `01a19e35-3a8c-7151-96a9-4ffc4452005f`，client 为 `526c460d-3160-4daa-859d-2036748b733c`，ED 为 `96fe02d4-ab93-413e-ba35-574ac4d3344c`；均与首轮不同。server input 仍为单项 text、`text_elements=[]`，正文如下且末尾一个换行：

```text
[remote-e2e:RA-20261103-queue-race-r2:edit-Q] 只回复 QUEUE_EDIT_R2_Q_DONE，不调用工具、不读写文件、不访问网络。
```

| 时间 | 原件与核对结果 |
| --- | --- |
| 05:40:50.194 | `edit-r2-network.json`：Q 入队 POST `24688.1857`，原 A `e64debb2-0049-471f-b783-928451b456e6` 为 completed／queued，准确绑定本轮 Q／client |
| 05:41:02.034 | `edit-r2-before-held.json`：原生显示记录41条、持久轮次40，T inProgress／waitingOnUserInput；原问题 `queue_edit_continue`、request ID 数字8；server 仅原 Q，native 组合显示投影也仅原 Q |
| 05:41:27.770／.795 | begin POST `24688.1866` 派发并以 Fetch ID `interception-job-53288.0` 暂缓；无 responseStatusCode／responseErrorReason，URL、POST 原串、幂等键与同 networkId 的 Network 请求一致 |
| 05:41:29.760 | 另一页唯一 answer POST `90484.935` 回答准确数字8／`queue_edit_continue=继续`，原 A `83034ae8-42ea-47b1-9da4-ec02adfe42ab` completed；没有 stop、steer 或补偿 send |
| 05:41:36.152 → .291 | 原 T 自然 completed／`QUEUE_EDIT_R2_GATE_DONE`；139ms后本轮 Q 的新 T 开始，05:41:42.984 completed／`QUEUE_EDIT_R2_Q_DONE` |
| 05:41:47.719 | `edit-r2-consumed-before-release.json`：原生42条、持久41轮，原 client 在两种历史中各仅有同一正式 userMessage，完整 content 与原 server input 相同；idle、pendingRequests 空、server／native 组合队列均空 |
| 05:41:59.442–.498 | `edit-r2-release-command.json`：对原 Fetch ID 唯一 `Fetch.continueRequest` 成功 `{}`，params 仅 requestId，没有改写请求；晚于消费快照 |
| 05:41:59.574 | 同 networkId 返回 HTTP200；`edit-r2-response-command.json` 读取的原回包与 `edit-r2-original-receipt.json` 逐字段相同，原 A `f8b3ba15-ab87-4837-a748-ae0623cbb866` 为 failed／ok=false／code=409，提示“此队列消息已执行或不存在，请刷新会话” |
| 05:48:19.010 | `edit-r2-after-release.json`：原生42条及持久41轮分别与消费快照逐字段相同，仍 idle、请求空、两种队列空 |

正式接受的新 T 为 `01a19e35-eb0e-7170-99ca-86fd2a7a4691`，userMessage 为 `01a19e35-ebb7-72a3-ab67-ededc5423b9d`。原生记录41→42→42和持久轮次40→41→41须分别报告：原生额外的一条既有目标虚拟记录为 `turnId=null/status=completed/items=[]`，本次没有删除它、补造 ID，或把41条都称为持久正式轮次。除正在完成的 T 外，原生前40条及持久前39轮保持；放行后的完整历史与持久读取均保持。控制会话仍为未加载0条投影／正式持久8轮，三个截点持久内容及两会话 `/settings` 均分别相同。

`edit-r2-network.json` 为305条（sequence8947–9592），`edit-r2-answer-network.json` 为293条（sequence5379–5993），均 `truncated=false/hasMore=false`。HTTP动作按唯一 network ID计：Q 入队 send 1、beginQueuedEdit 1、answer 1、markRead 4；更早的原生 gate `turn/start` 单独列明。七项幂等键均匹配 `edit-r2-action-responses.json` 原 A。四次 markRead 的 network ID 是 `24688.1873`、`24688.1882`、`90484.944`、`90484.951`；没有迟到 begin 的 loadingFailed、额外 begin、保存、取消或 dismiss。

本轮409回执正文没有 threadId／messageId，**未向原回包补字段**；其归属由原 POST 中准确的 M／Q／ED／client／createdAt、同一幂等键 A、Network ID 和真实正文读取命令共同核对。已目视核对 `edit-r2-after-response.jpg`：原 Q 完成回复与普通稿 `QUEUE_EDIT_R2_UNSENT_DRAFT_保留` 可见；`edit-r2-after-response-ui.json` 于05:42:17.560另记录同一普通稿及“发送消息”按钮，未停留在编辑发送状态。

## 第二轮启动前的自动重连：仍待查

`service-second-finished.json` 记录服务2于05:14:24.539因 `maximum-lifetime` 结束，`globalNativeDisposed=false`。本次只读取 `temp/remote-question-service-20261103/ready.json` 的服务3记录：PID24160，05:26:57.864启动；原值与摘要已另存入 `edit-r2-real-verification.json` 的辅助观察，避免把今后变化的 ready 文件当成本次快照。

`edit-r2-native-waiting.json` 于05:29:31.985已记录新 T 等待准确问题8，但 `edit-r2-before-refresh-ui.json` 于05:30:55.041显示 `connection-state online`／15ms、requestCards=0，消息仍是此前内容。执行方随后手动点击连接刷新；`edit-r2-refresh-thread-responses.json` 的实际 `/api/read/thread` 响应已有准确问题8及 waitingOnUserInput。该响应归档自身 `truncated=true`，不能扩张为重连全过程网络完整取证。

这些证据仅说明网页显示 online 与问题同步恢复不是同一件事，手动刷新后才得以继续本轮操作；未定位原因，不能将自动重连标为通过。该缺口发生在05:41 Request 暂缓链以前，不替代或否定后续已经单独核对的自然消费、同一请求成功放行和原 A 明确拒绝。

## 自动门禁与证据边界

初次独立复核发现，验收器曾要求操作前 serverQueue 和 localQueue 各有原 Q，而原件的 `localQueue.messages=[]`，来源明确为 `native-combined-minus-server-snapshot/atomicWithServer=false`；`nativeQueue.queuedMessages` 是含 server 对象的组合显示投影。后续已将新 race 契约改为核对 server 队列与 native 组合显示投影，不再要求虚构独立物理本地队列；完整 input、原 client 唯一接受、Request 暂停、同原 A 明确409等约束保持。契约版本为 evidence `1.3.6`／runner `2.5.9`，111项离线证据测试通过。

随后真实 remove 原件经透明适配调用 `verifyQueueDequeueRaceFacts` **通过**，返回上述原 A／network ID／client／正式新 T。结果存于同目录 `remove-real-verification.json`，记录10份原件的路径、大小与 SHA-256、56处适配字段来源、实际入参、验收器源码摘要及临时适配器全文。trace 明确取 sequence5521–5679，原入队 send 的 sequence5447 在窗口前，没有通过忽略窗口内重发来放宽门禁；所有来源字节在运行前后及结果复核时摘要一致。

编辑第二轮沿用同一映射方法、未修改验收器，也已调用该事实函数**通过**。`edit-r2-real-verification.json` 保存10份核心原件摘要、56处字段映射和适配器全文；trace 窗口为 sequence9006–9177，原入队 send 的 sequence8954 明确在窗口前。辅助来源另列 gate 请求／回执、虚拟记录计数和重连观察；没有将首轮超时原件改写为成功。

适配阶段名仅是文件角色；原件没有上游 readId，因此使用原件 SHA-256 作为确定性的**文件证据身份**，不冒充原生读取 ID；事实层动作身份沿用实际入队 network ID，没有补造 runner action ID。capture 时间均取原件 `/at` 或原回包读取 `/finishedAt`，状态和回执均来自原值。组合减 server 差集未用于证明独立物理队列。held 命令列表只按实际归档命令时间投影，未宣称存在完整 CDP 命令总日志。

两次通过只覆盖真实原件的事实函数；没有补造 runId、sourceFingerprint、UI步骤或 nativeEvidence，**尚未通过完整 runner 报告门禁**。编辑首轮超时保留为未通过，自动重连、独立物理本地队列、真实上游失败、安装包及全轮验收仍需各自证据。
