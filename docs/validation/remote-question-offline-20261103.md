# 异步问答离线、身份隔离与跳过局部验收（2026-11-03）

**已测子场景通过，整体部分确认。** 第一组两题在提问轮完成后继续待答，页面内切换及离线恢复保留选择；硬刷新后的未提交选择按既有范围重置，重新选择后仅有一份准确正式答案。第二组同名问题使用不同实体和消息身份，跳过不发送答案、不新增回答轮，刷新后未复活。第一段 HTTP 账本存在淘汰缺口，不能宣称全程 HTTP 唯一或全程零自动回答。

本次只读复核原件并新增本文，没有操作现场、修改产品或主表。原件根目录为 `dist/validation/remote-acceptance-20261103/`：业务证据位于 `question-offline/`，运行与恢复证据位于 `question-runtime/`。以下时间为北京时间；原 JSON 中的 UTC 日期为 2026-11-02，换算后本轮实际发生于 **2026-11-03 凌晨**。

## 第一组：离线禁用、选择范围与正式回答

测试页为 `http://127.0.0.1:45430/`，M 为 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，C 为 `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。基线 M 完整 31 轮，C 正式持久历史 8 轮。

第一组提问位于第 32 轮 `01a19d55-43f0-7820-a149-9c146f7a2a14`，源消息为 `call_pXCsrV1JI4vINtointERlQCF`，原生面板实体为 `tail:0:local:31cc28c5-c7ae-46ae-bbeb-e7f01569bbdf`。两道题的精确 ID 为 `["request_user_input_async","call_pXCsrV1JI4vINtointERlQCF",0]` 和同结构索引 `1`，分别询问颜色、形状。

`pending-completed.json` 记录该轮已 `completed`，原生面板仍持有同一实体下的两题；两题均为 `lastSubmission=null / isSubmitting=false / isSkipped=false`。这证明提问轮结束不等于题目已经回答。页面操作中的 M→C→M 返回结果见 `before-offline-ui.json`、DOM 和 PNG：选择为绿色/方形，普通消息草稿为 `QUESTION_OFFLINE_DRAFT_保留`；该返回状态不补齐已淘汰的早期切换网络事件。

01:39:15.516 的 `offline-ui.json` 实测 `navigator.onLine=false`，页面仍可见，两题四个 radio、跳过和提交按钮全部禁用，提示连接恢复后才能回答，绿色/方形与普通草稿保持。随后 `offline.json` 中原生面板、两题身份及上述未提交、未跳过状态均保持，历史仍为 32 轮。01:41:15.137 的联网恢复快照中，控件恢复可用，选择仍为绿色/方形。

硬刷新后的 `reloaded-ui.json` 仍显示这两道原题，选择回到默认蓝色/圆形，普通消息草稿仍在。**这符合现有 `docs/remote-access.md:842、846`：未提交问答答案仅在同一页面的重绘、会话切换中保留，不要求整页刷新后持久恢复，也不将答案写入持久存储。** 因此不将该重置判为回归，也不新增答案持久化要求。`reloaded.png` 是刷新中的过渡画面，稳定题卡及默认选择以随后保存的 UI JSON 为准。

用户重新选择绿色/方形后，`first-network.json` 保留的 POST `78928.233` 精确绑定第一组原 request、消息及两题 ID，`method=remote/requestUserInputAsync`，操作回执 A 为 `c93bab5a-a08b-465d-b3cf-1c788aa3f1d4`；捕获到 HTTP 200 和 `loadingFinished`。服务端原 A 为 `completed/ok=true`，见 `first-server-receipts.json` 及最终回执集。

`after-answer.json` 记录新回答轮 `01a19d5b-3fd9-7152-8ace-0db4153f2194` 已完成，M 为 33 轮：唯一正式回复消息 `01a19d5b-4010-7182-995f-00384e8c0b04`，client 为 `0d4bc36d-8d38-4e16-ac21-5677c39fd7e9`。其 `<send_user_message_question_reply>` 精确绑定第一组两题 ID，答案为绿色/方形，最终回复为 `QUESTION_121_ANSWER:绿色/方形`。原生面板为空；回答后及再刷新后的 UI 均无题卡，普通草稿保持。已完成提问轮的回答由原生开启新轮，符合原有约定。

## 第二组：同名不同实体，跳过不发送草稿或答案

第二组位于第 34 轮 `01a19d61-67c6-7ae1-9277-aec9bbbcf904`，源消息为 `call_ECeeMufiL1Xyvr0d9qQWRMkO`，实体为 `tail:0:local:e2347045-0b7e-4105-abb7-8ca85855734c`。题目标题和选项与第一组相同，但轮次、源消息、实体及两题复合 ID 均不同，不能仅按标题复用旧答案。

`second-pending.json` 的新两题均为 `lastSubmission=null / isSkipped=false / isSubmitting=false`；`second-initial-ui.json` 默认选择为蓝色/圆形，没有继承第一组的绿色/方形。测试随后在问答的其他回答输入中填写 `QUESTION_SKIP_UNSENT_不要发送` 并选择方形，普通消息框另保留 `QUESTION_OFFLINE_DRAFT_保留`。两份输入分别由 `before-skip-question-draft.json` 与 `before-skip-ui.json` 保存。

唯一跳过请求为 POST `78928.423`，回执 B 为 `b986508a-e1b4-41b5-98a0-db5b6ebdb9a7`，准确绑定第二组原 request；正文仅有 `response:{"skip":true}`，**没有 answers、选中的方形或其他回答草稿**。`second-responses.json` 保留真实 HTTP 200 正文，回执为 `completed/ok=true/skipped=true`。

`after-skip.json` 和 `final.json` 中，第二组 `knownQuestionStates` 的两题均为 `isSkipped=true / lastSubmission=null / isSubmitting=false`，原生面板为空。完整 34 轮在跳过前、跳过后和刷新后逐字段一致，没有新增回答轮；最终 UI、DOM 和 PNG 无题卡、pending 或队列，普通消息草稿保持。

## 历史、请求次数与证据边界

按深度 100 解析并序列化完整轮次对象，已加载历史与正式持久历史分别比较：

- M 基线前 31 轮保持；第 32 轮提问完成至离线、回答及最终状态的前 32 轮保持；第 33 轮回答完成至第二组及最终状态的前 33 轮保持。
- 第二组提问完成后，完整 34 轮到跳过及刷新后均不变。最终全历史只有一条第一组精确题目 ID 的正式回答，第二组对应答案为零；两种未发送草稿标识均未进入正式用户消息或引导消息。
- C 在全部七份原生快照中的正式持久 8 轮均保持。其已加载视图为 `notLoaded / isComplete=false`，不能将零轮视图当作持久历史。
- M、C 的 `latestThreadSettings` 和 `currentPermissions` 在全部快照中逐字段保持；未为问答更改模型、协作模式或原生权限。

第一段 `first-network.json` 为 483 条保留事件，`truncated=true`；首次提问 send 的早期账本已淘汰，刷新后原响应 body 未保留，不能以服务端回执补造原 HTTP 正文。`first-server-receipts.json` 只保存当时可取得的 A，不是完整实例账本。最终 `server-receipts-final.json` 才保存本实例 10 项回执，其中两次提问 send、第一组 A 和第二组跳过 B 均可准确关联；回执项数不等于 HTTP 尝试次数。

因此第一组结论限定为：已保留窗口中的准确 A 请求、服务端完成回执、原生全历史的一份正式答案及面板收敛相互吻合。**不宣称第一组全程只有一次 HTTP 尝试，也不凭截断日志宣称全程零自动回答。**

第二组各分段网络快照均 `truncated=false`，属于从游标 1616 逐步扩展的累计窗口，不能把同一事件在多个文件中重复计数。最大窗口至 2298，共 319 条事件：一次准备提问 send、一次准确 B 跳过及一次 markRead；跳过前无 answer，跳过后至刷新窗口没有追加 answer 或 send。该窗口结合完全不变的 34 轮及逐题跳过状态，证明本次跳过未将默认选项、其他回答或普通草稿发给模型。

## 运行身份、恢复与未测范围

问答运行 fixture 为 `ff7e588d-440f-41da-a954-f22b13147d18`，业务源码 121、兼容标记 106，epoch 为 `10be9291-1045-486f-8df3-d0d161eea8be`。`installed.json /after` 的实际运行摘要与结束时重新读取的引擎全文、待执行文本一致，均为 `564a31f5a6b5fb87310efe9a06bbbcfa90d1701c425823bfae38f05d7f7dfec1`。实际浏览器 app 摘要为 `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`，与服务源码快照一致。

结束后的实际恢复脚本与开始前 119/106 文本摘要同为 `5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`。独立比较开始基线与 `restored.json /after`：原 fixture 仅 `runtimeEpoch` 改为重新执行后的 `448b82b2-45f0-451a-b1e3-f6b8c0d32f79`；owned 名单恢复为空，写状态表及既有编辑终态记录保持。不是只根据 `verified=true` 判定恢复。

`server-finished.json` 记录测试服务 PID 69428 正常按停止文件退出；01:56:19.372 的 `environment-restored.json` 确认其已不存在，监听列表无 45430、原 45222 仍由 PID 13568 监听。诊断文件未创建，快照明确 `diagnosticsFilePresent=false`；如实记为未生成，既不当作错误，也不包装成经过完整日志扫描的“零错误”。旧 1432 页最终仍保存 `STOP_LIVE_2_UNSENT_DRAFT_保留`，pending/queue 为空，与新页草稿分别核对。

本报告只覆盖上述两组具体实测；未覆盖后台或系统回收、Android 真机、迟到旧事件、换授权、并发回答、长期 pending/unknown 或全部失败回执分支，也不代表正式安装版或整体问答验收完成。
