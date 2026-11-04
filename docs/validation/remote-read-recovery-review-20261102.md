# 原有远程读取、草稿与问答恢复：独立源码复核

日期：2026-11-02，Asia/Shanghai。

**结论：发现三项可由当前源码执行反例证实的时序缺陷，真实浏览器与原生业务验收均未执行。** 本次只复核原有能力，没有修改产品或测试，没有新增审批类型、权限层或恢复框架。`native.js` 头部版本为119；这只表示磁盘源码，不能证明安装程序或当前网页正在运行119。

本次只新建本文件。已读取仓库 `AGENTS.md`、`remote-acceptance-current-20261102.md` 中 R2-01/02/10/12/15、R3-08/15/17 及其直接相关原件；产品检查范围为 `app.js`、`channels.js`、`recovery-store.js`、`events-client.js`，并定向查看 `native.js` 的异步问答身份生成和既有测试。没有浏览器操作、RPC、数据库操作、新模型请求、用户会话写入、提交或推送。

## 1. 证据身份与判定范围

| 文件 | 本次复核时 SHA-256 |
| --- | --- |
| `assets/remote-access/app.js` | `2a03a772aa8c5b2693315528abc801f1514a1e5b4f7dbf6e6094eeaf3575eae3` |
| `assets/remote-access/channels.js` | `b154046c65cd8d4211213db985909aff124f8a7eb7e09fe30ceb4e6959c08127` |
| `assets/remote-access/recovery-store.js` | `ba4b0454f34c88d2253963e1e7fe34204484155fe4d3a21c1e3ac20445d87389` |
| `assets/remote-access/events-client.js` | `8d7810a7a40b08e12dff75d00711882d843b5585a8f2bade51609a1717d5613e` |
| `scripts/remote-access-ui-state.test.mjs` | `44dc880f766e74dc9775f2e2aa9c38972724a8d3a1a2cb9353934310c9c40b46` |

所有行号对应上述快照。主代理仍在处理其他授权修改，后续若摘要变化，应按函数名重新定位。

本次执行 `node --test scripts/remote-access-ui-state.test.mjs`，退出0，42/42通过。另在 Node VM 中直接抽取当前函数、提供内存依赖与可控 Promise，执行以下三个反例，退出0；没有生成测试文件或访问真实存储。这些结果证明指定输入与顺序下的代码行为，**不能填写为真实功能验收通过或119现场事故复现**。

收尾复核时发现共享工作区的 `app.js` 已由其他工作变为 `35443cd446ef9f8c1b01f85e364c9fb744e8032fb2b34c3ce7d2b8f8e2ec2b53`。本代理没有覆盖该修改；再次只读确认，三处关键行为仍在：`sendMessage()` 的成功后无授权核对清稿现位于11740-11747；`applyQuestionReceipt()` 的成功删卡且不推进问答版本现位于10994-11007；`offline` 回调未退役读现位于12576-12580。42/42和VM输出仍归属上表原快照，不自动升级为后来文本的完整回归。

## 2. P1：旧授权的普通发送成功会删除新授权同键草稿

对应 R2-02、R2-10、R3-17。

**适用约束：** 授权由 A 变为 B 后，A 已派发写操作的迟到响应只能解释原操作，不得删除 B 的草稿或更新 B 的会话显示。依据为基线 R2-10 的“旧授权异步响应不得写入新授权”和 R3-17 的“新授权不接旧响应”。

实际调用链：

1. `app.js:5047-5048` 的 `mutate()` 捕获 `selectionVersion`、`deviceId`、`recoveryGeneration`，但这些授权检查只出现在派发前的恢复记录写入阶段（5063-5079）。
2. `app.js:5093` 等待动作响应；成功路径直到5191返回 `result.result`，没有再次拒绝旧授权结果。`channels.js:271-276` 也只对 `read` 取消旧代次；这是已派发写操作不被当作未提交的合理边界，但调用方必须管住显示和草稿副作用。
3. `sendMessage()` 在 `app.js:11724` 等待后，无授权核对便执行11730的队列结果应用、11731的 `clearSubmittedDraft()`。
4. `clearSubmittedDraft()` 在 `app.js:2856` 即使 `current=false`，也会按提交时的会话键检查并删除 `storedDraft(key)`。该函数没有旧授权范围参数；`deleteDraft()` 的持久删除使用**当前** `recoveryScope`（1706-1708、1714-1725）。
5. `selectionVersion` 的变化只能阻止11737之后的导航，以及当前输入框的清空；不能保护2856删除的新授权持久草稿。

