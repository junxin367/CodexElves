# 121侧边关闭残留窗口：独立验收复核

日期：2026-11-03，Asia/Shanghai。

**限定结论通过：侧边正常创建、发送并完成后，关闭标签和关系；即使关闭后raw仍返回同ID的ephemeral对象，121的thread/history读取也明确返回HTTP410，没有再包装成可用main会话。** 本次确认的是该关闭残留窗口的读取拒绝，不是全部侧边生命周期、真实负面发送或121过期UI的完整验收。

## 1. 原件与准确对象

以下B为`dist/validation/remote-acceptance-20261103/side-lifecycle-121/`，R为`dist/validation/remote-acceptance-20261103/runtime-121/`。

- M：`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`。
- C：`01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。
- 本次唯一侧边S：`01a19d2f-4d20-7612-bcd9-be91eb22778f`。

本次只读取原件并新增本报告；未操作现场、重跑测试、改动产品、旧报告或覆盖总表，未创建新原件或派生代理。

## 2. 创建、发送与关闭均有真实回执

时间取`B/network.json`的wallTime，换算为北京时间；按唯一networkId的requestWillBeSent计派发。

| 动作 | 北京时间2026-11-03 | 唯一networkId | 原回执requestId |
| --- | --- | --- | --- |
| 从M创建S | 00:54:44.847 | `62868.212` | `6aafc9c9-867b-4bfe-8e69-fb067ddfbc5b` |
| 向S发送1条文本 | 00:55:25.617 | `62868.235` | `bbb0199d-5cc6-41a7-bc3b-bb0d00e97065` |
| 关闭准确S及父M | 00:56:58.275 | `62868.278` | `fe27dcb5-a706-446b-a912-2ce190049212` |

三项各一次，均有HTTP200。逐项核对`B/responses.json`：完整响应body中的requestId等于原请求receiptId，threadId均为S；创建和关闭的parentThreadId均为M，state均completed、result.ok均true，关闭明确`closed=true`。

发送回执为accepted/start，client为`3ec2ef78-9b52-4069-a739-dcb9bcb1ccd7`，返回turn为`01a19d2f-f97c-7520-b59d-b50ddd9a00e4`。不只依赖accepted回执：`B/before-close.json /side/history/value`为`isComplete=true`、完整1轮、同turn且completed；正式userMessage的client相同，正文是`[remote-e2e:R2-05:closed-metadata-121]`对应的原请求，原生input仅追加末尾换行；唯一final_answer为`SIDE_121_READY`。

00:56:14.728的关闭前快照中，S的tab类型为`local-side-chat`、tabId为`sidechat:<S>`，registry明确S→M、`closed=false`。创建回执初名为“侧边聊天”；登记owned后的原生tab/registry名为`[remote-e2`，主代理现场观察网页同名。本目录没有另存该帧网页UI，本报告只记录名称一致，不判断该名称的语义优劣或将十字符规则认定为产品要求。

`B/network.json`覆盖00:54:44.847—00:58:48.075，`truncated=false/hasMore=false`，188个事件对应94次请求及94次响应，没有loadingFailed。主代理记录本case没有新增故障拦截；不继承120场景的网络干预。另有3次自动markRead（`62868.214`、`.253`、`.267`），不混入创建／发送／关闭计数；窗口内没有第二次send或重复关闭。

## 3. raw残留真实存在，121读取明确拒绝

`B/after-close.json`采于00:57:11.817，按准确S核对：

| 来源 | 关闭后实际值 |
| --- | --- |
| `side.tab`、`side.registry` | 均null |
| `side.metadata`、`side.history` | 均`ok=true/value=null` |
| `side.raw` | `ok=true`，仍返回准确S；`ephemeral=true`、`path=null`、`source=vscode`、`parentThreadId=null`、`canAcceptDirectInput=true`、idle、空turns |

这真实复现了此前119材料中“标签和原生加载对象已移除，但raw临时metadata仍可读”的条件。raw读取成功本身不能证明侧边仍有效，也不能宣称底层对象已完全销毁。

随后`B/closed-raw-window-http.json`与`B/network.json`互相配对：

| 接口 | 唯一networkId／请求时间 | HTTP与原响应 |
| --- | --- | --- |
| `/api/read/thread`，threadId=S | `62868.286`／00:57:27.330 | 410，`{"ok":false,"message":"侧边会话已过期或已关闭"}` |
| `/api/read/history`，threadId=S | `62868.287`／00:57:27.621 | 410，同上 |

因此本次没有重现此前将关闭侧边包装为HTTP200、`viewKind=main`的读取结果。证据是先捕获raw成功截点，再核两项410；没有为每一个HTTP请求另取同步raw快照，不扩写为对整个残留时长的连续采样。

共享入口的源码依据为`assets/remote-access/native.js:1580`起的`readThread`：对缺少有效侧边／子代理身份的ephemeral候选，再核原生加载对象及侧边关系，确认缺失后抛410。`assertWritableThread`和动作前置guard也调用该入口。

`crates/codex-elves-core/tests/cdp_bridge.rs:5559`的`request_client_discarded_ephemeral_metadata_is_rejected_before_read_and_send`覆盖read、writable、thread、send，并断言410、准确ID及无派发副作用；这是源码回归范围。**本次没有对已关闭S提交负面send，也没有在本复核中重跑该回归，不报告真实发送拒绝通过或追加测试通过数量。**

## 4. M30／C8与设置权限保持

独立按准确ID比较`B/baseline.json`（00:54:27.868）、`B/before-close.json`（00:56:14.728）和`B/after-close.json`（00:57:11.817），不采用辅助verified字段代替深比较：

| 对象 | 原始历史来源 | 三个截点的独立比较 |
| --- | --- | --- |
| M | `history.isComplete=true`的30轮及`persisted.thread.turns`完整30轮 | history整个对象、persisted整个对象分别逐字段相同 |
| C | 当前加载0轮、`history.isComplete=false`；正式persisted完整8轮 | 加载视图和persisted分别逐字段相同；不把0轮视图当完整历史 |
| M、C各自设置 | `metadata.latestThreadSettings` | 整个对象逐字段相同 |
| M、C各自权限 | `metadata.currentPermissions` | 整个对象逐字段相同 |
| M、C队列及待处理请求 | `queue`、`metadata.requests` | 分别逐字段相同，队列均为空 |

role及hostId也分别相同。这里只证明侧边case三个截点的M30／C8；之后另一个引导场景形成的M31不参与本次比较，不能用更晚截点倒推本侧边case发生了历史变化。

## 5. 运行身份与恢复

三份业务快照均记录121业务／106兼容、fixture为`2f766d1f-0cf3-42b7-8c65-81e8a7e59928`、epoch为`e760fedd-747c-48e9-8335-8743f46f2c4f`。R中的脚本全文已独立重算：

| 材料 | 本次重算SHA-256与比较 |
| --- | --- |
| `R/native-original-121.js` | `54b79c2095b7486ce21a580fe61a571a90c6f2cf95d88f0de9350ec722e54c73` |
| `R/native-under-test.js`、`native-engine-before.js`、`native-engine-after.js` | 三者逐字节相同，均`cb14de43cb993c5690e74e8a9cd855bd84719060bbca3bb45773b8f13a15a181`，匹配对应Debugger身份记录 |
| `R/actual-app.js` | `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`，匹配actual-app-identity及服务source snapshot |
| `R/baseline-runtime-119.js`、`restored-engine-119.js` | 两者逐字节相同，均`5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749` |

`R/plan.json`明确4项运行变换：两处121→106版本映射、移除本次安装引导早退、沿用仅对owned测试ID的自动命名保护。业务快照fixture的`runtimeSourceSha256=97830d…`是compatibility body摘要；实际完整运行脚本是上表`cb14de…`，不混为同一计算对象。

独立比较`R/baseline-identity.json`与`R/restored.json /after`：fixture仅runtimeEpoch变化，owned名单均为空；恢复后的epoch为`00837805-d4d5-46ce-b12a-d2f745f04068`。三个未决提交计数均0，原8项编辑记录保持且无beginPending/terminalPending。恢复的是进入本轮前的119运行基线，不是原生安装版106；不报告121已持续安装或发布。

`R/server-ready.json`为端口45420、PID74324；`R/server-finished.json`记录01:10:57.391以explicit-stop-file停止、`globalNativeDisposed=false`。原端口45222/PID13568保持由主代理现场核对，本报告未另连接现场。

## 6. 验收边界与原件身份

本次可追加R2-05关闭终态及关闭残留窗口的读取拒绝证据；不覆盖运行中关闭、unknown丢回包、跨父竞态、所有临时会话或真实负面发送。正常创建／发送没有被此次guard误拦的限定路径已证；其他主会话、子代理和错误分支仍分别按源码回归及各自真实证据判断。

网页app脚本与120阶段相同，但本次没有重新操作并归档121的底部过期页、非空草稿保持或过期后返回按钮；不把120的UI通过继承为121新一轮全验，也不把未执行的独立项目判为本关闭读取场景失败。总表与73项通过计数不变。

| B中的原件 | SHA-256 |
| --- | --- |
| `baseline.json` | `ce0d8f9dff661ad3e9e7c1f91e9b3c530f31dd4bd97c21d4045783a5375ff09e` |
| `before-close.json` | `00fc87f6b7d715e137874032b86e576aee57e020df7968c853d642e3dca445a5` |
| `after-close.json` | `6af320986deed0904eebea9ea52f3d1ae43977fcd1b63e1969fba96a2fd2e353` |
| `closed-raw-window-http.json` | `103eb5f9eeb758ec98143376c108db74c9b180e35c86c2e7e2525f86c1774505` |
| `network.json` | `dd25146b9ed6c795362b7bc325b8024ef52ccc13577ad76e6b27ce7c5c37e0b8` |
| `responses.json` | `32e77591fb76e0d7d2c971b81c85d73c1182ed1385b9acc9a96dcc79b1239c16` |
