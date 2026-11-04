# 122 空闲状态保存队列消息：独立原件复核

日期：2026-11-03，Asia/Shanghai。以下时间均为北京时间。

**空闲保存后正式接受并完成的普通文本子场景通过。** 原 Q 在同步问答等待期间入队，网页取出编辑后原问答自然完成；此时 M 已 idle、队列为空且原 Q 客户端尚未被正式接受。保存编辑稿返回 HTTP 200／completed／committed／deliveryState=accepted，准确启动 T52，原客户端仅一次正式接受，最终回复 `QUEUE_SENT_EDITED`。原 50 轮、控制会话 C、普通稿恢复及模式／权限恢复均有独立比较支持。

网络原件是请求／响应摘录，页面实际执行脚本全文未在本目录归档，测试稿清理 JSON 与截图也不一致；这些边界分别保留。它们不替代原生历史已确认的业务结果，也不扩为全程 HTTP 无重试、整套队列、安装版或全部验收通过。

原件目录为 [queue-sent-live-122](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-sent-live-122/)，简称 B。独立计算结果在 [verification.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-sent-live-122/verification.json)：记录 37 份来源文件摘要、10 份原生快照的逐字段比较、完整输入及准确接受项。此次仅新增本报告与该 JSON，不操作业务、浏览器或服务，不修改产品、原件和主表，不重跑、不派发代理。

## 1. 身份与版本证据

| 对象 | 本目录能够直接支持的事实 |
| --- | --- |
| runId | `RA-20261103-queue-sent-live-122` |
| M／C | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`／`01a1996d-0b7a-71d0-b9ad-9c521d224ad9` |
| 网页 | 自建 tab `1283922948`，来源 `http://127.0.0.1:45430`，页面记录均绑定准确 M |
| 原生 fixture | `ebe5356e-2fc3-4066-bd8e-bfc9f2c274ed`，sourceVersion=122、compatibilityVersion=106 |
| runtime epoch | `22ec6684-09d5-45a3-9c26-b04573f6fa28`；10 份原生快照的 fixture 与 epoch 分别相同 |
| HTTP 资源身份记录 | `00-page-baseline.json.scriptIdentity`：status=200、size=692051、SHA-256=`64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387` |

上述来源、fixture、epoch 及业务对象支持本次属于同一独立服务的受控运行。**HTTP 资源身份记录不等于该 tab 实际执行脚本身份。** B 没有自存该页实际执行的 app.js 全文、Debugger script 身份与源码，也没有本阶段实际原生执行全文，因此本次不能逐字复算本页／本阶段执行源码，不能借 crossflow 另一页的起止脚本全文回填。

原生 fixture 的 sourceSha256、expectedRuntimeSha256 是登记值，不能改称本报告直接取得的执行源码摘要。crossflow 的起止及后续 after-sent 运行时恢复由主代理另行收尾；这里保留业务通过与执行源码身份未独立闭环这两个不同结论。

后续共享原生运行原件已补齐：主代理直接重算 `../crossflow-122/runtime/native-engine-before.js` 与 `native-engine-after-sent.js`，两者SHA-256均为`1b1b49f1a04ccfbd99209cd08a5a5bff858f60badf43cd180a7a0a340faeda7a`，epoch和fixture未变。本阶段原生运行身份因此得到起止执行文本支持；这仍不补齐本页前端实际执行脚本全文的缺口。

## 2. 真实前置状态与消息血缘

| 阶段／直接原件 | 独立核对结果 |
| --- | --- |
| `baseline-native.json`，11:42:43 | M 持久 50 轮、idle、requests=[]、queue=[]；C 持久 8 轮 |
| `original-question-attempt.json`／`original-question-receipt.json` | 原生准备 Plan 模式同步问答，client=`39430e8d-2945-4bd5-b94f-cfdfffc9355f`，T51=`01a19f83-49d3-7932-9ecd-c1513ea04f71` |
| `question-waiting-native.json`，11:47:55 | M 持久 51 轮，T51 inProgress，等待真实 `item/tool/requestUserInput` 的题目 `crossflow_color` |
| `q-enqueued-native.json`，11:51:55 | 原生队列准确存在一项 Q：`01a19f87-8e03-7e03-89b1-eb7ffff8a5a2`；client=`8c5b407e-1240-4a8a-80ec-dc3f88ee0244`；正文要求 `QUEUE_SENT_BASE` |
| `05-q-edit-active.json`／截图及 `q-edit-active-native.json`，11:55 | 网页出现原 Q 正文、保存按钮和“取消编辑并恢复未发送草稿”；原生队列已空，T51 仍等待问答，原 Q client 尚无正式接受 |
| `answer-submitted-native.json`／`post-answer-native.json`，11:58／12:04 | 原题实际回答蓝色；loaded 原轮的 userInputResponse requestId=13、completed=true；T51 completed，回复 `CROSSFLOW_ANSWER:蓝色`；M idle、requests=[]、queue=[]，原 Q client 仍零正式接受 |

