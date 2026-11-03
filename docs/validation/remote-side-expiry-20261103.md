# 侧边会话关闭、返回与过期显示：独立验收复核

日期：2026-11-03，Asia/Shanghai。真实运行跨越2026-11-02与2026-11-03的北京时间午夜。

**结论：两条侧边的关闭回执、原生标签移除及后续对象缺失均有证据；第一条最终返回准确父会话，第二条在准确ID的503缺失响应后保留已加载消息、隐藏输入区并显示过期提示。以上仅为限定子场景通过，R2-03、R2-05及R3-18仍不能整项改为passed。**

本次证据边界：第一条A端曾断网，两条路径的差异不能只用“是否离线”解释；第二条的关联列表读取存在拦截或中止，结论限定于该受控场景。`restored-final-A`两份文件与过期页面逐字节相同，支持解除网络限制后仍停留S2过期页，与本过期显示场景的预期一致。返回父会话按钮的实际操作属于独立未验范围，不构成本场景失败。过期提示原文为“侧边会话已过期／该会话已结束，当前仅可查看已保留的消息。”。

本次交付仅为原件覆盖审计，不判定新缺陷的根因。只新增本报告；没有启动浏览器或服务、调用RPC、修改产品或原件、重跑既有测试、提交或推送代码。

## 1. 原件与运行身份

以下使用B代表实际证据目录：

`dist/validation/remote-acceptance-20261102/side-expiry-live/`

已清点、读取并重算B中全部37个文件的SHA-256；JSON及NDJSON均可解析。截图已直接查看，未把文件名或汇总布尔值当作结果。第8节列出逐文件摘要。

| 对象 | 原件实际值 |
| --- | --- |
| 父会话M | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` |
| 第一条S1 | `01a19ceb-6afb-7111-a0a3-8c174885b570` |
| 第二条S2 | `01a19cf9-424b-7092-a5b4-41edde1d1991` |
| 另外用于缺失响应对照的既有对象S0 | `01a19a9a-0435-70b1-b979-e041ed4e25ed`，不是本次S1或S2 |
| 实际网页与服务地址 | `http://127.0.0.1:45420`；`server-ready.json /processId=5148` |
| 服务和网页源码 | 服务快照`sourceVersion=120`；实际归档网页脚本见`actual-app.js` |
| 编译副本中的原生标记 | `120→106`的guard和export两处映射，见`server-source-snapshot.json /nativeEdits` |
| 现场原生业务／兼容版本 | 各原生快照`fixture.sourceVersion=119`、`compatibilityVersion=106` |
| 原生fixture／epoch | `61b3c80f-2248-4a29-b908-c5e853e4321b`／`1f26a046-3ad0-4f02-9a9f-85c62627b91a`，六份原生快照一致 |
| 原生业务／运行变体摘要记录 | `d8c635875c50622194fdfebcede393afb540c9263bc44c6a3809231ceca00944`／`312103909fc1c62a6b783268566eccb856aaa7b9352ba25c8d1024b0d00277ed` |

`actual-app.js`重算为`53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`，同时匹配`actual-app-identity.json /sha256`及`server-source-snapshot.json /originalSourceFiles`中`assets\remote-access\app.js`的摘要。身份记录中的实际脚本URL为`http://127.0.0.1:45420/app.js`、scriptId为`36`。

`server-build-result.json`记录构建退出0，程序为`temp/remote-live-120-20261102/bin/remote-live-120.exe`。本次重算仍在磁盘的该程序，匹配记录摘要`7f9fc187132bc79cf309e33bf80e7816e583d872a4b90c84c2e83a8d321299cf`。这是独立验收服务程序的证据，不是新管理器安装包、安装或发布证据。本次未从Debugger重新读取原生运行脚本全文，原生源码身份按归档快照记录，不能宣称纯120原生运行或用户已安装120。

## 2. 跨午夜时序

表中时间优先取原件的ISO时间、`wallTime`或毫秒字段，统一换算为北京时间。UI文件没有嵌入采集时间时，只标明文件保存时间，不能拿它计算精确交互耗时。

