# 远程模型与思考深度真实请求验收（2026-11-02）

R1-04 的正常发送、两会话隔离、返回原会话后继承设置，以及未发送选择不改变原生状态，本轮通过实际请求核验。**R1-04 总项仍为部分覆盖**：本轮没有实测不支持等级的拒绝，也没有实测设置或发送失败后的已确认设置与草稿保留。

本报告独立读取既有 HTTP、原生完整快照、持久化完整历史和代理请求原件，不再次操作浏览器、调用业务 RPC、发送消息或写入设置。设置恢复已由执行方完成一次，本审计没有重复执行恢复。

## 对象、版本与证据目录

- M：`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`。
- C：`01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。
- 本次原生适配器版本：`119`；fixture ID：`61b3c80f-2248-4a29-b908-c5e853e4321b`。
- 原生源码 SHA-256：`d8c635875c50622194fdfebcede393afb540c9263bc44c6a3809231ceca00944`；插入观测代码后的运行源码 SHA-256：`312103909fc1c62a6b783268566eccb856aaa7b9352ba25c8d1024b0d00277ed`，二者不得混同。
- 原始验收目录：[models-final](../../dist/validation/remote-acceptance-20261102/models-final/)。
- 本文所有带 `Z` 的时间均为 UTC，换算北京时间需加 8 小时。

主要原件为 `before-resume.json`、`baseline.json`、`selected-m1.json`、`after-m1.json`、`after-c2.json`、`after-m3.json`、`restored.json`，以及每个 case 的 `*-action.json`、`*-http.json`、`*-response.json`、`*-proxy.json`。UI 原件包括 `m1-before-ui.txt`、`c2-before-ui.txt`、`m3-selection-ui.txt` 和三个 `*-completed-ui.txt`；UI 显示只作辅助，不替代 wire 和原生轮次。

## 三次实际请求

| case | 操作时间 | 完整标记 | HTTP 显式参数 | 原生轮次与实际 wire |
| --- | --- | --- | --- | --- |
| M1 | `2026-11-02T13:20:24.856Z` | `[remote-e2e:R1-04:model-M1]` | `model=gpt-6.1-terra`、`reasoningEffort=high` | `gpt-6.1-terra / high` |
| C2 | `2026-11-02T13:26:51.567Z` | `[remote-e2e:R1-04:model-C2]` | `model=gpt-6.1-sol`、`reasoningEffort=medium` | `gpt-6.1-sol / medium` |
| M3 | `2026-11-02T13:28:44.549Z` | `[remote-e2e:R1-04:model-M3]` | 只有 `prompt/threadId/clientUserMessageId`，没有 `model/reasoningEffort` | 仍为 `gpt-6.1-terra / high` |

三个 `*-http.json` 各记录一次 `POST /api/action/send`，HTTP 响应状态均为 `200`。对应 `*-response.json` 的请求回执均为 `state=completed`、`result.ok=true`、`status=sent`、`kind=start`、`deliveryState=accepted`。

| case | HTTP 幂等请求 ID | clientUserMessageId | 原生 turnId |
| --- | --- | --- | --- |
| M1 | `46e38094-c01a-4d83-b761-7675d644104a` | `5efced83-dab4-4bee-abfd-24696ae5a569` | `01a19c6b-1832-7f42-81d1-a6dda87f244a` |
| C2 | `5e4199b3-1d93-44d6-8569-a0821ebc7ca5` | `35e46910-8cea-4e54-a6f7-44201bd68047` | `01a19c71-004a-7a63-98d2-7e3a39013e16` |
| M3 | `78068436-ed18-425c-9e55-90a1d7c0ab0b` | `37db2b76-59c6-4514-bcdc-f5f0e34d3c9e` | `01a19c72-b5d3-7d02-a869-73cd292a2a22` |

原生三个轮次均为 `completed`。有效模型和等级位于各轮 `params.collaborationMode.settings.model`、`params.collaborationMode.settings.reasoning_effort`，并与 `requestedReasoningEffort` 一致；`params.model/effort` 本轮为 `null`，不能据此误判未应用。三个实际 wire 均为 `type=response.create`，`model` 和 `reasoning.effort` 与上表一致。

每条逻辑 client ID 在最终原生已加载历史与全量持久化历史中，均只出现一次 `userMessage`。其正文与对应 HTTP prompt 去除末尾空白后相等，wire 最后一条 user 输入也相等；本报告不复制原始 prompt。

### 协议与代理原件关联

三个请求均记录为 `responseProtocol=responses`、`transport=ws`、`relayName=CPA`、`outcome=succeeded`、`state=completed`，且 `upstreamAttempts` 各只有一次。`model/upstreamRequestModel/upstreamResponseModel` 均与该 case 预期一致，`reasoningSource=reasoning.effort`；实际 endpoint 为 `wss://cpahd.faibak.com/v1/responses`。

