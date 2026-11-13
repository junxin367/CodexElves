# 远程会话真实 E2E 回归

**2026-11-04 用户纠正侧边命名规则：** 原文档中“不从首条消息正文改名”是错误规则；同日用户将自动命名上限从 10 个字符调整为 15 个字符。默认名可由首条有效用户文字在合并空白后取前 15 个 Unicode 码点自动生成，写入并确认同一原生标签后同步远程；已有自定义名不得被自动命名或后续消息覆盖。完整规则见 [侧边会话需求](remote-session-tools.md#侧边会话)。以下早期撤销十字符断言或待裁定的记录只描述当时验收状态，不能再作为当前产品规则；本次文档更正不增加任何验收通过结果。

2026-11-02用户明确：本轮只恢复原有远程能力。文件审批记为能力缺失，排除主动开发及必做验收；`scenario.approval`继续验证已有命令审批，不据案例名称扩展文件审批。已撤回本轮新增的文件审批展示及专用测试。

`scripts/remote-access-e2e.mjs` 是用户要求保留的长期回归资产。当前 2.5.10 包含 **73 个已列案例和处理器**；处理器与 73 条业务基线不是一一对应，数量相同不代表完整覆盖。功能覆盖与预期以 `docs/validation/remote-acceptance-cases-20261101.md` 为基线，子智能体等新补业务用例尚需逐项接入并真实执行。导入模块不启动环境、不认领浏览器、不发送消息；模块只接受调用方已经通过 `chrome@openai-bundled` 明确绑定的真实 `tab`。

用户在 2026-11-03 至 11-04 的手动反馈及精确复现顺序见[手动反馈与防回归清单](validation/remote-user-feedback-20261103-20261104.md)：包含手机目标交互、旧消息/发送禁用恢复、侧栏异步关闭、提示时机、队列耗时、侧边模型往返和前后台连接诊断。后续相关修改按清单选择对应子场景，区分代码、真实验证、部署和真机状态；`UF-*` 仅为反馈索引，不代表新增处理器或整套 E2E 已通过。

案例实现、源码测试、语法检查和模块导入，只证明回归资产可用。没有本轮 Chrome、Codex 桌面、故障环境或 Android 设备的原始证据时，相应案例必须是 `blocked` 或 `not_run`，不能宣传完整 E2E 通过。旧 1.0 报告不能继承为 2.0 的通过结果。

恢复报告时，案例的编号、分组、标题和必测标记以当前源码定义为准，原执行状态、尝试、证据和动作记录保留。整体状态也按源码必测编号汇总：报告缺项按 `not_run` 处理，修改报告中的 `required` 不能隐藏未完成案例。严格原件校验仍独立执行，汇总的 `passed` 不代替该校验。

## 执行边界

- 每轮使用新 `runId`。新会话、消息、草稿和专用项目均带该标记。只更改本轮通过 UI 创建并登记的对象；明确接入的外部对象也必须是同轮新建，有原始创建回执、准确 thread ID 和可见原始消息标记。
- `evaluate` 只读 DOM。变更通过真实点击、填写、键盘或明确的外部 UI 驱动完成。不得调用页面应用函数、直接发业务 API、改存储、mock 响应或注入成功状态。
- 所有发送检查显式授权和剩余预算。默认短消息只请求校验词；长活动轮、停止、未读、审批、问答、目标、Skill、图片与优化消息由用户明确批准。所有消息仍追加唯一标记。
- 不能覆盖非本轮草稿，不能用用户已有会话测试发送、排队、停止、置顶、改名、关闭或删除。专用项目必须由本轮桌面 UI 创建；名称、路径均包含 runId。
- 自动排队案例从排队完成后只读等待，不设置 `deferAutomaticTurns`、暂停自动轮次、点击引导或重新发送。队列边界和并发案例同样不能用暂停、重排队或模拟消费构造通过。
- 每次服务动作在点击前写入稳定 actionId，先记 `unknown`。任何结果不明都禁止盲目重试。只有原始回执核对成功才确认；`recovery.unknown_receipt` 仅点“查询此消息的送达回执”，不会第二次调用发送。
- 永久删除与不可恢复的侧边关闭保留动作当时的人类批准门禁。普通授权、初始消息、恒定 `true` 均不替代具体对象预览后的批准。
- 不自动删除真实测试会话或项目。报告保留遗留对象。只读检查不触发构建、安装、发布、提交或推送。

默认输出为仓库 `temp/remote-access-e2e/<runId>/`。CUA 运行目录可能不同，实际调用传入仓库内绝对 `outputDir`。输出含可见会话内容和本机路径，仅保留在本机，不提交。URL 中名称含 key/token/auth/secret/password 的凭据参数会遮蔽。

## 覆盖矩阵

| 分组 | 案例 ID | 具体执行与通过条件 |
| --- | --- | --- |
| 起步与版本 | `read.ready`、`read.runtime_version` | 真实连接与控件；原始版本 JSON、实际安装程序和网页资产文件哈希同时匹配 |
| 普通会话 | `session.create`、`read.select_refresh`、`session.send_stream`、`session.stop` | 创建独立 main/control；切换、刷新；实际流式变化；停止原生轮次后可编辑，均按 ID 和消息标记核对 |
| 模型 | `session.model`、`session.model_effort_isolation` | 两个实际模型/深度设置，main→control→main 各发送一轮；核对每个真实 turn 的 modelId、protocol、reasoningEffort，不只看按钮 |
| 队列常规 | `queue.automatic`、`queue.edit_draft`、`queue.edit_save`、`queue.reorder`、`queue.remove`、`queue.steer` | 两条消息按 FIFO 自然承接；取回后作为普通未发送草稿，不显示编辑栏和取消按钮，未发送不恢复队列，重新发送回到原位置；再核对修改、键盘排序、删除及引导 |
| 队列边界 | `queue.after_stop`、`queue.after_failure`、`queue.approval_wait`、`queue.compaction` | 停止/真实上游失败后保留原队列，明确引导恢复；审批/压缩期间不抢先消费，结束后只消费一次 |
| 队列并发与恢复 | `queue.dequeue_edit_race`、`queue.dequeue_remove_race`、`queue.cross_session` | 等到真实原生出队窗口再操作；接受明确过期拒绝，不接受未知结果；原对象只能删除或消费一次；跨会话刷新保持同 ID |
| 侧边命名六项 | `side.default_name`、`side.first_message_name`、`side.same_id_name`、`side.desktop_created`、`side.desktop_rename`、`side.custom_name_refresh` | 首条消息前按同一真实 thread ID 核对原生默认标签，不写死默认语言或编号；默认名且首次出现有效用户文字时，按合并空白后的前 15 个 Unicode 码点计算期望，必须同时核对原生 tab 写入和远程列表、详情及刷新；已有自定义名不套用十五字符限制，两次连续改名分别留证并验证后续消息保持 |
| 侧边基础 | `lifecycle.side_create`、`lifecycle.side_switch`、`lifecycle.side_refresh`、`side.multiple` | 真实创建并首次发送标记；主侧往返；侧边刷新；两个不同侧边按 ID 独立切换且父子关系一致 |
| 侧边生命周期 | `side.late_creation`、`side.close_running`、`side.delete_parent`、`lifecycle.side_close` | 延迟实际创建响应后不抢占控制会话；运行中关闭经预览批准且终止执行；删除单独新建父会话后无孤立/活动侧边；普通关闭刷新后保持消失 |
| 已读 | `read.main_read_state`、`read.side_read_state`、`read.side_aggregate`、`read.cross_client` | 两轮真实新回复经历未读→阅读→刷新保持→再次未读；多侧边逐个清除聚合；Codex 阅读同步远程，下一轮再次未读 |
| 项目与会话元数据 | `project.pin_unpin`、`project.search_preferences`、`lifecycle.pin`、`lifecycle.rename` | 本轮专用项目置顶和取消并刷新；搜索指定项目，切换本客户端最近/项目视图并刷新保持；会话置顶；Codex 改名同步远程。项目手动顺序属于另一类原生数据，不能用修改桌面偏好代替客户端偏好验收 |
| 输入 | `composer.multiline`、`composer.image`、`composer.skill`、`composer.edit_latest` | 多行草稿切换刷新后逐字发送；真实系统图片选择器、预览、原生附件哈希；当前项目 Skill 的真实选择和原生输入；最新已发消息取消编辑、修改、单次重发及刷新 |
| 提示词优化 | `optimizer.restore`、`optimizer.cancel`、`optimizer.late_result`、`optimizer.no_injection` | 真实优化后恢复原文；pending 时取消；后来编辑及跨会话草稿不被迟到结果覆盖；最后一项专指**无 Codex 原生注入且 CLI 服务也不可用**时独立优化/取消/恢复，显式只读授权仍禁止写入，结束恢复环境 |
| 上下文 | `context.switch_update`、`context.zero_missing` | main→control→main 不串值，新实际 turn 更新最后请求用量；两个同轮专用真实会话分别提供零值和缺失原生字段，验证 0% 与“—”。不得改写 tokenUsage 制造夹具 |
| 通道 | `transport.native`、`transport.cli_fallback`、`transport.native_takeover` | 正常原生发送；真实断开 CDP 后仅合资格会话走 CLI，不使所有会话只读；恢复后原生接管，CLI 原消息不重放 |
| 恢复 | `recovery.reconnect`、`recovery.native_restart`、`recovery.unknown_receipt`、`recovery.draft` | 真实断线重连、原生重启后身份/轮次/草稿保持；只核对原未知回执；主侧与控制会话的文字/图片草稿切换刷新保持 |
| 回答回执恢复 | `recovery.question_receipt` | completed 多题刷新后单次回答；只丢准确回答回包并暂阻断原回执读取；新文档实际 GET 原 ID，原锁消失、删除入口恢复；两会话草稿及旧完整历史保留，无重答或额外业务写入 |
| 原生特殊场景 | `scenario.approval`、`scenario.question`、`scenario.question_completed`、`scenario.compaction`、`scenario.goal` | runner 真实发起并点击具体审批、填写获准回答、压缩已有上下文、创建获准目标并观察执行；completed 问答先取得原轮 completed 和原生未答状态，再刷新/回答/刷新核对，不能在原轮结束前抢答；已移除返回 passed 的自定义 scenarioHandlers |
| Android | `android.installed_version`、`android.background_resume`、`android.back_navigation`、`android.notification`、`android.resource_update` | 读取实际安装 APK/签名/资源；填写设备草稿后后台/恢复；浮层先关再返回父会话；点真实通知按 ID 打开且跨端已读；真实资源更新重载及失败后保留上一个有效版本 |
| 永久删除 | `lifecycle.delete_preview`、`lifecycle.delete_confirm` | 最后才删除 control，避免破坏其他案例前置；预览、取消、动作当时批准、真实 deleted 回执及刷新消失 |

13 个 suite 均可单独选择。单独执行不会自动创建前置对象或扩大授权。顺序见导出的 `REMOTE_ACCESS_E2E_CASES`：侧边默认名在首次发送前检查，关闭在已读/草稿案例后，control 永久删除在全部依赖案例后。

覆盖边界：当前矩阵没有子智能体列表/只读详情或变更/文件页面的专用处理器，不能用侧边案例或菜单入口存在替代。`session.model` 也不替代 UF-09 的“侧边成功发送新模型后往返、刷新及不重选续发”。已有目标的暂停/开始/删除按 UF-03 另核，不能从 `scenario.goal` 等待完成推导已验。浏览器入口需要 FRP 线路及受支持的 Android 原生能力，普通 Chrome 页面不提供该原生能力；App 缓存是客户端恢复机制，不是单独的工具菜单页。现行功能与权限边界见 [远程会话工具](remote-session-tools.md) 和 [远程访问](remote-access.md)，本段不增加执行或通过状态。

临时侧边的权威名称来自同一 thread ID 对应的原生 `tab.title`，证据字段为 `nativeTabThreadId/nativeTabTitle`。`getConversation.title`、thread.name 可以为 null，不能强制通过不支持临时会话的元数据 RPC 改名，也不能只看远程乐观状态。`side.first_message_name` 不发送新消息，使用 `lifecycle.side_create` 已确认的首条消息动作及其准确会话中的完成回复作为起点；若发送前仍为默认名且该消息含有效文字，须独立按产品规则计算前 15 个 Unicode 码点，并核对其已写入同一原生标签。发送前已有自定义名时，验证原名保持；纯图片、空文字、待提交或压缩输入不产生自动名。仅证明远程与原生同名不足以证明自动命名规则正确。`rename-side-tab` 驱动使用真实原生标签的命名能力；普通主会话改名使用 `rename-thread`。

每个命名阶段执行两次独立的 `desktop/observe-thread`，原始 `capturedAt` 至少间隔 1000 毫秒且 `nativeTabTitle` 相同后才继续。这个间隔是验收采样窗口，不是产品命名规则。每条原始观察必须同时包含 `threadId/parentThreadId/nativeTabThreadId/nativeTabTitle/visibleThreadId/visibleTitle/surface/capturedAt`，其中 `threadId/nativeTabThreadId/visibleThreadId` 均指向当前准确侧边，`parentThreadId` 指向其真实父会话，`visibleTitle` 与同条记录的 `nativeTabTitle` 一致。字段仍使用已有原始 JSON Pointer，不新增由驱动自报的 `stable` 或 `passed` 字段；标题必须来自实际原生 tab，不能从 `request.expectations` 或远程标题复制生成。

runner 将详情、父会话菜单中的唯一同 ID 行、整页刷新后详情分别保存为 DOM、截图及带哈希的 `files.observation` 原始只读观察 JSON，随后再做一次新的原生标签读取。`side-name` 阶段记录关联这三份原生读取和前后界面文件；默认名另须证明首次发送前没有 turn。离线门禁从文件重新读取，核对 ID、标题、读取身份、消息完成起点与时间顺序，并确认每份采样关联本阶段的真实 driver 步骤。同名其他 ID、旧读取复用、缺少刷新证据、刷新后回到旧名均失败。自定义名必须与对应原生改名动作的原始结果一致，后续消息不能覆盖；连续两次改名的阶段不得交叉复用。

`scripts/remote-access-e2e-evidence.test.mjs` 的既有正反例允许原生长摘要、其他语言及首条后名称未改变，仅能证明名称同步门禁，不能直接作为默认名自动生成场景的合格判据。自定义名场景不受自动命名的十五字符上限限制；自动命名场景须另行核对默认名前提、有效文字、前十五字符算法及真实原生写入。错误 ID、旧时间、重复 read ID、不同阶段拼接、报告伪改及只报 `passed` 仍不能通过。既有执行器和门禁的算法断言需另行补齐，本次仅修正文档，不改测试代码或通过状态；离线契约回归不能替代真实命名 E2E。2.3.0 及更早的命名通过结果不能继承到 2.4.0。

## 真实 Chrome 起步

### 登录及写入前的两项门禁

1. **先隔离浏览器授权，再登录测试服务。** 不同端口并不隔离同一主机下的Cookie。本产品使用 `codex_elves_device; Path=/`，在共享Chrome配置中登录另一个 `127.0.0.1` 或 `localhost` 端口，可能覆盖用户原入口的登录；临时服务退出也可能清除它。优先使用独立浏览器存储或仅属于本轮、且服务确实允许的访问主机名。必须复用主机名时，在登录前通过该浏览器支持的接口保存原Cookie及公开设备身份，确认能恢复后再继续；Cookie原值只放在本轮内存中，不写入报告、日志或源码。没有可核实的隔离或恢复方案就不执行登录。
2. **先确认实际加载资源，再写入。** 服务端文件与源码相同，不能证明页面执行了新版。必须记录同一浏览器Script请求、原始响应正文及摘要，并通过 `read.runtime_version`；新版Service Worker处于waiting时，按产品既有关闭重开流程启用，不能拿旧页面结果算新版通过。

收尾先退出临时服务的测试授权，再恢复原Cookie；随后核对用户原入口的设备ID、权限和可访问状态。这里的环境恢复不能用于伪造登录成功、改写应用业务存储或替换业务响应。复用另一条既有授权只能记作“等价访问恢复”，不能报告原授权身份已恢复。仅关闭标签、只比较端口、只记录测试服务PID或只看到登录成功页面，都不能证明授权隔离。

由外层按 Chrome 插件文档绑定 `tab`，再通过 `pathToFileURL` 得到脚本绝对路径。模块接受现成 tab，不发现浏览器、读登录密钥或启动服务。

```js
const remoteE2E = await import(scriptFileUrl);
const ctx = await remoteE2E.createRemoteAccessE2EContext({
  tab, targetOrigin, runId, outputDir, versionEvidence,
  onProgress: value => nodeRepl.write(value),
});
await remoteE2E.runRemoteAccessCase("read.ready", ctx);
await remoteE2E.runRemoteAccessCase("read.runtime_version", ctx);
```

要执行写入时在创建 context 时提供明确授权：

```js
authorization: {
  allowCreate: true, allowMessages: true,
  maxMessageSubmissions: approvedMessageLimit,
  allowQueueChanges: true, allowSideSessions: true,
  allowSessionMetadata: true, allowDrafts: true,
  allowDesktop: true, allowFaults: true,
  allowAttachments: true, allowProjects: true,
  allowOptimizations: true, maxOptimizations: approvedOptimizationLimit,
  allowApprovals: true, allowAnswers: true,
  allowCompaction: true, allowGoals: true,
  allowAndroid: true, allowResourceUpdates: true,
}
```

只打开本次批准的项。完整 73 项比旧 27 项有更多真实调用，不沿用旧例子的 17 次预算；按所选案例明确批准上限，达到上限即阻塞，不追加调用。优化有独立额度：restore/cancel/late_result 各一次，no_injection 两次；goal 及修改已发消息也计消息调用。`recovery.question_receipt` 预留两次消息额度，分别在提问和回答点击前扣除。多侧边聚合使用三次未读消息；其他已读单项使用两次，Android 通知使用一次。活动窗口太短、取消/并发窗口没捕获时保留 `blocked`，不为通过自动付费重试。

### 排队草稿与重新发送

2026-11-13 用户要求与 Codex 的排队编辑一致：点击编辑后消息离开待执行队列，输入框使用普通发送入口，不显示“取消编辑”按钮、编辑说明或保存图标。不再次发送时，该条消息不执行，也不阻塞后续队列；再次发送时保留原位置，已经执行的其他消息不回退。

2.5.10 用 `queue.edit_draft` 替换原 `queue.edit_cancel` UI 案例。新案例沿用只读原生驱动 `observe-queue-edit-cancel` 的 `before`、`editing` 两阶段，核对原输入、准确编辑标识和位置；实际改稿、刷新后仍须保持未发送，再通过普通“发送消息”按钮放回队列并核对顺序。`queue.edit_save` 同样使用普通发送按钮，并等待真实编辑器值变化后才提交。以下旧取消合同仅供历史报告兼容，不能作为当前界面的操作说明。

### 历史队列取消合同

`queue.edit_cancel` 原先已经调用真实 `fill` 填入“原文＋取消编辑的临时文字”；2.5.4及证据门禁1.3.2补的是实际值留证、原生ED与完整恢复条件。不能仅凭DOM摘要没有临时文字、队列预览相同或`canceled`布尔判通过，也不能继承旧版报告。

执行需开启`allowQueueChanges`和`allowDrafts`，main/control必须有本轮归属。两端原普通草稿分别保存；空稿才通过UI填写本轮不同标记，已有本轮稿不覆盖，其他来源草稿使案例受阻。取消后比较原稿，返回主会话后必须`loading=false`、编辑器可用、没有编辑/待确认锁且发送按钮实际可用。验证完成后只通过UI清空本例从空稿创建的测试稿，保留既有普通稿，避免干扰后续案例。

原生只读驱动须声明并实现`observe-queue-edit-cancel`，依次返回三份不同读取ID的原始观察，`rawFacts`指向实际文件字段。缺少该驱动时在创建活动轮、发送或入队前受阻：

| phase | 必需的原始事实 |
| --- | --- |
| `before` | 当前`threadId`及按原生顺序排列的`queuedMessages`；每项含准确`id/clientUserMessageId/input`，`input`是原生完整序列化输入，不能从预览或request复制 |
| `editing` | 取出事务的`editId/editState=editing/originalMessageId/originalClientUserMessageId/originalInput/originalIndex`，与before原对象、完整input和原索引相等；不等时不得继续取消 |
| `after` | 同ED的`editState=canceled`、真实`cancelReceipt`（外层`requestId/state`及原`result`）、最新完整`queuedMessages`、`nativeTurns/historyComplete=true`；取消回执绑定原thread/ED与恢复物理ID |

三份观察另带已有标准的`runId/caseId/origin/sourceFingerprint/operation/phase/capturedAt/readId`原始字段，operation固定为`observe-queue-edit-cancel`。恢复物理ID可改变，但逻辑client与完整input不变，取消回执的新物理ID必须对应同一逻辑对象；未编辑项的物理ID、完整input和相对位置也须保持。历史中已消费或已引导的同client不能仍作为恢复队列通过。

runner在取消点击前单独落盘`*-queue-cancel-editor.json`：`threadId/editId/capturedAt/value`中的value取自真实contenteditable；同时保存带哈希的原始observation、DOM和截图。value必须等于真实填写后的不同正文；ED来自该阶段原生事务，时间必须早于取消派发。DOM摘要即使省略contenteditable值，仍按原始JSON实际值核验，不能跳过该文件。

本处理器固定验“原对象仍在忙时队列中恢复”的正常子场景。原对象在取出前或取消后已自然消费时记录受阻并保留原现场，不再次创建、取消或重发补过；空闲取消后立即消费、真实unknown、出队竞争、暂停/send-only、附件和隐藏输入另作专项，不能由本处理器扩为整条基线通过。报告门禁按固定case ID强制三阶段及编辑器文件，删专项记录、把requiresQueueEditCancel改为false或只提供成功状态均无效。

`scenario.question_completed` 使用独立的 `messages.completedQuestion` 与 `completedQuestionUI`；消息应调用真实异步问答工具并结束原轮，不能维持长轮等待以避开缺陷。驱动须提交准确的原 turn、item、entity 和问题 ID，第一次及刷新后的 `completed/lastSubmission=null/isSkipped=false` 原始证据；回答结果还须对照同题的已接受消息及原生逐题状态。缺这些条件必须受阻，不得借用普通问答用例结果。

问答最终回复不使用普通发送的“含原提问标记的轮次”判断：原轮已结束时，原生协调器可以创建新的回答轮。runner 记录提交前轮次集合，限制准确会话中的唯一回答候选，并将 `originatingTurnId/answerTurnId` 一起交由原生证据确认。活动轮允许回复仍在原轮；旧轮、错误会话、失败或临时占位不算完成。

### 回答回执恢复的独立合同

`recovery.question_receipt` 对应 R3-09 的“成功回答但回包丢失”子场景。沿用获准的 `messages.completedQuestion`，`completedQuestionUI.fields` 需至少两项 `{question,label,value}`：`question` 是原题准确标题，`label` 是真实输入框的可访问名称，`value` 是已批准答案。它不包含 failed/404、换授权、后台或双端竞争验收；这些分支不能沿用本例通过结果。

该处理器要求原生与环境驱动提供以下固定步骤。缺少任意能力时在提交前受阻，不能把旧人工记录改写为本轮通过：

| 驱动/阶段 | 真实动作或原始事实 |
| --- | --- |
| environment / `start-question-recovery-observation` | 在提问前开始该授权 tab 的只读网络记录；纳入同一 `receipts` 租约，结束撤回观察与拦截 |
| native / `observe-question-recovery`，`before` | 原始 `thread/read` 请求（`includeTurns=true`）、完整轮次、空队列 |
| native / 同操作，`pendingInitial`、`pending` | 提问轮 completed；未答题刷新前后相同 request、turn、item、entityKey 及逐题状态 |
| environment / `drop-question-answer-receipt` | 仅匹配目标 thread 和原生 question request 的下一次 answer；读取真实 Idempotency-Key，保留服务执行、丢弃该响应，并暂阻断该 ID 的回执读取。不能阻断其他操作、伪造 unknown 或伪造响应 |
| environment / `observe-question-recovery`，`unknown` | 同回执的真实服务端 completed/ok 响应、客户端 unknown 记录、设备 `id+createdAtMs`。没有捕获 unknown 时不得补造或重答 |
| native / `observe-question-recovery`，`answered` | 原生回答轮已 completed，完整历史和逐题接受状态；必须在 unknown 删除菜单取证之前读取，不能借用恢复后的终态解释此前禁用 |
| environment / `release-question-receipt-on-reload` | 只在该 tab 的新文档开始后放开原回执读取；提前自动查询成功只能算即时恢复，不能算本例通过 |
| native / `observe-question-recovery`，`history` | 恢复后完整历史、空队列、同实体逐题 `lastSubmission` |
| environment / `observe-question-recovery`，`delivery` | 覆盖提问至末次草稿检查的完整 CDP 网络事件和实际 GET 响应正文；当前客户端回执元数据及设备身份 |

七份观察按 `before → pendingInitial → pending → unknown → answered → history → delivery` 留存，其中原生读取五次、环境读取两次。每份使用 `rawFacts` 指向实际原始字段，并带不同读取标识、phase、时间、origin 和源码指纹。完整历史阶段提供 `historyRequest/nativeTurns/queuedMessages`；pending 阶段提供 `requestId/originatingTurnId/nativeQuestionStates`；unknown 阶段提供 `answerReceipt/clientReceipts/device`；answered 和 history 补 `nativeQuestionStates`；delivery 提供 `networkStartedAt/networkFinishedAt/eventsTruncated/networkEvents/responseBodies/clientReceipts/device`。`nativeQuestionStates` 保留原生 `{key,sourceTurnId,sourceTurnStatus,state}` 结构，不能只返回“已回答=true”。

unknown 菜单检查前，必须等回答结束、连接恢复且菜单触发器可用。证据门禁拒绝 `send.action` 仍为 `stop/sending` 的菜单观察，并要求 answered 的完整历史与恢复后历史一致。整个菜单因断线或运行中禁用，不能证明独立的未知回执删除锁。

`networkEvents` 保留 `Network.requestWillBeSent/responseReceived/loadingFailed/loadingFinished` 的原始 `{method,params}`，包括 requestId、loaderId、wallTime、timestamp、type、URL、HTTP method 和必要的 POST body；headers 只保留本案所需的 Idempotency-Key，不保存 Cookie、Authorization 等凭据。`responseBodies` 为实际 GET 的 `{requestId,body}`，body 是 `Network.getResponseBody` 原结果。记录截断、缺少完整时间窗口、GET 属于旧文档、GET 尚未完成就声明解锁，均不能通过。

校验器从原始事件重新计算：恰好一次提问 POST、一次 answer POST；恢复阶段无额外发送、重答、跳过或删除，允许正常 markRead；真实刷新后的 GET 必须查询同一外层回执 ID，返回同一内层问题 request ID、thread 和成功终态。它重新解析完整历史中的回答封套，逐题比较已批准答案和原生接受结果，要求旧历史前缀逐字不变，仅新增准确提问及回答两轮。原生回执无 turnId 时不捏造该字段，实际回答轮用独立历史的 answerTurnId 绑定。

runner 另保存主/控制两份普通草稿、unknown 锁定菜单、刷新后的界面与解锁菜单的 DOM、截图和原始 observation 文件；只查看删除入口，不点击删除。专项记录及固定条件由源码强制启用，报告删除 `requiresQuestionReceiptRecovery`、专项阶段或历史也不能通过。旧 R2-question 人工材料仍是既有专项证据，不能伪装为 2.5.0 的新执行。

其他前置参数：

- `model`：从真实菜单观察的 `{name,id,protocol}`。`models` 为两项 `{name,id,protocol,effort,effortLabel}`，用于隔离验证。
- `messages`：获准的 `stream/active/stop/unread/multiline/image/skill/optimization/approval/question/goal`。字符串支持 `{ack}` 和 `{runId}`；脚本追加完整标记。`active/stop/unread` 需有明确且有限的真实执行时间窗口。
- `imageFixture`：真实本机图片 `{path,sha256,size,name}`；`skill`：当前测试项目获准的 `{name,path,displayName}`。
- `approvalUI`：已批准的具体 `{requestIncludes,buttonName}`；`questionUI`：`{fields:[{label,value}],submitName}`。不批准任意出现的工具或权限请求。
- `projectFixture`：本轮专用 `{name,path}`，两者均含 runId。桌面驱动通过真实项目 UI 新建，不复用用户已有项目。
- 侧栏偏好按用户确认在各远程客户端独立保存。`project.search_preferences` 只修改当前测试客户端的视图，并在 finally 恢复原视图和搜索；不修改桌面排序，也不为验证排序额外发送消息。它替代 2.0 的 `project.search_sort`，旧案例预期不能继承。
- `contextFixtures`：`{zeroKey,missingKey}` 指向两个本轮已登记且原生实际值分别为零/缺失的会话；夹具不存在时阻塞。
- `readOnlyTab`：调用方明确绑定、正在显示同一专用 main ID 的真实只读授权页，仅用于无原生注入优化的权限检查，不修改授权。

逐项运行 `runRemoteAccessCase(caseId, ctx)`；整体运行可传 `suites` 与 `caseIds`。只读起步未建立 main/control 时，依赖案例阻塞。长场景优先分次调用，外层超时应大于案例上限；脚本每十秒落盘等待进度。

所有整页刷新统一通过 `reloadRemoteAccessPage`：先登记新文档导航等待，再触发 reload，确认 `domcontentloaded` 后继续等待会话加载提示消失。随后按该案例的具体身份、标题和内容验收。不能将 reload 命令返回、旧文档缓存或加载骨架当作刷新完成；没有导航观察能力或导航失败时明确阻塞/失败，不自动重复导航。

## Typed 环境驱动

网页现有控件能完成的步骤由 runner 直接 click/fill/wait/assert。需要 Codex 桌面、系统文件选择器、原生日志、真实故障或 Android 的步骤，由 `drivers.desktop/files/native/environment/android` 明确接入：

```js
{
  contractVersion: "remote-e2e-driver/2",
  operations: ["observe-thread", "rename-thread"],
  async perform(request) { /* 执行声明的真实 UI/故障动作，保存原始回执 */ },
  async observe(request) { /* 只读等到真实条件成立，保存原始观察 */ },
}
```

`request` 包含 runId/caseId/origin/sourceFingerprint/时间范围、准确 threadId/parentThreadId、全部本轮 ownedThreadIds、具体 operation/payload、固定 expectations、稳定 actionId、获准消息及输出目录。驱动不接受任意用户会话作为目标，不启动新环境。发送由 runner 预扣预算并提供完整 `request.message`；不能在驱动里额外发送、重试、直接调用业务 API 代替 UI，或自行批准审批/关闭/删除。

驱动返回 `{records,uiEvidence}`，不返回 `status: passed` 或 `verified: true`。环境未就绪可以返回 `{status:"blocked",reason}`。每条 records 必须满足下节原始文件契约。desktop/android/files 的 uiEvidence 必须包含真实 UI 的 DOM/可访问性树与原始截图文件引用：

```js
{
  records: [originalEvidenceRecord],
  uiEvidence: [{dom: {path,sha256,size}, screenshot: {path,sha256,size}}],
}
```

runner 会执行固定步骤、保存远程页观察并读取 rawFacts 自行断言。导出的 `REMOTE_ACCESS_E2E_REQUIRED_DRIVER_STEPS` 定义每例必须出现的 operation 与次数；离线验收也重新核对，复制同一份读取证据不增加次数。

| 驱动 | 必需能力与真实环境 |
| --- | --- |
| `desktop` | observe-thread、create-side、rename-side-tab、rename-thread、open-thread；create-project、observe-project。必须使用真实 Codex UI，并保存 visibleThreadId/visibleTitle/nativeTabThreadId/nativeTabTitle 等原始观察，不通过同名标题猜 ID |
| `files` | choose-image：响应 runner 已打开的真实系统图片选择器，选择明确文件；证明 fileName/fileSha256/pickerUsed，保留实际 UI 图与树 |
| `native` | observe-context、wait-optimization-terminal、observe-draft、observe-request、observe-question-recovery、observe-project-membership、observe-side-aggregate、observe-queue-boundary、observe-dequeue-window、observe-queue-race、observe-side-lifecycle、observe-late-side-create、observe-compaction、observe-goal、observe-scenario。只读原始 RPC/bridge/日志及真实回执 |
| `environment` | acquire-environment-lease、restore-environment-resource、inspect-environment-lease；真实 disconnect-cdp、disconnect-service、disconnect-network、restart-native、drop-next-action-receipt、start-question-recovery-observation、drop-question-answer-receipt、release-question-receipt-on-reload、delay-side-create-delivery、fail-active-upstream；observe-transport/recovery/original-receipt/question-recovery/optimization/authorization。只操作原始证据核对的独立实例或授权 tab 租约，不伪造响应 |
| `android` | observe-installed、open-thread、fill-draft、background-app、resume-app、open-model-menu、press-back、observe-navigation、observe-notification、tap-notification、observe-notification-navigation、check/apply-resource-update、reload-webview、attempt-unavailable-resource-update、observe-resource-update。每步均有实际设备 UI 与版本/原生回执 |

当前仓库不捆绑某台电脑或设备的私有适配器。未接入以上驱动、没有可压缩的真实上下文、没有审批/问答请求、没有可捕获的真实并发/取消窗口或没有正式安装设备时，报告列明缺少的 operation/夹具并保持受阻；不是用 manual 标记或成功布尔值补成通过。

所有故障案例还必须传入 `environmentTarget`。独立服务格式为 `{kind:"dedicated-test-service",instanceId,origin,servicePort,processId,workspace,nativeProcessId?}`，instanceId/workspace 含本轮 runId；重启时还要准确 nativeProcessId。tab 范围为 `{kind:"authorized-tab",instanceId,origin,servicePort,tabId}`，只允许该 tab 的网络、回执或创建响应延迟，不能断开全局 CDP/CLI、上游或重启用户进程。**45222 是本轮受保护的用户服务，不能用于故障演练；allowFaults 不扩大这一边界。** 缺少目标或不满足隔离身份时，在调用驱动 perform 前阻塞。

租约获取必须用原始 JSON 同时确认 leaseKey、leaseOwnerRunId、instanceId、scopeKind、processId、workspace、servicePort，以及适用的 nativeProcessId/tabId；不能只检查调用方输入。所有后续环境步骤绑定同一份租约。获取前登记恢复责任，驱动先保存 originalState；故障 perform 已执行而证据/截图失败时，仍逐资源尝试恢复，并核对实际 currentState 等于原状态。某个恢复失败不跳过其他恢复；CLI 原来关闭则保持关闭。旧 restore-cdp 等内部调用也转为按租约恢复资源，不会无条件启用服务。

`scripts/remote-access-e2e-environment.mjs` 的本机控制流测试覆盖“真实状态先改变、随后取证失败”“首项恢复失败”“租约获取取证失败”“原服务关闭”“保护用户服务”“tab 越界”和“进程结束后的冷恢复”。这些辅助 mock 测试验证恢复调度，不算真实故障 E2E。

### 已提供的问答回执 CDP 驱动

`scripts/remote-access-e2e-receipt-driver.mjs` 提供 `createQuestionReceiptRecoveryDriver`。调用方传入已经明确绑定的 `tab/cdp`、输出目录、`authorized-tab` scope、真实 `readClientMetadata` 和新标签创建记录；驱动不发现浏览器、不读取登录密钥、不代替 UI 发送业务请求。创建记录必须保留实际 tab ID、runId、创建时间、初始 URL，以及没有既有 Fetch 拦截的明确所有权。

```js
const environment = createQuestionReceiptRecoveryDriver({
  tab, cdp, outputDir, scope, // 与 createRemoteAccessE2EContext 使用同一个 outputDir
  readClientMetadata, // 实际 { device: { id, createdAtMs }, clientReceipts: [...] }
  newTabProof,        // 本轮真实创建记录；不能自行填一个既有用户 tab
  timeoutMs: 60_000,
});
// 交给 createRemoteAccessE2EContext 的 drivers.environment。
// 只读原回答动作证据可由 environment.readAnswerReceiptEvidence(request, answerAction) 返回。
```

运行前先核对驱动与执行器的origin、outputDir、tabId及runId一致。获取前参数错误同样保留失败记录；不能在已登记unknown后直接清账本重跑。驱动记录使用`evidencePath`，读取原始文件时使用`verifyNativeEvidence`；若使用通用`verifyEvidenceReference`，须显式传入`path: record.path || record.evidencePath`及原始摘要，不能把字段不匹配当作产品恢复失败。

驱动在完整分页、连续 cursor 的基础上保存脱敏原始事件；只在匹配的 answer Response 阶段读取真实正文后丢弃一次，并只阻断该回执 ID 的 GET。Fetch 拦截必须显式列出 Fetch/XHR 两种资源类型：答案响应和回执请求各两条 pattern。Document 只观察，不拦截。

Browser Use 自身可能产生 `Fetch.requestPaused(resourceType=Document)`。这些事件在基线、观察和清理阶段都保留原始记录，但不登记为本驱动暂停请求、不接管、不发送continue/fail，也不要求本驱动解除。基线中的未知非Document暂停仍拒绝；常规阶段只控制与本驱动Fetch/XHR patterns匹配的事件。

`drop-question-answer-receipt.payload.receiptReadFault` 缺省为 `network-error`，保留网络失败变体；正式刷新专项显式选择 `pause`，仅挂起准确原 ID 的 GET，避免反复失败污染连接状态。release 登记真实主 frame/loader，撤销已知旧文档 GET；release 至新 Document 之间到达的旧 GET 仍撤销，只放行新主文档同 loader 的读取。每个 Fetch request 只有一个控制任务；取消须有成功的 CDP 确认或匹配的原始 Network 终态，不能按错误文案吞掉失败。收集新文档实际 GET 正文及完成事件后才验收恢复。`environment.close()` 是撤回本驱动控制的收尾入口，不得用它重答或重新启动故障。

驱动接入协议和保存的 DOM 观察都按 JSON 值归一化。Chrome 插件、Node 和 VM 之间对象原型可以不同；不得把 JSON 完全相同的数据因 `Object`/`Array` 原型差异判成错 scope 或伪造观察。字段、值、类型、额外键、原 leaseKey 和真实 tab/origin 仍需完整核对。

获取前失败只有在同一驱动实例留有明确记录，且未登记租约、未尝试 CDP 变更、没有暂停请求、故障和其他所有者时，才允许 `lease-absent-no-mutation`。已登记但取证失败仍须恢复原租约；丢失记录的新实例不能宣称没有副作用。清理失败不能标记已关闭；再次调用也不能以空结果冒充恢复，必须继续保留失败证据并仅核对、恢复同一个原租约。

在 CUA 中必须在有效工具调用内 **await 完整的 `runRemoteAccessCase`**。不能只启动 Promise 后结束工具调用，再让浏览器步骤在后台继续；实际接入已发现这种用法会在取证前报执行上下文失效。驱动内部事件观察循环随该 await 保持运行，并在环境租约 finally 中收回。工具超时不等于业务未发出，必须保留账本并只读核对原结果。

## 原始证据协议

`nativeEvidence(request)` 只读取原始文件，不提交或重发原动作。它按 actionId、thread/turn/queue ID、唯一消息标记、动作类型和时间关联原生 receipt；actionId 不是原生 receipt ID。已有 driver actions 的 result.records 可重复读取并返回，不能复制预期值生成新回执。

```js
{
  actionId, threadId, turnId, receiptId, // 原生标识按实际存在提供；数字回执 ID 可为 0
  source: "rpc", // rpc | bridge | native-log | desktop-ui | android-device
  evidencePath: absoluteOriginalJsonPath,
  sha256: originalFileSha256,
  outcomePointer: "/receipt", // 指向本次动作封套；平铺单回执显式使用 ""
  rawFacts: {
    operation: "/request/action",
    threadId: "/receipt/result/threadId",
    turnId: "/receipt/result/turnId",
    receiptId: "/receipt/id",
    receiptState: "/receipt/state",
    origin: "/capture/origin",
    sourceFingerprint: "/capture/sourceFingerprint",
    capturedAt: "/capture/capturedAt",
    // 其他由 request 固定要求的 title/modelId/protocol/reasoningEffort/queueId/closed/deleted 等字段
  },
}
```

指针是 RFC 6901 JSON Pointer，按真实原始结构提供。捕获元数据可以在原始封套外，但不能用它代替封套内的动作、身份和完成字段。报告门禁重读 JSON，重算哈希，按当前 runner 契约比较值；不信任自报 facts、verified 或 passed。闭合侧边必须 closed=true，删除必须 deleted=true。该动作范围内 outcomeUnknown、unknown、failed 不能当作成功；并发请求的明确 rejected 只在 staleWriteAccepted=false 等固定条件下合法。历史其他轮次的错误不会被当成本次动作失败。

同一阶段的语义必须由同一条原始观察同时满足，不能跨会话或跨时间拼字段。模型需真实 turn 事实；发送需真实 turn/完成回执；排队需真实 queueId 与消费事实；不能仅凭按钮、toast 或侧栏隐藏。

问答成功后还直接检查 `#notice` 不出现“回答已提交”；“正在确认回答结果…”的处理中提示和失败/unknown 反馈继续保留。模型选择与恢复原提示词都是本地 UI 步骤，不给它们伪造独立 RPC；模型实际生效由随后真实发送的 turn 核对。

已读还要求原始 `threadId/turnId/turnStatus/hasUnreadTurn`，侧边加 `parentThreadId/parentSideUnread`。每个 unread/opened/reloaded 阶段用独立原始文件和读取标识，不覆盖旧文件。支持 `readStatePointers`，但仍要 sha256、原始 read/receipt/event 标识及其指针。父聚合数组按精确侧边和父 ID 核对。

恢复原提示词是纯本地 UI 步骤，不伪造服务 RPC：runner 保存优化前后的 prompt、真实 DOM/截图，逐字验证恢复结果。真正的 optimizePrompt/cancelPromptOptimize 请求仍需原始服务回执。

恢复原文的原始优化记录还需 `rawFacts.originalInput`、`rawFacts.optimizedPrompt` 和 `optimizationState=completed`，同时绑定真实 beforeUi/afterUi 的 DOM/截图与 prompt，不能只在报告里写“已恢复”。

## 本轮外部测试对象接入

`adoptRemoteAccessE2EThread(ctx, input)` 只支持 main/control。调用时必须已经显示该同轮新建会话，DOM 有原始完整 marker；不能用后来编辑的普通消息取得权限。

`creationEvidence` 需提供原始 path/source/outcomePointer、operation/threadId/receiptId/prompt/requestThreadId/submittedAt 的准确指针，以及 rawFacts 中的 origin/sourceFingerprint/完成事实。提交时间位于 runStartedAt 后，原请求不能指定已有 threadId。脚本计算哈希并立即核验，保存 adoptions[].record 与真实 UI；离线门禁再次核验。只有 ID、created=true、截图或旧轮回执不足以接入。

## 运行版本与 Android 版本

`versionEvidence` 包含实际 managerVersion/managerExecutableSha256/codexVersion/remoteAssetSha256/adapterVersion/verifiedAt，另有：

- `managerExecutable`、`remoteAsset`：实际运行资产的 `{path,sha256,size}` 引用；可复制取证，不能以待构建源码/历史安装包代替。
- `browserAsset`：浏览器实际收到的 `/app.js` 内容文件引用。`browserAssetEvidence` 为同一次加载的 CDP 原始记录引用，包含 `capture`（runId/caseId/origin/sourceFingerprint/capturedAt）、`request`（Network.requestWillBeSent）、`response`（Network.responseReceived）及 `body`（Network.getResponseBody 返回值）。请求 ID 必须一致、资源类型必须是 Script，正文哈希必须与目标网页资产相同；后台预取、另行下载及 Service Worker 中的旧脚本不能替代实际加载。保存本案所需字段时排除 Cookie 和 Authorization。
- `evidencePaths`：原始版本 JSON 的引用数组；`records` 按上述 rawFacts 核对这些版本、origin/sourceFingerprint/capturedAt。
- `sourceCommit` 可单独记录源码，不作为安装或运行证明。

Android 需独立传 `androidExpected: {versionName,versionCode,apkSha256,certificateSha256,apk,manifest}`，apk/manifest 为有效文件引用。脚本读取更新清单并核对发布基准，再由设备驱动核对实际 installedApkSha256、签名和内置网页资产。包名为 `com.junxin367.codexelves`，Debug 包不算正式安装证据。

资源更新还需 `androidResourceUpdate: {manifest,resourceArchive,version,previousVersion,assetSha256,unavailableUpdateUrl}` 与获准真实测试更新服务。测试执行正常客户端更新和失败恢复；不擅自构建 APK、修改版本、生成发布标签或发布资源。没有合法可测的版本差异时明确阻塞。

## 不可恢复动作批准

`authorizeSideClose(preview)` 与 `authorizePermanentDelete(preview)` 接受本次已留图的准确对象预览，外层显示截图和风险并取得当时人类批准后返回：

```js
{approved, runId, actionId, threadId, parentThreadId, approvedAt, evidencePath}
```

永久删除无 parentThreadId 要求；侧边关闭必须匹配父 ID。批准时间晚于对应预览，批准后身份、确认文字和来源菜单不能变化。缺少批准时保持确认界面和 blocked；恢复时只核对同次明确批准，不因单纯经过时间重复询问。

## 恢复与离线验收

报告协议为 2.0，脚本为 2.5.3，文件证据门禁为 1.3.1。源指纹覆盖实际远程网页模块、Rust service/routes/core、Android 原生/依赖/资源、打包更新相关输入及回归脚本；源码变动必须新 runId，不能复用旧通过结果。目标用例先填正文再选择目标标签，提交前核对标签仍在且按钮为“创建目标”；原生完成态为 `complete`，不是字段名 `completedThreadGoal` 推导出的 `completed`。

`queue.automatic` 必须同时建立两条原生排队消息，此后只读等待。证据回调须给每条原排队回执提供 `queueId`、`clientUserMessageId`、`deliveryState=queued` 的原始指针，并给同一终态快照提供 `nativeTurns`、`queuedMessages` 数组指针及 `historyComplete` 指向原生完整性字段。只有完整历史才能证明没有遗漏或重复消费。校验器逐条匹配持久 `userMessage.clientId`，检查原文、唯一准确回复、独立轮次、前轮结束之后才开始后轮，以及最终队列为空。原生自动出队的 `turn.params.input` 可以为空，物理队列 ID 也可以不同于逻辑消息 ID；这些不能作为失败依据。只有 `queueEmpty=true`、回复文案或队列条目消失，均不足以通过。

上述 FIFO 条件由源码固定契约强制执行，不接受报告删除 `naturalQueue`、删除完整历史或自报不需要核验。基础发送和两条入队必须绑定准确原 actionId/queueId。完整历史还须排除相关 `steeringUserMessage`，包括直接/服务器逻辑 ID、恢复对象和服务器物理 ID；经过引导的消费不能补成自然 FIFO 通过。正文只允许原生在末尾追加一个 LF，不能用 trim 掩盖用户内部空行或多余换行丢失。

```js
const resumed = await remoteE2E.createRemoteAccessE2EContext({
  ...originalOptions, tab, resumeReport: reportPath,
  retryCases: ["明确解除阻塞的案例 ID"],
});
```

处理器全部 UI 步骤走完后才记录 uiStepsCompleted，然后读取分阶段原始证据。只缺原生证据的恢复仅补读 pendingNativeChecks，不重新执行 UI。中途失败不把已有第一阶段证据当作整例完成；已确认服务动作不重放，unknown 必须核对原结果。真实窗口已消失时保留阻塞并另用新 runId，不能补造旧截图。

如果进程在故障后被终止，使用独立的 `recoverRemoteAccessEnvironmentLease({reportPath,caseId,environmentTarget,drivers,authorization:{allowFaults:true},authorizedTabId?})`。它只按原报告完全匹配的 scope/leaseKey 检查并幂等恢复各资源，不重新 acquire、不重跑 handler、不重发故障或消息，也不把原 unknown 动作改成 confirmed。即使源码指纹已变化也可明确调用此仅恢复入口；原测试报告仍不通过。恢复不依赖远程页仍可打开，且每个恢复失败都保留证据。

每个案例均保存 passed/failed/blocked/not_run、执行尝试、耗时、实际 URL/身份、DOM、原始截图、阶段 request 与原始回执引用。截图按字节识别 PNG/JPEG/WebP，保留原始格式和对应后缀；不能假定浏览器工具总返回 PNG。证据模块不依赖宿主全局 `process` 或 `Buffer`；绝对路径直接使用，相对路径优先按调用方 `baseDir` 解析，未传时使用仓库根目录。未选的必测仍为 not_run，任一缺项都不能 overall=passed。离线验收：

```powershell
node scripts/remote-access-e2e.mjs --verify-report temp/remote-access-e2e/<runId>/report.json
```

只有 73 项均通过、全部固定驱动步骤/语义/文件哈希有效、无 unknown 操作且源码指纹仍匹配才返回 0。没有文件、文件被改、旧代码指纹、只写 passed 或仅有布尔 verified 都返回 1；该门禁通过仍不代表 73 条业务基线的全部分支已经完成。

## 可重复的源码回归命令

```powershell
node --check scripts/remote-access-e2e.mjs
node --check scripts/remote-access-e2e-evidence.mjs
node --check scripts/remote-access-e2e-environment.mjs
node --check scripts/remote-access-e2e-receipt-driver.mjs
node --test scripts/remote-access-e2e-evidence.test.mjs
node --test scripts/remote-access-e2e-environment.test.mjs
node --test scripts/remote-access-e2e-receipt-driver.test.mjs
node --test scripts/remote-access-ui-state.test.mjs
cargo test --workspace -- --skip remote_access::tests::
cargo test -p codex-elves-core --lib remote_access::tests:: -- --test-threads=1
git diff --check
```

PR 工作流的“远程会话回归证据校验测试”运行上述四组 Node 测试，包括回执恢复驱动测试。驱动测试只验证离线控制流和取证条件；CI 通过不能替代真实问答、队列或断线恢复验收。

remote_access 的授权夹具有 60/120 秒有效期，随机密钥共享单项 KDF 缓存；完整并行跑会产生缓存抖动与合法 heartbeat 时序导致的假失败。2026-10-31 原样对照：并行 27 passed/9 failed/1 ignored，串行 36 passed/0 failed/1 ignored，串行耗时 167.53 秒。CI 将其从 workspace 普通并行测试排除后单独串行运行，不修改产品授权期限。新增原生 outcomeUnknown 禁止 CLI 重发的回归已经历红灯复现与修复后绿灯。

这些是源码回归结果。ignored 外网手工案例、真实 Chrome 全流程、Codex 桌面动作、故障注入、Android 安装/后台/通知/资源更新均须另有本轮运行报告，不因上述测试通过而自动完成。

## 2.5.3 的真实事件口径补充

- `Fetch.requestPaused` 可先于对应 `Network.requestWillBeSent`，也可跨读取批次到达。元数据缺失表示尚不能判断，不能直接当作旧文档读取取消。驱动保留暂停，等待准确的 networkId、URL、method、frame、loader 和新 Document 身份，再处理同一请求；始终缺少关联证据则受阻。
- 不强制 `loadingFinished.timestamp >= responseReceived.timestamp`。Chromium 响应事件使用派发时刻，完成事件可使用传入的底层完成时刻。本轮前者比后者早 1.917ms；仍核对同一请求、两者均不早于请求起点，并用较晚时刻确认解锁之前完成。不以样本误差设置容差，也不删除 HTTP 200、正文和完成事件检查。原始数据及源码语义取证在 `R3-original-question-recovery/timing-contract-source.json`。
- 本轮分段真实操作完成了既有原题的刷新恢复，原始文件另经只读审核；这不把此前中断的自动处理器改成通过。完整套件及其他失败/恢复分支仍分别登记。

## 前后台连接诊断

手机继续使用现有“发送日志”入口。电脑端接收报告位于状态目录的 `remote-client-diagnostics/`；同一状态目录的 `codex-elves.log` 记录服务端连接阶段。采集前核对两端资源版本和日志时间范围，保留“源码、安装包、已安装运行、真机验收”的区别。

- `page.lifecycle`：可见性、当前在线状态、浏览器事件来源 `isTrusted`、本次后台时长 `backgroundMs`。
- `connection.state`、`stream.*`：连接状态变化、事件流建立/错误和实际重试等待。
- `socket.*`、`worker.*`：握手、关闭码 `closeCode`、是否正常关闭 `wasClean`、重试与超时。只记录连接转换；不采集消息帧或逐个心跳。worker 转交的诊断消息不刷新业务心跳时间。
- `http.*`：每次请求的独立诊断 `requestId`。Android 的 `/api/session`、`/api/channels` 探测与 WS 握手在 `diag` 参数携带该标识；业务写入的幂等 ID、请求正文和请求次数保持原样。
- 服务端 `remote_access.connection_request`：同一 `requestId` 的 `received`、`response_headers_ready`、`request_cancelled`，以及 `websocket_opened`、`websocket_handler_ended`。只保留固定路径、状态、耗时和固定关闭原因，不记录原始查询、headers、IP 或关闭文案。

| 手机与电脑的对应记录 | 可支持的判断 |
| --- | --- |
| 手机开始请求，电脑没有同 ID 的 `received` | 在确认服务日志覆盖完整、没有丢弃/轮转后，优先调查请求到达前的链路；缺一条日志不能直接证明网络断开 |
| 电脑有 `received`，没有响应头就绪 | 检查处理是否仍等待、被取消或进程退出；不要直接归为 WebView 故障 |
| 电脑响应头就绪，手机没有 `http.headers` | 服务端已经生成响应；继续核对返回线路与客户端接收，不能据此宣称手机收到了响应体 |
| 两端同一 WS ID 有握手/关闭记录 | 结合关闭码、固定关闭原因、后台时长和重试记录定位阶段；单个关闭码不能证明具体设备或公网节点故障 |

日志仍采用原有有界队列、清洗和手动上传机制。JS、Android、Rust 的字段白名单须同步回归；诊断失败不得改变业务返回，也不得借补日志改变超时、自动重发或权限策略。
