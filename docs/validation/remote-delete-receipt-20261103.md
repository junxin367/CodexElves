# 删除成功回包丢失：独立验收复核

日期：2026-11-03，Asia/Shanghai。

**限定结论：两次不同测试会话各删除一次，服务原响应均已成功，随后才在Response阶段人为丢弃；原生与原rollout文件均确认删除。第二次在故障前客户端已持久化同一事务的deleted状态，迟到网络错误未再报错，刷新后两条测试会话不复活、M原草稿保持。** 本例不覆盖所有确认来源长期不可用的unknown，也不把两次不同删除算成重试。

原件目录为`dist/validation/remote-acceptance-20261103/delete-receipt/`，下文简称B，第二次原件位于`B/r2/`。本次只读原件、检查源码并新增本报告，没有操作浏览器、服务、RPC、其他会话或产品，没有提交代码。

## 1. 准确事务、原幂等键与丢包位置

| 独立事务 | 准确删除目标N | 原幂等键／回执ID A | 唯一POST networkId／Fetch ID |
| --- | --- | --- | --- |
| N1 | `01a19ea1-6ca6-7523-bc26-3486e56a5f6c` | `7a6c0067-3532-40c3-9b35-a350080f1b35` | `71364.424`／`interception-job-56937.0` |
| N2 | `01a19ea7-b474-76e0-ac29-ce403e5a3944` | `3c0c2d00-4a7e-425f-9d26-4fe47f8569fa` | `71364.689`／`interception-job-57354.0` |

逐项核对[第一次原成功响应](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/delete-receipt/accepted-delete-response.json)和[第二次原成功响应](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/delete-receipt/r2/accepted-delete-response.json)：

- 原始`Network.requestWillBeSent`均为唯一POST `/api/action/deleteThread`，正文只指向各自N，header中的`idempotency-key`等于各自A。N1发于07:54:36.300，N2发于08:07:03.565（北京时间）。
- `Fetch.requestPaused`的networkId、Fetch ID、原请求正文及幂等键均匹配；事件包含`responseStatusCode=200`与响应头，证明暂停在Response阶段，非派发前拦截。
- 两份Fetch body均为`base64Encoded=true`。本次重新解码后，分别与`B/decoded-delete-receipt.json`及第二份原件的`receipt`整个对象相同：`requestId=A/state=completed/result.ok=true/threadId=N/deleted=true/status=local_deleted`。解码后各183字节，与原Content-Length一致。
- `B/response-dropped.json`及`B/r2/response-dropped.json`分别记录对上述Fetch ID执行Aborted；对应after-drop网络原件均有**同一networkId**的`Network.loadingFailed/net::ERR_ABORTED/canceled=true`，没有把其他读取失败当作删除丢包。

因此两次均是“服务已经完成删除，原成功HTTP回包被丢弃”，不是服务未执行、删除失败或伪造成功回执。

## 2. 唯一派发与确认顺序

已合并核对N1的held、next、after-drop、refresh、after-refresh，以及N2对应的start、held、after-drop、refresh、after-refresh共10个网络窗口，均`truncated=false/hasMore=false`。按requestWillBeSent的唯一requestId去重，仅有上表两个删除POST，各绑定不同A/N；Fetch暂停、失败事件及重复出现在归档中的原请求不另算派发。

这些窗口没有第二次删除POST，也没有`/api/receipts/<A>`读取。**本例不要求已收到准确删除终态后再GET回执**；不把“没有GET”判为失败，也不据有限窗口声明整个应用从未执行其他动作。

N1初始`B/after-drop-ui.json`的status采样末项为`Failed to fetch`，同文件还采到了隐藏DOM中的桌面未连接、侧边过期等文案；后两者不能判成可见故障。稍后的`B/after-drop-visible-ui.json`及截图未显示该提示。该轮没有归档原生删除事件与迟到异常的先后顺序，不能断定提示一定发生于客户端已确认deleted之后，更不能据此给N1强判根因。N1后续持久记录仍为同A/N1、state=deleted，刷新后无N1行。

N2则有独立的[故障前确认记录](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/delete-receipt/r2/confirmed-before-drop.json)：08:07:29.208客户端已将**同A/N2、准确deviceId `9274b4d8-e65e-4a3c-8eef-9d738a730e55`**持久化为deleted，当时M草稿为`STOP_LIVE_2_UNSENT_DRAFT_保留`；08:07:44.542才丢原成功回包。[丢包后可见UI](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/delete-receipt/r2/after-drop-ui.json)及截图仍为准确M，未显示`Failed to fetch`，截图保留同一M草稿。

