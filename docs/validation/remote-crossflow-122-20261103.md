# 122 同版本连续业务链：独立原件复核

日期：2026-11-03，Asia/Shanghai。以下时间均为北京时间。

**结论：本次浏览器连续正常子链通过；R3-20 整体验收仍为部分完成。** 同一运行版本、同一主会话依次完成真实同步问答、目标标签本地编辑与删除、Q1 队列保存后自然消费，以及 Q2 队列保存后引导进入原活动轮。普通文本草稿在上述阶段及刷新后保持。Android 真机恢复仍待用户验收，本次也未覆盖整链失败／unknown 恢复，不能据此把 R3-20 或 73 项基线整体改为通过。

原件目录为 [crossflow-122](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/crossflow-122/)，下文简称 B。复核直接读取冻结的 UI、DOM、截图、HTTP 分段、原生持久历史和实际脚本全文，并重算摘要；本次只新增本报告，不操作浏览器、服务或业务对象，不修改五份主验收文档，不重跑测试、编译或提交代码。

## 1. 运行身份与适用边界

| 对象 | 原件核对结果 |
| --- | --- |
| runId | `RA-20261103-crossflow-122`；各阶段原生快照一致 |
| 主会话 M | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` |
| 控制会话 C | `01a1996d-0b7a-71d0-b9ad-9c521d224ad9` |
| 独立测试服务 | `http://127.0.0.1:45430`，服务 PID **52596**，见 `runtime/service-ready.json` 和 `runtime/ready-summary.json`；`80200.*` 是 HTTP 网络请求 ID，不能作为服务 PID |
| 原生业务／兼容标记 | sourceVersion **122**／compatibilityVersion **106**；实际接口 `version=106`，不能据此把业务源码判为旧 106 |
| fixture | `ebe5356e-2fc3-4066-bd8e-bfc9f2c274ed` |
| runtime epoch | `22ec6684-09d5-45a3-9c26-b04573f6fa28`；它是 epoch，不是 fixture ID |
| 实际网页脚本 | `app-runtime-before.js`、`app-runtime-after.js`、`runtime/app-under-test.js` 逐字一致，SHA-256 均为 `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`，也匹配复核时 `assets/remote-access/app.js` |
| 实际原生脚本 | `runtime/native-engine-before.js` 与 `runtime/native-engine-after.js` 逐字一致，SHA-256 为 `1b1b49f1a04ccfbd99209cd08a5a5bff858f60badf43cd180a7a0a340faeda7a` |

`browser-identity.json` 与 `browser-identity-after.json` 对同一测试页 `1283922928` 记录相同网页脚本摘要，均不是 Live Edit；前者明确通过正常关闭重开更新页面，`bypassCache=false`。原生起止快照的 targetId、scriptId、实际源码摘要、methodSha256、epoch、fixture、ownedThreads 分别相同，支持上述连续执行确实使用同一组实际脚本。

这仍是独立验收部署：两处版本标记从122映射至106、移除本次安装的引导早退、沿用已登记测试会话的自动命名保护。`runtime/runtime-plan.json` 的 `transforms` 文字沿用了旧模板中的“121”，该说明字段过时；原件不回写，实际以 `native-original-122.js`、执行变体和SHA-256为准。原始122源码摘要为 `b4e92bc7c760c59fdb50d8940e0efbf96c0c3a5710dfbfeec0cb1d2e2c5eb04e`，匹配复核时 `assets/remote-access/native.js`；它与实际运行适配文本的摘要不同，不能混称为未适配安装版原件，也不能据此证明正式安装包已经运行该版本。

## 2. 同步问答 → 目标标签本地编辑

同步问答起始参数及回执分别在 `question-attempt.json`、`question-receipt.json`：原生准备了 Plan 模式轮次 `01a19f63-2c48-77a0-83f3-16c40b12515a`，客户端身份为 `1410b244-aa01-49dd-bf27-cb03b9313ef6`。网页 HTTP 账本没有这次起始发送，不能把原生准备等同于网页普通发送已验收。

`question-pending-native.json` 中存在真实 `item/tool/requestUserInput` 请求 `12`，绑定上述 M、turnId 和题目 `crossflow_color`，`isBlocking=true`；原生状态为 `waitingOnUserInput`。`question-pending-ui.json`、DOM 和截图显示同一道题、蓝色／绿色选项及提交按钮，底部仍为 `CROSSFLOW_122_UNSENT_DRAFT_保留`。本次直接检查了 `question-pending.jpg`，不是只依据测试判定字段认定题卡存在。

