# 侧边关闭成功回包丢失：独立复核

日期：2026-11-03，Asia/Shanghai。

执行方收尾补充：同目录 `runtime-capture-after-side-close-compaction.json` 确认本轮结束原生仍为121夹具实际摘要 `4543c4728aa5123a142e771722ca4fcaf1dd16d74eef7084805e3b3ae10eeab2`；`runtime-restored.json` 已验证恢复原119全文 `5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`，新epoch为 `a28d96f2-154f-437c-8746-fe505e23c0c2`。`browser-restored.json` 记录拦截清除与当前草稿保持；原标签已缺失、原因未观测，当前保留测试页1283922838供继续目标使用。后续压缩产生的M46轮不得反填为本关闭场景44轮的变化。

**限定子场景通过：空侧边S仅关闭一次，服务已返回成功体后原HTTP回包被丢弃；unknown记录、原回执ID及普通草稿刷新后保持。用户点击现有“查回执”后，仅GET原A取得同一completed结果，操作记录清空、S退场，草稿不变。** 这是用户主动查询恢复，不报告自动GET或所有来源长期unknown已通过。

原件目录为`dist/validation/remote-acceptance-20261103/side-close-compaction/`，下文简称B。本报告只取baseline、side-before/after-close、side-create／close／between-create-close材料及运行身份；后续compaction材料不参与本案比较。本次未操作现场、产品、主表、服务或其他会话，未提交代码。

## 1. 准确对象及唯一创建／关闭

