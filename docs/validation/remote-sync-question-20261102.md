# 远程同步问答真实验收（2026-11-02）

状态：**局部通过，整体部分确认**。本次取得两道真实阻塞式问答的原生请求、网页回答和原生完成记录：正常回答为青色；另一道回答为橙色，在服务端成功后丢弃原 HTTP 响应，页面随后读取会话状态并收敛，未在已捕获窗口重复回答。同步问答的原回执查询、事件流同时不可用及其他未知结果分支仍未完成真实验收，不能据此宣布所有问答、审批或性能目标通过。

## 证据范围与运行身份

原件目录：`dist/validation/remote-acceptance-20261102/sync-question-final/`。本文仅核对该目录已保存的请求、回执、网络事件、原生历史、页面快照和运行脚本。时间均为 UTC；北京时间为表中时间加 8 小时。

专用测试会话为 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，工作目录为 `C:\Users\junes\Documents\Codex\2026-11-02\remote-e2e-ra-20261102-input-integrity`。`preflight.json` 的时间为 `2026-11-02T08:20:57.998Z`：原生状态 `idle`，待处理请求和队列均为空，已有 13 轮历史，当前会话模式为 `default`；原生预设列表实际返回 `Plan / plan` 和 `Default / default`。

两轮提问由同一原生 owner 的 `turn/start` 创建，显式传入 `collaborationMode.mode=plan`，模型为 `gpt-6.1-sol`、推理深度为 `max`、`developer_instructions=null`。对应 `turn-start-request.json`、`turn-start-receipt.json` 及 `recovery-turn-start-*` 保留原请求和真实 `inProgress` 回执；回答均由远程网页的同步题表单提交。此证据确认本次可通过单次请求选择 Plan 产生同步题，不证明会话模式会自动恢复或全局配置已被核验。

测试使用源码版本 117 的临时原生适配器，兼容标记为 106，fixture 为 `b8efa7cb-1124-44d8-8335-7981c1916f88`，运行 epoch 为 `193954af-d947-4ac4-8ba3-a5980df952b3`。因此快照中的 `adapterVersion:106` 应结合 fixture 解释。

| 核对对象 | SHA-256 与结果 | 原件 |
| --- | --- | --- |
| 源码 117 | `4ce7129ea8d11da315b622bc2c973837edadd6b4e5a3df4ccff9b2db8e45232a`；登记原始源码和插桩源码摘要相同，复核时与 `assets/remote-access/native.js` 一致 | `native-final117-before-identity.json /sourceInfo` |
| 实际引擎脚本 | `5e3ac1d40fc337c6246ef11e875900b7c7987eaa2c96fc0c0d70880e41c4125d`；原件文件摘要、登记预期值和引擎读取值一致，`matches=true`，`scriptId=74715` | `native-final117-before-runtime.js`、`native-final117-before-identity.json` |
| 浏览器脚本 | `072afb6e66f350188bf505e9520c8538b64529abca30825c6ccc3b25c2788a7f`；复核时与 `assets/remote-access/app.js` 一致 | `browser-app.js` |

本节具有开始前的运行脚本原件。结束后的运行身份、恢复及最终安装包核对留待下文补充，不从开始前记录推定。

## 正常回答：青色

| 关联对象 | 实际值 |
| --- | --- |
| 提问客户端消息 ID | `ae1760f0-2334-45cd-a768-cd100f1daf08` |
| 原生用户消息 ID | `01a19b5c-8378-7cb3-85f0-29d469f87198` |
| 目标轮次 | `01a19b5c-82f1-7093-92f7-396725def43f` |
| 原生请求 | `method=item/tool/requestUserInput`，数字 `id=0` |
| 原生工具项 | `call_3I3VqDQe4lTnyZQ12Pfq7YFQ` |
| 问题 ID / 题目 | `receipt_color` / 本轮回执选哪种颜色？ |
| HTTP 操作回执 ID | `7d6f903a-feed-46bd-85f7-a8b6ef7e3824` |
| 浏览器网络请求 ID | `74908.355` |

