# 2026-11-02 远程命令审批与排队续跑验收

## 结论与范围

已对留存的 HTTP、原生快照、界面完整文本和最终截图独立进行离线交叉核对。本次审计只新增本文档，没有操作 RPC、浏览器、产品或测试线程，也没有修改证据原件。

| 验收项 | 状态 | 本次证据支持的结论 |
| --- | --- | --- |
| R1-17：真实命令审批，单次 `accept` | 通过 | 网页提交原生 request `3` 的单次批准；同一命令 item 随后以 `exitCode=0` 完成并输出固定标记；同一轮最终回复 `COMMAND_APPROVAL_EXECUTED` |
| R3-07：命令待审期间发送一图加 Skill，批准后自然续跑 | 本组合通过 | 消息先进入真实原生队列，待审时未进入历史；批准后仅消费一次，完整输入保持并最终回复 `APPROVAL_QUEUE_DONE` |
| 旧历史、未发送草稿与线程设置保护 | 通过 | 原有 19 轮全部字段保持；最终为 21 轮；草稿仍在输入框且未进入用户消息；完整线程设置和当前权限恢复为基线 |
| 拒绝、文件修改审批、故障及其他组合 | 未测 | 不纳入上述通过结论；R1-17 总项仍为部分覆盖 |

证据目录为仓库内 `dist/validation/remote-acceptance-20261102/approval-final/`。下文文件名均相对于该目录，另有明确说明的路径除外。本报告没有将“待决请求消失”单独用作执行成功判据。

## 环境与关联标识

测试线程为 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，工作目录为 `C:\Users\junes\Documents\Codex\2026-11-02\remote-e2e-ra-20261102-input-integrity`。快照中的 owner 始终为本机 `hostId=local`、`role=owner`，epoch 为 `aab44d88-0ab6-44db-9b4e-e5baf0b1846a`，模型为 `gpt-6.1-sol`，思考深度为 `max`。

本次运行使用桌面兼容注入夹具：`fixture.sourceVersion=118`、`fixture.compatibilityVersion=106`，实际暴露的 `adapterVersion=106`。应表述为“118 源码在现有桌面兼容夹具下完成此次验收”，不能表述为“已安装 118 安装包完成验收”。夹具记录的原始 `native.js` SHA-256 为 `63f99482a1d0a3fa44e9d327b56c013a8f4e18da731bd2490c34012e8ade10b3`。

| 对象 | 实际标识 |
| --- | --- |
| 原生命令审批 request | 数字 `3`，method 为 `item/commandExecution/requestApproval` |
| 命令轮 turnId | `01a19c17-cf59-7cd1-8f9f-4fdf446e179e` |
| 命令轮 userMessage.clientId | `227859f6-4d1f-452a-a257-16dc34f57ff6` |
| 命令 itemId | `call_PiytncUHG6onVzBq8zN0wH8z` |
| 后继队列 clientUserMessageId | `e846d8f7-e852-4be8-af96-295470fbce9d` |
| 原生队列消息 id | `01a19c21-5849-7770-b964-41bb0334584d` |
| 后继实际消费 turnId | `01a19c23-7544-7432-83a9-1806bc2e02cc` |

### 捕获辅助字段的已知限制

`native/pending-1.json`、`native/queued-before-decision.json` 和 `native/after-decision.json` 中的辅助字段 `testedTurnId/testedTurn` 为 `null`。捕获工具在 `turn.params.clientUserMessageId` 查找，但此次真实历史的 `turn.params.input=[]`，消息 clientId 位于 `turn.items[type=userMessage].clientId`。

本报告以 `native/start-receipt.json` 返回的真实 `result.turn.id` 定位命令轮，再用 `native/prepared-request.json` 的 clientUserMessageId 与该轮 userMessage.clientId 交叉验证；两者唯一匹配，且准备请求的完整 input 与该 userMessage.content 相等。后继消息也以真实 userMessage.clientId 定位，未依赖空 input 或上述辅助字段。原件中的 `null` 保持原样。

## 命令审批证据链

1. `native/pending-1.json` 和 `native/queued-before-decision.json` 均包含原生 request `3`。其 threadId、turnId 和 itemId 与上表一致，`kind=command`、`environmentId=local`，cwd 为测试目录，命令为 PowerShell 执行 `Write-Output 'R1_17_COMMAND_APPROVAL_20261102'`。命令 item 当时 `status=inProgress`，`processId/exitCode/aggregatedOutput` 均为空，线程为 `activeFlags=["waitingOnApproval"]`。
2. `approve-http.json` 内仅有一次 `POST /api/action/approve`，浏览器网络 requestId 为 `88792.572`、sequence 为 `399`，HTTP 状态为 `200`。body 中的业务 requestId 为数字 `3`，method 为 `item/commandExecution/requestApproval`，decision 为字符串 `accept`。未使用会话级批准或 execpolicy amendment。
3. 独立保存的原响应 `approve-response.json` 为 `state=completed`、`result.ok=true`，应用请求追踪 id 为 `66453475-c2ae-4689-9917-dfacba770161`，返回同一线程的 `requests=[]`、相同 epoch 和 `requestsRevision=9`。该追踪 id、浏览器网络 requestId 与原生 request `3` 分属不同层，不能混用。
4. `native/after-decision.json` 中，同一 turnId 已完成，同一命令 itemId 为 `status=completed`、`exitCode=0`、`durationMs=289`，`aggregatedOutput` 精确为 `R1_17_COMMAND_APPROVAL_20261102\r\n`；同轮存在唯一最终 agentMessage `COMMAND_APPROVAL_EXECUTED`。
5. `completed-ui.txt` 和已人工查看的 `completed.png` 均显示最终回复 `COMMAND_APPROVAL_EXECUTED`。执行成功同时具备原生请求、单次批准 HTTP 回执、命令 item 完成及真实输出、最终回复和最终界面证据。