| 对象 | 原件值 |
| --- | --- |
| S | `01a19ed7-264a-7be3-948d-aa882ef9037c` |
| M | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` |
| C | `01a1996d-0b7a-71d0-b9ad-9c521d224ad9` |
| 原创建A | `37e11acd-637b-48de-a748-447e61110fc9`，networkId=`26108.79` |
| 原关闭A | `3c48f6f9-d237-45bb-bcf5-18f7597f44ae`，networkId=`26108.166` |

`B/side-create-events.json`的唯一POST createSideThread于08:37:42.312发出（北京时间），正文threadId=M，幂等键等于原创建A；[原创建响应](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-close-compaction/side-create-response.json)为同A、completed/ok、准确S及parentThreadId=M。关闭前原生tab为`sidechat:<S>`、kind=local-side-chat，registry明确S→M且closed=false；完整侧边历史`isComplete=true/turns=[]`，没有向空S发送消息。

唯一POST closeSideThread于08:41:11.161发出，正文准确S与M，原header幂等键等于关闭A。[close-accepted-response.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-close-compaction/close-accepted-response.json)内的Fetch暂停事件对应同networkId及`interception-job-58322.0`，包含HTTP200及响应头，证明是**Response阶段**暂停；body为Base64，本次重新解码后与保存的receipt整个对象相同：

```json
{"requestId":"3c48f6f9-d237-45bb-bcf5-18f7597f44ae","state":"completed","result":{"ok":true,"threadId":"01a19ed7-264a-7be3-948d-aa882ef9037c","parentThreadId":"01a19a01-ebb4-7ee1-a8e1-90d180656ac1","closed":true}}
```

解码后213字节，与原Content-Length一致。`B/close-response-dropped.json`于08:41:30.150记录同Fetch ID执行Aborted并清除拦截；`B/close-dropped-events.json`随后记录准确`26108.166`的net::ERR_ABORTED，不以其他请求失败替代本动作丢包。

## 2. unknown刷新保持与手动查回执

| 截点 | 直接核对结果 |
| --- | --- |
| 丢包后08:42:10.441 | [close-before-refresh-state.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-close-compaction/close-before-refresh-state.json)仅一条closeSideThread记录：原关闭A、准确S→M、state=unknown、autoOpen=false、原scope及selectionToken均保留；UI显示“关闭结果待确认，原请求标识已保留”和“查回执” |
| 刷新后08:43:16.621 | [close-restored-state.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-close-compaction/close-restored-state.json)的operations整个数组与刷新前逐字段相同；draft仍为`STOP_LIVE_2_UNSENT_DRAFT_保留` |
| 用户点击“查回执”08:43:17.489 | 唯一`26108.259`为GET `/api/receipts/3c48f6f9-d237-45bb-bcf5-18f7597f44ae`；[查回的原回执](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-close-compaction/close-queried-receipt.json)HTTP200，解码后整个对象与原关闭成功体相同，没有新关闭POST |
| 查询后08:43:48.812 | `B/close-finished-state.json`的operations为空，STOP草稿逐字保持；`close-finished-ui.json`及截图为准确M，待确认提示与查询按钮消失。主代理收尾检查菜单无S，准确原生关系消失另见下一节 |

独立核对side-create、between-create-close及5个close网络窗口，均`truncated=false/hasMore=false`。按唯一requestWillBeSent计派发：create1、close1、send0、自动markRead2；GET原A1次，仅出现在close-query窗口。响应、Fetch暂停及同一ID的归档重复不另计派发。查询前的refresh窗口没有GET原A，不能把用户点击改写为自动恢复。

执行中因手动误关菜单发生的一次定位失败属于测试操作，未形成额外close POST；不据它判定产品故障或将此单次场景扩大为菜单稳定性全验。

## 3. 原生关闭与M／C保护

只比较[baseline.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-close-compaction/baseline.json)（08:36:20.947）、`side-before-close.json`（08:38:50.288）、`side-after-close.json`（08:42:35.772）：

- S的tab和registry由准确S→M变null；桥接metadata/history均ok但value=null，raw直接读取为准确S的thread not loaded/-32600。关闭结果有原生终态支持，不只依赖网页菜单消失。
- M、C各自整个归档thread对象在三个截点分别逐字段相同，包括完整persisted历史、已加载历史、latestThreadSettings、currentPermissions及queue。
- M的persisted为44轮、已加载history为45轮且标记完整，两种来源分别保持；C的persisted为8轮、当前加载0轮且不完整。不将加载数量与持久轮数混用，也不把后续压缩导致的M变化归到本案。

## 4. 运行身份、收尾边界与原件摘要

`B/app-runtime.js`实际脚本全文重算为`64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`，与本次读取的当前app源码逐字节相同。原生为121业务／106兼容，fixture `633df1c2-1d8c-4aff-b2fc-200e87bfe1ba`、epoch `1fe806bb-39a4-45f9-9ba2-f11bb54286bd`。`B/native-test-121.js`重算`4543c4728aa5123a142e771722ca4fcaf1dd16d74eef7084805e3b3ae10eeab2`，匹配runtime-installed及业务快照中的运行摘要。

丢包记录支持本次Fetch拦截已清除；原生恢复留待整轮最终收尾，**本报告不提前报告已恢复119或整个环境已清理**。不覆盖运行中关闭、未知结果长期无法查询、所有来源unknown、Android或任意跨端竞态。

| 关键B原件 | 本次直接重算SHA-256 |
| --- | --- |
| `baseline.json` | `d08462ec11cc036ea4ea67ba6034f0c080dc2bcb4f6c20efbd253ce70f4a0f35` |
| `side-before-close.json` | `1ca37edcc06b753a65d394cd20111a7f110a9f38017721f2f095719009a42505` |
| `side-after-close.json` | `93245595b7af7a42d208a8e9c0335a8d044bb31d21e23a34232f99c53ae7d07b` |
| `close-accepted-response.json` | `4a954878725f25645d3dab334868517d89c90a2d3962943668e292fec50c1b64` |
| `close-before-refresh-state.json` | `3cbc2f1d54953699f764e22402e925fbc39e435d2b722eaca2903587570c48f5` |
| `close-restored-state.json` | `a421f3ce18a54fe47683559bbd59c110e5cd9fde99a3cc366fef0f6bea0bd2ad` |
| `close-queried-receipt.json` | `0a0580954602664028b23ebe8b6dec9137e469d723430f83665983c81d04f75f` |
| `close-finished-state.json` | `32cec9d56d3cba04d3ffef3bcc1bcefe73edfb178092b6436b90285769c263d4` |

透明复算结果另存B中的`close-independent-verification.json`。本报告完成后停止写入。