11:12:55 前后的网页提交为原网络请求 `80200.131`，POST `/api/action/answer`，原始正文携带请求 `12` 及 `crossflow_color.answers=["绿色"]`；HTTP 200，业务回执 `6140bd47-e271-43d0-9d86-3ad5fb29a9e4` 为 completed，requests 清空。`question-answered-native.json` 的 loaded 原轮包含 `userInputResponse`，`completed=true`、答案为绿色；持久历史中同轮 completed，最终回复为 **`CROSSFLOW_ANSWER:绿色`**。问答没有另启替代轮。`question-restored.json` 记录恢复成功，最终原生 `latestThreadSettings`、`latestCollaborationMode` 和 `currentPermissions` 分别与基线相同。

随后同一页进行目标标签操作：

| 时间／原件 | 直接观察 |
| --- | --- |
| 11:14:35.119，`goal-selected-ui.json` | 出现 `inline-goal` 标签，正文仍为原普通草稿；发送按钮切换为“创建目标” |
| 11:14:35.842，`goal-edited-ui.json` | 标签后正文改为 `标签后编辑：CROSSFLOW_122_UNSENT_DRAFT_保留` |
| 11:14:49.240，`goal-removed-ui.json` | 仅标签消失；`draft` 和 `promptHtml` 均为上述编辑后正文，按钮恢复“发送消息” |

`network-5-goal-local-complete.json` 的归档窗口无 `/api/action/`；整份账本也无目标提交动作。该段证明目标标签的**本地选择、编辑、删除及正文保留**，没有证据支持将它写成服务器目标创建／完成通过。正文继续留在后续保存、刷新和最终页面，未混入正式业务消息。

## 3. Q1 保存并自然消费；Q2 保存并引导入原活动轮

两项队列的稳定客户端身份、原生队列 ID、编辑事务和最终接收分别核对，不能只凭队列行消失或提示文案判成功。

| 对象 | Q1：自然消费 | Q2：原轮引导 |
| --- | --- | --- |
| clientUserMessageId | `8e2547ba-0ea5-4a65-96ab-978568bb3cc1` | `237b2f06-b6b0-4eac-8f60-c5f0fadadc11` |
| 入队时活动轮 | `01a19f67-7142-7450-8abb-17b79832a7be` | `01a19f6b-a24f-7a11-a951-e92db159ea40` |
| 原队列 ID | `01a19f68-9116-7d03-80cd-f357d61274e5` | `01a19f6c-1152-7561-a833-9fbc90eb2ef9` |
| editId | `024ec67b-ddf0-4de4-87a8-900ad4cf58c9` | `0b122617-9dc0-42ed-b816-14e686fda4e8` |
| 保存后队列 ID | `01a19f69-f6f2-76a1-aabb-1b6caf99e122` | `01a19f6c-f102-7573-a004-fb98b7bc214e` |
| 原文 → 保存正文标记 | `CROSSFLOW_QUEUE_ORIGINAL` → `CROSSFLOW_QUEUE_EDITED` | `CROSSFLOW_STEER_ORIGINAL` → `CROSSFLOW_STEER_EDITED` |
| 最终接收 | 新轮 `01a19f6a-6f48-72e3-9f76-308d32e320ff` | 原活动轮 `01a19f6b-a24f-7a11-a951-e92db159ea40` 内 |

Q1 的 `queued-before-edit-native.json` 与 `queue-saved-native.json` 均显示第一活动轮 inProgress。保存前后队列正文与服务端队列 ID 改变，客户端身份保持，原始 begin／edit 回执分别为 editing／committed。`first-queue-completed-native.json` 显示活动轮三次真实 55 秒等待均自然完成、exitCode=0，11:18:35 回复 `CROSSFLOW_BASE_DONE`；随后 Q1 于同秒启动独立新轮，11:18:42 完成，持久用户消息 clientId 为原 Q1，正文为编辑后文本，最终回复 **`CROSSFLOW_QUEUE_EDITED`**。在 Q1 入队至该结果的归档 HTTP 中没有 `steerQueued`，也无同一客户端身份的第二次 send，支持本项是单条保存后的自然消费。