| 北京时间 | 对象与事实 | 原始依据 |
| --- | --- | --- |
| 2026-11-02 23:39:30.803 | 端口45420实例启动，PID5148 | `B/server-ready.json /startedAtMs` |
| 2026-11-02 23:44:50.595 | S1标签、registry和完整1轮原生历史存在 | `B/native-before-close.json /at` |
| 2026-11-02 23:45:14.037 | A的session请求随后报`net::ERR_INTERNET_DISCONNECTED` | `B/network-A.json`，`85004.191` |
| 2026-11-02 23:46:07.988 | B唯一一次关闭S1请求 | `B/network-B.json`，`76580.91` |
| 2026-11-02 23:46:18.690 | S1标签、registry、原生metadata/history读取均为空 | `B/native-after-close.json /at` |
| 2026-11-02 23:46:37.578起 | A读到M的关联列表为空；随后读取S1、M及M历史 | `B/network-A.json`、`B/after-online-responses-A.json` |
| 2026-11-02 23:50:14，保存时间 | S1关闭后A显示M标题、主会话历史和正常输入区 | `B/normal-closed-return-A.png` |
| 2026-11-02 23:54:06.036 | S1直接metadata及includeTurns读取均报准确ID的`thread not loaded` | `B/native-closed-metadata.json /at` |
| 2026-11-02 23:56:50.965 | S2标签、registry和完整1轮原生历史存在 | `B/native-second-before-close.json /at` |
| 2026-11-02 23:57:50.366 | B唯一一次关闭S2请求 | `B/second-close-network-B.json`，`76580.246` |
| 2026-11-02 23:58:33.083 | S2标签及桥接读取为空，但直接metadata读取仍返回同ID的ephemeral对象 | `B/native-second-after-close.json /at` |
| 2026-11-03 00:00:00.852 | S2直接metadata/history均报准确ID的`thread not loaded` | `B/native-second-closed-check-2.json /at` |
| 2026-11-03 00:04:24.049 | A发出读取S2请求，收到准确ID缺失的503 | `B/expired-A-network.json`，`85004.382`；`B/expired-A-http-responses.json` |
| 2026-11-03 00:06:12.016—00:06:13.007 | A再三次读取S2，各收到相同503缺失响应 | 同上，`85004.403`、`.406`、`.408` |
| 2026-11-03 00:06:33，保存时间 | S2仍选中，已加载消息保留，底部过期提示可见、输入区隐藏 | `B/expired-A-dom.txt`、`B/expired-A-ui.json`、`B/expired-A.png` |
| 2026-11-03 00:07:41.589 | 网络恢复记录：offline为false、拦截patterns和blockedUrls为空 | `B/network-restore.json /at` |
| 2026-11-03 00:08:43，保存时间 | 最终DOM及截图仍与S2过期页面完全相同 | `B/restored-final-A-dom.txt`、`B/restored-final-A.png` |
| 2026-11-03 00:09:27.846 | 服务终止记录：`explicit-stop-file`，`globalNativeDisposed=false` | `B/server-finished.json /stoppedAtMs` |

目录日期20261102不能代替运行日期：第二条过期503、最终UI、网络恢复和服务停止都发生在2026-11-03北京时间。

## 3. 第一条：准确关闭与返回父会话

### 3.1 关闭对象及原生终态

`native-before-close.json`中S1的`nativeTab.conversationId`等于S1，tabId为`sidechat:01a19ceb-6afb-7111-a0a3-8c174885b570`，类型为`local-side-chat`；registry明确`parentThreadId=M`、`closed=false`。父关系由原生标签及registry建立，不按同名“侧边聊天”推断。

完整原生历史为`isComplete=true`、1轮，准确turn为`01a19ced-5c4d-7d82-8ebc-7a74242efa27`、状态completed，包含一个userMessage与一个final_answer `SIDE_EXPIRY_CONTENT_OK`。与关闭前网页正文及回复相符。