`pending.json` 于 `08:25:36.534Z` 记录第 14 轮处于 `inProgress`，会话状态为 `active / waitingOnUserInput`。`metadata.requests[0]` 的会话、轮次、工具项和问题 ID 完整对应上表，`isBlocking=true`。同轮已有 `user-input-response-0`，其 `requestId=0`、答案为空、`completed=false`。`before-answer-ui.txt` 同时显示同步题卡、两个原始选项、已选择的青色以及“提交回答”按钮。

`answer-http.json` 保留唯一一条 `/api/action/answer`：

```json
{
  "threadId": "01a19a01-ebb4-7ee1-a8e1-90d180656ac1",
  "requestId": 0,
  "method": "item/tool/requestUserInput",
  "response": {
    "answers": {
      "receipt_color": { "answers": ["青色"] }
    }
  }
}
```

该请求携带与上表操作回执一致的 `idempotency-key`。原 HTTP 为 200，回执 `state=completed / result.ok=true`，返回同一会话、`requests:[]`、上述运行 epoch 和 `requestsRevision=7`。`Network.requestWillBeSent` 至 `Network.loadingFinished` 为 **243.161 ms**，与原件 `durationMs` 一致。该值为本次 HTTP 样本，不包括人工选择等待，也不能推出稳定性能改善。

`answered.json` 于 `08:29:17.568Z` 记录同一第 14 轮已完成，会话回到 `idle`，待处理请求和队列为空。同一 `user-input-response-0` 变为：

```json
{
  "requestId": 0,
  "turnId": "01a19b5c-82f1-7093-92f7-396725def43f",
  "answers": { "receipt_color": ["青色"] },
  "completed": true
}
```

原生最终回复为 `SYNC_QUESTION_OK:青色`，开始时间为 `08:28:06.571Z`；`answered-ui.txt` 显示相同回复且已无待答题卡。正常题的创建、网页提交、原 HTTP 成功和原生消费相互对应。

## 成功响应丢失：橙色

| 关联对象 | 实际值 |
| --- | --- |
| 提问客户端消息 ID | `94024d9c-0b57-4380-9e90-20fa36d4574c` |
| 原生用户消息 ID | `01a19b67-8edf-7440-bbed-4d3081fa44c5` |
| 目标轮次 | `01a19b67-8e21-7550-8ac4-4d6b80a1a7d6` |
| 原生请求 | `method=item/tool/requestUserInput`，数字 `id=1` |
| 原生工具项 | `call_chpn3KjCfQzao6mTATLRSw5u` |
| 问题 ID / 题目 | `recovery_color` / 恢复测试回执选哪种颜色？ |
| HTTP 操作回执 ID | `ca0de8cd-1310-4fd2-873e-99259b31979e` |
| 浏览器网络请求 ID | `74908.488` |
| Fetch 拦截 ID | `interception-job-37484.0` |

`recovery-pending.json` 于 `08:37:29.682Z` 记录第 15 轮等待真实用户输入，原生 `user-input-response-1` 尚未完成。`recovery-before-answer-ui.txt` 显示同步题卡、橙色选择及提交按钮。

故障准确命中了**已成功的响应阶段**，证据链如下：

1. `recovery-network.json` 的 `sequence=1415` 记录实际发出的唯一 `/api/action/answer`，`requestId=1`，正文为 `response.answers.recovery_color.answers=["橙色"]`，幂等键为上表操作回执 ID。
2. 同一原件的 `sequence=1422` 为 `Fetch.requestPaused`，`networkId=74908.488`、`responseStatusCode=200`。`recovery-accepted-paused.json` 保留相同拦截对象、真实响应头及响应体。
3. 对 `rawResponse.body` 按 Base64 解码，所得 JSON 与保存的 `receipt` 完全一致：`requestId=ca0de8cd-1310-4fd2-873e-99259b31979e`、`state=completed`、`result.ok=true`、同一会话、`requests:[]`、同一运行 epoch、`requestsRevision=10`。
4. 同一网络请求最终记录 `Network.loadingFailed / net::ERR_ABORTED / canceled=true`，没有交付给页面的 `Network.responseReceived`。这证明原回答请求已经到达并成功，不能解释为请求未发送。