网页队列菜单原件 `05-queue-menu-open.txt` 和截图绑定准确 Q。`05-q-edit-active.json.cancelEditControlCount=0` 与同一文件的可见“取消编辑并恢复未发送草稿”行及实际截图不一致，因此本报告没有采用该计数判断控件不存在，直接核对编辑器原文及按钮。

这组原件支持“原 Q 已从队列取出进入网页编辑，随后原活动轮结束”，不是“Q 已自然消费后又新建同文消息”。但 B 未归档 begin 的原 HTTP 请求／回执、原生移除 token 或包含 editId 的编辑状态表；`a025a3a4-3b37-4c0c-b25d-37b208e3ec0c` 可由后续保存请求及原业务响应对应，不能补造成已取得 begin 原始回执。该缺口独立保留。

## 3. 保存回执与准确 T52 的独立核验

12:07:16 的 `07-q-edited-before-save.json` 记录编辑器正文为：

```text
[remote-e2e:RA-20261103-queue-sent-live-122:Q-edited] 只回复 QUEUE_SENT_EDITED，不调用工具。
```

`08-q-sent-network.json` 归档的保存摘录如下，未使用其中派生的 `receiptMatches` 作为独立依据：

| 字段 | 原摘录值 |
| --- | --- |
| HTTP 请求 | `65008.446`，sequence=197，POST `/api/action/editQueued` |
| 原 postData | 准确 M、editId=`a025a3a4-3b37-4c0c-b25d-37b208e3ec0c`、上述编辑后正文、retainedImageSources=[] |
| 原响应 | sequence=209，HTTP 200 |
| 业务回执 A | `15294344-7ec9-4fb8-8fcf-d4d4d9136d18`，state=completed、ok=true、editState=committed、deliveryState=accepted |
| 逻辑 messageId | 原 Q client `8c5b407e-1240-4a8a-80ec-dc3f88ee0244` |
| acceptedTurnId | T52=`01a19f98-3c5d-74b2-80fa-69bf75cb12c4` |
| acceptedMessageId | `01a19f98-3c7c-7d33-b68c-dc5e2f00397e` |
| 结果队列 | queuedMessages=[]、queuedEdits=[] |

本次另外直接遍历 `post-send-native.json` 和 `post-restore-native.json` 的全部持久正式消息，原 client 只对应 T52 中上述一项 `userMessage`，没有第二项同 client 正式消息。loaded 历史中也只有一个 params.clientUserMessageId 对应原 client；本次 Q 没有 accepted steeringUserMessage。这些投影分别核对，没有把同一消息的 params、loaded item 和持久 item 重复计成多次接受。

完整输入比较不是只搜回复标记：原 Q 的 input 仅有一个 text 元素，`text_elements=[]`，正文含尾换行。本次复制这个完整数组，仅替换该 text 正文为保存新稿并保留尾换行；所得数组与 **T52 的 loaded params.input 及持久 userMessage.content 均逐字段相同**。type、数组顺序、text_elements、尾换行均相同，无额外元素。本次没有图片、Skill 或隐藏引用，不能将这个纯文本结果扩为复杂输入保留。

T52 持久状态 completed，12:08:36 开始、12:08:43 完成，final_answer 恰为 **`QUEUE_SENT_EDITED`**。12:08:56 原生快照已经 M idle、requests=[]、queue=[]。直接检查 `09-turn52-complete.png`，可见准确编辑后用户正文、对应回复及已恢复的普通底稿；没有依赖 `targetUserMessageCount/targetAnswerCount` 来判定唯一接受。`08-q-sent-page.png` 仍显示编辑器，因此它只作提交附近画面，不作为保存后 UI 退出编辑的证据。

## 4. 原历史、控制会话、普通稿和模式恢复

10 份原生快照的 M 前 50 个持久 turn 对象均逐字段等于 `baseline-native.json` 原 50 轮，C 的**整个 control 对象**也分别与基线相同，非仅“轮数相同”。M 最终为 52 轮，只新增原问答 T51 和保存接受 T52；原 Q 在保存前的所有采样中均未正式接受。loaded 历史为 51→53，前后多出的既有无 turnId 本地记录不计为本次新增轮次。

普通未发送稿在 `04-draft-and-queue-before-edit.json` 为：

```text
[remote-e2e:RA-20261103-queue-sent-live-122:draft] 普通测试草稿，验收后清除。
```

`05-q-edit-active.json` 另记原稿保护值；最终 `09-turn52-complete.json.composerText` 与原稿逐字相同，editAreaVisible=false、发送按钮恢复“发送消息”，实际截图也显示这份原稿。完整正式历史没有将普通底稿当作额外消息发送。普通稿恢复通过；它不等于本次已经验证编辑期间跨页刷新或存储持久化。