B发往`/api/action/closeSideThread`的原请求正文为：

```json
{"threadId":"01a19ceb-6afb-7111-a0a3-8c174885b570","parentThreadId":"01a19a01-ebb4-7ee1-a8e1-90d180656ac1"}
```

网络ID`76580.91`的HTTP状态为200，`receiptId=b3b1f4bd-6c9c-4d55-a5ce-614255156ed3`。`close-response.json`为同requestId、`state=completed`、`result.ok=true`、同threadId/parentThreadId且`closed=true`；该整个对象与最终服务回执表中的唯一同项深比较相同。

23:46:18的原生快照中`nativeTab=null`、`registry=null`、`nativeMetadata.value=null`、`nativeHistory.value=null`。但其直接includeTurns读取仍报“ephemeral threads do not support includeTurns”，这条错误在关闭前也存在，不能独自证明对象已销毁。23:54:06的补查才同时得到直接metadata及includeTurns读取的准确S1缺失错误。

### 3.2 返回父会话，不是过期横幅

A窗口记录过离线错误。恢复读取后，`after-online-responses-A.json`显示：

- `85004.193`及`.197`均返回M的`sideThreads=[]`。
- `85004.196`读取准确S1却返回HTTP200，thread仍用S1的ID，但标题为“新会话”、`viewKind=main`、`parentThreadId=null`。
- 紧随其后的`.198`读取准确M，`.199`读取M历史；截图显示M的独立测试标题及其`STOP_QUEUE_2_OK`历史，输入区可见，没有底部过期提示。

由准确M读取和截图，支持“关闭后最终导航回原父M”的限定结论。不能把中间S1的HTTP200包装对象解释为原侧边仍有效，也不能据最终返回判定所有读取分支均已保持正确侧边身份。

本次关闭来自B网页的`closeSideThread`动作，确实移除了原生标签；没有原生桌面菜单关闭操作的录像或回执。不能替代“用户在原生桌面UI关闭”的独立交互验收。

`before-close-A.png`确实显示未发送草稿`SIDE_EXPIRY_UNSENT_DRAFT_120_保留`。没有S1关闭后的该草稿对象对照，因此不追加S1草稿保留通过。

## 4. 第二条：准确缺失错误与保留历史的过期页

### 4.1 准确关闭，不能省略中间metadata状态

`native-second-before-close.json`中S2标签和registry均指向准确S2及父M，registry为未关闭。完整原生历史为1轮、`isComplete=true`，turn为`01a19cf9-8845-7c32-8b0e-9128e533def2`、completed；正式用户item为`01a19cf9-889b-71e2-a266-e655b6735881`，client为`609d222b-fe01-42d5-9c57-a2b9942af8ed`，final_answer为`SIDE_EXPIRY_SECOND_OK`。

B网络ID`76580.246`只有一次关闭请求，正文为准确S2和M；HTTP200，receiptId为`6968c4ea-640f-453e-9ae0-7873654b0177`。`second-close-response.json`同requestId、completed/ok、`closed=true`，与最终服务回执中的唯一同项深比较相同。

23:58:33的`native-second-after-close.json`中标签、registry及桥接metadata/history已为空，**但`directMetadataRead.ok=true`，返回准确S2、`ephemeral=true`、idle、空turns的对象**；当时直接includeTurns仍是ephemeral不支持错误。不能将这份快照概括成“所有原生读取立即不存在”。

00:00:00.852的`native-second-closed-check-2.json`中，直接metadata与includeTurns均变为`ok=false`、`code=-32600`、准确S2的`thread not loaded`。这支持后续截点对象缺失；不证明23:58时直接metadata为何仍可读，亦不证明关闭后绝无短暂对象重建或可读状态。

### 4.2 503响应与网页结果

四份HTTP原响应均为：

```json
{"ok":false,"message":"thread not loaded: 01a19cf9-424b-7092-a5b4-41edde1d1991"}
```

