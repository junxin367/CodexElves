# 命令审批取消、原队列恢复与删除预览：独立验收复核

日期：2026-11-02，Asia/Shanghai。

**结论：命令审批 `cancel`、取消后原队列保留、用户单次显式引导原消息并完成的限定链路通过。** 原23轮逐字段保持，最终25轮；准确命令项declined、输出/退出码为空，原队列完整input只接受一次，测试线程设置和权限已恢复。

附录的C删除预览取消正常子场景也成立：准确C的风险确认框出现后消失，本次网络窗口零删除请求，正式持久读取中的完整8轮逐字段不变。原生已加载视图只有6轮且不完整，两种来源分别核对，不能混用。初始普通草稿为空，不报告非空保稿。

以上不代表停止按钮、任意原生失败后的队列策略、自然FIFO、活动轮内steer、未知回执或完整三轮通过。本次只读原件并新增本报告，没有修改产品、原件或旧报告，没有操作Git、RPC、浏览器、模型或测试线程，没有提交或派生代理。

## 1. 来源与对象

本次直接读取原始JSON、HTTP事件及UI记录，不采用辅助工具的 `acceptanceVerdict`、`restoration.verified` 或 `native-verification.json` 布尔值代替独立比较。

| 缩写 | 实际路径 | 内容 |
| --- | --- | --- |
| W | `dist/validation/remote-acceptance-20261102/approval-cases-119/command-cancel/` | 网页排队、取消审批、引导的原请求/响应，取消前后及完成UI |
| N | `temp/remote-approval-cases-119-20261102/evidence/command-cancel/` | 原生基线、真实待审批请求、队列、取消、恢复执行和设置恢复原件 |
| D | `dist/validation/remote-acceptance-20261102/delete-preview-final/` | 删除确认框、取消后UI、原网络窗口 |
| M | `dist/validation/remote-acceptance-20261102/models-final/` | 删除预览前后两线程原生快照，包含正式持久历史读取 |

N是本次读取时原生材料的实际位置；本报告没有挪动、归档或删除它。后续若由主代理归档，可按第7节SHA-256确认同一原件。

主链测试会话为 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`。所有原生主链快照记录：

- `fixture.sourceVersion=119`、`adapterVersion=106`、`fixture.compatibilityVersion=106`。
- fixture ID为 `61b3c80f-2248-4a29-b908-c5e853e4321b`，epoch为 `1f26a046-3ad0-4f02-9a9f-85c62627b91a`，前后保持。
- 业务源码SHA-256记录为 `d8c635875c50622194fdfebcede393afb540c9263bc44c6a3809231ceca00944`；兼容变体运行源码摘要记录为 `312103909fc1c62a6b783268566eccb856aaa7b9352ba25c8d1024b0d00277ed`。

这是119纯业务/106兼容fixture的真实子场景，不能写成用户已安装119。本次核对了快照中身份记录的一致性，没有另外连接Debugger重新取得脚本全文。

## 2. 真实请求、命令项与取消决定

原始依据：`N/prepared-request.json`、`N/capture-pending.json`、`N/capture-queued.json`，以及 `W/queue-pending-ui.txt`。

| 身份 | 原件实际值 |
| --- | --- |
| 会话 | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` |
| 审批轮 | `01a19c75-8741-7e32-b513-fd2444ab21f0` |
| 原生request ID | 数值 `4` |
| method | `item/commandExecution/requestApproval` |
| item ID | `call_a08cnKjEz68NTWWD8fo0XPTa` |
| 命令 | 原生pwsh进程包装的 `Write-Output 'R1_17_CANCEL_20261102'` |
| cwd | 该测试会话的 `remote-e2e-ra-20261102-input-integrity` 目录 |

原生 `availableDecisions` 为 `accept`、一个 `acceptWithExecpolicyAmendment` 对象和 `cancel`，没有字符串 `decline`。网页UI真实显示“允许一次”和“取消本轮”。因此本例核对的是用户选择**原生允许的cancel**，不能为了满足旧预期要求额外存在“拒绝”按钮。

`W/cancel-http.json` 中准确请求为：

```json
{
  "threadId": "01a19a01-ebb4-7ee1-a8e1-90d180656ac1",
  "requestId": 4,
  "method": "item/commandExecution/requestApproval",
  "decision": "cancel"
}
```

