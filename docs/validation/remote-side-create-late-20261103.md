# R2-05侧边创建成功回包迟到：独立复核

日期：2026-11-03，Asia/Shanghai。

**限定子场景通过：M创建空侧边S，成功响应被暂缓；按已登记路径点击返回后切换C，放行响应后仍留在C，C草稿逐字不变；随后回M可见该侧边。** 本报告不覆盖通知入口直接切换、创建失败或unknown，也不据此将R2-05整项改为passed。

原件目录为`dist/validation/remote-acceptance-20261103/side-create-late-121/`，下文简称B。本次交叉读取原请求、原响应、UI采样、截图及原生快照，未以`verification.json /status=passed`代替核对；只新增本报告，未操作浏览器、RPC、产品或总表。

## 1. 对象与证据链

| 对象 | 准确身份 |
| --- | --- |
| 父会话M | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` |
| 控制会话C | `01a1996d-0b7a-71d0-b9ad-9c521d224ad9` |
| 本次创建并清理的S | `01a19d7e-5300-7612-b4f3-ff021f6140a8` |
| 独立页面／服务 | `http://127.0.0.1:45430`，PID78728 |

时间均为北京时间。网络时刻取[network.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/network.json)的wallTime及对应单调时钟。

| 时序 | 可复核原件与实际结果 |
| --- | --- |
| 02:21:03.817，从M创建 | 唯一请求`62056.149`发往`/api/action/createSideThread`，正文threadId=M，receiptId为`d9426de4-6389-412e-9ee3-09e323d9793b` |
| 成功响应暂缓 | 同networkId出现`Fetch.requestPaused`，interception ID为`interception-job-48244.0`，`responseStatusCode=200`；[create-response.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/create-response.json)为同requestId、completed/ok，准确S及parentThreadId=M |
| 02:21:18.372起，已切C | `.156`读取C、`.157`读取C关联列表、`.158`读取C历史，均早于创建responseReceived |
| 02:21:36.615，放行前 | [c-before-release.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/c-before-release.json)：URL准确C，draft=`R2_05_C_DRAFT_不被迟到回包覆盖` |
| 约02:21:36.729，创建响应到达网页 | `62056.149`的responseReceived为HTTP200；[action-responses.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/action-responses.json)中该body与create-response整个对象深比较相同 |
| 02:21:49.421，放行后 | [c-after-release.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/c-after-release.json)仍为同一C URL、同一draft；[现场截图](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/c-after-release.jpg)独立显示C页面及该非空草稿 |
| 02:22:29.497，原生身份 | [native-before-close.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/native-before-close.json)：标签`sidechat:<S>`、conversationId=S、类型local-side-chat，registry明确S→M、closed=false |
| 02:22:46.330，回M查看 | [parent-after-return.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/parent-after-return.json)的URL准确M，draft=`R2_05_M_DRAFT_父会话保留`，菜单含“侧边聊天”；[父列表截图](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/parent-side-list.jpg)相符 |

S的准确身份由创建回执及原生标签／registry建立，不只凭同名菜单项。关闭前原生历史`isComplete=true/turns=[]`，本次没有向S发送消息。

创建请求到responseReceived的单调时钟差为`(299325.961818−299293.049076)×1000=32912.742ms`，约32.9秒。该数值包含人为暂缓成功响应，不能当作服务处理耗时、正常创建性能或重试时间上限。归档的loadingFinished携带较早网络时间，不能用它替代放行后的responseReceived判定页面何时收到创建结果；本例也没有逐帧连续采样整个32.9秒的页面。

## 2. 派发次数与关闭终态

`B/network.json`为`truncated=false/hasMore=false`，sequence589—1340。只按requestWillBeSent及唯一requestId计动作，不把Fetch暂停、responseReceived、loadingFinished或重复归档响应另算一次派发。

| 动作 | 次数与network ID | 核对结果 |
| --- | --- | --- |
| createSideThread | 1：`62056.149` | 原成功回执绑定S→M，没有第二次创建 |
| closeSideThread | 1：`62056.181`，02:23:06.668 | 原请求准确S和M；HTTP200、completed/ok、closed=true，requestId=`bbd31dc5-3714-489c-8051-8a6d702ea2e5` |
| send | 0 | 没有为建立结果向空S或M/C补发消息 |
| markRead | 4：`.159`、`.171`、`.223`、`.227` | 与创建／关闭单列；前三项有HTTP200，`.227`在归档窗口中未配得响应，不把4次请求扩写为4次成功回执 |

`B/action-responses.json`保存创建、关闭及前两项markRead的完整body，各自requestId均与原请求receiptId相同。网络另有一次`/api/session`的ERR_ABORTED（`.228`），不对应创建或关闭动作；本例不据此宣称所有后台读取都无异常。

