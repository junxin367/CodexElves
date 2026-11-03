# 网页通知导航与侧边创建回包竞争：独立复核

日期：2026-11-03，Asia/Shanghai。

**网页层限定子场景通过：旧版已复现通知切到C后仍被创建面板阻塞、迟到成功回包又退回M；新版在回包前解除阻塞，C继续输入的草稿在回包后保持，回M可见正确S。正常创建仍自动打开新侧边，返回后M草稿保持。** Android通知事件派发、系统通知点击及真机效果仍交用户验收；本报告不将它们标为通过。

原件目录为`dist/validation/remote-acceptance-20261103/notification-side-create/`，下文简称B。本次只读交叉核对原件并新增本报告，没有操作现场、改动产品或总表、重跑测试或提交代码。

## 1. 真实入口、版本与环境

入口是实际app模块的`openNotificationThread(C)`边界函数调用，保存的函数见`B/notification-function.js`；不是模拟创建或回执。`B/before-fix-identity.json`明确`bridgePreloadApplied=false`、`androidEventDispatchTest=false`、`androidDeviceTest=false`，因此只确认网页导航边界。

M为`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，C为`01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。网页地址为`http://127.0.0.1:1434`，由开发进程77892读取磁盘网页并代理到真实后端`http://127.0.0.1:45430`（PID72312）。45430程序内嵌网页仍为旧版，不能把后端服务重启等同于新网页生效。

| 实际Debugger脚本全文 | 本次直接重算SHA-256 |
| --- | --- |
| `B/app-before.js`，690680字节 | `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22` |
| `B/app-after.js`，691144字节 | `641b08ac6f4571ba59dd71e8c94df279c0ae5ece85a22f4885e83ea34c00881b` |

首次刷新曾被Service Worker命中旧脚本；执行方随后仅对本测试tab绕过缓存，并取得上表新脚本全文。`B/client-cleanup.json`记录同一新摘要及缓存设置复原。本次不把绕过缓存后的源码生效当作PWA自动更新验收，也不报告新安装包已安装或发布。

## 2. 旧版与新版的独立对照

下列时刻均为北京时间。网络原件分别为[旧版network](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/before-fix-network.json)和[新版network](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/after-fix-network.json)；两个窗口均`truncated=false/hasMore=false`。

| 阶段 | 直接观察及原件 |
| --- | --- |
| 旧版创建等待中打开C | 02:50:22.105，[pending JSON](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/before-fix-pending.json)的URL已为准确C，但`inert=true/panelPresent=true`；history仍含原创建requestId `c7d58473-7b14-42e6-8d91-83d38ada7c35`。截图实际仍覆盖“正在创建侧边会话”面板 |
| 旧版原成功回包到达 | `70708.206`约02:50:22.224收到HTTP200；02:50:39.661，[arrived JSON](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/before-fix-arrived.json)及截图已回准确M，draft变为M的`NOTIFY_M_DRAFT_保留`，原创建history标记仍在 |
| 新版回包前打开C | 03:02:53.201，[pending JSON](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/after-fix-pending.json)为准确C，`inert=false/panelPresent=false`，history只含conversation position，旧创建标记已清除 |
| 新版在C继续输入 | 03:03:14.429，[typed JSON](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/after-fix-typed.json)及截图显示draft已从`NOTIFY_C_DRAFT_保留`变成`NOTIFY_C_DRAFT_回包前继续输入`，仍为可用C页面 |
| 新版原成功回包到达 | `70708.578`约03:03:14.556收到HTTP200；03:03:29.577，[arrived JSON](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/after-fix-arrived.json)及截图仍为准确C、同一继续输入后的draft，inert和panel仍为false，history没有原创建标记 |
| 回M查看侧边 | 03:04:38.864，`B/after-fix-parent.json`为准确M，草稿`NOTIFY_M_DRAFT_保留`，菜单出现“侧边聊天”；更早的`B/native-sides-before-cleanup.json`已独立确认该S的原生标签、未关闭registry及准确父M |

旧、新创建均只暂缓已有HTTP200成功响应，两个`Fetch.requestPaused`分别绑定原network ID及原receiptId；完整create-response与各action-responses中的对应body深比较相同，不能把旧版错误归因于创建失败或伪造回执。

按requestWillBeSent到responseReceived的单调时钟差复算，旧、新暂缓样本分别为39.132秒和33.988秒；均包含人为等待，不是服务性能对比或重试上限，也不代表逐帧连续观察。

## 3. 正常创建及三条侧边收尾