确定触发顺序：

1. A 在专用会话 M 发送一次短文本，真实服务已成功，暂缓原 HTTP 成功响应。
2. 同页按正常授权切换流程进入 B，恢复 B 的 M；B 在 M 保存与原输入相同的文字/Skill/图片作为自己的未发送稿。
3. 放行 A 的同一成功响应。旧 `mutate()` 返回成功，`clearSubmittedDraft(..., false)` 匹配并删除 B 的 M 草稿，持久删除也使用 B 的 scope。

源码执行反例输出：

```json
{"case":"late-send-after-authorization-change","currentDevice":"device-b","newScopeDraftExists":false,"removals":[{"key":"M","remove":true,"scope":"scope-b"}],"interpretation":"SOURCE_VM_COUNTEREXAMPLE_ONLY"}
```

同键、同内容是本反例的必要条件，不把它扩大为“任意新稿都会丢失”。普通输入框在发送等待期间仍可编辑，见 `app.js:11580-11581`，所以不能以全局 `busy` 认为新稿不可产生。

最小修复：在普通发送入口保留原 `privateInputScope(session)` 和 `recoveryGeneration`；每个 `await mutate()` 后，在队列结果、草稿清理、导航与提示之前核对原授权仍然有效。旧操作的 `finally` 也只释放自身的 `sending`/`busy` 所有权，避免以后清理新操作。无需重发、切换传输或增加新权限层；原动作仍按原回执解释。

现有测试缺口：问答专项的换授权测试（`remote-access-ui-state.test.mjs:372-386`）只覆盖问答 GET 和旧回执ID，没有覆盖普通发送成功后的草稿清理。

真实验收限制：该例需要两个**独立专用授权**，两个会话本身不能替代授权变化。本次没有获取、撤销、复用或改写用户 Cookie；若专用授权受设备上限阻止，保留为受阻，不操作用户原授权。

## 3. P1：问答成功后，已经在途的旧读会复活题卡并移除重复提交锁

对应 R2-01、R2-12、R3-08；与 R3-09 的未知回执收敛也有关。

**适用约束：** 准确回答已确认成功后，回答前开始的旧读不得将该题重新作为可提交问题展示。依据为 R2-11/12 和 R3-08 的“旧读取不复活题”“已答/跳过题不复活”；不能仅用第二次读最终收敛代替该约束。

实际调用链：

1. `reloadThread()` 在 `app.js:10275` 捕获 `questionVersion`，开始读取含待答 Q 的会话快照。
2. Q 的原回答回执先到，`applyQuestionReceipt()` 在 `app.js:10985` 清除 `questionReplyReceipts` 和问答草稿，10988-10989从当前 `thread.requests` 删除 Q。
3. 这条成功路径没有推进 `questionVersion`、更新 `notifiedQuestions` 或使回答前的读取失效。
4. 成功路径10991调用 `reloadThread()`；若旧请求仍在途，10265-10266仅设 `again=true`，继续共用旧 Promise。
5. 旧快照返回时，10293-10309的选择/前台/读取版本均仍匹配，`questionReadVersion === questionVersion`，因此旧 Q 被装回 `thread.requests`。
6. `renderAsyncQuestion()` 的锁取自 `questionReplyReceipts` 或请求 `submissionState`（11059-11062）。回答前快照的该状态为空，原锁已经删除，所以中间一帧会显示可提交的已答题；下一次新读完成后才会清除。

必要触发条件：异步问答状态事件尚未到达或事件流中断。若 `remote/asyncQuestionsChanged` 的空快照先到，12089-12097会推进 `questionVersion`，可挡住这个反例；本次没有把正常事件先到的路径报错。

源码执行反例输出：

```json
{"case":"answered-question-resurrected-by-inflight-read","questionsBeforeFreshRead":["Q"],"locked":false,"questionVersion":0,"calls":2,"frames":[{"questions":[],"locked":false},{"questions":["Q"],"locked":false}],"interpretation":"SOURCE_VM_COUNTEREXAMPLE_ONLY"}
{"case":"after-followup-read","questions":[],"interpretation":"SOURCE_VM_COUNTEREXAMPLE_ONLY"}
```

该反例未声称服务端会重复接受回答；服务端仍可能准确拒绝已处理 Q。已证实的是 UI 会把原生已确认成功的题重新当成可答，导致用户看到复活题卡并可能发起第二次回答请求。