[native-after-close.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/native-after-close.json)采于02:24:16.999：S的nativeTab和registry均null，桥接metadata/history均`ok=true/value=null`，直接metadata及includeTurns读取均为准确S的`thread not loaded`。这确认本次空S已清理；不继承运行中关闭、丢回包或unknown分支通过。

## 3. M34／C8及设置权限保持

独立比较[baseline.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/baseline.json)（02:18:59.575）和[final.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/side-create-late-121/final.json)（02:24:17.617）：按准确M/C选择的整个thread对象分别逐字段相同，包括以下对象：

| 检查对象 | 原始范围与结果 |
| --- | --- |
| M历史 | `history.isComplete=true`完整34轮及`persisted.thread.turns`34轮，前后分别相同 |
| C历史 | 加载0轮、`history.isComplete=false`；persisted完整8轮，前后分别相同；不把0轮加载视图当完整历史 |
| 设置／权限 | 两者`metadata.latestThreadSettings`及`currentPermissions`整个对象相同 |
| 队列、requests、问答及其他原有字段 | 包含在各thread整个对象的深比较中，均相同 |

C的回包前后两个UI采样还单独证明非空draft逐字一致。M回访时可见对应测试草稿；本例没有整页刷新、跨浏览器或长期草稿持久性的独立验收。

## 4. 运行身份与收尾

`B/runtime-installed.json /after`与`runtime-native-engine-after.json`记录同一121业务／106兼容fixture `cccde29f-4037-4b6c-97be-c0350f93b711`、epoch `8ceb170e-8b69-4515-b16c-635b28e90566`。起点实际脚本摘要与终点全文`runtime-native-engine-after.js`重算一致，均为`b1cd3dbf36cb7cf25a569d53aacb9567e35269aac7292f0ae99d932af79daa06`。

`B/page-source-identity.json`记录同页清理后刷新取得app摘要`53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`，与`service-source-snapshot.json`中网页源文件摘要一致。这里核对的是归档身份记录，没有另连接浏览器重取网页全文。`runtime-plan.json`登记的变换仍为两处121→106映射、移除安装引导早退及owned测试ID自动命名保护，不报告已安装或发布新的管理器版本。

收尾分别核对：

- `B/page-cleanup.json`记录独立测试页在M、draft为空、捕获拦截器已清除。保留客户端`retained-client.json`位于端口1432、准确M，原草稿为`STOP_LIVE_2_UNSENT_DRAFT_保留`；两个页面不混作同一草稿对象。
- `B/runtime-restored-engine-119.js`重算为`5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`，匹配进入本轮前119脚本摘要。独立比较installed.before与restored.after：fixture仅runtimeEpoch变化，owned均为空，三个未决提交计数均0，原编辑记录相同；恢复epoch为`275c9422-6714-49fb-b2d1-cd91fc09f6e1`。
- `B/service-finished.json`记录独立PID78728于02:27:11.258以explicit-stop-file停止；`globalNativeDisposed=false`与119恢复记录分开核对，不据单个停止布尔值推断原生恢复。
- 主代理补充：本轮`temp/remote-side-late-native-121-20261103/`已归档并核对哈希，但删除调用被自动审批以`blocked by policy`拒绝，目录保留且未重试。**临时文件清理未完成；运行恢复和独立服务停止已完成。**

本次只确认“返回后切C，再收到原创建成功回包”的限定路径。没有通知入口直接切换、迟到失败／unknown、不重复执行unknown或全部R2-05生命周期的真实通过证据；本报告完成后停止写入。

## 5. 关键原件摘要

以下均为本次直接重算，便于定位相同原件：

| B中的原件 | SHA-256 |
| --- | --- |
| `network.json` | `063adb193b4ee7a880fa551c3d78a53a341be8e26eab12b4f032234997dd5bc0` |
| `create-response.json` | `caac3c324106e93c05efe635aa1bcaca34b9ad306354378223b1be739ee47c7f` |
| `action-responses.json` | `2da7456771f35644b431e1a7aecbddf7862715690bc7448dbbaaf436ccfa4c4e` |
| `c-before-release.json` | `48eeef75685eeb71471e8ca46a1d13c652b60e87a72af7154becdc91c7c2b3b5` |
| `c-after-release.json` | `76cc79811b3df051afc1151d1381062ceeaadd6c4a5780bcbf94adae369d31af` |
| `parent-after-return.json` | `b856231993acd48bc2b539f4a06e2dd1baf8f529089278a2be6ec466a923b4bd` |
| `native-before-close.json` | `4dfb422aa620a0c582ce4a4ab354d5842f52bd9b3db333d27bb0a4d1544469c4` |
| `native-after-close.json` | `cebcc3b6c4a1ed42e731997899859ce854cbd00cd71e565a593942447caf9e75` |
| `baseline.json` | `758d9a7a33bed7b87981a3a29723f1770ba60689cea56766294c2cde07f8951b` |
| `final.json` | `6a3542833b72bbc91831d3b0bc3779f6c3e51b68a7024313b8c0dedae31139ae` |
