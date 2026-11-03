# 指定项目目标创建与阶段暂缓恢复实测（2026-11-03）

结论：R1-22 的指定项目纯文本创建、目标完成子场景通过；R3-10 的“会话及首条已接受、目标尚未委托时刷新，只查询原 A 后收敛”在第二事务通过。第一事务刷新晚于安全定时放行，不计阶段恢复通过。两条基线的其他失败、unknown 和保新稿分支继续保留未验。

本次独立复核只读取已保存原件及两个测试 N 的原始 rollout，新增本报告、透明核验 JSON 和逐字工具记录；未操作浏览器、RPC、产品或 runner。

## 身份与输入

原件目录 `dist/validation/remote-acceptance-20261103/goal-partial/` 以下简写为 D，`D/r2/` 简写为 R。原件时间为 UTC，下表时序统一换算为北京时间 2026-11-03。

| 对象 | 准确身份 |
| --- | --- |
| P | `22195cc3-55de-49f6-8c22-2e18fbfb811c`，名称 `workspace` |
| N1／A1 | `01a19ea1-6ca6-7523-bc26-3486e56a5f6c`／`43652179-e97a-44a8-8a5c-1d5bbecf7d8b` |
| N2／A2 | `01a19ea7-b474-76e0-ac29-ce403e5a3944`／`da15bdd4-bc5b-4821-8bbd-a35d3d6495d9` |
| 本轮原生 | sourceVersion `121`／compatibilityVersion `106`，epoch `fec066b9-c266-46da-a744-ec1109faca15` |

`D/native-test-121.js` 重算 SHA-256 为 `42351e5724acf0777ebc9478cd8456297fcd255a088a934b23759b4a28768bbc`，与 `runtime-installed.json`、`runtime-plan.json` 一致。`D/app-runtime.js` 与复核时当前 `assets/remote-access/app.js` 摘要相同：`c8bcf027197484b274ed275b5b59fc7c3a1239f6b9fc15c4303d7715e5d593b1`。

P 的原生登记见 `D/project-before.json`，唯一 `rootPaths` 为 `E:\code\junes\github\CodexPlusPlus\temp\checkpoint-live-run-2026-09-02-01\workspace`。两次真实 `startConversation` 都携带同 P 的 `projectAssignment={projectKind:"local",projectId:P}`，`cwd/workspaceRoots` 与该根路径相同；原生 metadata 为 `workspaceKind=project`，最终网页也将 N 列于 `workspace`。独立 `thread/read` 的 `cwd` 相同，但其原始 `projectId=null`，本报告不将该字段改写为非空，也不据此虚构服务器端项目 ID 证据。该测试路径位于当前仓库内，不称为独立 Git 仓库。

`prepared-fields.json` 记录目标标签各 1 个；DOM 文本含“目标”标签文案，实际 POST 的 `prompt` 则逐字等于各自 `objective.txt`，不含该 UI 标签。两个请求均只有 `prompt/projectId/goalMode` 三个字段，`goalMode=true`。原生首条完整输入均为单项 text、`text_elements=[]`，正文是 `/goal <原目标正文>`，在持久化首轮中仅出现一次。`D/prepared-ui.png` 已独立目视核对项目选择及目标标签。

## 两次事务的真实阶段

两次 gate 都只匹配原生创建结果中的准确 N、完整目标正文和 `appendTranscriptItem=false`。held 时记录 `action=create`、`phase=goalSet`、`firstTurnAccepted=true`、`goalSet=not-attempted`，`delegatedGoalCalls=0`；同场独立 `thread/goal/get` 返回 `goal:null`。这证明原生创建／首条接受已成立而目标设置尚未委托，不能表述为目标 RPC 已受理后的持久化失败。

| 事务 | 已核时序 | 判定 |
| --- | --- | --- |
| N1 | `07:39:02.634` held；`07:39:11.253` 独立读取目标为空；`07:39:23.413` 以 `safety-timeout` 放行；新 Document 到 `07:39:28.557` 才开始 | 指定 P 创建及完成通过；刷新晚于放行约 5.144 秒，阶段恢复未验 |
| N2 | `07:45:54.292` held；`07:46:02.384` 仍未委托且目标为空；`07:46:15.723` 新 Document；`07:46:16.929` 起读取原 A2；`07:46:29.411` 安全定时放行 | 刷新及原 A2 的 pending 读取真实发生于 held 窗口内 |

N1 的实际暂缓时长为 20.779 秒，`D/pending-original-receipt.json` 为空，不能借后来的正常 completed 回执补成 pending 恢复。正常结果见 `D/create-original-receipt.json`、`D/after-first-create.json`。

N2 的实际暂缓时长为 35.119 秒，`R/final-gate.json` 与 `R/restored-gate.json` 均记录 `releaseReason=safety-timeout`；不是操作者手动释放。原调用放行后只委托一次原 `setThreadGoal`，以同一参数得到 active G；没有补发目标设置。

N2 的新 Document loader 为 `311F71A2345F0986D5FC575D67ABA5C1`，不同于原 POST 的 loader。`R/pending-recovery-events.json` 与 `R/pending-receipts.json` 匹配到同一 A2 的 8 次 GET：

