# 既有会话目标创建实测（2026-11-03）

结论：R1-21 的纯文本正常创建、执行并完成链路通过（本子场景）。本次实际目标要求 `get_goal` 核对归属后 `update_goal({"status":"complete"})`，不是暂停目标方案。仅独立复核已保存原件和原始 rollout；未操作浏览器、RPC 或产品状态。

## 对象与运行身份

| 对象 | 准确身份 |
| --- | --- |
| M | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` |
| 对照会话 C8 | `01a1996d-0b7a-71d0-b9ad-9c521d224ad9` |
| 原请求 A | `43cbd20e-4121-43bf-9979-2dc030c8a0af` |
| 目标 G | M、正文标记 `[remote-e2e:RA-20261103-R1-21-M]`、`createdAt=1793653033` |
| 唯一新增执行轮 T | `01a19e0d-4969-74e3-8f8f-0a7540c23426` |

业务原件目录为 `dist/validation/remote-acceptance-20261103/goal-create-existing/`；以下简写为 D。共用的运行身份原件在相邻 `queue-dequeue-race/`，简写为 Q。

- 原生使用 sourceVersion `121`／compatibilityVersion `106` 测试夹具，epoch 为 `371c41e3-4a6c-4fa8-856b-a6da173e702e`。`Q/runtime-native-engine-before-goal.json` 的实际脚本摘要与同名 `.js` 重算结果一致：`5b97e84abf7933129193a3bb0230bc9fc3feb39fd0705fef65ff1c7af930cf78`。
- 网页 `Q/draft-fixed-empty-baseline.json` 的 A、B 两页摘要相同，且与 `Q/app-draft-fixed.js` 全文重算一致：`a4d9c02b2b222608cd73df59ccea6d99b6e2c99d7130fb035df9f074f3a37c55`。
- 后续主执行方已补 `D/identity-index.json`、`D/final-ui.json` 和 `D/goal-completed-view.jpg`；截图是编辑第二轮结束后回看目标完成记录，不作为创建瞬间的界面证据。结束原生全文及恢复记录分别为 `Q/runtime-native-engine-after.js/json` 和 `Q/runtime-restored.json`，实际121文本起止相同，随后恢复原119文本。

## 创建与完成证据

| 阶段 | 已核事实 | 原件 |
| --- | --- | --- |
| 前置状态 | 已恢复 `default` 协作模式；独立 `thread/goal/get` 返回 `goal:null`。该时点 M 有 38 条完整 canonical 历史，不采用更早的 34 轮作为创建基线 | `D/before-goal.json` 的 `after` 和 `goal` |
| 输入 | 目标标签数量为 1，完整目标正文与实际 POST 的 `prompt`、回执目标 `objective` 逐字一致；POST 没有 UI 标签文案 | `D/prepared-ui.json`、`D/goal-network.json`、`D/set-goal-receipt.json` |
| 单次提交 | 捕获未截断、无剩余页；仅 1 次 `POST /api/action/setGoal`，网络请求 ID 为 `24688.1324`，幂等键为原 A；`threadId=M`、`goalMode=true` | `D/goal-network.json` |
| 创建成功 | HTTP 200；回执 `state=completed`、`result.ok=true`；G 为 `active`；`goalSubmission.action=setGoal`、`phase=complete`、`firstTurnAccepted=false`、`goalSet=succeeded` | `D/set-goal-receipt.json` |
| 目标完成 | 原始 `get_goal` 读取同 G 为 `active`；唯一 `update_goal({"status":"complete"})` 回执仍是同 M、同正文、同 `createdAt`，状态为 `complete` | 下述 rollout 原始调用与回执 |
| 原生终态 | `completedThreadGoal` 为同 G、`status=complete`；当前 `threadGoal=null`。独立 `thread/goal/get` 再次返回 `goal:null`；T 完成，最终消息为 `GOAL_CREATE_COMPLETE_OK` | `D/final-history.json`、`D/native-goal-final.json` |

原始工具证据来自 M 的 `metadata.rolloutPath`：

`C:/Users/junes/.codex/sessions/2026/11/02/rollout-2026-11-02T10-11-23-01a19a01-ebb4-7ee1-a8e1-90d180656ac1_01a19a06-8dd1-7341-be92-d1b0872208cb.jsonl`

- 第 695／697 行：`get_goal({})`，`call_I92lyzx8ItNvzLncXczUxdjv` 及其匹配回执，返回 G 的 `active` 状态。
- 第 701／703 行：`update_goal({"status":"complete"})`，`call_UbSTiGqcxtTlF0sTfdYZgBMY` 及其匹配回执，返回 G 的 `complete` 状态，`tokensUsed=2031`、`timeUsedSeconds=20`。
- T 中全部原始工具调用仅上述两次；第 708 行为最终 `GOAL_CREATE_COMPLETE_OK`。原生历史视图未展示这两项工具，不能仅用该视图或最终文本替代真实工具回执。

主执行方已将上述4条工具原始行逐字保存为 `D/native-goal-tool-records.jsonl`；`D/native-goal-tool-records-source.json` 登记原文件、行号、截点摘要和两组call_id，不需要依赖后续可能继续追加的完整rollout文件。

回执 `completed` 只说明原 A 的动作处理完成；目标完成另由 `update_goal` 的 `status=complete`、`completedThreadGoal` 和独立当前目标读取共同证明。

## 历史与作用范围

- M 的 loaded 历史为 40 项，persisted 为 39 轮。loaded 前 38 条完整记录与创建前 canonical 记录逐项相等；persisted 前 38 个轮次 ID 及顺序保持。新增持久化执行轮仅 T。
- loaded 另有一项 `/goal <正文>` 记录，`turnId=null`、`items=[]`、`status=completed`；该项未出现在 persisted 的 39 轮中。此结构差异不能计作第二次真实执行，也不足以推断其内部生成机制。
- 捕获窗口中普通 `send`、`create` 和入队写请求均为 0；另有 1 次 `markRead`，不把全部写动作误报为只有 `setGoal`。M 最终 server／native／local 队列均空，无待处理请求，运行状态为 `idle`。
- 相对 `D/before-goal.json.after`，M 的 `latestThreadSettings`、`latestCollaborationMode`、`currentPermissions` 完全相等，保持 `default`、`gpt-6.1-sol/max`、`approvalPolicy=never` 和 `dangerFullAccess`。
- C8 相对 `Q/before-edit.json`，原 8 个 persisted 轮次完整记录及整个 `settings` 均完全相等；最终三层队列均空，无待处理请求。

## 验收边界

仅将 R1-21 的既有会话、纯文本正常创建并完成子场景记为通过。未覆盖附件／Skill 目标、真实拒绝、未知结果、回包丢失、原生阶段部分成功或指定项目新建；R1-22／R3-10 不因此升级状态。

本轮全部测试结束后的公共收尾在 `Q/environment-restored.json`：临时1435／45430服务停止，原1432／45222保留。M在后续编辑第二轮后已恢复default配置且无队列、待答请求；这不改变本目标自身以38→39持久轮次判定的范围。
