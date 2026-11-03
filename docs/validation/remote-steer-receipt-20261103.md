# 队列引导成功回包丢失与刷新恢复验收（2026-11-03）

**限定链路通过：准确队列消息引导成功后，原 HTTP 成功回包被中断；同页刷新后 GET 原回执，网页解除待确认状态，没有再次引导或发送。** 原消息在既有活动轮内仅接受一次，普通草稿保持，M 原 30 轮及 C 持久 8 轮完整保持。旧文档 GET 的全部清退证据仍有一处缺口，不能据此宣告全部未知结果或环境清理分支通过。

本报告独立只读复核已保存原件，仅新增本文。原件根目录为 `dist/validation/remote-acceptance-20261103/`，业务证据位于 `steer-receipt-121/`，运行身份与恢复证据位于 `runtime-121/`。下文时间使用北京时间；原 JSON 的 UTC 时间为 2026-11-02，换算后本次操作发生于 **2026-11-03 凌晨**。

## 对象与真实引导

测试页为 `http://127.0.0.1:45420/`。主会话 M 为 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，对照会话 C 为 `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。

本次准确身份如下：

- 原队列 Q：`01a19d33-9670-7721-ae02-8419992c142d`。
- 逻辑客户端 ID：`2db3c361-6d92-4c13-86d5-fe3a399bebf2`。
- 原活动轮 T：`01a19d33-8e9c-7890-9438-7c45390ebdfa`。
- 引导操作回执 A：`9c59a394-5c55-425a-b1ce-62deaaf510f3`。

`baseline.json` 记录 M 空闲、完整 30 轮、无队列。准备阶段两次正常 `/api/action/send` 分别建立等待测试轮及 Q；`before-steer.json` 记录 M 已有 31 轮，T 活动中，Q 按上述客户端 ID 携带完整纯文本 input 真实排队。两次准备发送不能计作故障后的重发。

01:00:03.851，网络请求 `62868.329` 向 `/api/action/steerQueued` 发出一次 POST，正文精确为 M/Q，幂等回执标识为 A。`original-response.json` 与网络事件 `sequence=1684` 对应同一 Fetch 暂停 `interception-job-46539.0`：**真实 HTTP 200，原正文为 `requestId=A / state=completed / result.ok=true`，并返回 M、Q、T 和 `queueSyncPending=true`**。后者是队列同步待定标记，不是未发送。

随后该响应被 Fetch 故障控制中断；`fault-controls.json` 保存命令确认，`network.json` 的 `sequence=1831` 保存同一 `62868.329` 的 `Network.loadingFailed / net::ERR_ABORTED / canceled=true`，未出现该 POST 交付页面的 `Network.responseReceived`。因此故障命中已成功的回包，不能解释为请求未发出。

## 待确认、旧 GET 与新文档恢复

01:02:27.916 的 `pending-before-reload.json` 保存原 pending：`action=steerQueued`、`id=A`、`payload=M/Q`。页面和 PNG 已显示最终回复，同时仍显示“队列引导结果待确认，原回执已保留，不会重复引导”，普通输入为 `STEER_121_UNSENT_DRAFT_保留`。这份 pending 记录不含服务端 `state=unknown`，不能将网络异常表述为实际取得了 unknown 回执。

原 A 的 GET 在请求阶段被暂停。旧文档请求 `62868.352 / .359 / .362 / .365 / .372` 均有准确匹配的 `loadingFailed / ERR_ABORTED`，耗时约 15 秒，与已保存实际 app 中回执读取的 15 秒上界一致；这些属于客户端超时取消，不能冒写成故障控制命令取消成功。`62868.376` 另有取消命令确认及网络终态，`62868.380` 也有准确失败终态。

**旧 GET `62868.383` 是明确的证据缺口。** 它归属旧 loader，保存了请求与 Fetch 暂停，但账本中没有对应 `loadingFailed`、`loadingFinished` 或响应；控制记录为 `alreadyEndedUnconfirmed`，错误原文 `Invalid InterceptionId.`。这条命令失败不证明请求已取消，`old-receipts-retired.json` 也未列入它。因此不能声称导航前所有旧暂停 GET 均已清退。该缺口不改变后述新文档成功读取原 A 的事实。

01:03:52.898，同一主 frame `799AA807CFDB3E5A430EE99039656780` 的 Document 刷新请求由旧 loader `610C2CD13C41FCAD62D486C8F90EF9F1` 切换为 `8E7DCA4EE924D82D4398CD3BC8454CD8`，Document 本身正常返回 200。刷新后早期 A GET 仍有暂停后取消，不能将它们当作成功恢复。

最终被明确放行的是新文档的 **`GET /api/receipts/A`，网络 ID `62868.463`**：

- `sequence=2371` 的请求绑定新 loader、同一 frame，wallTime 晚于新 Document；`2372` 对应 Fetch 暂停 `interception-job-46703.0`，控制记录为 `fresh=true / continued=true`。
- `2375` 为 HTTP 200，`2377` 为 `loadingFinished`。
- `recovered-response.json` 保存的真实正文与原 POST 成功回执逐字段一致，仍为原 A 的 `completed/ok`，并精确绑定 M/Q/T。
- 随后的 `final-ui.json`、DOM 和 PNG 显示待确认横幅消失、队列区域为空，普通草稿仍为原文。这里确认的是可见待确认状态解除；最终 UI 原件没有另存 sessionStorage 的 pending 值，不额外声称已独立读取其删除结果。

事件流一直可用，不能宣称完全断网验收。本例与仅靠事件收敛的样本不同：暂停期间页面已取得完成回复但 pending 仍保留，且确实捕获了新文档对原 A 的成功 GET。已保存的实际 app 也显示，队列恢复在取得原回执并核对 M/Q 后清除 pending；本文不把 SSE 状态更新代替原回执证据。

## 唯一消费、历史和草稿

`network.json` 为 490 条已整理网络事件，`truncated=false`，覆盖游标 1533 至 2512。故障起点的引导请求 `sequence=1682` 至账本结束，`steerQueued=1`、追加 `send=0`、`removeQueued=0`；另外两条写动作是正常 `markRead`。两次准备 send 位于故障起点之前，单列为测试准备。

`after-recovery.json` 中，仅一条正式接受记录匹配 Q 的逻辑客户端 ID：

- `steeringUserMessage` 项 `eb294be9-a2a0-44f1-9ed4-f0a0407fa530`，`status=accepted`，`targetTurnId=T`。
- 其完整 `input` 与 `before-steer.json` 中 Q 的 input 逐字段一致，`restoreMessage.id=Q`。
- 服务器消息 ID 为 `01a19d34-6c02-7103-8290-adcf8dc6692e`；同轮 `steered` 项引用该 ID，不是第二次消费。
- 原 T 完成，最终回复 `STEER_RECEIPT_121_OK` 恰一次，M 回到 idle，队列与待处理请求均为空。

M 最终仍是 **31 轮**：引导在原 T 内完成，没有额外开启第 32 轮。按深度 100 解析并序列化完整轮次对象，比较结果为：

- M 基线前 30 轮与引导前、恢复后的前 30 轮均逐字段一致；正式持久读取的前 30 轮同样保持。
- C 的正式持久读取在三份快照中始终为 8 轮，完整轮次逐字段一致。C 的已加载历史为 `notLoaded / isComplete=false`，不能将其零轮视图当作持久历史丢失或拿来替代 8 轮核对。

新页普通草稿在刷新前后均为 `STEER_121_UNSENT_DRAFT_保留`，没有进入 M 的正式用户消息或引导消息。旧 1432 页的 `old-client-final.json` 仍保存 `STOP_LIVE_2_UNSENT_DRAFT_保留`，pending/queue 为空，URL 仍指向 M，切换按钮为“切换到最近会话”；它与新服务的测试草稿分别记录，没有混作同一份输入。

## 运行身份与收尾

`runtime-121/` 中的实际引擎脚本起止原件、待执行文本三者 SHA-256 均为 `cb14de43cb993c5690e74e8a9cd855bd84719060bbca3bb45773b8f13a15a181`；fixture 为 `2f766d1f-0cf3-42b7-8c65-81e8a7e59928`，业务源码 121、兼容标记 106，运行 epoch 为 `e760fedd-747c-48e9-8335-8743f46f2c4f`，与业务快照一致。实际浏览器 `actual-app.js` 的摘要为 `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`，与服务源码快照及脚本身份记录相同。本例证明该测试运行身份，不等于正式安装版已升级。

收尾重新读取的 `restored-engine-119.js` 与开始前 `baseline-runtime-119.js` 摘要同为 `5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`。独立比较 `baseline-identity.json` 与 `restored.json /after`：原 119/106 fixture 仅 `runtimeEpoch` 改为重新执行后的 `00837805-d4d5-46ce-b12a-d2f745f04068`；owned 名单恢复为空，三张写状态表及已有终态编辑记录与基线一致。该结论来自字段与实际脚本摘要对照，不只采用 `verified=true`。

`server-finished.json` 记录临时服务 PID 74324 于 01:10:57.391 按 `explicit-stop-file` 退出，且 `globalNativeDisposed=false`；Fetch 控制记录为已清除。补入的 `runtime-121/environment-restored.json` 为 2026-11-03 01:18:58.211 的只读快照：PID 74324 已不存在，45420 无监听，原 45222 仍由 PID 13568 监听。该快照补齐临时服务退出及原服务端口保持证据，不补齐旧 GET `62868.383` 的网络终态。

本次未验证原生长期 unknown、完全断网、回执失效、跨授权或跨进程恢复，也未验证队列删除丢回包、Android 真机或稳定性能收益；旧 GET `62868.383` 的终态缺口继续保留。
