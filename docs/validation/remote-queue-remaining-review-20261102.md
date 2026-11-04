# 队列、停止与失败恢复的剩余审查（2026-11-02）

本次是只读源码及既有原件审查，只新增本文件。未修改产品、测试或旧报告，未运行测试、模型请求、浏览器、RPC、Git 写操作或用户数据操作。文件审批新增已撤回，本报告仅保留未覆盖记录，不要求增加入口或能力。

**结论：R3-04 自然出队竞争、R3-06 普通停止／真实上游失败、R3-03 派发后未知均未完成真实验收。** 已有命令审批取消证据只证明原 Q 保留及一次显式恢复；本次还定位到删除结果分类、引导未知回执保留及旧验收脚本预期的源码问题。下文“确定源码缺陷”指可从控制流直接定位的问题，不表示已取得相同现场失败或完整根因闭环。

## 1. 独立预期与身份

依据为 `docs/validation/remote-acceptance-cases-20261101.md:79-82、111、116-122、172-176`，以及 `docs/remote-access.md:93、209、213、215、219、241`。预期在读取实现前已经存在：

| 前提 | 必须结果 | 不能据此追加的产品要求 |
| --- | --- | --- |
| 原 Q 尚未接受，编辑／删除与自然出队竞争 | 原生接受这次操作，或明确告知对象已过期；最终逻辑消息最多消费一次，旧写不覆盖已接受内容；unknown 单列，先只读核对 | 不要求暂停原生调度来获得必过窗口 |
| 活动 T 停止或真实上游失败，另有 Q | 原 Q 的完整输入不丢、不重加；不自动重放失败请求；恢复后原 client 最多接受一次 | 不预设所有失败都暂停，也不预设必须点引导 |
| 引导已经派发而结果未知 | 保留原请求 A、原 Q/client、目标 T；只查原结果，不能再派发第二次引导 | 不能用普通 send、另起同文消息或 CLI 重送补过 |
| 编辑已经取出原 Q | 保留原 Q→ED／移除令牌→返回物理 Q′→实际接受消息的血缘、普通草稿及修改稿 | 不要求 Q′ 的物理 ID 永远等于 Q |

`M` 为准确会话，`T` 为点击前真实活动轮，`Q` 为物理队列项，`client` 为逻辑消息身份，`ED` 为编辑事务，`A` 为 HTTP 幂等键／原回执 ID。统计真实接受时只数正式 `userMessage` 和 `accepted steeringUserMessage`；同一消息的 `params.input`、投影和 turnHistory 不重复计数。

## 2. 已覆盖，且不应重做成全新缺口的子场景

| 子场景 | 本次读取依据 | 本次结论及边界 |
| --- | --- | --- |
| 117 保存 B、取消 C 的成功回包丢失；原回执恢复；B→C 自然 FIFO | `remote-acceptance-current-20261102.md` E29；`remote-queue-recovery-20261102.md` | 报告定位成立；本次没有重新读取该目录全部原件，不追加一次真实执行。它没有覆盖自然出队临界编辑／删除、全部网络断开、长期 unknown 或原生重启 |
| 真实审批等待中图／Skill 入队，允许一次后自然唯一消费 | 当前表 E33；`remote-approval-20261102.md` | 已有正常子场景，不能再称全部未测；审批取消、普通停止和失败分别判定 |
| 119 命令审批 cancel 后原 Q 保留；用户一次引导原 Q 后另起新轮完成 | `remote-cancel-queue-20261102.md`；下述归档原件直接复核 | 本次直接比较取消前后 `/queue/data` 完整数组相同；最终完整历史25轮中原 client 仅一个正式接受项，队列为空。不是普通停止按钮，不是真实上游失败，也不是活动 T 内 steer |
| 双客户端旧 begin 与新保存冲突、旧排序与删除冲突 | 当前表 E11/E12 | 属于两端旧请求竞争，不能充当未暂停调度的自然出队竞争 |
| 引导准备契约、最终接受核验、工作树恢复前拒绝、错误目标拒绝 | `crates/codex-elves-core/tests/cdp_bridge.rs:1700、1870、1944、2007` | 既有源码回归覆盖；本次只读，没有重新运行。相关原生类冻结后在内存服务执行，不能写成真实桌面／真实模型通过 |

119 cancel 原件现已在：

`dist/validation/remote-acceptance-20261102/approval-cases-119/command-cancel/`

其中 `native/capture-queued.json`、`native/capture-canceled.json`、`native/capture-after-steer.json` 已直接读取，而不只采用辅助文件的 `acceptanceVerdict`：