Q2 的 `steer-queued-before-edit-native.json` 与 `steer-saved-native.json` 均显示第二活动轮仍 inProgress。11:21:59 的 `steer-refreshed-ready-ui.json` 保留保存后新队列 ID、编辑后文本及底部普通草稿。之后 `steerQueued` 原始回执明确返回原活动 turnId。`steer-accepted-native.json` 的 loaded 原轮有真实 **`steeringUserMessage`**：`status=accepted`、`targetTurnId` 指向该轮、客户端身份为原 Q2，`restoreMessage.id` 为保存后队列 ID，`serverUserMessageId=01a19f6d-9561-7b12-9455-b362bf5e7546`。持久历史中该服务端用户消息也位于原轮内，正文为编辑后文本，最终回复 **`CROSSFLOW_STEER_EDITED`**；没有新增第五个持久轮次来替代引导。

第二活动轮提示前部要求八次等待，结尾仍残留“三条都完成”的文字。原生实际声明八次，归档中只执行两次 45 秒等待、均自然完成且 exitCode=0，随后接受 Q2 并完成；没有第三次等待。这里依据原轮活动状态、真实命令和 accepted 记录认定引导成立，**不据提示文字声称八次等待已执行完毕**。

两次保存后的 UI 一度显示“消息已发送，队列记录尚未同步；请勿重复提交。”，所以本报告以原生队列与编辑事务、最终接收为准。Q1 刷新就绪时已自然消费且队列为空；Q2 刷新就绪时仍显示准确的新队列对象。两次刷新后普通文本草稿均为 `标签后编辑：CROSSFLOW_122_UNSENT_DRAFT_保留`，后续 `final-ui.json` 仍一致，未残留编辑器内容。

## 4. 原 HTTP 账本与采集缺口

`network-ledger.json` 的 1173 个事件，逐项等于 `network-1-start.json` 至 `network-19-cleanup-final.json` 分段按原 sequence 合并的结果；各段均 `truncated=false/hasMore=false`。归档为 request／response／loadingFinished 三类事件的筛选结果，保留的序号范围为 192—2763，**不是该范围内所有 CDP 事件的连续副本**，也不证明 WebSocket 逐帧或原生事件流完整。

归档共有 15 次 `/api/action/` POST：answer 1、send 4、beginQueuedEdit 2、editQueued 2、steerQueued 1、markRead 5；15 个网络请求 ID 各不相同，均保留 postData。四次 send 分别是两个活动轮及 Q1／Q2 入队，问答起始原生准备另计；不能将 markRead 或 `/api/read/*` 的 POST 算成重复业务发送。已脱敏的认证头与 URL 凭证不影响保留业务身份，但不能用归档重放授权。

| 业务动作 | 原网络 requestId | 原 request／response sequence | 核对结果 |
| --- | --- | --- | --- |
| 同步回答 | `80200.131` | 449／457 | HTTP 200，原回执 completed |
| 第一活动轮发送 | `80200.169` | 715／718 | HTTP 200，kind=start，准确活动 turnId |
| Q1 入队 | `80200.190` | 855／858 | HTTP 200，deliveryState=queued |
| Q1 开始编辑 | `80200.208` | 963／970 | HTTP 200，原 editId，editState=editing |
| Q1 保存 | `80200.213` | 1005／1008 | HTTP 200，原回执 committed，新队列 ID |
| 第二活动轮发送 | `80200.312` | 1532／1535 | HTTP 200，kind=start，准确活动 turnId |
| Q2 入队 | `80200.323` | 1607／1610 | HTTP 200，deliveryState=queued |
| Q2 开始编辑 | `80200.331` | 1660／1668 | HTTP 200，原 editId，editState=editing |
| Q2 保存 | `80200.339` | 1717／1720 | HTTP 200、完整请求正文在；原响应正文读取失败 |
| Q2 引导 | `80200.405` | 2029／2031 | HTTP 200，准确保存后队列 ID 和原活动 turnId |

Q2 保存的原 `Network.getResponseBody` 失败仍原样保留在 `network-ledger.json.bodies["80200.339"].readError`：`No resource with given identifier found`。这属于**采集器未取得原响应正文**；不能将它改写为服务端响应丢失、网页自动恢复成功或原 POST 响应正文完整。