该请求发往 `/api/action/approve`，不是 `/api/action/stop`。网络ID为 `53184.413`，时间为2026-11-02 21:34:07.514；Idempotency-Key与原响应回执ID均为 `50f94a25-5d8b-484d-99e7-268d16c150f1`。HTTP200、对应 `loadingFinished`、完整234字节原响应均存在；响应为completed/ok，准确会话的requests为空。

仅有成功回执和卡片消失不足以证明命令未执行。本次继续核对 `N/capture-canceled.json`：

- 准确审批轮为 `interrupted`。
- 同一item `call_a08cnKjEz68NTWWD8fo0XPTa` 为 `commandExecution/status=declined`。
- `aggregatedOutput=null`、`exitCode=null`、`processId=null`；该轮只有这一项commandExecution，没有另一个替代命令。
- 对应request已移除。

这里的 `declined` 是原生命令项的结果状态，**不表示网页提交过decision=decline**。不能在用户提示或旧历史中搜索到命令标记，就把提示正文当作命令输出。本例以准确item的null输出、null退出码和declined状态判断命令取消，未要求被原生直接中断的轮次另补一条模型回复。

## 3. 原队列保留与单次显式恢复

### 3.1 排队时及取消后是同一个完整对象

`W/queue-http.json` 的唯一普通send请求于21:33:21.801发出，client为 `2b55334f-388c-4c59-a5d0-a5f4295b1212`。原回执 `f60e7cb1-2fef-4acf-aa49-847407af3b58` 为completed，`deliveryState=queued`，物理队列ID为 `01a19c76-edff-7a02-bece-d3a79d253725`。

原请求正文是本轮唯一短文本标记：

```text
[remote-e2e:R3-06:cancel-queue] 这是取消审批后的原队列行为验收。只回复 CANCEL_QUEUE_OK，不调用工具，不读写文件。
```

`N/capture-queued.json` 与 `N/capture-canceled.json` 的 `/queue/data` 均为一项，**完整数组逐字段相同**，包括：

- 原physical ID `01a19c76-edff-7a02-bece-d3a79d253725`。
- 原client `2b55334f-388c-4c59-a5d0-a5f4295b1212`。
- 原完整 `input`：一项text，正文保留，原生追加一个末尾换行，`text_elements=[]`。

取消前网页有一条原生待执行队列和同一审批卡；取消后UI显示原轮“已停止”，但仍保留同一队列行、准确 `queued-menu-01a19c76…` 和“引导此队列消息”。这与取消后的原生快照相符。

本次输入是纯文本，不能扩为图、Skill、隐藏引用或所有提交配置保留通过。

### 3.2 用户一次引导原消息，原生另起新轮

`W/steer-http.json` 于21:35:11.584记录唯一 `/api/action/steerQueued` 请求：

```json
{
  "threadId": "01a19a01-ebb4-7ee1-a8e1-90d180656ac1",
  "messageId": "01a19c76-edff-7a02-bece-d3a79d253725"
}
```

回执 `e477488a-b873-4d87-bdbf-fd91e2138a62` 为completed/ok，仍绑定原physical ID，返回新turn `01a19c78-9cc5-79b0-9449-02f8462addaf`，同时声明 `queueSyncPending=true`。

本次先前审批轮已经interrupted，因此这是**显式恢复原队列消息后另起新轮**，不是向原活动轮追加accepted steeringUserMessage。不按R1-16的活动轮steer结果判定，也不冒充自然自动消费。

独立核对 `N/capture-after-steer.json`：

1. 完整历史中的上述client只匹配一个正式 `userMessage`，位于该新turn；服务器正式消息ID为 `01a19c78-9cef-7b03-8905-03d1464168e4`。
2. 该userMessage的完整content，与原队列input逐字段相同；同轮 `params.input` 也与原队列input完全相同。
3. 新turn为completed，唯一final_answer为 `CANCEL_QUEUE_OK`。
4. 原生队列为空、待处理requests为空；完成UI显示准确队列正文和该回复，不再显示原待执行队列。

物理队列ID与最终正式userMessage ID不同，是两个不同阶段的身份，不能要求二者相等。血缘由原send回执、原steer请求、原client、返回turn和完整input共同建立。

`queueSyncPending=true` 本身不是完成或失败证据；本次完成结论依据后续原生接受、完成历史和UI，不能据它再次发送或引导。

## 4. 请求次数、完整历史与设置恢复

### 4.1 HTTP动作按request ID去重

三个 `*-http.json` 窗口均为 `truncated=false/hasMore=false`。本次仅按 `Network.requestWillBeSent` 计派发，响应与完成事件不另算请求，重复出现在归档中的相同request ID也不重复计数。