- 网络 ID `71364.313/.315/.316/.318/.320/.321/.323`：7 次 HTTP 200，均为 `state=pending/result=null`，请求均在放行前。
- 网络 ID `71364.325`：`07:46:29.588` 请求得到 `state=completed`，`result.ok=true`、`conversationId/threadId=N2`、原首轮 T 不变，`goalSubmission.action=create/phase=complete/firstTurnAccepted=true/goalSet=succeeded`。

回执 `completed` 与目标 `complete` 分开核验：该回执返回 active G；后续真实目标工具及独立原生终态才证明同 G 已完成。刷新造成旧 HTTP 不再被原页面接收，不是本次阶段成立的唯一依据，更不等于原生目标保存失败。

## 唯一性与目标工具

捕获窗口去重后，每个事务各只有 1 次 `/api/action/create`，原幂等键和完整正文均与 `original-action.json` 一致；普通 send、网页 setGoal、入队写请求及第二次 create 均为 0。N1 另有 2 次 `markRead`，不能将其隐藏为全流程零其他 action。各事件文件 `truncated=false/hasMore=false`；本结论限所保存窗口，并与 gate 的真实调用计数及原生历史交叉核对。

两个 gate 的 `createCalls/goalCalls/delegatedGoalCalls/releases` 终值分别均为 `1/1/1/1`。每个 N 的持久化历史恰有两个不同的 completed 轮次：

| N | 首轮 T：唯一 `/goal` 用户输入 | 后续目标执行轮：无新增用户输入 |
| --- | --- | --- |
| N1 | `01a19ea1-6e3c-7292-8808-9c1f9d345ef3` | `01a19ea1-cc4b-77c3-9946-8bf5258aa5ca` |
| N2 | `01a19ea7-b5fe-79a0-a634-e0dcd5d27f3a` | `01a19ea8-41b9-7c23-829e-ffacbe56dfef` |

首轮 `get_goal` 返回 null，回复 `GOAL_FIRST_TURN_NO_GOAL`；放行后目标调度轮读取 active G，再将同 G 标记 complete，回复 `GOAL_PROJECT_COMPLETE`。第二轮不是重新创建或重发首条。

| N／真实工具 | call_id | 已核回执 |
| --- | --- | --- |
| N1 `get_goal` | `tool_GK7EvnD8TbkHBqNpTzxo66Zg` | `goal=null` |
| N1 `get_goal` | `tool_XooZFama1JE5Qg2rsRyB6d35` | 同 N1／原正文，active，`createdAt=1793662763` |
| N1 `update_goal` | `tool_u30yPQ2Pp7oYqu8lonV2r5cg` | 同 G，`status=complete` |
| N2 `get_goal` | `tool_Xjx7gbWIVMDxvixtvgnXxPG1` | `goal=null` |
| N2 `get_goal` | `tool_dTJTqiQdVtOVNH41y9CMxTMo` | 同 N2／原正文，active，`createdAt=1793663189` |
| N2 `update_goal` | `tool_orYd6Vkm70NmvxjC0W5qFtvO` | 同 G，`status=complete` |

两个 N 的原始工具调用均只有上述各 3 次。已将调用和匹配回执各 6 条原始行逐字归档至 `D/native-goal-tool-records.jsonl`、`R/native-goal-tool-records.jsonl`；相邻 `*-source.json` 保存源路径、原行号、截点摘要、记录摘要及准确轮次。N1 的记录已在后续删除验收前归档。最终 `after-first-create.json`／`after-recovery.json` 的 `completedThreadGoal` 均与本次 G 一致，独立当前目标读取为空、队列为空、无待处理请求且 runtime 为 idle。

## 网页终态、收尾与未验分支

`R/pending-ui.json` 的实际采样已在放行后，显示“会话和目标已创建”，只能作成功提示证据。恢复后的 `R/after-recovery-ui.json` 仍是新建页，不能声称自动进入 N2。主执行方随后从 `workspace` 手动打开准确 N2；`R/open-created-ready-ui.json` 的 URL 绑定 N2，`R/open-created-ready.png` 已独立目视确认原输入及两条完成回复。较早 `open-created.png` 是加载画面，不作终态证据。

两次 gate 均在原调用终结后恢复原方法，记录见 `D/gate-restored-first.json`、`R/restored-gate.json`。产品原生切回 119 的公共收尾由主执行方另留证，本次不提前报告已恢复。透明字段核验和输入摘要见 `D/independent-verification.json`，共 4 项公共检查、第一事务 24 项、第二事务 23 项，全部通过；这不是 73 条整体验收。

后续收尾：`D/runtime-restored.json` 已确认原119全文恢复，摘要仍为 `5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`，原fixture只更新为实际新epoch `ef679de6-97f3-431f-a838-a23e912693d8`。两N在目标原件归档后用于删除验收，最终均已按用户测试数据授权删除；P原登记逐字段保持，见 `D/project-before.json`／`D/project-after.json` 及 `remote-delete-receipt-20261103.md`。没有重启用户服务或Codex。

未在 pending 窗口输入新草稿，不声明该分支保新稿通过。真实原生拒绝、目标设置派发后 unknown、长期未知、附件／Skill、指定项目权限失败等均未因此覆盖。此次通过的是准确阶段暂缓后的原 A 恢复，不将其改写为原生部分失败，也不将成功 HTTP 回包丢失替代该阶段证据。