补证是明确单列的诊断读取：原 POST 的 `idempotency-key=517e07a3-9d97-4c9f-a8cf-7b7c6ce99f63`；后续 GET `/api/receipts/517e07a3-9d97-4c9f-a8cf-7b7c6ce99f63` 为网络 `80200.430`，sequence 2197／2200，HTTP 200。其业务回执为 completed、准确 editId、新队列 ID、`editState=committed`，与保存后原生队列及后续真实引导一致。`steer-save-diagnostic-receipt.json` 保留该诊断角色，没有覆盖失败原记录。结论是保存结果已有多源确认，**原 POST 响应正文采集完整性仍有一个缺口**。

从同一原账本按requestId关联请求起点与`loadingFinished`，同步回答HTTP为225.181ms，两次保存分别为251.000ms、336.798ms，引导为1370.328ms；计算结果另存 `http-durations.json`。它们是独立请求的单次完整往返样本，不是同条件A/B或稳定速度承诺。两次保存都发生于活动轮中、返回排队对象，不能据此证明122新调整的“空闲时sent保存”路径已真实验证。

## 5. 轮次、隔离与收尾

本表只统计 `history.thread.turns` 持久历史，不混用 metadata 的空 turns。loaded 历史从 47 增至 51，前后都比持久历史多一条既有无 turnId 的本地记录，不能把它计成本次新增轮。

| 原件／时间 | M 持久轮数 | 新增轮及实际状态 | C 持久轮数 |
| --- | --- | --- | --- |
| `baseline-native.json`，10:59:56 | 46 | idle | 8 |
| `question-answered-native.json`，11:13:35 | 47 | 同步问答轮 completed，11:10:39—11:13:04 | 8 |
| `active-before-queue-native.json`，11:15:52 | 48 | 第一活动轮 inProgress，11:15:19 开始 | 8 |
| `first-queue-completed-native.json`，11:19:32 | 49 | 第一活动轮及 Q1 新轮均 completed | 8 |
| `steer-queued-before-edit-native.json`，11:20:39 | 50 | 第二活动轮 inProgress，11:19:53 开始 | 8 |
| `steer-accepted-native.json`，11:22:22 | 50 | Q2 在第二活动轮内接受，该轮 11:22:07 completed；M idle，requests=0，queue=[] | 8 |

本次独立比较确认，M 基线的 46 个持久 turn 对象逐字段保持为最终历史前缀；C 基线与最终的整个 `control` 历史对象逐字段相同，C 的状态仍为 notLoaded，不冒称它在网页完整加载或实时活动。新增持久轮恰为问答、第一活动轮、Q1 自然消费轮、第二活动轮四个，普通底稿没有被当成第五条消息发送。

`runtime/native-engine-before.json` 与 `runtime/native-engine-after.json` 直接比较确认：`goalSubmissions`、`queuedSendSubmissions`、`asyncQuestionRecoveries` 三表起止均 0；原有 8 项终态编辑逐字段保留，最终增至 10 项。新增两项正是上述两个 editId，均 committed，`beginPending=false/terminalPending=false/finishRequests=0`。这证明两个采样点无未决写状态，不扩写为运行期间写表从未出现活动。

浏览器收尾证据已经补齐：直接检查 `final-main.jpg` 可见真实引导结果及底部编辑后的普通稿；`cleanup-empty-after-reload-ui.json` 于 11:26:19 显示独立 45430 页刷新后 promptHtml、draft、approval、notice 均空、queue=[]。原 1432 页面在 `original-origin-draft.json` 与 `original-origin-final.json` 中均保持 **`STOP_LIVE_2_UNSENT_DRAFT_保留`**；本次清空的是独立来源的自建测试稿，没有据此宣称用户原稿已清空。`browser-cleanup.json` 于 11:26:37 记录两张自建页 `1283922928`、`1283922922` 已关闭、网络采集关闭、`cacheBypass=false`、`desktopUiActions=0`。

初始 `runtime/capture-after-verification.json` 是收尾前采样，`runtimeRestored=false/serviceStopped=false`，不能用它证明恢复。后续主代理已另读 `runtime/native-engine-after-sent.js`、`native-final-119.js`、`runtime-restored.json` 和 `shutdown-process-ports.json`：122实际引擎在后续空闲保存场景结束后仍与本轮起点逐字一致；119恢复文本与原基线SHA-256均为`5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`，fixture只更新runtimeEpoch为`110c83d1-b1bb-4f20-891c-ea9c9f339ca0`。12:20:43的进程记录确认52596及其子进程不存在、45430无监听，旧1432/45222保持。三张写表为空、最终11条终态编辑保持；其中第三条新增编辑属于另报的空闲保存场景，不回填为本正常整链的新增动作。