原生最终回复 `SYNC_RECOVERY_OK:橙色` 于 `08:38:23.307Z` 已产生，早于该回答 Fetch 在约 `08:38:48.038Z` 被终止。`recovery-completed.json` 于 `08:40:11.746Z` 记录第 15 轮完成，会话 `idle`，待处理请求和队列为空；准确的 `user-input-response-1` 保存 `answers.recovery_color=["橙色"]`、`completed=true`。该原生消费证据独立于页面是否收到原 HTTP 成功回执。

### 状态读取与事件流的边界

`recovery-network.json` 未截断、无下一页，捕获到 `08:39:59.993Z`。故障窗口包含三组 `/api/read/thread` 与 `/api/read/history`：两组位于回答响应仍被暂缓期间；第三组紧随 `net::ERR_ABORTED`。整个已捕获窗口中 `/api/action/answer` 恰为一次，`/api/receipts/<ID>` 为零次，`/api/action/send` 和引导发送为零次。

本次故障只阻断回答的 HTTP 响应，事件流未作为故障对象关闭；事件流和普通只读请求仍可参与收敛。保存的浏览器脚本也明确支持 `remote/requestsChanged`、`serverRequest/resolved` 和后续读取更新阻塞请求集合。页面在这种其他通路仍可用的条件下移除了已完成题卡，不能把该现象归因为同步题按原操作 ID 查回执成功。网络原件没有保存 SSE 事件正文，因此本文也不把某一条具体 SSE 事件认定为唯一收敛原因。

`browser-app.js` 中的同步题走 `submitBlockingRequest`：同一请求提交中有内存锁，网络异常后安排 `reloadThread()`，权威请求快照或完成事件可移除已结束请求。异步题另有 `questionReplyReceipts / recoverQuestionReceipts` 的原 ID 查询路径。本文不会将异步题的恢复机制套用到此次同步题。

`recovery-ui.txt` 显示准确的最终回复且没有待答题卡。`recovery-after-reload.json` 于 `08:40:46.222Z` 记录 `questionCount=0`、`replyVisible=true`；对应 UI 快照和 PNG 仍显示 `SYNC_RECOVERY_OK:橙色`，问题未重新出现。**已捕获的异常后读取窗口没有再次回答；整页刷新后的题卡未复活。**刷新后的网络账本未单独归档，不能把刷新后零题卡扩展为一个额外完整网络窗口的次数证明。

## 唯一消费、历史与普通草稿

复核直接解析 JSON 并比较完整轮次对象；对每轮以深度 100 的压缩 JSON 序列化进行区分大小写的逐字比较，没有只比较消息正文或轮次 ID。

| 对照 | 比较范围 | 结果 |
| --- | --- | --- |
| `preflight.json` → `answered.json` | 已有前 13 轮完整对象 | 全部一致 |
| `answered.json` → `recovery-pending.json` | 已有前 14 轮完整对象 | 全部一致 |
| `answered.json` → `recovery-completed.json` | 已有前 14 轮完整对象 | 全部一致 |

两份提问原请求的客户端消息 ID，在各自完成历史中均只匹配一条正式 `userMessage`；其文本与原 `turn/start` 请求逐字相同。两轮各只有一个对应 `userInputResponse`，其数字请求 ID、轮次及问题 ID 均与原待答请求一致，完成历史中分别记录青色和橙色。结合各回答窗口唯一的 HTTP answer，可确认这两次实测各只有一份已观察到的正式消费；没有把同文回复、页面题卡消失或操作回执 UUID 当作原生请求身份。

正常题回答在原第 14 轮内完成，故障题回答在原第 15 轮内完成，回答没有另外创建普通用户消息轮。两轮完成后队列均为空。

`recovery-after-reload.json` 仍保存普通消息草稿：