- M：`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`；原 T：`01a19c75-8741-7e32-b513-fd2444ab21f0`。
- 原 request：数值 `4`，`item/commandExecution/requestApproval`；item：`call_a08cnKjEz68NTWWD8fo0XPTa`。取消后 T interrupted，item declined，输出、退出码、processId 均为 null。
- Q：`01a19c76-edff-7a02-bece-d3a79d253725`；client：`2b55334f-388c-4c59-a5d0-a5f4295b1212`。取消前后原 Q 的完整数组逐字段相同。
- `steer-response.json` 中 A 为 `e477488a-b873-4d87-bdbf-fd91e2138a62`，completed／ok；返回新 T `01a19c78-9cc5-79b0-9449-02f8462addaf`，`queueSyncPending=true`。
- 该新 T 的正式用户项为 `01a19c78-9cef-7b03-8905-03d1464168e4`；完整 input 与原队列一致，最终 completed。不能因物理消息 ID 改变就判重复或丢失。
- fixture：`61b3c80f-2248-4a29-b908-c5e853e4321b`；epoch：`1f26a046-3ad0-4f02-9a9f-85c62627b91a`；sourceVersion119／compatibilityVersion106。业务摘要 `d8c635875c50622194fdfebcede393afb540c9263bc44c6a3809231ceca00944`，兼容运行摘要 `312103909fc1c62a6b783268566eccb856aaa7b9352ba25c8d1024b0d00277ed`。

以上只核归档身份，不声称当前仍运行119。

## 3. 确定源码缺陷与明确的验收缺口

### D1：原生删除已确认后，后续队列读失败会被写成 failed

位置：

- `assets/remote-access/native.js:3061-3071`：RPC coordinator 操作已返回 result 后，额外读取 fresh 队列；读失败抛出带 `queueMutationAcknowledged=true`、原 `queueMutationResult` 和 `queueConfirmationStage=queue-snapshot` 的错误。
- `assets/remote-access/native.js:3099`：`removeQueuedMessage` 也复用这个包装器。
- `assets/remote-access/native.js:4645-4651`：`removeQueued` 捕获错误后只返回 `ok:false/code/message`，丢掉“变更已确认”及未知结果信息。
- `crates/codex-elves-core/src/remote_access.rs:1579-1585`：仅 `outcomeUnknown=true` 才记 unknown；上述返回值会记成 failed。

因此，在“删除实际返回非空 token，随后 fresh 队列读失败”的前提下，回执分类与实际删除阶段不一致。这条分支由控制流可直接确定；本次没有制造该故障，也未证明它在119真实现场发生。最小修复对象是该结果分类／原确认保留，不需要改变队列调度或增加新队列。

删除菜单也允许用户明确“移除已发送记录”（`app.js:8941`），所以不能仅因删除路径没有复用引导的接受拒绝检查，就直接认定它会重复执行或非法删除正式历史。

### D2：派发后 unknown 的引导没有保留网页原回执与防重复状态

位置：

- `assets/remote-access/app.js:5055-5063`：`mutate` 默认 `reliable=false`，只有可靠操作保存 pending；独立追踪集合只有删除会话、创建／关闭侧边会话。
- `assets/remote-access/app.js:9193`：引导未启用可靠回执。
- `assets/remote-access/app.js:5175-5188`：unknown 分支未给该引导动作保留可恢复的原回执身份。
- `assets/remote-access/app.js:9222-9235`：catch 清除临时 `pendingSteer`；finally 清除 Q 的忙标记。`app.js:8918-8924` 的可用判断没有对应的未知引导记录。
- 原生 `native.js:6103-6118` 已将真正派发后无回执保留为 `outcomeUnknown`，但这个状态没有进入上述网页持久恢复链。

确定的源码事实是：该网页入口丢失本次 A 的恢复状态，不能满足 R3-03“恢复只查原 Q/T/A”的闭环；在队列事件／读取尚未收敛时也可能重新显示可操作行。**没有据此认定原生已经重复接受。** 原生锁内的完整历史核验仍有保护（`native.js:6093-6104`；既有测试 `cdp_bridge.rs:1870-1938`）。

同类入口 `app.js:9155、9162-9166` 的删除也没有持久回执，仅显示错误后解除忙状态；它是 R3-04 unknown 的明确未覆盖点，不能拿 begin/save/cancel 的 reliable 恢复通过代替。若后续修复，应复用既有回执保护能力；本次没有新增恢复功能。

### D3：旧停止／失败处理器强制暂停并引导，会误判合法原生策略

位置：

- `scripts/remote-access-e2e.mjs:1018-1019` 固定 `queuedCountAfterStop/Failure=1`。
- `scripts/remote-access-e2e.mjs:2524-2530` 要求停止后原 Q 仍在，随后无条件 `steerRetained`。
- `scripts/remote-access-e2e.mjs:2544-2551` 对失败采用相同要求。
- `scripts/remote-access-e2e.mjs:2486-2493` 没有可用引导就 blocked，并主要按 marker 所在轮次数确认恢复。