最小修复：成功回执在清锁时，同步推进现有问答版本，并将当前已知问题快照更新为“只移除原 Q、保留其他题”；让回答前的 thread 读取按现有版本合并保留该终态。还须覆盖选中 M 但初始 thread 尚未返回的分支，不能只以 `thread?.id === M` 决定是否使旧读失效。可以沿用当前版本和请求所有权字段，无需增加新的恢复框架。

现有测试缺口：`remote-access-ui-state.test.mjs:303-327` 将 `reloadThread()` 替换为立即完成的记录函数。其“完成后不清后来的问题”测试只看同步删卡结果，没有执行“旧 thread 读取 → 回执成功 → 旧读迟到 → 补读”的真实函数组合。

## 4. P2：断网再联网未退役显示读取，自动恢复继续等待原请求

对应 R3-15，并关联用户前台恢复后的读取等待场景。它与已修复的 `visibilitychange` 后台代次问题分开。

**适用约束：** 已明确经过离线/联网恢复后，原显示读取不能阻止新一次恢复读取；草稿、已显示内容和已经派发的写操作仍需保留。依据为 R3-15 的自动恢复要求及既有 `retireForegroundReads()` 的职责。

实际调用链：

1. 一次 `refreshAll()` 或 thread/history 读取在途，尚未完成。
2. `app.js:12560-12564` 的 `offline` 处理只停止目录刷新和改变连接显示，没有调用 `retireForegroundReads()`，也未推进 `foregroundReadGeneration`。
3. 联网12557-12559调用 `retryConnectionRecovery()`；12505再次请求 `refreshAll({ fresh:true })`。
4. `refreshAll()` 在12279-12281看到同一设备、同一前台代次，设置 `again=true` 后返回旧 Promise；`reloadThread()` 与 `loadHistory()` 也按同一代次共用旧请求。
5. 新读取因此只能等旧请求完成或到期后发起。`app.js:4489` 的普通 `/api/read/` 超时为60秒；实际 fetch 若已立即失败则不会命中该等待，本结论只适用于离线后仍悬挂的旧读。

源码执行反例调用实际 offline/online 回调和实际 `refreshAll()`：

```json
{"case":"offline-online-reuses-pending-display-read","generation":0,"readsBeforeOldReadFinished":1,"again":true,"interpretation":"SOURCE_VM_COUNTEREXAMPLE_ONLY"}
{"case":"after-old-read-finished","reads":2,"interpretation":"SOURCE_VM_COUNTEREXAMPLE_ONLY"}
```

最小修复：在现有 `offline` 回调中复用 `retireForegroundReads("网络已离线")`，取消并释放旧显示读取；不触碰在途写操作与未知回执。联网仍走原 `retryConnectionRecovery()`。无需新重试器或新超时参数。

现有测试缺口：`remote-access-ui-state.test.mjs:878-949` 已覆盖手工调用 `retireForegroundReads()`、hidden/visible 和写请求保持，但没有执行实际 offline/online 回调，因此42项全通过仍不能排除这个调用入口缺口。

## 5. 两个现有专用测试会话的真实验收顺序

以下只给出可执行步骤，**本次未执行**。已有原件 `dist/validation/remote-performance-20261102/followup-118/final/verification.json` 确认两条会话为专用远程测试对象，且真实模型、用量不同：