`B/app-fixed-runtime.js`为实际DOM onclick所属脚本全文，本次重算为`64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`，与本次读取的`assets/remote-access/app.js`逐字节相同。`deleteSidebarThread`的catch在准确device/thread已deleted时不再notice迟到异常（当前源码`:4367`起），同时保留未确认对象的原错误处理。N2实测符合这个限定分支，不据此宣称任意网络错误均应静默。

## 3. 删除终态、其他会话和刷新

四份原生快照按准确ID独立深比较：`B/before-delete.json`、`after-delete.json`、`before-delete-r2.json`、`after-delete-r2.json`。

| 对象 | 独立核对结果 |
| --- | --- |
| N1 | 删除前persisted完整2轮；删除后metadata=null，persisted为准确ID的thread not loaded，queue为准确ID的no rollout found |
| N2 | N1删除期间整个对象保持；自身删除前persisted完整2轮，删除后同样metadata=null、persisted与queue准确缺失 |
| M `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` | 各次前后整个归档thread对象逐字段相同，含persisted完整44轮、settings、permissions和queue |
| C `01a1996d-0b7a-71d0-b9ad-9c521d224ad9` | 各次前后整个归档thread对象逐字段相同，含persisted完整8轮、settings、permissions和queue |
| 已删除的N1在N2期间 | 归档对象逐字段保持缺失，没有复活 |

`B/rollout-after-delete.json`与`B/r2/rollout-and-controls-after-delete.json`分别记录各自**原rollout绝对路径**`present=false`，与原生缺失证据配对，不只依赖网页行消失；本次未重新访问文件系统原路径。

[N2最终刷新记录](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/delete-receipt/r2/after-refresh-ui.json)及截图于08:12:30.717确认URL为准确M、`remainingTestRows=[]`、草稿仍为`STOP_LIVE_2_UNSENT_DRAFT_保留`。N1阶段保留的是N2页面的`DELETE_CONTROL_DRAFT_保留`，两个控制草稿不混作同一对象。

## 4. 运行与范围边界

四份原生快照的fixture/epoch一致：121业务／106兼容，fixture `ecfd06c7-ffd4-4d66-8e1c-0c69a14c1860`、epoch `fec066b9-c266-46da-a744-ec1109faca15`。关联原件`dist/validation/remote-acceptance-20261103/goal-partial/runtime-capture-after-goals-and-delete.json`记录结束运行摘要`42351e5724acf0777ebc9478cd8456297fcd255a088a934b23759b4a28768bbc`。后续`goal-partial/runtime-restored.json`已确认恢复原119全文，摘要`5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`、新epoch `ef679de6-97f3-431f-a838-a23e912693d8`，原fixture除实际epoch外保持；不将其描述为生产安装升级。

两份response-dropped均记录`interceptionCleared=true`，执行方已把Fetch patterns清空。本复核没有再更动现场配置。

本次可追加真实永久删除、成功回包丢失、另一确认来源已先收敛后迟到异常不再报错、刷新不复活及无重复删除的限定证据。**长期所有来源均unknown、真正未确认时的持续回执核对与恢复、服务重启后恢复、Android及全部删除竞态仍未验。** 报告完成后停止写入。

| 关键B原件 | 本次直接重算SHA-256 |
| --- | --- |
| `accepted-delete-response.json` | `3c202193aea9772acf0f05d35a245c8c685adf898edc902229de04c82afa7424` |
| `decoded-delete-receipt.json` | `f470efa111013f7ca7f80763bb4ba5ba5318416e810368d2f365ff57b6239805` |
| `r2/accepted-delete-response.json` | `a724d5e147b1e6a72000e60bd7b89dd275979b8f5d0e87605892e6da4adc1637` |
| `r2/confirmed-before-drop.json` | `bf098a785de5d0343f14be4f7631df885b68b1b34144c0f468007fea12bee042` |
| `before-delete.json` | `70ff2c46f863bfd05c4ef5582ae6928f4faac209480524ae0c5a2b9b888243ed` |
| `after-delete.json` | `2a7c612a1fc8ef5ae13a88590447af84476479c4b307679e71291123a8c7290d` |
| `before-delete-r2.json` | `38a703e4acafac5afe505eb6649ef8641a4c3164393345f0d6972842ba3207f3` |
| `after-delete-r2.json` | `4e42b4360c8787deec62bd80121624201452767fa09fde54b83b85811acd263c` |