```text
[remote-e2e:RA-20261102-sync-question:unsent-draft] 回答后保留这条未发送草稿。
```

全部已归档原生历史中，该草稿标识没有出现在正式 `userMessage` 或 `steeringUserMessage` 内；回答网络窗口没有普通发送请求。上述证据确认该草稿没有随这两次回答进入已归档会话，且刷新后仍在输入草稿中。页面 UI 文本快照未展开普通输入框的值，本文以 `recovery-after-reload.json /draft` 为草稿内容证据。

## 本次结论与未覆盖项

| 子场景 | 结论 |
| --- | --- |
| 原生 Plan 预设与单次 `turn/start` 产生真实同步题 | 通过 |
| 远程正常回答、原 HTTP 成功、原生准确消费 | 通过，单次青色样本 |
| 原生成功后丢失 HTTP 响应、后续状态读取、已捕获窗口未重答 | 通过，单次橙色样本；事件流仍可用 |
| 整页刷新后不复活题卡、普通草稿仍可取得 | 通过，按已归档页面状态 |
| 原 13 / 14 轮完整历史保持 | 通过 |
| 同步题按原操作 ID 查询 receipt 后解除未知状态 | 未验证，本次原 ID 查询次数为零 |
| 回答响应与事件流同时不可用、长期 pending/unknown、404、明确失败、授权或来源切换、迟到回执、多题并发 | 未验证 |
| 同步问答性能稳定改善、全部审批路径、正式安装版及 Android 真机 | 未验证 |

源码 117 的阻塞问答分支直接从原生当前 `requests` 精确查找请求，调用 `replyWithUserInputResponse` 后返回阻塞请求快照，见已捕获原生脚本约 6970–7011 行。此次真实功能结果与该路径相符，但没有适配器调用次数探针或对照样本；网页自己的历史刷新仍出现在网络记录中，不能据此声称整条用户流程从未读取历史，或量化移除完整历史读取带来的耗时收益。

## 运行结束与环境恢复

主验收补齐了 `native-final117-after-identity.json` 及运行脚本全文：与 before 同为 scriptId 74715、fixture `b8efa7cb-1124-44d8-8335-7981c1916f88`，实际脚本SHA-256均为 `5e3ac1d40fc337c6246ef11e875900b7c7987eaa2c96fc0c0d70880e41c4125d`。业务源码117/兼容标记106的起止文本逐字相同，网页实际响应亦与当前app.js一致；见 `final-evidence-check.json`。

随后进行了普通发送及队列恢复验收。全部结束后，`thread-mode-restored-after-queue.json` 中测试会话的完整 `latestThreadSettings` 与 `preflight.json` 逐字段相同，恢复Default且会话空闲。临时适配器已恢复106，写入状态表为空、fixture为空，见 `fixture-restored.json`、`runtime-restored.json`。本轮两个浏览器标签已关闭；1432调试服务停止记录位于相邻 `client-preferences-final/service-stopped.json`。没有退出或撤销原授权。

最终 `/api/session` 返回200，设备ID为 `9274b4d8-e65e-4a3c-8eef-9d738a730e55`，`readOnly=false/allowApprovals=true/nativeWriteAvailable=true`；见 `session-final.json`。这里只证明本轮结束状态，不将其说成历史Cookie身份已恢复。第二个localhost客户端登录被设备数量上限拒绝，见相邻 `client-preferences-final/second-client-blocked.txt`；未撤销其他设备来取得测试条件。

补读的 `recovery-after-reload-network.json` 标记 `truncated=true`，不能把其中没有answer请求扩大为完整刷新窗口的零重答证明。它不替代已完整保存的故障回答窗口。

Windows15:56包、Android14:16包摘要重新核对未变；本轮未改产品代码、未重新构建、安装、提交或发布。HTTP证据中的9处鉴权头已脱敏，业务请求、回执正文和事件顺序未修改，变更路径及前后摘要见相邻 `evidence-http-redaction-20261102.json`。