`expired-A-http-responses.json`记录HTTP503，网络ID分别为`85004.382`、`.403`、`.406`、`.408`；逐一匹配`expired-A-network.json`中的准确S2读取正文及503响应事件。不是任意503、权限失败或其他会话缺失。

`server-diagnostics-final.ndjson`同时记录S2的native缺失、service `thread/read`缺失及`remote_access.side_thread_expiration_detected`，与HTTP及UI方向相符。诊断中的service fallback是读取链路，不能据此声称向CLI重发了普通消息。

`expired-A-ui.json`的实际结果：

| 字段 | 实际值与可支持结论 |
| --- | --- |
| `url` | hash仍为准确S2，没有导航回M |
| `composerHidden` | true，输入表单被隐藏；不能只写“禁用按钮” |
| `expiredHidden` | false，过期提示可见 |
| `expiredText` | `侧边会话已过期\n该会话已结束，当前仅可查看已保留的消息。` |
| `messages` | S2原用户正文及`SIDE_EXPIRY_SECOND_OK`仍可见，与关闭前完整1轮内容相符 |
| `draft` | `SIDE_EXPIRY_SECOND_DRAFT_保留`，非空值仍存在于该UI采样字段 |

截图及DOM独立支持消息可读、底部过期提示和输入区消失；“返回主会话”按钮仍可见，菜单中“新建侧边会话”为disabled。

草稿的现场工具输出与目录原件分开说明：本线程题为“隔离第二条侧边的关闭广播和关系刷新，保留真实内容读取”的CUA输出记录`online=false`、URL为准确S2、`draft=SIDE_EXPIRY_SECOND_DRAFT_保留`，messages含原正文与`SIDE_EXPIRY_SECOND_OK`，且`sendDisabled=true`；B关闭后另一次现场输出仍显示同一draft，最终`expired-A-ui.json`也保存同值。这些现场输出支持非空草稿在关闭前已存在，并与关闭后采样相同。

`second-before-offline-A.png`和相应DOM采到的是上一帧空placeholder，缺少该非空值的归档基线；仅独立读取B目录时，草稿证据仍限于终态非空。本报告未补造新原件，也不追加全过程连续不变或整页刷新后持久保留通过。

### 4.3 网络干预与代码解释

`expired-A-network.json`从00:00:27至00:06:31的窗口中，session/sidebar等读取存在成功响应；12个`relatedThreads`请求却以8次空错误及4次`net::ERR_ABORTED`结束。`network-restore.json`还记录5个拦截任务的释放尝试：4个已结束或不可用、1个continued，最终patterns与blockedUrls为空、offline为false。

因此本例包含针对网络读取的干预，不能当作“恢复后所有接口均正常”的自然路径。37个文件中未见S2离线开始设置的控制回执，不能精确复算其完整离线区间；文件名`second-before-offline`本身也不是控制回执。

保存的实际`actual-app.js:1971`起，检查同时读取当前侧边thread与父relatedThreads：关联列表成功且不含当前ID时走`retireNativeSideThreads`并返回父会话；捕获准确ID的缺失错误时走`retireExpiredSideThread`。`actual-app.js:2005`起限定503必须对应当前侧边及准确缺失ID；`:2028`起将当前侧边标记expired/readOnly、保留消息显示并同步布局；`:2111`起的正常关闭分支则调用`returnToParentSession`。

这解释了两类UI分支与归档结果的一致性，属于代码与原件的对应关系；没有调试器逐行执行轨迹，不能声称已完全复原每次并发返回的先后顺序。S1的缺失响应未落入503分支；S2的关联列表干预也可能影响哪条结果先决定页面状态，不能只用“是否离线”解释两者差异。

### 4.4 解除网络限制后保持过期页

独立字节比较结果：

- `expired-A-dom.txt`与`restored-final-A-dom.txt`完全相同，均1174字节、SHA-256相同。
- `expired-A.png`与`restored-final-A.png`完全相同，均30432字节、SHA-256相同。