这与用例基线 `remote-acceptance-cases-20261101.md:175` 的“实际自动／暂停策略先核原生，不能强制所有失败后都必须点引导”直接冲突。它是验收预期缺陷，不是产品必须暂停的证据。下一轮不能直接把这两个旧处理器的结果当作 R3-06 结论；真实 client、完整 input 和唯一接受需要独立核对。

### D4：后台终端清理未确认仍显示清理成功

`native.js:6884-6891` 已区分停止成功与 `cleanupPending=true`，但 `app.js:11779-11780` 丢掉结果并统一提示“已请求停止并清理此会话的后台终端”。这是确定的反馈失真路径；既有 `cdp_bridge.rs:4670-4719` 只覆盖原生 cleanupPending，不覆盖网页提示。本次无真实终端清理失败样本。

## 4. 候选风险，当前不能判真实失败

| 候选 | 精确位置与事实 | 仍缺什么 |
| --- | --- | --- |
| 自然消费先于 begin/remove，可能落入过期或取出未知 | begin 检查原 client／createdAt／正文及接受状态后才 remove：`native.js:5368-5382、5413-5417`；删除检查存在／queued后 remove：`native.js:4641-4646`。两者检查与原生异步操作之间都有时间窗口 | 同一 Q 的真实出队时间、原生 remove 返回值、ED/token、原 client 唯一接受。当前源码不足证明原生原子删除或本地锁在现场失效 |
| 点停止时 T 已结束，旧请求可能影响后继轮 | 网页只发 threadId：`app.js:11776-11779`；原生调用也是 thread级 `stopThread(id,"user")`：`native.js:6881-6883` | 实际原生 stop 对活动轮的绑定合同，以及点击、派发、T结束和后继启动的真实顺序；不能仅凭未传 turnId 就认定误停 |
| 上游失败后的真实自动／暂停分支 | 本次源码没有网页自行调度／重加原 Q 的证据；停止路径只调用原生停止和该会话终端清理 | 真实 failed T、原生队列状态与后续事件。审批 cancel 的 interrupted 不能代替 failed；HTTP读取失败、手造 JSON 或纯源码夹具都不是上游失败 |

内存回归的边界在 `cdp_bridge.rs:3241-3242` 明确写出：原生函数冻结，但服务端、owner、storage和通知是内存夹具，不连接真实应用。`cdp_bridge.rs:2201-2244` 已覆盖 begin移除后的换连接、快照失败及丢确认保护；前端 `scripts/remote-access-ui-state.test.mjs:672-745` 只对引导成功返回值验证合并刷新。上述均不能填补本表的真实时序。

## 5. 下一条立即执行：带原 Q 的普通停止