- M：`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，input-integrity 专用会话。
- C：`01a1996d-0b7a-71d0-b9ad-9c521d224ad9`，queue-status 专用会话。

上述原件是118截面，本次没有宣称其当前仍为空闲或当前用量未变。执行前先核对当前网页/原生源码身份和这两条会话的活动、队列、请求及完整历史。凡需新模型请求的场景，在获得本轮明确业务操作授权后才执行；当前只读检查不产生请求。

| 优先级 | 真实操作 | 必须记录的独立结果 | 能补齐的范围与边界 |
| --- | --- | --- | --- |
| 1 | M/C各保存不同的两行文字、Skill位置、两图顺序和目标标签草稿，不发送；M→C→M，再刷新。将一次旧 C 读取延迟到成功返回后才放行 | 当前ID/正文/模型/上下文始终属于M；复合草稿逐项一致；send/create/setGoal/answer均为0；旧响应 `loadingFinished` 或等效成功送达记录 | 补R2-01/02的无写分支；`ERR_ABORTED`只能证明取消，不能替代迟到成功响应被忽略 |
| 2 | 保留M草稿，暂缓一份已发出的显示读；只对专用tab断网再联网，全程不刷新，观察是否立即开始新读 | 原请求的开始/取消/结束、新读开始时间、原请求身份；草稿与原写回执保持；联网后是否仍共用旧读 | 验本报告第4项及R3-15；如果实际断网立即终止旧读，应记录未命中悬挂条件，不能算该竞态通过 |
| 3 | 有真实待答异步题Q时，只延迟当前thread成功响应并暂断该tab的事件投递；Q正常提交一次或核对原已成功回执，放行旧读，再延迟补读 | 精确thread/turn/item/question/request、原answer及原receipt各一次、原生成功、旧读前后题卡和可提交状态；send/answer总次数不因恢复增加 | 验本报告第3项及R3-08/09；无真实待答题则保持not_run，不用模拟问题或普通文本代替 |
| 4 | 在M/C中形成相似题的不同真实turn/item/entity；只放行第一题的旧完成事件，保留第二题未提交选择 | 新题不关卡、不继承答案或锁；普通M/C草稿不变；逐题原生身份与事件完整对应 | 补R2-12。不同标题或两张截图不能代替准确实体；没有第二真实实体则该分支未验 |
| 5 | 仅在独立专用授权A/B已存在时执行第2项发现的迟到发送授权切换序列 | A原HTTP/receipt和B的device.id/createdAtMs；B同键新稿保存成功后放行A回包；B稿刷新后仍保留，原send只有一次 | 验本报告第2项及R2-10/R3-17；两会话不等于两授权，禁止借用户授权/Cookie填补测试条件 |

存储失败与真实坏缓存继续独立留项：只在专用存储scope制造一次失败，核对未发送图文仍在、提交前可靠保存失败时动作请求为0、恢复后图片未被静默删除。当前源码的 `mutate()` 对大记录先持久保存、失败则不提交（`app.js:5060-5088`）；小记录允许明确提示后使用 sessionStorage。不能把“所有IDB失败都必须禁止纯文字提交”当作当前产品规则。

## 6. 已排除的泛化结论与剩余边界

- 正常M→C切换的旧thread/history响应已有 `selectionVersion` 与ID约束；后台退休还会推进 `foregroundReadGeneration` 和 `threadReadVersion`（`app.js:12394-12405`）。本次未找到“任意旧会话成功读取都会覆盖当前会话”的通用缺陷，不能用这一泛化结论要求重写全部读取。
- tokenUsage通知在当前ID分支推进 `tokenUsageVersion`；selection/reload两处在应用旧读前恢复新通知值。`contextUsage()` 区分真实0与missing，也允许降低（`app.js:3032-3037`、9834、10295、12134-12140）。R2-15仍缺真实反序/零/missing验收，不能从静态代码升级为通过。
- `recovery-store.js` 已核对内存owner与IDB owner；`restorePrivateInputs()` 以设备ID和创建时间构成scope，并在读取后核对授权代次。不能把第2项写回调缺口扩大成整个IDB存储没有授权隔离。
- `ChannelEventMerger` 拒绝已退休epoch与非当前epoch序列（`channels.js:593-627`），`events-client.js` 关闭时移除端口和事件回调。当前没有充分反例证明“所有旧epoch事件都能入页”。配置revision变化与服务epoch变化的真实双路切换仍须执行R3-15，不能靠VM替代。
- `createRecoveryStore.list()` 的坏图片解包会使整次list失败，`restorePrivateInputs()` 的四项 `Promise.all` 会一起失败（`recovery-store.js:130-140、211-226`；`app.js:2696-2699`）。这是R3-17值得验证的故障耦合线索；本次没有真实损坏记录，也没有证明现场丢失过原回执，故不列为第四项已证实业务缺陷，不据此增加新恢复框架。
- 问答未提交答案只在当前页面内保存，刷新不要求恢复问答选择；这是基线追加定义（`remote-acceptance-cases-20261101.md:335-342`）。不能误报为刷新丢问答稿；普通图文/Skill/目标草稿的持久恢复是另一项要求。

三项源码反例之外，前台真实生命周期、双路channel切换、同scope多tab、真实坏缓存/存储拒绝、撤权API和复合草稿真实恢复均仍未验。本报告没有更新73条基线状态，也没有把现有42/42源码测试写成用户侧验收完成。