最终文件仍是S2原正文、原回复和底部过期提示，支持解除网络限制后保持过期页的场景预期。`restored`结合`network-restore.json`说明网络条件的恢复；本次没有操作返回按钮，返回M及其后草稿编辑属于独立未验范围，不作为本过期显示场景的失败，也不据此推断全部现场收尾完成。

## 5. 请求数量与对照证据

仅按`Network.requestWillBeSent`及唯一requestId计动作；responseReceived和服务回执不重复算派发。四个窗口均`truncated=false/hasMore=false`，只对各自记录的时间窗口负责。

| 窗口 | 请求时间范围（北京时间） | 显式动作 |
| --- | --- | --- |
| `B/network-A.json` | 11-02 23:45:14.037—23:46:59.052 | 自动markRead 1次（`85004.200`），没有关闭或发送 |
| `B/network-B.json` | 11-02 23:45:36.793—23:48:09.371 | 关闭S1 1次（`76580.91`） |
| `B/second-close-network-B.json` | 11-02 23:57:36.793—11-03 00:08:36.811 | 关闭S2 1次（`76580.246`） |
| `B/expired-A-network.json` | 11-03 00:00:27.688—00:06:31.295 | 无`/api/action/`请求；四次准确S2读取均为503 |

上述窗口没有重复关闭、普通send、删除或重试派发。创建和先前发送发生于其他时间，不能据本表声称整个运行仅执行两次动作。

`service-side-receipts.json`与`service-side-receipts-final.json`整个数组逐字段相同，共10项、requestId均唯一。两次关闭回执分别与其中唯一对应项完整相同；另有两项创建、两项accepted发送及四项仅含ok/threadId的回执。回执数组自身没有完整的动作名与请求正文，不能单靠后四项猜测它们全部是什么动作。

`known-expired-old-http.json`及`known-expired-120-http.json`比较的是S0：两者thread/history均503，前者为通用桌面不可读取提示，后者保留准确S0缺失错误。它支持已归档响应文本有差别，不能冒充本次S1/S2实际过期页，也不能在缺少URL、运行身份完整配对时宣称完成了所有旧新版本受控对照。

## 6. 可追加范围与仍待验收项

| 基线 | 本次有原件支撑的增量 | 不继承的范围 |
| --- | --- | --- |
| R2-03 侧边返回与刷新 | S1关闭后最终回到准确父M；S2准确缺失后仍保留已加载消息 | 两侧独立非空草稿与阅读位置、刷新／后退、缺父关系、S2过期后返回按钮实际操作 |
| R2-05 侧边创建迟到／关闭 | B各关闭一次准确S1/S2，原生标签和registry移除，后续直接读取均缺失 | 迟到创建不抢C、运行中关闭、立即所有内存不可读、失败／unknown、原生桌面菜单关闭 |
| R3-14 原生失效与CLI兜底 | S2缺失错误经读取兜底保留并到达网页过期分支 | 全部主会话兜底、子代理／侧边写入资格、派发后unknown与禁止重送的完整验收 |
| R3-18 关闭／删除未知与终态 | 两次正常关闭的唯一HTTP请求、原回执及后续缺失截点 | 丢回包、unknown核对、不重复关闭的未知分支、删除、其他侧边与主历史的完整前后比较 |
| 过期UI | 当前ID的503与原缺失文本配对；消息保留，composer隐藏，过期提示可见 | 任意503误判反例、权限／跨ID／迟到错误真实运行、刷新后持久恢复、Android真机、已安装管理器 |

整套73项仍不能据此宣布完成。本报告不改写既有覆盖表或通过计数，也不把既有39/39、桥接测试或源码反例替代本次真实场景。

后续独立验收项包括：正常网络下关联列表与thread并发收敛；S2过期后返回M的实际按钮操作及草稿跨刷新持久保留；关闭后直接metadata短暂可读的独立只读调查；运行中关闭和关闭unknown。这些项目不改变本次受控场景中解除限制后仍显示过期页的通过结论。当前原件不足以决定metadata中间状态的修复方案。