| case | 代理请求 ID | 代理请求时间（Unix 毫秒） | requestBody SHA-256 |
| --- | --- | --- | --- |
| M1 | `local-ab8634e2-958d-4d82-8282-7508d83c7897` | `1793625626798` | `1ea78dcc66d115f0363c775b8039af28ee10da2c36ce58294fcb79888f4fd126` |
| C2 | `local-7b0a21b9-30dc-4aa3-985f-93de6a16b3a3` | `1793626016023` | `f6f0658d5177ca999f11c544380828e12e812c3cd7e414e2a4e5f645bb2ff85e` |
| M3 | `local-5192f4c5-4111-4c8c-b35e-fc3c12582784` | `1793626125818` | `e36fd21ad9c3882be280c8686b6908ff6c6bdb6b477e7f289f265d7d362bf435` |

读取办法为：先按对应 action 时间窗和 `threadId` 筛索引，按 `id` 保留最后更新，再核对原始详情中的完整 marker。三个 `*-proxy.json` 均只有一个匹配且完成的候选；C2 的时间窗原有两个索引候选，按完整 marker 后才得到唯一匹配，并非选择第一条记录。

本审计重新读取三个 `detailPath` 指向的原始文件，计算 `requestBody` UTF-8 字节的 SHA-256，均与上表相同；三个 wire body 均可完整解析。原始详情路径及字段投影保留在相应 `*-proxy.json` 中，本文不复制鉴权头或完整请求体。

标记可能随历史再次发送：M3 的 wire body 同时含 M1 和 M3 标记各一次。因此不能把“请求体含 M1”算作 M1 又发送了一次。本次逐一确认当前标记只命中一条 user 输入，且该输入是 wire 的最后一条 user 输入，再通过当前 client ID、时间和 turnId 对照原生及持久化历史。

C2 的代理摘要 `turnId=null`，保持原样，不回填。独立解析原始 wire 的 `client_metadata["x-codex-turn-metadata"]` 后，其 `turn_id` 为 `01a19c71-004a-7a63-98d2-7e3a39013e16`，与 HTTP 回执及原生唯一用户输入所属轮次一致；M1、M3 的摘要及原始 wire turnId 本身就与回执一致。

三个发送回执都带 `queueSyncPending=true`，只说明该回执时点未确认队列快照。验收依据是其后已完成原生轮次、全量持久历史与成功 wire 记录，不能仅凭此回执字段推定失败或允许重发。

## 未发送选择、隔离与继承

`baseline.json` 为 `2026-11-02T13:15:18.293Z`，`selected-m1.json` 为 `13:19:53.642Z`。后者 UI 已选择 `gpt-6.1-terra / high`，但尚未发送。对 M、C 分别逐字段比较，两份快照的 `metadata/history/persisted/queue/role/hostId` 全部相等，包括完整 `latestThreadSettings`。因此本次仅选择没有修改原生设置、历史或持久化内容。

M1 完成后，C 的整个线程证据对象与 baseline 相等；C2 完成后，M 的整个线程证据对象与 after-m1 相等。M 此时仍保存 `terra/high`，C 保存 `sol/medium`。回到 M 的 UI 显示 terra/高；M3 的 HTTP 不再显式传入设置，原生实际轮次和 wire 仍使用 terra/high，证明本次继承了 M 的已确认设置，没有带入 C2 的 sol/medium。