| 动作 | 本次窗口唯一network ID | 原回执ID | 实际次数 |
| --- | --- | --- | ---: |
| send：将Q加入原生队列 | `53184.403` | `f60e7cb1-2fef-4acf-aa49-847407af3b58` | 1 |
| approve：原request4，decision=cancel | `53184.413` | `50f94a25-5d8b-484d-99e7-268d16c150f1` | 1 |
| steerQueued：恢复原Q | `53184.432` | `e477488a-b873-4d87-bdbf-fd91e2138a62` | 1 |
| 自动markRead | `53184.415`、`53184.443` | 不混入消息派发计数 | 2 |

三项显式动作各自的Idempotency-Key均与配对原回执ID相同；HTTP200、完成事件、Content-Length与归档响应字节数一致，分别为1349、234、266字节。没有用读取次数或响应事件数推算消息次数。

这些窗口中没有stop、removeQueued、editQueued或第二次send/steer的请求。启动真实审批测试轮使用的是另外保存的正式原生turn/start，其 `N/dispatch-intent.json`、`N/start-receipt.json` 与prepared client `f7b8958a-9dd3-4ee8-8168-563558affbd9` 单独关联，不能伪装成网页又发送了一条普通消息。

上述次数只对这些明确归档窗口和当前逻辑操作负责，不声称覆盖整个桌面所有线程的动作总数。

### 4.2 23→25轮及唯一接受

`N/baseline.json /history` 为 `isComplete=true`、23轮。pending、queued、canceled、after-steer、restored各阶段的完整历史均标记完整，且各自前23轮与baseline数组**逐字段深比较相同**。

最终25轮仅新增：

- 第24轮：准确命令审批测试轮，最终interrupted，命令项declined。
- 第25轮：准确原client对应的恢复执行轮，completed，回复 `CANCEL_QUEUE_OK`。

原队列client只在完整25轮中接受一次，不把 `params.input` 与正式userMessage各算一次，也不以同文案或队列空推定唯一性。

### 4.3 恢复的是该测试线程的设置

`N/restore-intent.json` 时间为21:36:06.165，明确 `retryAllowed=false`。`N/restore-receipt.json` 于21:36:06.205记录同threadId、`applied=true`；`N/restored.json` 于21:36:06.224取得恢复后原始状态。

独立比较恢复后与baseline：

- `/metadata/latestThreadSettings` **整个对象逐字段相同**，包括cwd、approvalPolicy、approvalsReviewer、sandboxPolicy/activePermissionProfile、model、effort、serviceTier、collaborationMode和其他原有字段。
- `/metadata/currentPermissions` **整个对象逐字段相同**。
- 完整25轮与after-steer一致，未因恢复设置新增执行轮次。

这支持该测试线程设置恢复通过，独立结论不依赖 `restoration.mismatches=[]/verified=true`。它不证明全局配置变更，也不表示119测试适配器已经撤回；恢复后的快照仍记录同一个119/106 fixture。

## 5. 通过范围及不应继承的结论

| 对应基线 | 本次新增的准确证据 | 仍保留的范围 |
| --- | --- | --- |
| R1-17 命令审批 | 原生availableDecisions确实允许cancel，网页一次回复原request4；同itemdeclined、无输出/退出码，原轮interrupted | 不覆盖所有审批类型、accept/会话级规则、文件审批、同步问答、失败和unknown |
| R3-06相关：取消审批后的原队列恢复 | 截点中原Q/input保留；用户一次显式引导后原client唯一完成 | **没有操作普通停止按钮，没有制造上游失败**，不能写成这两种触发已经通过，更不能规定所有失败后必须手动引导 |
| R3-07相关：审批等待期间入队 | 当前真实审批仍待处理时，一条纯文本消息成功进入原生队列，没有抢先替代该审批轮 | 不覆盖图/Skill、压缩中入队、所有能力/unknown分支 |
| R1-10、R1-16 | 本次不追加自然FIFO或活动轮steer通过 | 使用显式恢复入口，并在原轮中断后另起新轮，不能替代两条自然FIFO或引导进入原活动T |

本轮没有为了符合旧“失败后一律必须引导”的假设去改动原生队列策略。它只记录本次准确对象在取消后的观察结果，以及随后明确用户操作产生的恢复。

## 6. 附录：C删除预览取消

### 6.1 准确对象、风险提示和零删除