`original-question-restored.json` 于 12:13:21 记录恢复成功；`post-restore-native.json` 于 12:13:32 的 latestCollaborationMode 已回到 default，且 latestThreadSettings、latestCollaborationMode、currentPermissions 三个完整对象分别等于基线。T52 执行时仍继承 Plan 模式，不能把最终 default 恢复倒写成 T52 本就在 default 下运行。

## 5. 网络与清理的证据边界

`08-q-sent-network.json` 只有 request／response 摘录，eventCounts 为 requestWillBeSent=1、responseReceived=1、**loadingFinished=0**；缺原完整事件列表、原始单调时间戳及前后请求窗口。它支持摘录中的一次 editQueued 请求与 HTTP 200 回执，不能据此计算完整 HTTP 耗时、宣称全程未发生网络重试、或断言 begin／回答／保存各自只有一次 HTTP。原 client 唯一正式接受由完整原生历史独立确认，网络采集不足不改写这项已验证事实。

清稿记录存在明确差异：`10-test-draft-cleared.json` 于 12:13:02 记录 draftAfterClear=""、sendButtonEnabled=false、editAreaVisible=false，**但配套 `10-test-draft-cleared.png` 仍清楚显示原普通稿正文**。本报告直接检查了该图，不将空稿 JSON 自动提升为截图佐证。可以记录“JSON 支持已清空”，视觉清理闭环未证实；不据此反推草稿恢复失败。

主代理转述执行者工具已关闭自建 tab `1283922948`。B 没有详细关页记录，本次未再操作／查询浏览器，因此该项只归属于工具声明，不能报告为独立关页确认。原生 119 恢复及测试服务停止由主代理另行完成，本次不以 default 模式恢复、清稿或关页替代运行时恢复。

主代理随后尝试独立重开页面核对清稿，但45430在复开前已按先前指令停止；新页未挂载会话输入框，见 `11-reopen-after-service-stop.json`、`11-reopen-boundary.json`。该观察不能作为“草稿为空”的新证据，清稿JSON与截图的差异继续保留，也未据此修改产品。这个新观察页`1283922971`已关闭。

原生119恢复和服务停止的后续证据见 `../crossflow-122/runtime/shutdown-summary.json`、`shutdown-process-ports.json`及实际恢复脚本：119文本与原基线摘要一致、仅epoch更新，11条终态编辑保留，52596退出、45430关闭，旧1432/45222保持。临时服务目录清理被自动审批拒绝，未绕过；本场景单文件临时驱动已归档为`native-driver.mjs`并清理，原始业务证据保持。

本次只关闭“真实问答期间 Q 被取出编辑，活动轮结束后空闲保存直接接受并完成”的限定普通文本分支；活动轮内保存回队列、引导、自然出队竞争、丢回包／unknown、复杂附件输入、Android 真机及正式安装版继续按各自原件判定，没有新增全排列验收要求。

## 6. 关键原件摘要

下表由本次直接重算；全部 37 份来源文件摘要见 verification.json。复核未读取任何派生 verdict 作为业务事实的唯一依据。

| 原件 | SHA-256 |
| --- | --- |
| `00-page-baseline.json` | `88b39d662b82dbfcdec45090bac00e77c746ba0a83ca2cffc1497d4f1df3a069` |
| `baseline-native.json` | `51ea09269395c0ed64db1471fcc96188e3964e3412a8769cfb09f795535a7d92` |
| `q-enqueued-native.json` | `ad9aaff099e6a7ce7f74a37beeaffaeec47c5329b72da668113ae065dcf719f0` |
| `q-edit-active-native.json` | `3efad034e2f3c47c25612b541cf8f2034e8ad6969b1c75601bccb275b08388c9` |
| `post-answer-native.json` | `0d25992f8a101574cbf5f71c64fb87fe0b39e67c09ace9461dc0d9f150dee5d3` |
| `08-q-sent-network.json` | `eaa3ff43d5efe317a37f6175887ea322bd431bed62df515a44bce7c56c9c535e` |
| `post-send-native.json` | `246ba224adecef586b86ce033b6792758820019e2cc4eff04467eefa4962f4f1` |
| `post-restore-native.json` | `335a56d037df05a08b9f85e2603df403c814ca00a201807766bc32a16fda94ee` |
| `09-turn52-complete.png` | `183c2562e14a82b4c4ac96b70140190ad2ad3afe6793fa95030cfd5e59b55e9a` |
| `10-test-draft-cleared.json` | `a23bf3d5eaf10705d4eb3f92c04f5962257e17ffa5f9f2b711383ca8539edfcd` |
| `10-test-draft-cleared.png` | `fe1a701b9cc13d0d8062a70c240b029569a71a9b47c44b61dac5557da9f44e9c` |