执行者为主验收，使用已获准的专用测试会话和独立服务；本子任务未执行。优先复用 M `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，但必须先确认当前属于本轮、原 Q/ED/请求均已结束，不能用21:36归档推定当前状态。

1. 核实际网页和原生源码身份，保存完整历史基线及该 M 的设置／权限；另记一个控制会话 C 的原状态。保留一份不会发送的普通草稿 D。
2. 用一次已获准的短任务形成活动 T，例如仅运行一次 `Start-Sleep -Seconds 20` 后回复 `STOP_SOURCE_DONE`，不读写文件。必须实际看到准确工具仍运行；若取证时已结束，此次停止窗口记 `not_run`，不把后续普通发送补成停止。
3. 活动期间，从网页只发送一次纯文本 Q：`[remote-e2e:R3-06:stop-queue] 只回复 STOP_QUEUE_OK，不调用工具，不读写文件。` 记录 A、原 client、物理 Q 及完整 input，原生明确 queued 后继续。
4. 记录 D 后暂时清空输入以显示原有“停止生成”按钮；只点一次普通停止。保存 `/api/action/stop` 的原请求、回执和前后同 T 状态。不能点审批“取消本轮”替代。
5. 先只读观察原生：若原 client 已自动唯一接受，直接核对其完整 input、T′和终态；若 Q 仍保留，记录原生 pausedReason／submission状态及可用恢复入口，再只对**原 Q**执行一次既有显式恢复；若原生状态不完整则保留 unknown，只读核对。不得再 send Q 或凭等待超时强制引导。
6. 完成后核对原 T interrupted、Q原client至多一次正式接受、input完整、原历史前缀和 C 不变、D可恢复；将停止与后台终端清理分别记录。只有队列与编辑均无未决状态后，再从正常入口发送一条获准短文本，验证没有 ghost 阻塞。不能通过手工删影子或重加原 Q获得通过。

这个步骤直接补 R3-06 的普通停止分支；不会同时宣称上游失败、自然出队竞争或引导unknown通过。

## 6. 随后的最小独立事务

| 场景 | 可以直接执行的动作与成功分支 | 必须保存的额外原件 |
| --- | --- | --- |
| R3-04 自然出队与开始编辑 | 新活动 T中排一条纯文本Q；不暂停 coordinator，不设deferral。在T自然完成边界只点一次Q的“编辑消息”。若begin成功，保存一次预先写定新文；若409过期，停在该结果；若unknown，只查同ED/A | 点击前真实Q；T自然完成／Q接受／begin派发的时间；原ED/token及Q′；最终原client完整input与接受次数。没有真实重叠则记窗口未覆盖 |
| R3-04 自然出队与删除 | 另一个独立Q，在自然出队边界只点一次删除。删除确实胜出时应无该client接受；出队胜出时应明确过期，原client一次接受；unknown不算成功 | 原delete A；原生删除结果或变更已确认标记；完整server/local队列和同client历史，不能只凭行消失 |
| R3-03 派发后unknown | 专用活动T与可引导Q只点一次引导；仅中断这次已经真实派发的响应，并暂缓原A读取。记录网页未知状态后刷新一次；随后恢复GET同一原A，只读核对同T/client。全程不能再次点引导 | 原Q完整输入、点击前T、原生真实派发证据、A及原回包、刷新所属Document、GET同A、事件与最终接受项。若只丢网页回包而服务A已completed，明确只覆盖传输丢回包，不声称原生派发结果长期unknown |
| R3-06 真实上游失败 | 另起独立活动T并加入Q；仅用已有、已核归属的独立测试上游故障入口使原请求失败，然后先观察原生自动／暂停策略，再恢复原Q | 准确失败供应商请求及T、真实错误、故障范围／原状态／恢复记录、失败请求重放计数、Q的完整input与唯一接受。不得用stop、审批cancel、读失败或手造failed代替 |

当前仅定位到 `remote-access-e2e.mjs:2535-2543` 的 `fail-active-upstream` 驱动需求；`remote-access-e2e-environment.mjs:1-3` 明确自身不连接服务、不执行故障。本次未发现可据此认定已经就绪的真实故障驱动／租约原件。**真实上游失败分支在该证据补齐前为受阻／未执行**，不应修改用户供应商配置、重启其原生进程或用45222演练。

## 7. 每次必须同时采集的原生身份

| 原件 | 必须字段／对象 |
| --- | --- |
| 起止运行身份 | 实际Codex版本；targetId／renderer URL；实际加载native.js完整文本及SHA-256、sourceVersion、compatibilityVersion、fixtureId、epoch；网页实际app.js版本／摘要与Document loader |
| 服务与授权 | 精确origin、serverId／channelEpoch、独立服务PID／端口、测试deviceId、写能力；不归档Cookie／Token明文 |
| T与消息血缘 | threadId、点击前turnId、runtimeStatus、真实工具itemId／状态；Q物理ID、client、createdAt、完整input；编辑另记ED、移除token、Q′，引导另记expected／accepted目标T |
| 唯一派发 | 一段从动作前到恢复后的完整HTTP窗口，按requestWillBeSent的network ID去重；Idempotency-Key=A；对应原生动作及供应商请求，不把读取、markRead或投影算新发送 |
| 同一截面的结果 | 原生server队列和local投影分别保存，不能互代；完整历史须说明isComplete／分页读完；pending admission不算正式接受；记录真实结束时间、错误、pausedReason及request集合 |
| 保稿与恢复 | 操作前D及编辑新稿；unknown后本地恢复身份；设置／权限恢复前后完整对象；控制C原历史与状态 |

本次审查的源码快照摘要：

| 文件 | SHA-256 |
| --- | --- |
| `assets/remote-access/native.js` | `d8c635875c50622194fdfebcede393afb540c9263bc44c6a3809231ceca00944` |
| `assets/remote-access/app.js` | `2a03a772aa8c5b2693315528abc801f1514a1e5b4f7dbf6e6094eeaf3575eae3` |
| `crates/codex-elves-core/tests/cdp_bridge.rs` | `f19156520aa47e9999a921457dc57fe720a18617cb8eee34ac21d6f930b90c1a` |
| `scripts/remote-access-ui-state.test.mjs` | `44dc880f766e74dc9775f2e2aa9c38972724a8d3a1a2cb9353934310c9c40b46` |
| `scripts/remote-access-e2e.mjs` | `d8b3461df8b35d6e2be83b990ead905aa20837da9b50105d0926f15285da417e` |

行号对应上述快照；后续修复或其他代理修改后，按函数名和快照重新定位。构建、安装、Android实机、发布及完整三轮仍沿用主报告的独立状态，本次不追加完成结论。