临时服务目录`temp/remote-crossflow-122-20261103`的删除被自动审批以`blocked by policy`拒绝，258个文件保留，没有重新尝试。归档的`cleanup-result.json`明确区分子代理的拒绝声明和主代理对目录仍存在的核对；本轮独立业务驱动已归档并按准确文件路径清理。临时目录未删不改变服务已停止及原生已恢复的独立证据。

## 6. 覆盖判定与原件摘要

| 范围 | 本次可记状态 |
| --- | --- |
| 同版本浏览器同步问答 → 目标标签本地编辑／删除 → 队列保存／自然消费／原轮引导正常子链 | 通过，限定为本次 M、普通文本与两项独立队列对象 |
| 底部普通草稿在问答、目标标签和队列编辑／刷新间保留 | 通过，限上述实际文本与采样；本次没有 Skill、图片或隐藏引用输入 |
| R1-10 两条未经排序、编辑或引导的 FIFO | 本次未测；一条编辑后自然消费及另一条人工引导不能替代该基线 |
| R1-09 已发送消息编辑／引用上下文保留 | 本次未测；队列编辑不是已发消息编辑 |
| R3-20 整行 | 部分完成；Android 真机部分仍待用户验收，失败／unknown 整链恢复未由本次覆盖 |
| 正式安装版／Android／公开发布 | 本次未验证 |
| 浏览器测试稿与自建页清理 | 原件已确认 |
| 原生恢复119／独立服务停止 | 后续原件已补齐；临时目录清理被策略拒绝，保持原样 |

以下 SHA-256 为本次直接从 B 下原件重算。它们固定的是本报告所读文件内容，不把声明型采样中的结论当作替代原件；运行身份、轮次及编辑终态均已另读对应 before／after 原件确认。

| 关键原件 | SHA-256 |
| --- | --- |
| `app-runtime-before.js`／`app-runtime-after.js` | `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387` |
| `runtime/native-engine-before.js`／`runtime/native-engine-after.js` | `1b1b49f1a04ccfbd99209cd08a5a5bff858f60badf43cd180a7a0a340faeda7a` |
| `baseline-native.json` | `bb9a2fe65549af169be3b77bea766650671084c3374725a7035f46b284342298` |
| `question-pending-native.json` | `2f364def0004942c5347ae526a2093f8a392080748cf603f828667cf77c46971` |
| `question-answered-native.json` | `33251e9a266c2df309f75ae96959637a3a1d430669dca8634376ddf0b9357ab6` |
| `goal-removed-ui.json` | `076a6e78d1eb39ac65871b422199eeafa3974ac525281f4702c3dc6209953870` |
| `queue-saved-native.json` | `bc210e3e0feac9f2e07870ce1b624046c7ce39ae44e7f237a45e801eec67054a` |
| `first-queue-completed-native.json` | `e6c2a26a8ae8d2c8973a220ec2b9b484dea3843ec04e0b309bd8b6e4cd429bf6` |
| `steer-saved-native.json` | `f237e9f33398390c2f242c3d27bd2d964dab1ecf18d9a96812d3763a866d6135` |
| `steer-accepted-native.json` | `2272066fbb72872310e83d5aa1e3cb1c813d698b029c5d1c209b3fc94c4bf393` |
| `steer-save-diagnostic-receipt.json` | `8935436d17dc2faf29ea1bb86e3f4ecc46c40a94c60f75155bfd54d6114963ae` |
| `network-ledger.json` | `ad40631f6754ded8ae2eee03348f3ae0cacac7987beda8688d67902f323449cc` |
| `runtime/capture-after-verification.json` | `7cfd3f511d5478c1898b54e6d2864a57b23b1eb360c912d6513258dec58c7568` |
| `final-main.jpg` | `35daf67af2ebecafcfd5e327d17dcb891584643057f8aa95b30517287c3012f0` |
| `cleanup-empty-after-reload-ui.json` | `2edd838dc40997ee46b23b5e5374e3782198167cc3a7360571cc6c1a0cc77af7` |
| `original-origin-final.json` | `4dcf562e3dec0e1db8b7b86ba775d15d1cc677b7a0ad7f36a085057f018d4f4d` |
| `browser-cleanup.json` | `2e683f58dc063fe0d074b87c3fa40fdf6acb0bfdbd2e59fe3c3d2004e7c32013` |