`after-approve-ui.txt` 只记录点击后“正在提交，请勿重复操作”、批准和取消按钮禁用的即时状态，不是完成态。等待审批所占的时间也不能视为命令执行耗时。

## 待审消息排队、输入保持与自然续跑

### 真实排队与批准后的消费顺序

`queued-send-http.json` 内只有一次本次 `POST /api/action/send`，浏览器网络 requestId 为 `88792.544`、sequence 为 `240`、HTTP 状态为 `200`。请求携带上表后继 clientId、一张图片和一个明确选择的 Skill。

`queued-send-response.json` 返回 `ok=true`、`confirmed=true`、`deliveryState=queued`、`queueRevision=16`，原生队列消息 id 和 clientId 与请求对应。`native/queued-before-decision.json` 进一步证明：

- 真实命令审批 request `3` 仍待决，线程仍为 `waitingOnApproval`。
- 原生队列只有这一条消息，`nextCursor=null`。
- 历史仍为 20 轮；整个历史中不存在该后继 clientId 的 userMessage，因此没有在待审期间抢跑。

批准后，`native/after-decision.json` 的历史为 21 轮。整个历史中该 clientId 只匹配一个 userMessage，位于 turn `01a19c23-7544-7432-83a9-1806bc2e02cc`；该轮完成并仅产生一次最终 `APPROVAL_QUEUE_DONE`。队列与待决 requests 均为空，线程恢复 `idle`。

命令轮按 `turnStartedAtMs + durationMs` 于北京时间 **20:02:11.914** 完成，后继轮于 **20:02:12.061** 启动，顺序相差 147 ms。留存的批准阶段 HTTP 窗口没有再次 `/api/action/send`、立即发送或引导队列的写操作，结合真实队列前后状态和时间顺序，支持本次由原生队列自然续跑的结论。

### 完整输入核对

原生待执行消息的 `input` 与最终后继 userMessage 的 `content` 逐字段比较完全相等，元素顺序均为 `text → image → skill`；比较包含 `text_elements`、图片 URL 与 `detail`、Skill 名称与完整路径，没有只比较可见文字或元素数量。

| 对象 | 核对结果 |
| --- | --- |
| 文本 | HTTP prompt 接纳为原生输入时仅在末尾追加一个 `\n`；从待执行队列到实际消费的文本与所有相关字段完全相等 |
| 图片 | HTTP dataUrl、待执行 image.url、最终 image.url 完全一致；HTTP 声明大小和实际 Base64 解码大小均为 **172 字节** |
| 图片 SHA-256 | `63db222d0886ab1a22d1a675af881b00a9aed9ea8ef69c8dbef2909aee8a06d4`，与图片资源标识一致 |
| Skill | `name=root-cause-analysis`；路径精确为 `C:\Users\junes\.agents\skills\root-cause-analysis\SKILL.md`，HTTP、原生队列和最终消息一致 |
| 完整原生输入 SHA-256 | 对对象键排序、保留数组顺序后序列化计算，队列和最终内容均为 `8458187fd6ecc8a62915f44ea53c9d6f799d3cf00f526ea44e4de3314a7b4806` |

最终截图可见蓝色图片和 `Evidence Closed RCA` 标签；完整界面文本亦保留对应图片链接和 Skill 元素。本次证明图片和 Skill 输入完整交付，不据此宣称模型进行了图像语义识别或实际读取了 Skill 文件。

`queued-pending-ui.txt` 包含真实队列、一张图片及待决审批卡片。旧 `queued-pending.png` 的视口未显示该文本快照中的队列与新草稿，不能用于证明这些对象；该阶段以原生快照、HTTP 和完整界面文本交叉核对，最终显示以 `completed-ui.txt/completed.png` 为准。

## 历史、草稿与设置恢复

### 原有 19 轮历史

以 `native/baseline.json.history.turns` 为基线，对待审排队、完成和恢复后快照的前 19 轮进行完整递归比较：只将对象键排序，保留所有字段值、数组顺序、嵌套内容和空值，没有排除时间、状态、附件、命令输出或 hook 字段；19 轮差异数均为 **0**。相关历史快照均为 `isComplete=true`。

基线 19 轮、完成后前 19 轮、恢复后前 19 轮的规范化 JSON SHA-256 均为：