| 场景 | 准确S | 唯一create／close network ID |
| --- | --- | --- |
| 旧版通知竞争 | `01a19d98-8ecc-7f81-a842-8be32521b53e` | `70708.206`／`70708.253` |
| 新版通知竞争 | `01a19da4-6bc0-75b3-bcd5-bf5529b36e53` | `70708.578`／`70708.614` |
| 新版正常创建 | `01a19da6-cfc6-7430-b134-26b080341cda` | `70708.626`／`70708.655` |

逐项核对`B/before-fix-action-responses.json`及`after-fix-action-responses.json`：三个create、三个close各一次，均HTTP200、completed/ok；原请求receiptId与响应requestId逐一相同，父会话均为M，close均`closed=true`。两个窗口合计send为0，自动markRead为6，响应与Fetch暂停不重复计算派发。

正常第三次创建于03:05:18.363返回成功；03:05:29.098的[normal-created-ui.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/notification-side-create/normal-created-ui.json)、DOM及截图表明：已自动选中准确第三个S，related history为side、ephemeral、parent=M，`editable=true/inert=false/panelPresent=false`。03:06:05.316的`B/normal-returned-ui.json`返回准确M，草稿仍为`NOTIFY_M_DRAFT_保留`。

最终`B/final.json /sides`逐项列出上述三个准确ID，全部`registryPresent=false/metadata=null`，与三个真实关闭回执相符。这里只确认本次三个空侧边的正常创建和关闭，不扩为创建失败、丢回包、unknown、运行中关闭或Android返回行为通过。

## 4. 修复源码与数据保持

独立比较两份实际网页脚本，唯一差异为`selectThread`中新增9行（当前`assets/remote-access/app.js:9801`起）：离开创建父会话且非preparedView时，先dismiss创建面板并将原事务autoOpen置false；只清除owner和requestId均匹配的创建history键。既有`dismissSideCreation`恢复workspace原inert状态且保留原事务，正常preparedView路径继续使用原流程。真实旧、新表现与这处差异相符。

`scripts/remote-access-ui-state.test.mjs:1681`起新增5项回归定义，分别覆盖立即解除面板、迟到回执不退C不覆盖继续编辑、正常自动打开、主动返回只后退一次、只清理本页本记录history键。本报告只核对源码定义，没有重跑或用测试数量替代真实验收；Skill／图片草稿等额外断言也不继承为本次真实通过。

独立比较`B/baseline.json`（02:49:24.557）和`B/final.json`（03:07:29.147），准确M和C的整个thread对象分别逐字段相同：

- M的完整加载历史及persisted均34轮；C加载0轮且不完整，persisted完整8轮，分别保持，不混淆两种历史来源。
- 两者latestThreadSettings、currentPermissions、队列、requests及原有其他字段均保持。
- `B/native-runtime-before.json`与after除capturedAt外整个对象相同：实际仍为119业务／106兼容，完整运行摘要`5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`、epoch `275c9422-6714-49fb-b2d1-cd91fc09f6e1`未变；owned为空，三个未决提交计数均0。本次没有将原生运行切成121。

## 5. 环境收尾与剩余范围

`B/client-cleanup.json`记录测试页草稿为空、缓存覆盖设置复原、拦截清除；执行方已关闭测试页。`B/service-finished.json`记录PID72312于03:08:27.310以explicit-stop-file停止；`B/environment-restored.json`于03:08:31.789记录临时后端和开发进程77892均停止，保留监听45222/PID13568及1432/PID63860。本报告仅复核归档收尾记录，没有重新查询现场。

**结论范围为网页实际通知入口边界＋真实HTTP/native创建。** Android桥接注入、`codex-elves-notification-open`事件派发、系统通知点击、真机输入及PWA更新均未在本轮执行，Android仍待用户验收。报告完成后停止写入，不改动主表或扩大全部侧边验收结论。

| 关键B原件 | 本次直接重算SHA-256 |
| --- | --- |
| `before-fix-network.json` | `16b2da1c92dc262a86f3efe4359fbb7ee350e90bde641b55e754571098a7065f` |
| `after-fix-network.json` | `84e56eaac9b052611e7dd32cd8834357668b01eccc4bf4249beb20208d64bec2` |
| `baseline.json` | `be8af7ecd46e964122e4d15c938dac867c2f9fff9d85e5a8c59388fd88ef294f` |
| `final.json` | `e452d095582b3e1c8c6fad2f9ad34f994140fe7c632d72a7646eb48a016437c6` |