## 7. 原件、主文档与现场边界

`server-diagnostics.ndjson`是`server-diagnostics-final.ndjson`的字节前缀；前后服务回执数组相同。`server-finished.json`支持本实例记录为停止，且明确没有dispose全局原生适配器。`network-restore.json`支持记录中的网络条件复位，不等于浏览器标签已关闭、全局119/106适配器已撤回或临时目录已清理。本次未额外连接现场核验这些状态。

本轮首次读取时，以下五份主文档的摘要如下；旧交接摘要中部分值已与现场不一致，本次没有恢复旧内容或将差异归因于本轮。

| 受保护主文档（均位于`docs/validation/`） | 本轮读取基线SHA-256 |
| --- | --- |
| `remote-acceptance-cases-20261101.md` | `74381d8048fe60f9383d54bca64d3041ed98eb83268adf3c1a8cfaba62392a8c` |
| `remote-acceptance-coverage-20261101.md` | `ba8a69db886577d79d0c5b54b313896f2dd64e50e29e38b651bd98a1d51de5cf` |
| `remote-acceptance-progress-20261101.md` | `9ca468967a35f44210af930522d3a3a590025956ec6d6764cdc98b1f8e91f645` |
| `remote-acceptance-progress-20261102.md` | `70bdd9e28cf6e3f80cf526f90847d0510af01f25e6de21197747e2d2f37ce4f7` |
| `remote-performance-20261102.md` | `ca23b4d90421942f62d00971bdf73cf497fb953819d4ac3ebf6f9a5666a1b7d0` |

## 8. 全部37份原件摘要

下表均为本次直接重算。按文件名升序，将每项拼为`文件名 + NUL + 十进制字节数 + NUL + 小写SHA-256 + LF`，UTF-8编码后再取SHA-256，37项整体清单摘要为`be8866c885f389b352e9d74408e45f5a706f44a08e3d6e0b3917bf3598f4ef22`。它用于确认本轮前后材料未变，不构成外部签名或最初采集后从未修改的证明。