```text
9ecfc241bdde19d9396ae01adac20998d2eb819471431f572b272dd1840963c7
```

最终新增的两轮仅为本次命令审批轮和后继队列轮。`after-decision.json` 与 `restored.json` 的整个 `history` 也完全相等，恢复设置没有再改变最终 21 轮历史。

### 未发送草稿

`queued-pending-ui.txt`、点击后的 `after-approve-ui.txt` 和最终 `completed-ui.txt` 的输入框值均为：

```text
GIT_TOOLS_DRAFT_20261102：仅保留，不发送。
```

最终截图也实际显示该草稿。最终 21 轮中没有包含此标记的 userMessage。此结论覆盖待审排队后到最终完成的草稿保持；`before-queued-send-ui.txt` 的输入框当时是待发送的测试消息，不冒称该时间点已存在同一草稿。

### 完整线程设置和当前权限

`native/restored.json` 的恢复回执为 `applied=true`、`verified=true`、`mismatches=[]`。此外，独立比较基线与恢复后原始对象，而非只信任辅助验证字段：

- `metadata.latestThreadSettings` 的完整 **14 个字段**及所有嵌套值完全相等：`disabledPluginIds`、`cwd`、`approvalPolicy`、`approvalsReviewer`、`sandboxPolicy`、`activePermissionProfile`、`model`、`modelProvider`、`serviceTier`、`effort`、`summary`、`collaborationMode`、`multiAgentMode`、`personality`。
- `metadata.currentPermissions` 的完整 **5 个字段**及所有嵌套值完全相等：`activePermissionProfile`、`approvalPolicy`、`approvalsReviewer`、`runtimeWorkspaceRoots`、`sandboxPolicy`。
- 因而原始 `approvalPolicy=never`、`approvalsReviewer=user`、`sandboxPolicy.type=dangerFullAccess`、profile `:danger-full-access`、原始 workspace roots、模型、effort、默认 collaborationMode 及其 developer_instructions 均已恢复。
- 恢复后仍为同一 local owner、同一 epoch、`idle`、空 requests、空队列且无待确认编辑。

## 证据原件与校验

`native/` 下的 10 个 JSON 文件已逐字节与 `temp/remote-approval-final-20261102/evidence/` 中同名原件比较，全部一致：`baseline`、`before-start`、`prepared-request`、`dispatch-intent`、`start-receipt`、`pending-1`、`queued-before-decision`、`after-decision`、`restore-intent`、`restored`。审计没有修补辅助关联字段或重写任何快照。

| 文件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `approve-http.json` | 178203 | `fc2bef9d751ee373e7b883ee0ae2b5a8ac7fa2ff1a5b151a6d009d2a05f03236` |
| `approve-response.json` | 233 | `1374a64a5383b34af604d4442d5d0edc9fdc5641aa31c28d7011867368345dd7` |
| `queued-send-http.json` | 72206 | `41325460c375ab5598267c2aedfeffe31a260eadcf962522b0daca5fe76d2ddd` |
| `queued-send-response.json` | 1903 | `e0839c311d43a89243519640b8a412657d037796ff012d2ca93d34e07a100562` |
| `completed-ui.txt` | 38295 | `00448707aab7ca65a8fdcdde8fc761d69c524183007ee0c8422571f5e5c08dc1` |
| `completed.png` | 110009 | `1e76aadb31a89170e6d636ef2214b4ca279355269cd3ec8df2dbbc8f366a18a9` |

关键原生快照 SHA-256：

| 文件 | SHA-256 |
| --- | --- |
| `native/baseline.json` | `0550368cc2e5faf8428c0806e80a3420bb5e2bd2172646350a9194bc373f858f` |
| `native/queued-before-decision.json` | `3c30026520d967291cb519b4dc9068460b3bc810d63fbce24fdd5df028ee7473` |
| `native/after-decision.json` | `915b9542e804611ad46165e73b37d4a0787755f26b629426d26530bdd4d140f4` |
| `native/restored.json` | `0986ccc9ff1a5bd2a85d05de3002d96caaeb15fee6611681417dc78d208d0b80` |

## 未覆盖与观察边界

- 未测真实拒绝或取消命令后命令不执行、队列处理及最终拒绝反馈。
- 未产生或验收 `item/fileChange/requestApproval`，未验证文件批准、文件拒绝或文件回滚，也没有为此创建目标文件。
- 未测重复审批、过期 request、owner/epoch 改变、断线重连、HTTP 或原生回执未知、超时、恢复失败等故障分支。
- 未测会话级批准、永久规则批准、多待决请求、多条队列及其他附件组合。
- HTTP 窗口中出现图片缩略图 `404` 后原图 `200` 的回退以及 `/api/channels` 的 `404`；本次完整图片输入与最终可见图片均有独立证据，但没有把这些旁路请求评为已修复或完成专项验收。
- 本次是指定测试线程和兼容注入环境的一次成功样本；不据此认定完整 R1-17、全部审批类型、生产环境或已安装产品版本均已通过。