`latestReasoningEffort` 不是本次单独判定依据：M1、C2 后，该便利字段可为 `null`，但完整 `latestThreadSettings.effort`、实际轮次的协作设置、`requestedReasoningEffort` 和 wire 等级一致。

## 持久历史与 C 的真实恢复

| 时点 | M 持久化轮数 | C 持久化轮数 | C 原生已加载历史及所有权 |
| --- | --- | --- | --- |
| baseline / selected-m1 | 21 | 7 | `0` 轮、`isComplete=false`，`needs_resume/notLoaded`，role 为 `null` |
| after-m1 | 22 | 7 | 仍为上述未加载状态 |
| after-c2 | 22 | 8 | `6` 轮、`isComplete=false`，`resumed/idle`、role=`owner` |
| after-m3 / restored | 23 | 8 | 仍为 `6/8` 部分加载，`resumed/idle`、role=`owner` |

以 baseline 的全量 `persisted.thread.turns` 为基线，按原 turn ID 逐字段比较 after-m1、after-c2、after-m3 和 restored：M 的旧 21 轮、C 的旧 7 轮均存在、内容完全一致、顺序不变。最终只增加 M1、M3 两轮和 C2 一轮，三条 client ID 各唯一。

C 的恢复不是只读到本地索引就算完成：发送前原生无 owner 且为 notLoaded；C2 后已取得真实 owner，原生和持久化历史都存在匹配的已完成 C2 轮次，且有成功上游请求。C 的原生历史仍未加载全部 8 轮，本文不宣称它已全量载入；旧历史保留由完整持久化记录证明。

## 一次恢复后的设置与权限边界

[restore-intent.json](../../dist/validation/remote-acceptance-20261102/models-final/restore-intent.json) 记录 `2026-11-02T13:30:05.378Z` 的一次恢复意图，只恢复两会话 baseline 的 `model/effort/collaborationMode`；这三个目标字段均与 baseline 相等。`restored.json` 的执行结果为 `[true,true]`。恢复后逐字段比较完整 `latestThreadSettings`，两会话均与 baseline 完全相等：M 为 `gpt-6.1-sol/max`，C 为 `gpt-6.1-terra/max`。

权限按 [restoration-verification.json](../../dist/validation/remote-acceptance-20261102/models-final/restoration-verification.json) 与原始快照独立复核：

- M 的 `currentPermissions` 原始字段完全相等。
- C 的唯一权限字段差异为 `runtimeWorkspaceRoots`：新增 `C:\Users\junes\.codex\visualizations\2026\11\01\01a1996d-0b7a-71d0-b9ad-9c521d224ad9`，原列表中已有同目录的 `C:\Users\junes\.codex/visualizations/2026/11/01/01a1996d-0b7a-71d0-b9ad-9c521d224ad9` 拼写。
- 统一 Windows 分隔符、大小写并按集合去重后，C 的根路径集合与 baseline 相等；权限的其余字段完全相等。新增拼写在 `after-c2.json` 已经存在，`after-c2.currentPermissions` 与 restored 原始字段相等，故不是恢复调用新增。
- C 的便利字段 `latestReasoningEffort` 从 baseline 的 `null` 变为 restored 的 `max`；其 baseline 完整设置本来就是 `max`。C 的加载状态也保留真实恢复后的 `resumed/owner`，没有恢复为未加载。

因此恢复结论是“完整线程设置恢复相等，权限路径集合等价”；**不是“全部原生 metadata 或全部权限原始字段完全相等”**。本审计未为消除这些原始差异再写设置，也未重复执行恢复。

## 保留的验收缺口

1. 未实测不支持或被限制的思考等级明确拒绝。
2. 未制造原生设置更新拒绝、网络失败或供应商错误，尚无本轮失败保留已确认设置及草稿的真实证据。
3. 本轮实际供应商协议只有 Responses over WebSocket；未覆盖 Chat Completions、Anthropic 或跨协议转换。
4. 未覆盖排队消息、编辑消息、临时侧边会话和并发多设备下的模型设置语义。

上述缺口保持未完成，不能用三次正常发送成功或既有隔离测试替代。