| B中的原件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `actual-app-identity.json` | 144 | `5b2a69acae6de84e958be09388dc28b88c055d8a68eedd5a3798abc9b1c44c8a` |
| `actual-app.js` | 690680 | `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22` |
| `after-online-responses-A.json` | 11519 | `db8a15502e72918e67fe7dc3b0f0878c5e12307e87bf2caf7af5612fd806fd0b` |
| `before-close-A-dom.txt` | 1534 | `04fe2b7db39b4542d5a294ee6049afb7a37251e4736cfcab3eaf45c616a6be12` |
| `before-close-A.png` | 34436 | `db05e37076f771f5e0a8c9077fda26908afd47e12f3a5161bb0577dc7484f62d` |
| `close-response.json` | 213 | `d73279ee1bda55badfc97e933991d711eb5cfc58bcd5b5e6257cede372e684b1` |
| `expired-A-dom.txt` | 1174 | `965e18901ba23324103698950d81b2dfd9d216772380aa06ac6ab778fca0c4ff` |
| `expired-A-http-responses.json` | 962 | `6d2352f627e301b3ad56a3917e53d21d29cc3d3e9329ac172e866b6e2402254c` |
| `expired-A-network.json` | 30614 | `3b84c87cefd5b360845243e73f310d23bd00d7b64336d16b883eadac6b748dc5` |
| `expired-A-ui.json` | 457 | `833ad830a948dcf0d91c9b3a60cf6f7f505c32acbaec65792e22f47e526f461c` |
| `expired-A.png` | 30432 | `c85d23436b31c63c3ad4fdc670b3d5f70377ed4aec7b10d77d34bd7aadad6ae4` |
| `known-expired-120-http.json` | 580 | `e877085be8dc056b846c5bd96e04518603976dc4ac1f37658ab79b0699d3ed93` |
| `known-expired-old-http.json` | 964 | `71ccf8a445d44a92a1380022a6bf5573a30e0220cf446865c7adff10d947c2fd` |
| `native-after-close.json` | 1276 | `9d296ce41fbff3d122860d7217b1bd0915d55412a04e02f7f7d37eba7bbc19e7` |
| `native-before-close.json` | 24300 | `4f0ecd38160c5ef5faa1c4369ca83cc0e62760c59733b8e4efd405e3c01ca548` |
| `native-closed-metadata.json` | 1427 | `20ae8680003bf99c910cf007d71d592f3f3d18f076fe1bffd6e45db68f9c880a` |
| `native-second-after-close.json` | 3124 | `e73fb235814a486745287bc7a5cae79f7971ff913040c802fd92384b3f92138c` |
| `native-second-before-close.json` | 25943 | `6ea70bca5ba668467b04f0eb2e756ff0d62079a2c1be0867f04a44bd6a0d0c4d` |
| `native-second-closed-check-2.json` | 1427 | `eb2d4e9da8795745a7a0a3c883122e4a268bb81237482ce5e361d498d820916d` |
| `network-A.json` | 6966 | `5a1d14c54bd6cae3a3c57cb3a9b2f4b5bf1ecff8820021918117e1d1db777029` |
| `network-B.json` | 19694 | `4f57652174f3b0b54d37e07fc43d8a0041227d2ce91b5d41c6e2d26a0fd99f61` |
| `network-restore.json` | 917 | `ac68607c2ec4bb090d275cc5934cd43db240442efb601c019b725b8e4be6e92a` |
| `normal-closed-return-A.png` | 111371 | `c09343b074702aa9dfb46e6cb148e40a27a8d0279abc5b2ebd8be4bcdc7cfdc6` |
| `restored-final-A-dom.txt` | 1174 | `965e18901ba23324103698950d81b2dfd9d216772380aa06ac6ab778fca0c4ff` |
| `restored-final-A.png` | 30432 | `c85d23436b31c63c3ad4fdc670b3d5f70377ed4aec7b10d77d34bd7aadad6ae4` |
| `second-before-offline-A-dom.txt` | 1482 | `770c24dec74dc96cd782e3ac03927fae4f4a9c34fd6f2952a8ee66a7e6507636` |
| `second-before-offline-A.png` | 31000 | `c30a59b40b54297de9d0070a61f8543dd9c7b1b58b40221cf0f1afda42d30783` |
| `second-close-network-B.json` | 66335 | `9029fc6f095072030dd10d696e38f2c8f5b0489ca681c0e488a2c7065738c0bc` |
| `second-close-response.json` | 213 | `7e5865bc4ccff7c66e79b1c04c7604be33d9a06db57d972e04c35e9353e6ea12` |
| `server-build-result.json` | 412 | `ecba2e69215156b7353bea5abff347f3810aae44832399dd60f8cfef7579f8ea` |
| `server-diagnostics-final.ndjson` | 9809 | `ad66ada20f3b6c20dad1f881d163e3b32df849f77e8273ed242ae37e2e4779a2` |
| `server-diagnostics.ndjson` | 3632 | `f9fd3ad8e2d4d4157b418f33d0d1f3026962d1012d7907fce90c23f73f665565` |
| `server-finished.json` | 122 | `11ac8588ddb93dbc40519ad9e2559a0cb28ed763411f087f0b3371c23a7254ec` |
| `server-ready.json` | 876 | `43d5dc725e22fa34f8186e149d485825aa3dfaf257a497aac78402f75bbb0aa2` |
| `server-source-snapshot.json` | 35410 | `e97f62d2896c11ab166ee31ee7729ec14f7cedb44e40c126701f7b661e7e2c5f` |
| `service-side-receipts-final.json` | 5681 | `21989a1121065010bd7070ade9e061ec41f9738de22785b18b5c55a3ab1e81f6` |
| `service-side-receipts.json` | 5681 | `21989a1121065010bd7070ade9e061ec41f9738de22785b18b5c55a3ab1e81f6` |