C为 `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。`D/preview-ui.txt` 的当前URL明确选中C，确认框为“删除会话”，显示该测试会话准确标题和“永久删除…及其会话记录，此操作不可恢复”的风险说明，同时提供“取消操作”“删除会话”，焦点在取消操作。

`D/canceled-ui.txt` 返回同一C的会话页面，确认框不再存在。`D/http.json` 的本次归档窗口为sequence3414—3442，`truncated=false/hasMore=false`；按原始requestWillBeSent检查，**没有任何 `/api/action/` 请求，因此delete请求为0**。

这是本次预览/取消窗口的零删除证据，不是整个测试期间或所有会话从未发生删除的全局声明；没有执行真实永久删除。

### 6.2 完整8轮来自persisted，不来自已加载6轮

独立读取 `M/delete-preview-before.json`（21:45:09.111）和 `M/delete-preview-after.json`（21:46:30.865），按准确C ID选择线程对象：

| 来源字段 | before／after | 独立比较 |
| --- | --- | --- |
| `/threads/[C]/history/turns` | 各6轮，`history.isComplete=false` | 两份已加载数组逐字段相同，但不能用它证明完整8轮 |
| `/threads/[C]/metadata/turnsPagination` | olderCursor非空、hasLoadedOldest=false | 明确还有更早历史，不把未加载数据当成不存在 |
| `/threads/[C]/persisted/thread/turns` | 正式 `thread/read includeTurns:true` 的完整8轮 | 两份完整8轮数组逐字段相同，没有删除或新增轮次 |
| `/threads/[C]/metadata/latestThreadSettings` | 原始完整设置对象 | 前后深比较相同 |
| `/threads/[C]/metadata/currentPermissions` | 原始完整权限对象 | 前后深比较相同 |

`persisted`中的8轮包括两个没有出现在已加载6轮视图中的较早turn；本次已经直接核对这两个正式持久读取对象，不再只引用 `D/native-verification.json /turnCount=8` 的汇总。

附录中的同份快照还包含后续已达26轮的M，属于另一时间截点，不与主取消链21:36时的25轮终态混为同一次计数。

### 6.3 草稿和页面边界

本次预览前草稿为空，不报告“非空普通草稿保留”。取消后AX中消息输入没有非空Value，并可见原会话及内容；该快照同时记录输入框/部分按钮禁用，未另取得其加载就绪后的控件终态，因此本附录**不追加全部写控件已恢复可用的结论**，也不凭这一帧判定新的产品失败。

R1-32的正常预览取消、风险说明、准确对象和历史保留子场景可追加通过；身份变化中止、非空草稿、失败/unknown、实际永久删除及R3-18关闭/删除unknown不继承通过。

## 7. 原生原件身份与剩余项

以下SHA-256为本次直接读取文件后重算，便于后续归档定位；没有改写原件。

| 原件 | SHA-256 |
| --- | --- |
| `N/baseline.json` | `9522ccfe76c4ddb0b4acf4313a47e19ae10d737a135c613f1c22fdeeac6a0412` |
| `N/capture-pending.json` | `f4ed1f2c439efd3c752049190ff8c47f781e4357d8a549fcc44e7c7d8f56e7bd` |
| `N/capture-queued.json` | `d5c08dc62a5b985286c36c828ac2fc56fcca36a17549a83afd593dee2ea64cc6` |
| `N/capture-canceled.json` | `17c7ce702c3c1a4be575203a772941fe51f24c21fe730ed88694d5908ce82902` |
| `N/capture-after-steer.json` | `ab5d72a0e31464ecd405297da792377f437c99aed6064fa5bf60d26d2a86356d` |
| `N/restored.json` | `29adebf85fea43b1ac7b523b6eb610bc25da431af4ce1394d16ea440aa25913c` |
| `M/delete-preview-before.json` | `97a27206a01907182aa5af4495e50ee95de04e932509f3d9b278a221d3fc95a5` |
| `M/delete-preview-after.json` | `0595c56e854878b25f4bad0515d506905832f9cb525d1519b0cebef396873a24` |

仍需独立执行：普通停止按钮及后台终端范围、真实上游失败后的队列策略、复杂输入、取消/引导丢回包与过期竞争、跨端处理、非空草稿的删除预览，以及真实关闭/删除unknown。主链设置恢复已证；全局适配器、浏览器、服务及临时文件的完整收尾由主代理另行登记，本报告不以线程恢复成功推断现场全部清理完成。
