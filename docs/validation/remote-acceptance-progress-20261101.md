# 远程会话目标执行进度

**2026-11-04 用户纠正侧边命名规则：** 原文档中“不从首条消息正文改名”是错误规则；同日用户将自动命名上限从 10 个字符调整为 15 个字符。默认名可由首条有效用户文字在合并空白后取前 15 个 Unicode 码点自动生成，写入并确认同一原生标签后同步远程；已有自定义名不得被自动命名或后续消息覆盖。完整规则见 [侧边会话需求](../remote-session-tools.md#侧边会话)。以下早期撤销十字符断言或待裁定的记录只描述当时验收状态，不能再作为当前产品规则；本次文档更正不增加任何验收通过结果。

日期：2026-11-01，Asia/Shanghai。目标状态：**active，未完成**。本文件是阶段进度，不是完整三轮验收报告。

用户要求先按产品功能建立用例，再修复并至少执行三轮有不同侧重的真实验收。独立基线为 `remote-acceptance-cases-20261101.md`，包含 73 条具体业务用例，已有逐项覆盖映射。该阶段脚本 2.5.3 的 73 个处理器与这些基线不是一一对应关系，尚未全量执行。

本文件按时间保留阶段进度；下文“当前”“未恢复”和版本/测试数均以所在记录截点为准，不代表今天仍处于同一状态。后续证据见 [现状汇总](remote-acceptance-current-20261102.md) 和 [手动反馈清单](remote-user-feedback-20261103-20261104.md)，现行能力以 [远程会话工具](../remote-session-tools.md)、[远程访问](../remote-access.md) 为准；本次文档核对没有重新运行验收。

**最新检查点（原题恢复实测后）：** 原生业务源码仍为106，生产app.js为`cda5c76d…055fe4`。复用live5有完整创建证据的原未答题，分段真实完成回答丢回包、原生接受、新文档GET原回执、原锁解除、双草稿保持及完整历史2→3且旧前缀不变；实际create=0/send=0/answer=1，原始材料28项核对通过。**该功能子场景已通过；旧自动处理器的失败/中断记录不改写，完整套件和三轮仍未完成。** Fetch/Network到达乱序和时间戳取值阶段差异已修入验收器，完整Node **178/178**、退出0。此前桥接172/172未无故重复。服务1445、注入、授权、标签及私有副本已收回，上一轮遗留标签也已确认不在列表。完整原生恢复接管、已发送消息安全编辑、Android真机及管理器启动视觉仍未完成；安装版未更新。
**历史检查点（16:30）：** 原生业务源码仍为106，生产app.js为`cda5c76d…055fe4`。最近完整桥接172/172；包含输入/侧栏行就绪等待和Document边界修正的完整Node四组 **169/169**，退出0。R3-live2/live3真实完成同题刷新、单次回答丢回包、撤故障后的自动GET原回执、零重答及草稿/完整历史保持。live4发现的Document误判已修，live5实际通过租约获取并重新发现原completed双题；随后浏览器执行超时、调试连接中断，答案未派发。**正式刷新恢复专项仍未完整通过，失败/中断证据保留。** 测试服务1440—1444及兼容fixture已收回、测试授权已撤销、本轮私有副本已删除；live5原标签因调试连接中断未能显式关闭，辅助收尾标签已关闭。旧拒绝清理未重试。**完整原生恢复接管、已发送消息安全编辑、完整三轮、Android真机与管理器启动视觉仍未完成；安装版未更新。**

## 已完成源码修复

| 项目 | 当前改动 | 交付边界 |
| --- | --- | --- |
| 子智能体 | 新版桌面 discovery RPC 与原生行状态、名称、排序；确认父子/孙级关系；只读身份优先；有界 watcher、超时与释放；失败独立降级；完成分组可展开名称 | 当前生产脚本 106；子智能体局部实测使用当时 105 业务逻辑，未打包安装 |
| 子智能体完整性 | 真实复测发现新 discovery/原生行已是 13 条，较旧 signal 缓存仍为 11 条。移除要求两个缓存成员必须相等的错误条件，改为新的完整 discovery 中每个有效成员都有原生行 | 原始失败保留，新增回归先失败后通过，再真实复测通过 |
| 队列编辑与引导 | 区分原生物理队列 ID 与当前逻辑 clientUserMessageId；保留线程、输入、目标 turn、恢复上下文校验；仅规范化图片合法缺省 detail:null；未知确认保留阶段/原因/已恢复 ID，不重写 | 三图保存/引导、取消恢复、排序删除与两条自然 FIFO 已局部真测；未知/冲突及其他输入分支仍待验 |
| 客户端侧栏偏好 | 按用户选择独立存入本客户端 localStorage，按设备授权范围隔离；保留现有最近/项目切换与置顶入口，不新增排序菜单；保存失败保当前页选择并提示 | 两个真实授权浏览器 origin 的视图隔离、交叉刷新与离线保存已局部验证；现有置顶完整交叉、原服务设备上限及不同物理设备未验证 |
| 前后台读取 | 显示读取按新代次隔离；后台取消并退役旧的历史、元数据、侧栏和整页刷新；保留当前内容、草稿、写请求与未知回执；迟到任务不能清理新任务 | 控制流回归通过；真实浏览器慢请求及 Android 真机仍待验收 |
| 欢迎页草稿保护 | 未选中会话且未处于新建模式时，不把欢迎页空编辑器保存为新会话草稿；保留有效会话/新建页的主动清空和队列编辑保存 | 旧代码真实丢稿、定向回归红灯、修复后真实权限切换恢复及主动清空均已验证；新增3项长期回归 |
| 问答回执恢复 | 只GET原回执，合并并发核对，校验授权与操作身份；成功清原锁，明确失败保答案草稿，unknown/404不重答 | 真实丢回包后原生已完成但删除锁残留的反例已修；同一原回执恢复及零重答真测通过，新增6项长期回归并独立复审 |
| 独立提示词优化 | 继续使用管理器供应商设置，功能与原生连接/CLI独立；原有请求身份、取消和迟到结果保护保持 | 无原生连接且没有CLI实例的隔离服务已真实优化/恢复；取消先到时供应商调用为0；运行中供应商取消窗口仍未取得 |
| Windows 启动 | 后台进程仅用 CREATE_NO_WINDOW，移除与 DETACHED_PROCESS 的冲突；修正 .cmd 启动转义；隐藏 where/版本/目录/Appx 探测；service 初始化/传输失败统一冷却 | 真实无业务副作用 Windows 子孙进程、stdio 与退出码验证通过；未重新安装管理器做启动视觉验收 |
| 验收脚本 | 2.5.3/证据1.3.1：七阶段问答证据、输入与侧栏就绪等待、Document只观察、等待准确请求元数据处理乱序、纠正完成/响应时刻互相递增的错误断言 | 分段真实刷新恢复已证；完整自动处理器仍待新轮执行；73个处理器不代表73条业务基线全部覆盖 |

## 已取得证据

本机证据目录：`dist/validation/remote-acceptance-20261101/`，由 Git 忽略。

- `cdp-159-final.log`：完整桥接回归 **159 通过、0 失败**，退出码 0。
- `node-tests-current.log`：证据、环境及 UI 状态回归 **85 通过、0 失败**，退出码 0。它们是源码回归，不能代替真实产品验收。
- Windows 修复代理：service 13/13、catalog 7/7、真实子孙进程隐藏+三路 stdio+退出码 1/1、core check 和格式检查通过。
- `preference-version-mismatch.json`、`loaded-app-before.js`：首轮真实页面仍加载旧脚本，实际发出了 sort 请求；该轮失败，没有算新代码通过。
- `preferences-real-results.json`：用哈希匹配的源码网页重测，视图切换、刷新保持、测试 tab 断网保存后恢复均通过，观察期 `/api/action/*` 请求为 0。明确记录本次只在测试 tab 绕过旧 Service Worker/缓存；不是正式安装或正常资源更新的验收。
- `native-subagents-incomplete-initial.json`、`subagents-snapshot-lag-red.log`：保留第一轮 13 条记录正确但完整性误判的真实失败及回归红灯。
- `subagents-snapshot-lag-green.log`：相关 3 项回归通过。
- `native-subagents-after-105.json`：原生与远程 13 条记录的 ID、名称、状态、分组顺序、完成数、完整性和一个子会话的父关系/只读身份一致。
- `subagents-menu-105.json/.jpg`、`subagent-readonly-reloaded.json/.jpg`：真实网页逐项核对 13 个完成记录；打开同一 ID，刷新后编辑器仍实际隐藏，模型显示正确，并返回原主会话。
- `native-engine-source.json/.js`：通过实际运行函数的 scriptId 读取引擎源代码，确认与本轮临时运行脚本逐字相同。

## 初始运行版本和现场边界（历史记录）

现场当前安装的原生适配器为 **101**，当前目标源码为 **105**。问答、目标及部分引导代码已经存在于后续源码，但 101 的旧白名单和旧读取路径仍阻断它们；不能将源码测试通过解释为用户当前安装已经恢复。

为只读验证 105，本轮临时加载同一份业务逻辑，保留旧后台所要求的 101 标记，并将自动侧边命名限制到测试白名单（本轮为空）。这是明确登记的测试变体。最终目标源码 SHA-256 为 `cd07833ac2dc7c602718793f2106ce7f3087d9138ba0cab31a93997897ea2780`；测试变体及实际引擎摘要见原始证据。未使用这个版本数字冒充安装完成。

上述只读阶段结束时曾恢复原安装脚本 101，恢复后的 epoch 为 `99cc9b5a-4542-4777-9e7a-87e49e6a6b24`，并关闭该阶段测试 tab、1432/1433 服务。**这不是下面 R1 阶段的当前状态。** 整个阶段未操作用户根会话的队列编辑、保存、引导或重发。

后续临时测试控制脚本位于 `temp/remote-acceptance-20261101/`。原生目标当前为端口 55277、page `1F3A21A10790397B437939D2E4B59CC5`；复用前必须重新核对，不能沿用已失效的 9229。此前 `temp/remote-regression-completion-20261031/` 的 9 个文件清理曾被自动审批拒绝，本轮未触碰。

## 初始待执行清单（后续状态见追加记录）

1. 将独立基线逐项映射到已修订的脚本/真实操作，尤其子智能体、异步问答、目标、附件队列与前后台恢复；未映射不能宣称覆盖完整。
2. 在有明确创建证据的测试会话中，验证问答实时到达/回答、目标标签创建至 complete、带图队列保存/取消/引导及自然承接，逐次关联原生输入、物理/逻辑 ID、目标 turn 和原回执。
3. 对真实浏览器进行受限慢请求、前后台、刷新和重连交叉验证；对 Android 使用实际设备做前后台及资源版本核验。现有 Android 日志未覆盖原超时时段，不能把后续读取成功当作已排除故障。
4. 新的第二客户端登录遇到“授权设备已达上限”，未撤销任何用户设备。真实跨客户端验收仍受阻；源码隔离测试不代替该项。
5. 验证实际 CLI 初始化/主会话兜底，以及重新运行管理器时的完整窗口行为。当前未启动完整 app-server 或重启用户 Codex进行故障演练。
6. 三轮都须有明确版本、前置条件、预期/实际、失败恢复和证据；目前只有局部真实结果，**三轮完整验收均未完成，目标不得标记 complete**。

当前未提交、推送、打包、安装或发布。Windows/Android 交付与真机状态单独记录，不能由测试数量推定。

## R1 真实操作追加记录（04:01—04:33）

运行组合仍是源码网页与临时原生适配器，不是安装交付。`R1/runtime.json` 保存浏览器实际 `/app.js` 摘要 `21f4dfbb5ea676382287b7d0e76199f2ded883316149d00e7914e7afeb4c9cdf`；原生 fixture 为 `938f30c8-0b5e-4c5a-89bc-3f107cf80c27`，业务 105/运行标记 101，epoch 为 `e878d3d6-6e74-4aff-b685-bb3fc3591f55`。临时网页服务为 1433/PID 52496；Chrome 3 的本轮 tab 为 1283922538，缓存/SW 绕过仍仅作用于该 tab。用户原服务 45222/PID 46216、helper 45221/PID 38204 与 CDP 55277/PID 48596 未停止。

测试主会话 `01a1938d-d3e8-7440-8f4b-bcda032ac277`，另一条目标/队列测试会话 `01a193a0-1755-7ad3-a984-2620c79154d0` 均有真实创建回执。所有动作在 `R1/actions.json` 逐次登记；两条会话均不是用户原有会话。

| 功能/基线 | 本阶段实际结果 | 证据及尚缺分支 |
| --- | --- | --- |
| 普通新建/首条发送 | 创建回执 completed；准确首 turn completed，唯一 `RA-R1-READY` | `create-main-http.json`、`create-main-native.json`；不覆盖项目创建分支 |
| R1-18 异步问答 | **失败**：两题最初出现，原 turn 在 13 秒后 completed，未回答卡片却消失 | `question-pending-native.json`、`question-disappeared-ui.json/.jpg`、`question-send-http.json`。原生当前 panel/逐题状态仍存在、未提交且未跳过；适配器仅从 inProgress turn 提取问题，已进入独立修复。未重发问题或代答 |
| R1-20 目标标签 | “＋”与“/”入口、标签后真实键盘输入、删除标签保正文、全选替换取消模式均通过，观察期 action 请求 0 次 | `goal-draft-local.json`；Skill/图片共存的标签分支仍未执行 |
| R1-22 新会话目标 | 创建/首轮/设目标均获明确回执；原生 `completedThreadGoal.status=complete`，对应唯一首 turn completed、唯一目标完成回复 | `goal-create-http.json`、`goal-complete-native.json/.jpg`；既有会话设目标、更新/取消及部分失败分支仍未执行 |
| R1-12/14 多行队列保存与增图 | 原两图保留、追加第三图，正文及内部空行保持；原生只恢复一份队列对象，返回 committed | `queue-image-before-native.json`、`queue-edit-begin-http.json`、`queue-edit-save-http.json`、`queue-image-saved-native.json`。物理 ID 从 `01a193a3-2c47-7dd3-b4ed-a224c558a10a` 改为 `01a193a4-9e03-7113-a7cd-723f114f75f6`，逻辑 ID `917f119f-96b5-4a66-95fa-92e1b6fd1378` 保持；原图/新增图逐字节 SHA-256 均吻合。纯图、移图、Skill/隐藏上下文、保存后刷新仍待覆盖 |
| R1-16 引导 | 三图消息以一次 accepted steer 进入原活动 turn `01a193a2-9a3d-7b21-97e5-be008d6904fe`；恢复对象/逻辑 ID/三图/正文准确，队列空，唯一完成回复 | `queue-steer-http.json`、`queue-steer-complete-native.json/.jpg`、`queue-image-checks.json`（14 项原始对象对照）；不把它算作自然自动出队通过 |
| 前后台恢复 | **未完成验收**：已挂起两个真实读取响应，但本次切换未产生真实 visibilitychange，不能算后台恢复通过 | `foreground-inconclusive-events.json`、`foreground-lease.json`。Fetch 拦截和临时观察器已撤回，背景辅助 tab 1283922539 已关闭；恢复后草稿仍在且页面可正常读取 |
| Windows/Android 真机 | 受阻 | Windows 仍锁屏，已请求解锁；当前尚未获得可操作的 Android 真机通道。不得用浏览器结果替代 |

当前 R1 仍未完成；问答修复后的真实复测、其他正常分支、R2/R3 仍须继续。原生 fixture 和本轮主 tab 尚在使用，收尾时必须检查未知写/编辑状态后再恢复安装适配器及缓存设置。

## R1 后续修复与验收（04:35—05:03）

### 新确认的问答缺陷及修复

`asyncQuestionRequests` 原先只从 `inProgress` 轮次读取异步题。真实原轮 `01a1939b-a6f8-7f01-ae69-d38e78c6e879` completed 后，原生逐题状态仍为未答，远程却返回空请求。现在按每个会话的原生 panel、准确 entity/item/question 状态延续待答题，排除已答、已跳过和提交中的题；原生协调器继续决定从空闲状态启动下一轮或引导当前轮。未知写仍只查询原接受记录，不重发。

修复后的 `native.js` SHA-256 为 `e8bef50efe38977831aaeb057e590eb192a9fe8ed00e699352cfce52ea73efe5`，未改变桌面/Android 版本号。04:49 临时 fixture 更新为 `7bcade13-1dda-4e56-88f3-8ec3c3c1b864`，epoch `f2d5c266-95dd-41bb-8d8c-4188b132f4f5`。`R1/native-engine-after-question-fix.json/.js` 通过引擎实际 scriptId 6079 读取脚本，实际/预期摘要均为 `3966fb3f94aea9c78710dd392da605dfce1efacb1468aae2b205b6d09651dfba`。前段目标/队列证据保持原 fixture 身份，不拼为同一版本的完整通过。

| 功能 | 结果 | 原始证据及边界 |
| --- | --- | --- |
| 原失败对象的 completed 问答 | 通过该子场景 | 原两题重新出现；刷新后选绿色/方形并单次回答。原生在新 turn `01a193bb-145f-7360-a5cd-91448a3ad930` 接受准确问题 ID 的答复，逐题 `lastSubmission` 匹配；刷新后不复活，独立草稿保留，无“回答已提交”提示。见 `question-after-fix-native.json`、`question-answer-http.json`、`question-desktop-sync-native.json`、`question-answer-identity.json`、`question-answer-reloaded-ui.json` |
| 仅远程新建的 completed 问答 | 通过该子场景 | 新会话 `01a193bd-bc0a-7ee3-9281-60a41ca1e87d`，本轮未在桌面打开它；原轮 completed、原生逐题状态已存在，远程刷新后仍能答蓝色，新回答轮为 `01a193be-f3ff-77e2-990f-f7e37bb4cb81`。见 `create-question-fresh-http.json`、`question-fresh-pending-native.json`、`question-fresh-answer-http.json`、`question-fresh-complete-native.json`。`nativeLocation` 的固定 index.html 不能单独证明桌面选中关系，未打开的依据为本轮操作记录 |
| 两题跳过 | 通过该子场景 | 在 `call_jvlnusFEvn4ZvtsgF3QYfJef` 题卡输入临时文字后跳过；请求只有 `skip:true`，两题原生 `isSkipped=true/lastSubmission=null`；历史逐字相同，没有新增轮次或发送临时文字，刷新无卡片。见 `question-skip-pending-native.json`、`question-skip-http.json`、`question-skip-complete-native.json`、`question-skip-checks.json` |
| 纯文本队列取消/刷新 | 通过该子场景 | 原正文、上下文、逻辑 ID、独立草稿恢复；物理 ID 以原生回执返回为准，刷新仍一致。见 `cancel-edit-*-http.json`、`queue-cancel-restored-native.json`、`cancel-edit-reloaded-ui.json`、`cancel-edit-checks.json` |
| 取消后的单条自然出队 | 通过该子场景 | 前轮 `01a193ac-d256-7ba3-aae7-f6f60f5b736e` 完成后，唯一后续轮 `01a193af-ad5d-7e12-8f57-2dac6fb59347` 消费原逻辑消息；取消文字和独立草稿均未发送。见 `queue-natural-complete-native.json`、`queue-natural-checks.json`。服务端自动出队的 `turn.params` 可以不含输入和 client ID，身份应核原生 `userMessage.clientId`；保留错误校验初稿 `queue-natural-checks-initial.json`，没有为迎合校验改业务 |
| 实际 CLI service 初始化与读取 | 通过只读子场景 | 用当前 Rust `CodexCliService` 启动真实 `C:\nvm4w\nodejs\codex.cmd`，initialize、12 个模型、354 条会话、准确测试会话详情及历史读取成功；四项耗时分别 811/16832/15/140 ms。见 `service-read-real.json/.log` 和归档探针源码 `service-read-probe.rs`。不证明 CLI 真实发送或完整浏览器接管；完整侧栏读取 16.8 秒仍需性能评估 |

### 验收代码再次纠错

脚本已升级为 2.2.0，新增独立 `scenario.question_completed`。它必须等原轮 completed 后确认待答、刷新、回答、再刷新；不能趁原轮尚在执行时抢答。独立覆盖审查又指出，原通用 `finalReply` 只到带原提问标记的轮次中找回复，无法处理 completed 问题答复开启的新轮。这与本次真实 `question-answer-complete-native.json` 一致。

已改为从准确会话中定位回答后出现的唯一新轮；活动轮问答允许仍在原轮。原轮 ID 与回答轮 ID 都必须进入原生证据核对，拒绝借用旧轮、其他会话、失败、临时占位或多个候选。新增三个校验器反例回归。子代理问答源码回归先红后绿，完整桥接 **165/165**；本阶段 Node 验收/环境/UI 状态回归 **88/88**，退出码 0，日志 `node-tests-question-answer-turn.log`。这些数字仍只是源码门禁，不能代替三轮真实执行。

`remote-acceptance-coverage-20261101.md` 已逐条映射 73 条业务基线，其 04:44 冻结视图保留当时未修复项；本节是后续更新。还有运行/等待子代理、更多队列/目标/授权/并发分支、第二客户端、Android、真实前后台、Windows 完整启动视觉等待执行。另记两次成功以 medium 创建后，新建页重新显示 Max 的现象，证据 `new-chat-effort-observation.json`；尚待核对默认深度与已保存偏好的约束，未宣称根因或修复。

### 本阶段资源恢复

05:02—05:03 已恢复原安装适配器 **101**，epoch `23846ee0-dc40-41b9-82af-de8b248c92a5`；本轮 fixture 已移除。Chrome 测试 tab 的 Fetch 拦截、缓存/SW 绕过和观察器已撤回，两个测试 tab 已关闭；1433/PID 52496 已停止。用户 45222/PID 46216、45221/PID 38204、55277/PID 48596 保持监听。证据见 `browser-cleanup.json`、`native-restored-final.json`、`native-restored-inspect.json`、`service-cleanup.json`。所有本轮账本动作已确认，无本轮未知写；原始失败和测试会话保留用于后续复核。

临时 Rust 现场探针已从 `tests/` 删除，源码随证据归档。仍需复用的本轮控制脚本/测试图片在目标完成前保留；旧被拒绝清理的目录未触碰。源码未提交、推送、安装或发布，用户当前安装仍不包含本次修复。目标保持 active，完整三轮验收未完成。

## R1 补充验收与 service 写入反例（05:08 起，进行中）

本段证据目录为 `R1-followup/`，不是第二轮完整验收。沿用有创建原回执的专用测试会话，新的动作使用 `RA-20261101-R1-followup` 标记。运行原生源码仍为上述 `e8bef50...efe5`；fixture 为 `de41c57c-78e6-48b7-8818-4ace72279d27`，epoch `e42c7c8a-6e0c-446b-b4ec-d451bdccca72`。1433 临时服务 PID 59944、Chrome 3 tab 1283922542；本段调试服务配置为 WebSocket，已核实际浏览器加载的 `app.js/events-client.js/recovery-store.js` 摘要与磁盘一致。配置为 WebSocket 不单独证明每次事件都由该传输到达。

| 用例/功能 | 当前结果 | 原始证据与限制 |
| --- | --- | --- |
| 新建模型与默认深度 | 排除原疑点，未改业务逻辑 | 历史明确要求仅记住新建模型；当前产品文档也要求采用模型有效默认深度。实际目录 `gpt-6-luna.defaultReasoningEffort=max`，新建页沿用该模型/Max，回原会话仍为 medium，期间没有非 markRead 写请求。见 `models-native-http.json`、`new-chat-model-default-ui.json`、`new-chat-model-default-return.json` |
| R1-15 排序/删除正常分支 | 通过本子场景 | 三条 A/B/C 入队；键盘排序 B/A/C，删除 C，刷新 B/A；之后真实轮次按 B→A 各消费一次，C 没有被接受或回复。原生前后/回执/客户端 ID 链路见 `queue-before-order-native.json`、`queue-reorder-http.json`、`queue-delete-C-http.json`、`queue-order-delete-reloaded-ui.json`、`queue-after-order-delete-native.json`。未测试并发冲突/丢回包，也不替代未经排序的两条 FIFO 场景 |
| R1-21 既有会话设置目标 | 通过本子场景 | HTTP 为 `setGoal`，没有普通 send/create；`goalSubmission.firstTurnAccepted=false`、goalSet succeeded。原生自行执行目标轮，后续 complete；见 `goal-set-existing-http.json`、`goal-before-edit-native.json`、`goal-after-update-complete-native.json` |
| R1-23 编辑还原/取消/保存/完成 | 通过本子场景 | 还原恢复原正文，取消写请求 0；保存带 expectedCreatedAt/expectedObjective/expectedStatus，原创建时间保持，更新回执仍 active，最终相同目标 complete。独立草稿未丢。见 `goal-cancel-ui.json`、`goal-update-http.json`、`goal-updated-reloaded-ui.json`、`goal-after-update-complete-native.json`。初始计数 0、预算 null，不能据此证明非零计数或非空预算的保留；未测 stale baseline/unknown/原生 clear |
| R1-05 停止生成 | 通过本子场景 | 准确轮 `01a193d5-7c3b-7dd3-82b9-6680bdfe3921` 从 inProgress 变 interrupted，没有结束回复，另一测试会话历史逐字不变；见 `stop-*-native.json`、`stop-active-http.json`。后台终端列表为空，尚未覆盖有运行终端的停止 |
| R1-26 上下文口径 | 通过当前真实样本 | 圆环 35,499/353,400=10%，原生 last=35,499，累计=499,207，未混用累计；见 `context-current.json`。不替代零值、缺失值或跨端恢复 |
| R1-27 会话置顶 | 通过正常分支并恢复 | 原生 pinnedThreads.list 核对置顶、刷新及取消；取消后完整 threadIds 顺序与原始一致。项目视图真实显示置顶/项目/最近及 19 个当前项目分组，但未据此证明全部项目成员/项目置顶正确。见 `pin-before/after/restored-native.json`、`pin-thread/unpin-thread-http.json`、`pin-project-ui.json` |
| service 实际发送 | **failed，明确未发送** | Rust `CodexCliService` 对空闲的同一测试主会话发一次；独立 CLI initialize 成功，但 thread/resume 被拒绝：`already has an active writer`，`notSent=true/outcomeUnknown=false`。没有进入 turn/start，原历史逐字不变；见 `service-send-live.json/.log`、`service-send-confirmation.json`、`cli-before/after-rejected-native.json`。临时探针已归档为 `service-send-probe.rs` 并从 tests 删除，禁止重复运行旧消息 |

### service 反例为何改变后续方案

当前桌面即使空闲仍持有持久会话 writer；“新起 CLI 能 initialize/list/read”不能推导其可以接管同一会话写入。过去只验证读取的 demo 没覆盖这个前提，不能继续当发送兜底验收。

只读调查已确认当前 Windows 桌面的 app-server 为其自己的 stdio 子进程；默认共享 daemon/control socket 不存在，当前打包桌面的自动 daemon 路径还有 Windows 平台限制。因此不能直接套用上游 main 的 daemon/proxy 文档声称已经可用，更不能强占 writer、删除锁或 fork 成另一个会话冒充原会话。

进一步发现桌面自带 `\\.\pipe\codex-ipc`：带长度帧和版本的 `thread-owner-discovery`、`thread-follower-read-model-settings` 已对本测试 ID 只读验证成功，可定向到原 owner。它是当前安装包实际内置、带版本的 IPC 契约，**不是已公开承诺稳定的 API**。正在进行严格单次的同 writer 发送 demo，生产代码尚未改用该路径；仍须验证消息/回复闭环、来源资格、协议变化、owner 变化与未知写禁止二次路由。此项失败未被改写成设备全局只读。

本段临时 fixture、1433 和测试 tab 仍在使用；待 IPC demo 及现场取证完成后登记恢复。完整目标仍 active，尚不能交付“CLI 已兜底发送”的结论。

## 同 writer IPC、队列正反例与验收门禁追加（06:00—06:40）

同 writer 单次 demo 已完成：在 Q=`01a193a0-1755-7ad3-a984-2620c79154d0` 通过现存桌面 IPC owner 接受 client ID `e4a4b55d-bb47-42f2-bf8e-686b7d163131`，唯一 turn `01a193f5-ab97-7c30-9d99-9f00fa964c48` completed，准确回复 `FOLLOWUP-IPC-DONE`。后续纯 IPC following/snapshot 也读到同一完成记录；已取消该临时订阅。见 `R1-followup/ipc-send-{attempt,request,receipt,confirmation}.json`、`ipc-owner-snapshot.json`。这证明现存 writer 的可行路径，不能直接替代产品 HTTP/浏览器接管验收。

| 检查 | 实际结论 | 原始证据 |
| --- | --- | --- |
| 第一组队列可见性试验 | **未成立**：基础轮先结束，后续回执为 `kind=start`，不能作为非空队列样本 | `queue-visibility-next-http.json`、`queue-visibility-next-native.json`。原消息已完成，不重发 |
| 全局状态文件的非空反例 | 原生确认 P=`01a1940e-8b61-79c0-9d57-7c1f0dfdc68e`、L=`85aea3e9-421a-444e-b35c-1c9cd28fe8cb` 在活动轮中排队，但 `.codex-global-state.json` 没有该 thread 的 `queued-follow-ups` 条目。缺键不能证明队列为空 | `queue-visibility-positive2-{native,global,checks}.json`、`queue-visibility-next2-http.json`。此组独立 CLI 读取晚于自然消费，不冒充同时非空证据 |
| CLI 与原生队列的真正正样本 | 同一活动轮中，两侧按相同顺序返回 A3/B3 的 P/L；独立 CLI `thread/queue/list` 可以看到桌面的持久 server queue | `queue-visibility-positive3-{service-queue,native,checks}.json`。探针仅 initialize 与只读 queue/list，未 resume/send，子进程已退出 |
| R1-10 两条未经编辑/排序/引导的 FIFO | **通过该正常子场景**：基础轮 completed 后，A3→B3 各形成一个独立 completed turn，准确正文及回复保持，队列最终为空 | `fifo-A3-http.json`、`fifo-B3-http.json`、`queue-fifo-complete3-{native,service-queue,checks,ui}.json/.jpg`。A turn=`01a19416-6512-7691-9e25-ba3ecaeb2f36`，B turn=`01a19416-74c6-7a81-a7d1-bcde039f5356` |
| 当前测试适配器身份 | 重新读取引擎实际脚本，匹配当前 fixture，而非借用上一轮摘要 | `R1/native-engine-followup-current.json/.js`：scriptId 6578，实际/预期 SHA-256 均 `6fabcd18d33d593cdfcbc3ec3fadbd52dcba8c3d4b5f527f5f5a0e56d80c55d4`，fixture 仍为 de41、业务源码 e8bef50 |
| service 原子派发授权 | 源码已衔接：预检完成后重新取得授权许可，锁覆盖同步有界请求入队；网络等待不持锁。撤权/过期、只读/停止、原子入队与释放三项回归通过 | `remote_access.rs` 新增 service guard 适配；本机 `service_dispatch` 3/3、exit 0。不是实际设备撤权验收 |

脚本现为 **2.3.0**，案例数仍为 72。`queue.automatic` 从一条消息改为两条 FIFO；新增独立原始历史校验器，使用持久 `userMessage.clientId` 与原排队回执绑定，按原生 `turnStartedAtMs + durationMs` 核先后，拒绝同文案旧消息、重复消费、顺序颠倒、抢先执行、正文空行丢失、残留队列及仅 `queueEmpty=true` 的证明。该校验器已读取本轮真实 FIFO 原始文件并通过；离线门禁新增四项，Node 共 **92/92**，日志 `R1-followup/node-fifo-tests.log`。脚本完整 72 项并未因此被标记通过。

现场 FIFO 界面又发现一个新缺陷：最新 A/B 的持久用户输入完整，但 `turn.params.input=[]`，远程却把“编辑并再次发送”挂到更早的 base3 上。独立分析确认原生编辑会使用传入的 turnId，误选旧轮可能回滚后续历史；已继续修复准确最新消息选择与附件安全边界。未在现场执行任何历史编辑。

同 writer 生产代码已由子代理交还：`desktop_owner_ipc.rs` 8 项、`codex_service.rs` 18 项、core check 通过；仍须真实源码 HTTP/浏览器验证。`events/notifications` 的完整持续事件投影尚未实现，不能将健康探测或空事件视为消息实时到达通过。下一步使用独立 loopback 测试监听器和独立授权目录，仅放行本轮 Q 的标记发送；现有 45222/45221/CDP 保持不动。该验证器直接连接源码 service，**不冒充 NativePreferred 全故障切换验收**。

当前所有已发网页测试动作均有终态记录，无待重试未知写；测试 fixture、1433、原测试 tab 仍在使用。Windows 完整启动视觉、Android 真机、第二客户端及完整三轮仍未完成；目标保持 active。

## 源码 HTTP 验收暴露的功能失败隔离问题（06:42—07:07）

隔离验证器运行于 `127.0.0.1:1434`、PID 19152，授权目录为本轮新建目录，不改用户的 45222/45221/CDP。它使用真实 `RemoteAccessController`、HTTP 鉴权、回执及 `CodexCliService`，桥接固定为 service，且只允许对 Q 发送本轮指定前缀的消息。**此组合验证 service 主流程，不证明 NativePreferred 的自动故障切换。**

实际发现：`/api/read/relatedThreads` 返回正常 JSON 格式的功能不可用 503，前端 `requestRemote` 却对共享线路执行退避；随后 `/api/session` 可能在发出 HTTP 前就报“当前没有可用的请求线路”。调试调用栈表明 `startConnectionPoll` 因此执行 `setConnection(false)`，当时 `serviceConnected=true`、事件流 `readyState=1`。单项能力异常影响了主会话发送，违反功能隔离要求。证据为 `R1-followup/service-route-failure-evidence.json`。前两次测试操作均确认未派发、没有新轮或后端动作回执，草稿保留；没有重发原消息。

已最小修复：

- `app.js` 标记成功解析的服务业务错误；`channels.js` 对功能读取的业务 503 保留该功能错误，但不封锁共享线路、不换线路重读同一业务错误。
- 网关 HTML、非业务 JSON、响应损坏、网络失败、整个 `/api/session` 的失败，以及 429/`Retry-After` 仍保留退避；未知写入不重放。
- 使用旧实际网页脚本与修改前 channels 运行新反例，得到预期红灯“当前没有可用的请求线路”；新代码 UI/状态/传输 **18/18** 通过，含新增 4 项。日志 `ui-feature-503-before-red.log`、`ui-feature-503-tests.log`。

随后通过 `localhost:1435` 源码网页代理连接同一 1434 后端，实际 `Script` 响应摘要为：`app.js=f81327c136693adc26f852d8ca1be266929d8c295d6e87bce3ebda6a4a09c0ce`，`channels.js=1ffeb7b0cd2ffe6ffd994944db240186ff1aa0c73c11e58b2b11b169b4b3127c`。见 `service-fixed-web-runtime.json`；历史事件缓冲的较早前缀被截断，匹配的两份 Script 响应及正文仍完整，该记录没有用于证明发送次数。

| 子场景 | 结果 | 证据 |
| --- | --- | --- |
| 源码 service 经真实网页发送 | **通过单次发送/原生完成子场景** | 原 HTTP 请求只出现一次，回执 `b8d2fa11-7f35-4bd0-9112-c2b7ef4de8dc`，`nativeBridge=desktop-owner-ipc`；client `65a106f4-154a-40f4-b19f-8648af312297` 唯一进入 turn `01a19432-1f66-7e73-8ad5-ae0a4fd60db7`，completed 且精确回复 `SERVICE-HTTP-THREE-DONE`。见 `service-http-send3-http.json`、`service-http-send3-native-native.json`、`service-http-send3-receipt-store.json` |
| 最终回复自动显示 | **未完成验收，需重新执行** | 首次 UI 快照仍显示“47秒，仍在处理”，但取得快照后文件名错误导致取证中断，后来保存时间为 07:02:21；测试后端已在 07:02:11 按 20 分钟上限退出。原生完成约 07:01:40，时间窗口混入环境终止，不能拿该最终截图单独判产品失败。代码中空事件投影缺口仍成立，正在修复后重新验收 |
| 编辑最新自动出队消息 | **源码已修、真实编辑未执行** | 现在按最新真实 `userMessage` 选轮，纯文本空输入使用原生正式编辑能力；缺失图片/引用等原生输入时只禁该入口，绝不退选旧轮或重造上下文。增加模型配置/RPC 等待后的目标复核。新增 4 项，完整桥接 **169/169**。源码 `native.js=979a24fb739c4d2deb319030bb35fee0c2b96d4b1575835326c75230d380c0d9`，现场 de41 仍运行旧 e8bef50，尚未部署本修复 |

07:07 已确认 1434 验证器正常终止，1435/PID 58532 已停止；Chrome 测试 tab 1283922543 已关闭，两个 Cookie 经测试目录中的 token hash 核对归属后删除；调试跟踪断点已移除。临时访问密钥副本及设备授权文件已删除。见 `service-browser-cleanup.json`、`service-fixture-cleanup.json`。最后的空监听器查询返回非零，但清理文件和两个授权文件的不存在已独立核实，未重复执行清理。

下一次隔离服务必须记录明确 `startedAt/stoppedAt/stopReason`，留足验收时长并主动收尾；不得再将测试环境到期与产品失败混为一谈。service 持续事件、标题、完整故障切换及三轮其余场景仍待完成。两个生成 schema 目录的递归清理被自动审批以 `blocked by policy` 拒绝，相关生成文件保留，未尝试绕过。

## 第二轮独立审查与 service 实时验收（07:10—07:49）

本节的“第二轮”只指 service 子场景，**不代表目标规定的整组 R2 已执行完成**。此前 de41 临时注入已撤回，恢复安装适配器 101，epoch 为 `5cb12fbc-4041-4f55-bbad-76248cf781e0`；1433/PID 59944 与原测试 tab 1283922542 也已收回。用户三个原监听器保持 45222/46216、45221/38204、55277/48596。见 `native-restored-post-service.json`、`native-browser-phase-cleanup.json`、`phase-cleanup.json`。

独立代理按功能语义复审，又确认四项遗漏：

| 问题 | 处理及边界 |
| --- | --- |
| 原生编辑内部仍有等待窗口 | 真实原生 `tdn`/writer 在内部等待时可出现更新的空 `params.input` 用户轮，然后仍回退旧轮。更多调用前检查不构成原子保证。当前仅对新版 `__codexElvesRequestClientManager===true` 隐藏/拒绝远程已发送消息编辑；移除危险映射，队列编辑、普通发送和原生 UI 保留。真实原生函数内存夹具封堵前会 revert/rollback，封堵后调用数为 0；完整桥接 171/171。**这是阻止潜在历史回滚，安全编辑能力仍待恢复，不能标原功能恢复成功。** |
| 报告可以删除 FIFO 专项条件 | `queue.automatic` 的源码固定契约现在强制基础发送加两条入队动作，并绑定原动作 ID/队列 ID；报告删除 `naturalQueue`、历史证据或自填不需要检查均拒绝。增加完整报告可通过、删条件后失败的反例 |
| 同一消息的引导被漏算 | 完整原生历史额外检查 `steeringUserMessage` 的逻辑 ID、恢复 ID、serverUserMessageId；出现相关引导记录即不能作为自然 FIFO 通过，不能只数持久 userMessage |
| 迟到业务 503 清掉网络退避 | 功能业务错误改为对共享健康状态保持中性，既不标失败也不清除并发服务/网关失败。并发反例先红后绿，见 `ui-feature-503-concurrent-red.log`；Node 全组 98/98，`node-second-review-fixes-final.log` |

FIFO 还强制读取原生完整性字段，不能以分页尾部证明没有重复。本轮原始历史 `isComplete=true`、共 10 轮，已只读重验，见 `queue-fifo-complete3-verified-full.json`。验收辅助代码首次生成报告夹具时给被 JSON 省略的 undefined 字段留下指针，已修正夹具的序列化指针；没有放松生产证据校验。

service 新增真实 owner 长订阅、完整快照变更记录及 turn/status/item/notification 投影。基线在启动订阅前登记；连接代次独立于 owner/revision，覆盖同 owner 重连 revision 从 1 开始、旧连接迟到消息及首次快照已经 completed 的竞态。初始旧历史不会当作新完成通知重放。标题回退采用真实 sidebar/raw metadata。

新的隔离服务 PID 33688、端口 1434，生命周期明确记录为 `startedAtMs=1793489663233`、`stoppedAtMs=1793490576259`、`stopReason=explicit-test-cleanup`。在其存活期间完成以下验收：

- 实际发送回执 `3d99e28a-19b8-4b5f-ab3e-b7baef768aad`，同 writer client `a318de23-8dd0-4f32-af92-73749d5a1348`，唯一新增 turn `01a19455-c30f-7fc0-8e73-1077d15b88fd`。
- 网页运行中真实显示 `SERVICE-R2-START`，随后在没有手动刷新的情况下显示 `SERVICE-R2-DONE`，输入框回到空闲发送状态；原生独立读取确认 completed、同 client 唯一消费及准确回复。原会话标题正常显示。
- 证据：`service-r2-send4-http.json`、`service-r2-{before,final}-native.json`、`service-r2-send4-ui-{active,final}.json`、`service-r2-send4-checks.json`、`service-r2-completed.jpg`、`service-r2-runtime.json`。这证明源码 service 的通信/自动显示子场景，仍不证明 NativePreferred 的完整故障切换。

同轮工具状态仍发现新反例：网页在 `23:40:20.306Z` 把 sleep 显示为“✓”，原生同一 item 实际在 `23:40:29.985Z` 才结束。见 `service-r2-tool-status-counterexample.json`。首次 PowerShell 检查将自动解析的 DateTime 再转字符串而丢掉 UTC 偏移，错误初稿已保留；修正版直接读取原始 ISO 字符串，时间关系明确成立。当前源码已统一按真实完成字典/开始完成时间/原始结果投影 history 与事件；缺证据仍输出 unknown。前端也已对明确 unknown/中断显示对应状态，避免默认成功勾号；新增工具渲染回归，UI 22/22。**进行中/完成两态的再次真实验收尚待执行。**

本轮后台另外有两条自动 markRead 回执为 unknown。按仓库的 `[action,payload]` SHA-256 规则已识别，它们由隔离验证器的写白名单在任何调用前拒绝；完整回执已保存，没有重放，也没有记成已读通过。见 `service-r2-background-action-audit.json`、`service-r2-receipts-all.json`。显式发送账本均已确认；这不等于所有后台动作均成功。

本次浏览器与测试 Cookie、缓存/SW 绕过已收回，源码服务通过 stop 文件正常停止，临时密钥和设备授权副本已删除，见 `service-r2-browser-cleanup.json`、`service-r2-finished.json`。NativePreferred 的真实断开前兜底还发现派发前普通错误无法触发的缺口，正由另一代理按类型化派发边界修复；派发后未知结果仍禁止换路重发。目标仍 active，完整三轮及 Windows/Android 真机项未完成。

## 工具状态复测与 NativePreferred 真实兜底（07:49—08:44）

本节仍是 R1-followup 的专项补测，文件名中的 service-r3/preferred **不是整组 R3 验收完成**。以下证据均位于 `R1-followup/`，每次发送使用新的标记，未重放前次消息。

### 工具进行中、完成与未知

源码 service 验证器 PID 54764 的存活区间为 `1793491331974—1793492661778` 毫秒，结束原因为 `explicit-test-cleanup`。send5 的 HTTP 回执为 `cd6797e7-c443-4eea-8ba6-329deadb8126`，客户端消息 ID 为 `c2237185-592b-439d-99b8-478626127e68`，唯一轮次为 `01a1946e-613b-7e22-a9f0-f88e6a7f3d0f`。

| 原始状态 | 实际网页表现 | 判定与原始证据 |
| --- | --- | --- |
| sleep `call_QiJGNvfuqryP40Pcif9LCVdj` 已开始、`completedAtMs=null`、`completedItemIds[id]=false`，轮次 inProgress | 显示“sleep 进行中”，没有成功勾号 | 通过该活动子场景；`service-r3-active-native.json`、`service-r3-send5-ui-active.json`、`service-r3-send5-checks.json` |
| 同一工具完成时间为 `1793491658921`，准确轮次 completed | 工具展开后显示完成勾号，自动出现 `SERVICE-R3-DONE`，输入框回到空闲；发送后没有手动刷新 | 通过该完成子场景；`service-r3-final-native.json`、`service-r3-send5-http.json`、`service-r3-completed-tool.jpg` |
| Q 的历史中断轮 `01a193d5-7c3b-7dd3-82b9-6680bdfe3921` 缺少其 sleep 的完成证据 | 显示“? sleep 状态未知” | 通过该未知展示子场景；`service-r3-unknown-tool-ui.json`、`service-r3-unknown-tool.jpg`。这是既有真实历史，不是修改状态得到的夹具 |

service 对历史与实时事件统一使用原生明确完成证据；父轮结束、预计 duration 或墙钟经过不能推断工具成功。前端保留 unknown/中断语义。后台回执另存于 `service-r3-receipts-all.json`，显式 send5 已确认，但后台动作未逐项归类，不宣称全部成功或已读功能通过。

### 原生优先桥接的真实连接失败

验证器使用生产 `NativePreferredRemoteBridge`、真实 `NativeCdpBridge` 与真实 `CodexCliService`。唯一故障设置是给本测试实例的 NativeCdpBridge 指定端口 **63927**：该端口由本轮 socket 保留但不监听，并先实测连接拒绝；用户 CDP 55277 未改。不是伪造原生失败响应，也不是固定 service 桥接。

- HTTP 回执 `bb9ddf09-b491-4cff-9492-248e90739d02`，client `086d2550-e171-468f-9ef4-3a2f0ad1b3f5`，唯一 completed turn `01a19488-0294-7900-b4ca-654a5494c604`。
- 原生发送尝试 **1 次**、service 发送尝试 **1 次**；同 writer 接受唯一消息。网页无手动刷新自动显示 `SERVICE-PREFERRED-DONE`。原始请求、前后完整历史、计数与画面分别见 `preferred-send6-http.json`、`preferred-{before,final}-native.json`、`preferred-send6-checks.json`、`preferred-send6-completed.jpg`。
- 实际 session 为 `readOnly=false/nativeConnected=false/serviceConnected=true/capabilities.sendText=true`。模型菜单可打开，列出 12 个模型及深度项，未改选；不等于实际模型切换已验收。见 `preferred-session-state.json`、`preferred-model-menu.json`。
- 两条后台 markRead 回执 `f0cd802b-be49-4aa7-9aa0-9ae067085e15`、`98dfbb7c-9e55-4977-b7ac-bf025fd136aa` 为 `failed/notSent=true`；它们没有 service 兜底能力，未重放，也不计为已读通过。见 `preferred-receipts-all.json`。
- PID 21484 的存活区间为 `1793492998931—1793493708696` 毫秒，明确主动停止，结束计数仍为 1/1；1434 和 63927 已释放。见 `preferred-ready.json`、`preferred-finished.json`。

派发前连接/准备失败现在使用内部 `CdpCommandNotDispatched` 类型证明。注入/管理器发现阶段只有确切未派发条件才带结构化证明；业务拒绝、权限失败、动作内部伪造证明及派发后的超时/断线/unknown 均不得转 service。没有兜底的动作在确切未派发时记 `failed/notSent`，不能混成送达未知。相关回归覆盖了这些反例；**当前真实试验只覆盖连接拒绝分支，正常原生恢复接管仍待验。**

### 当前源码、检查与收尾

| 当前源码 | SHA-256 / 验证 |
| --- | --- |
| `assets/remote-access/native.js` | runtime **106**；`B5E24A56C25222A5B8D305599F6BAEC5530CAD4CADF88BE6AF14466B9BEBD74A` |
| `crates/codex-elves-core/src/codex_service.rs` | `5A2FF52FB08D3D4013BD21A79AE132D657E62D18CA6F5E24D8DAB469EAF170DF`；service 27/27 |
| `crates/codex-elves-core/src/desktop_owner_ipc.rs` | `D8A764CC69DAA9310D67DD39BC49CCAF9E6005F90C67484D3DC8160D390966D9`；IPC 12/12 |
| workspace / 格式 | `cargo check --workspace`、`cargo fmt --all --check` 通过；`workspace-check-final.log` |
| 完整桥接回归 | **172/172**，退出 0；`cdp-106-final.log` |
| Node 验收、环境、UI 状态回归 | **101/101**，退出 0；`node-final-stage.log` |
| 差异检查 | `git diff --check` 通过 |

上述都是源码/隔离实例证据，安装适配器仍为 101；106 尚未作为正常原生 fixture 真测。不能直接让源码 106 与用户旧后台互相替换适配器，也不能把此前 105 的局部成功全量回填为 106 正常主流程通过。

所有显式动作账本均已确认终态，其中包含明确未发送；“confirmed”不代表每次业务成功。所有本阶段测试服务和标签已结束，测试 Cookie、缓存/SW 绕过、密钥/授权副本已收回；两种临时 Rust 探针已归档并从源码删除，临时模块入口已移除。`latest-cleanup.json` 确认 1433/1434/1435 不监听，原 45222/45221/55277 保留。两个 schema 目录的清理曾被自动审批以 `blocked by policy` 拒绝，保持原状，未绕过。

接下来仍须完成：73 条基线未执行的正常/失败/恢复分支、正常原生恢复接管、子智能体完整生命周期、真实前后台、Windows 完整启动视觉、Android 真机及第二客户端。新版已发送消息编辑目前仅封堵危险回退，**安全编辑能力未恢复**。未构建、安装、提交、推送或发布；目标继续 active。

## 生命周期补测与命名验收纠正（09:31—09:55）

证据目录为 `R2-lifecycle/`。本段复用有创建回执的 M/Q 做读取、草稿和偏好补测，另只读观察本任务真实子智能体；**不是整组 R2 已完成**。没有发送新的模型消息，没有操作用户根会话的队列、目标或问答。

### 运行身份

- 当前 106 源码以明确登记的兼容变体运行：旧后台所需版本标记仍为 101，自动侧边命名仍只允许测试白名单（本轮为空）。fixture=`9bdc3d95-6f38-4ca4-8fe5-8f772ea97cba`，epoch=`e2825f13-3796-4b2b-ade5-71afca714831`。业务源码摘要仍为 `b5e24a56…b74a`；实际引擎 scriptId 12762 的完整摘要 `48213b307bd3f2b2634191b7595a1aae7acf4c574e91f56fc5af6aeb6be49b50` 与登记变体逐字一致，见 `native-installed.json`、`native-engine-current.json/.js`。
- 独立源码 HTTP 服务为 1436/PID 51404，运行区间 `1793496720799—1793498099134` 毫秒，主动停止。它使用真实授权/HTTP/回执和当前原生适配器，读取时校验 fixture 身份，**不执行正常 NativeCdpBridge 的注入过程，也不是 NativePreferred 恢复接管验收**。唯一允许的写动作是对三条已登记测试会话 markRead，其他写操作在原生调用前拒绝。归档源码 `source-http-lifecycle-probe.rs`。
- Chrome 3 的测试标签为 1283922552（localhost）和 1283922555（127.0.0.1），两个 origin 使用同一测试服务但各自真实登录，device ID 分别为 `a76fd424-1ffc-4c22-be0a-e6f2fe3de29b`、`8b628063-2b5a-432d-9b61-f8ecd73cf958`。这是同一电脑的两个浏览器客户端作用域，不是两个物理设备或 Android 真机。
- 实际 Script 响应与源码匹配：app.js=`2733981990fa04aa74c5c796c696485334ea00dd7e3d41c3406ecbabc456bcb4`；channels.js=`b154046c65cd8d4211213db985909aff124f8a7eb7e09fe30ceb4e6959c08127`。其他模块摘要、客户端身份和原始响应索引见 `browser-runtime.json`、`preferences-client-identities.json`。

### 实际结果

| 基线 / 子场景 | 结果与独立证据 | 保留的边界 |
| --- | --- | --- |
| R2-01 切换会话时取消旧读取 | Q→M 时挂起 M 的真实 200 thread/history 响应，切回 Q 后两请求出现 `canceled=true/net::ERR_ABORTED`；Q 的历史、模型和上下文保持正确。见 `held-12760.166.json`、`slow-read-before-return.json`、`slow-read-network.json`、`slow-read-after-release.json` | 旧响应已取消，后续继续传递得到 Invalid InterceptionId；这是取消生效的证据，不能宣称“迟到结果已经送达但被忽略”分支也执行了 |
| R2-02 两行文字草稿 | `RA-20261101-R2:foreground-draft\n第二行（未发送）` 在 Q→M→Q、整页刷新及网络故障恢复中逐字保持。见 `draft-reloaded.json/.jpg`、`offline-preferences-final-refresh.json` | 图片、Skill、目标模式和系统回收仍未验证。收尾只清除了这份本轮草稿 |
| R2-09 两个客户端视图独立 | 甲项目/乙最近→各自刷新保持；再改为甲最近/乙项目→各自刷新保持；同一 Q 只有甲有上述草稿，乙为空。见 `preferences-first-refresh.json`、`preferences-second-refresh.json`、`preferences-client-identities.json` | 原服务设备上限没有被修改；跨物理设备、Android、桌面改排序的同步边界未执行。当前可见 UI 仅找到视图切换，没有排序选择入口；不能用存储模块测试冒充排序 UI 验收 |
| 本客户端偏好不依赖连接 | 只对测试 tab 施加网络故障，真实 sidebar/thread/models/skills/history 请求均出现 `ERR_INTERNET_DISCONNECTED`、页面显示连接中断后，仍可切换并保存最近视图；联网刷新保持，草稿未丢。见 `offline-disconnected-network.json`、`offline-disconnected-ui.json`、`offline-preferences-final-refresh.json` | 用户服务/CDP 未断开，故障范围仅该 tab 的传输；不等于完整原生故障切换通过 |
| R3-15 网络恢复 | 恢复网络后自动恢复可用；真实 `/api/session` 为 nativeConnected=true/readOnly=false，最终 UI 发送可用、草稿保持。见 `network-recovery-session.json`、`network-recovery-latest.json` | 恢复时间 `01:48:07.595Z`，首个成功 session 的 lastSeenAt 为 `01:48:47.063Z`，下一次界面采样确认可用是 `01:49:33.401Z`。约39秒的首次成功读和约86秒的确认上界不是精确界面恢复耗时；短等待失败保留，不能宣称立即恢复或性能验收通过 |
| R1-28 / R2-06 子智能体 active→done | 当前真实子智能体 `01a194b8-9870-72c2-bcad-36d9a33bebdd` 运行时，原生/远程同为14项，其中1项 active、13项 done；完成后同 ID 进入已完成组，显示14完成，名称 `Side name acceptance contract` 保持。准确父关系及只读身份一致，详情没有可见消息输入框。见 `native-subagents-during-acceptance-review.json`、`native-subagents-after-acceptance-review.json`、`subagent-active-menu.json/.jpg`、`subagent-completed-menu.json/.jpg`、`subagent-readonly-view.json/.jpg` | 数量来自本次原生列表，不是固定期望。等待状态、孙级、多父和订阅失败/恢复仍未执行；本轮未创建专用子智能体来制造状态 |
| 真实前后台恢复 | **仍未成立**：创建第二真实 tab、导航、bringToFront 后，原 tab 一直 visible，没有 visibilitychange。已撤回仅用于观察的监听器。见 `foreground-not-established.json` | 不发送合成可见性事件补过；本轮慢读取/断网成功都不能替代前后台与 Android 验收 |

第一阶段偏好网络记录完整，没有 sort 操作；第二阶段浏览器事件缓冲已截断，不能据其空列表宣称全程零写入。独立服务最终保留的 **7 条动作回执全部经原始 `[action,payload]` 摘要确认是 M/Q 的 markRead，均 completed**，见 `receipts-final-store.json`、`receipts-final-audit.json`。这只是后台已读动作审计，不是两轮 unread→read 生命周期通过。Q 的15轮完整历史前后逐字相同、队列仍为空，见 `lifecycle-no-send-check.json`。

### 侧边命名验收修订

独立修订将 runner 升至 **2.4.0**、evidence 升至 **1.1.0**。六项侧边命名案例取消“首条正文前十字”、固定语言/编号的错误假设；以准确 ID 的原生 `tab.title` 为准，先取得两次间隔至少1000毫秒的独立同名采样，再核远程详情、父列表、整页刷新及刷新后的新原生观察。1000毫秒只是取证采样窗口，不是产品命名规则。自定义名仍必须与真实改名回执一致。

新增反例拒绝错 ID、旧读取、相同 read ID 复用、择取较早同名快照、缺刷新文件、报告伪改及借用其他消息。修改仅限既有 runner/evidence/长期测试和说明，未改变生产命名逻辑。新的 Node 全组 **107/107**、退出0，见 `node-2.4-final.log`；**新命名流程尚未在真实桌面 UI 执行，不能沿用旧2.3.0结果填通过。**

### 本阶段收尾

两个测试登录已通过真实 UI 退出，测试 Cookie 清除，127.0.0.1 上测试前已有 Cookie 的值及有效作用域/安全属性已恢复核对；私有备份随后删除。缓存/SW绕过、Fetch读取拦截、离线设置和观察器均撤回，测试标签全部关闭，见 `browser-cleanup.json`、`cookie-cleanup-provenance.json`、`logout-ui.json`。辅助标签1283922553已在可见性试验后关闭。

1436验证器正常退出；临时 Rust 文件已归档删除；测试密钥、设备授权、锁文件和 Cookie 备份均已删除。恢复安装适配器101，epoch=`86dfc88b-d2c0-4282-9882-328fce60c063`，原安装器函数摘要保持一致，进行中/未知写状态映射均为空；原45222/45221/55277及其PID保持。见 `service-finished.json`、`native-restored-inspect.json`、`cleanup.json`。之前被拒清理的schema目录和旧脚本没有触碰。

目标保持 active。缺口仍包括真实前后台/Android、Windows视觉、完整原生恢复接管、已发消息安全编辑、排序入口以及各用例尚未执行的异常分支。未提交、推送、打包、安装或发布。

## 独立优化与只读草稿的新反例（10:06—10:34）

本段是专项失败/恢复补测，不是整组R3完成。证据目录分别为 `R3-optimizer/` 和 `R3-draft-recovery/`。原计划继续问答跨会话审计，实际独立优化复测发现草稿丢失后优先修复；问答跨会话仍列未完成，不以本段替代。

### 独立优化的真实供应商闭环

服务1437/PID62128从 `10:06:06.460+08:00` 运行至 `10:22:45.014+08:00`，明确主动退出。该独立实例的原生传输只连接本轮保留但不监听的socket，实际连接拒绝；没有创建CLI service，也没有注入或停止用户Codex。页面实际session为 `nativeConnected=false/serviceConnected=false/readOnly=false`，优化能力可用而会话发送不可用。**这验证独立优化，不是NativePreferred切换或安装版验收。**

使用本机原有优化设置的隔离副本：模型gpt-6-luna、high、concise；只提交本轮合成草稿，没有选择用户真实会话或附带其上下文。实际加载app.js摘要为旧的 `2733981990fa04aa74c5c796c696485334ea00dd7e3d41c3406ecbabc456bcb4`。原始HTTP、供应商诊断和UI分开保存：

| 子场景 | 原请求 / 实际结果 | 结论和边界 |
| --- | --- | --- |
| 优化并恢复原文 | `43b3b977-e536-42f6-875b-ce4bb07e5702`；供应商HTTP200、667 Token，页面文字逐字等于真实结果；点击恢复后逐字回到两行原稿 | 通过该子场景。`optimize-1-http.json`、`optimize-1-result.json`、`optimize-1-restored.json`、`optimize-success.jpg` |
| 尝试取消运行中请求 | `7ec03c02-4dd9-40c6-8beb-6eac906b4fc0`；实际优化已在点击取消前完成，取消请求数0 | **取消场景未执行，窗口错过**；真实完成结果保留，没有重发该请求。`cancel-2-window-missed.json`、`cancel-2-http.json` |
| 完成结果迟到 | `20ab9a32-7fe8-41f5-8f81-fa0900fd7bdf`；仅暂缓真实 `promptOptimization` 完成响应，后输入新稿，再释放同一个响应；Network.loadingFinished证明已送达，页面仍为新稿 | 通过该迟到子场景。一次optimize、一次编辑触发的cancel；服务端此时已完成，不能把它说成上游运行中取消。`late-3-held-completed.json`、`late-3-edited-before-release.json`、`late-3-network.json`、`late-3-after-release.json`、`late-3-checks.json` |
| 取消先于start到达 | `8cf071b8-99e0-41b3-a765-c6ed5ecb4145`；仅延迟原optimize请求，UI取消先得到服务端cancelled；然后释放原请求，仍返回cancelled，原稿保持 | 通过该竞争子场景，**供应商调用0次**。`cancel-4-before-release.json`、`cancel-4-final.json`及两份租约恢复记录 |
| 真实只读授权 | 修改仅本测试服务的read_only策略，刷新后优化和输入均禁用，观察期0条action请求 | 权限禁用子场景通过；随后恢复可写时发现下面的新草稿丢失，不能把整个权限生命周期标通过。`readonly-session.json`、`readonly-ui.json`、`writable-restored.json` |

`provider-evidence.json` 按真实device ID和原请求ID重新计算服务内部作用域ID，与原始日志关联：前三项各启动/完成1次，第四项0次；共3次实际上游请求及3次HTTP200响应，总计2090 Token，没有failover重试。`diagnostics-original.jsonl`保留原日志，过滤摘要不包含endpoint。原生桥接计数只有events/models/sidebar，没有send/optimize调用，没有CLI实例。

一次临时计数把Network.requestWillBeSent与responseReceived同时计入，显示2；已按原始方法重新核对为请求/响应各1条，并在 `late-3-checks.json` 固定“只数requestWillBeSent”的规则。没有将该取证计数错误解释为产品重复派发。

### 新会话草稿丢失的原因和修复

实际反例：取消先行测试留下的原稿在只读页刷新后消失；恢复可写并刷新仍为空。原device.id和createdAtMs均保持，排除了换授权导致的隔离清理；第四项没有供应商调用，也没有会话发送。

原因链：恢复已有 `$new:` 草稿 → 初始只读页进入 `showWelcome()` → 页面尚无selectedId/creating，编辑器为空 → 无条件 `rememberDraft()` 把空输入写入 `$new:` → 页面退出又可能保存该空占位 → 恢复可写时读到空稿。缺陷是把没有实际所属会话的欢迎页当作可保存草稿的编辑页；已有回归覆盖前后台读取、授权作用域和普通草稿，但没有只读欢迎页的空输入覆盖场景。

最小修复仅在 `rememberDraft()` 加入 `!selectedId && !creating` 的提前返回。真实会话、新建模式中的主动清空和准确队列编辑事务仍走原保存路径。新增长期回归3项：欢迎/只读初始页不得覆盖原草稿、真实编辑页主动清空仍生效、队列编辑不覆盖普通新稿。旧实现得到 `2 !== 0` 的两次非法保存红灯，修复后3/3通过，见 `draft-owner-red.log`、`draft-owner-green.log`。

### 修复后的真实复测

新服务1438/PID36920运行于 `10:25:27.229—10:31:34.835+08:00`，其真实Script响应摘要为 **`11439f4686d415a5c291db2e94ea7158bf640d1a760a05052b02f67302e7b97f`**，与修复源码一致。保持原生连接拒绝、无CLI、没有模型调用，只操作本轮两行新草稿：

1. 正常输入后进入真实只读模式并刷新，session.readOnly=true，输入/优化禁用。
2. 再次刷新后恢复可写并重新加载，两行原稿逐字恢复；device.id/createdAtMs保持。第二次读取取证时后台已更新为可写，所以不把它声称为“第二次只读状态确认”。
3. 用户动作主动清空输入，再刷新仍为空，没有因保护逻辑而复活旧稿。

原始对照见 `draft-before-readonly.json`、`readonly-first.json`、`readonly-second.json`、`writable-recovered.json`、`permission-identity-comparison.json`、`draft-recovered.jpg`、`explicit-clear-after-reload.json`。`final-checks.json`确认本复测0次optimizer启动/0次上游请求。当前Node完整组 **110/110**、退出0，日志 `node-final.log`；未变化的Rust桥接没有重复运行。

### 收尾与未完成项

两套测试服务均主动结束，1437/1438已释放；两个测试登录通过UI退出，Cookie、缓存/SW覆盖及Fetch拦截已恢复，标签关闭。临时Rust验证器已归档并从tests删除；用户原45222/45221/55277和PID保持，本段未替换任何原生适配器。

包含旧私有文件清理及下一轮环境准备的组合命令被自动审批以 **`blocked by policy`** 拒绝，整段未执行。没有拆分重试其中的删除动作；后续采用不含删除的新环境完成复测。`temp/remote-acceptance-20261101/optimizer-source-http/` 与 `optimizer-draft-recovery/` 的本轮配置/授权副本仍保留在忽略目录，未提交或发布；清理状态见 `R3-draft-recovery/cleanup.json`。此前被拒的schema目录和旧脚本也未动。

运行中上游取消、跨真实会话优化迟到、上游失败与未知回执的完整分支仍待验；本段三个成功子场景不等于全部优化用例通过。真实前后台/Android、Windows完整启动视觉、原生恢复接管、安全编辑及完整三轮仍未完成。目标继续active；本段未提交、推送、打包、安装或发布。

## 问答切换、真实丢回包与残留删除锁（10:50—11:33）

本段为 `R2-question/` 专项补测，不是整组R2/R3完成。使用原有可追溯测试M=`01a1938d-d3e8-7440-8f4b-bcda032ac277` 和控制Q=`01a193a0-1755-7ad3-a984-2620c79154d0`，不操作用户根会话的待确认数据。

### 运行身份与取证纠正

- 原生106业务逻辑/101兼容标记，fixture=`cded4b3f-7b89-4314-b2b9-3dd2854d20cd`，epoch=`cf3d27e7-880d-40d4-ac84-d9cb9314c3eb`；实际引擎scriptId14994，摘要 `5283516414185c7dfa28824122b3c336bc74971bc2ec0ef34174bdcb2d166daf` 与登记变体一致。
- 源码HTTP1439/PID45316，运行区间 `1793501424519—1793503989348` 毫秒，主动停止。桥接只放行M中本轮前缀的发送、M中经过原生证据核对的准确问题ID回答，以及M/Q的markRead；写入前仍使用最终授权派发检查。它调用真实适配器但不执行正常NativeCdpBridge注入过程，不能冒充NativePreferred完整接管验收。
- 修复前网页app.js为 `11439f4686d415a5c291db2e94ea7158bf640d1a760a05052b02f67302e7b97f`；修复后为 **`cda5c76dac0f20ed75a3bbd3d31394842f411b71257195350ae5ffeb29055fe4`**。后一次只在同一测试tab重载时，将app.js静态响应替换为当前仓库的准确源码，真实HTTP后端、授权和原回执保持不变；实际Script响应正文已逐字核对，随后撤回拦截。没有替换业务响应或注入成功状态，也没有更新安装资源。见 `runtime-before-receipt-fix.json`、`original-receipt-recovered.json`、两份 `loaded-app-*.js`。
- M开始时为needs_resume/notLoaded，`getLoadedConversationHistory`为空且isComplete=false；这不是没有历史。原生发送协调器正常恢复该主会话；后续内存历史也仅有7轮、仍不完整。因此完整性和唯一次数另用独立CLI **只读** `thread/read includeTurns=true` 核对，未resume或发送。原始JSON指针为 `/result/result/thread/turns`，前后10→12轮；两个只读CLI进程均已结束。

### 真实用户操作结果

| 子场景 | 结果及准确关联 | 原始证据 |
| --- | --- | --- |
| 原生普通发送与双题发现 | 发送回执 `eb41e762-b673-4737-a82a-740128069c5d`，client `bb15a09a-b9a3-49d4-b6f9-4e14ad12f47d`；原提问轮 `01a19508-d927-7e10-9f3b-1b2d64b957a6` completed，item `call_EjKn7bv1rLpADrBAIj33BHS1` 的两题仍可答 | `ask-1-http.json`、`question-pending-native.json` |
| 未提交选择/文字跨会话 | M选绿色、输入“方形（未提交的文字草稿）”，再到Q填写独立普通草稿，再回M；选择、文字及M普通两行草稿均保持，期间answer请求0次 | `question-draft-before-switch.json`、`control-draft.json`、`question-draft-after-switch.json`、`question-selection-network.json`，事件未截断 |
| 刷新仍发现原未答题 | 同一turn/item/问题ID在刷新后保留，原生仍未答 | `question-after-refresh-native.json`、`question-after-refresh-ui.json`。未提交的问答选择/答案不持久保存，刷新不承诺保留它们；普通输入稿另有持久恢复 |
| 单次提交绿色/方形 | 回执 `a05eca15-76f6-4c55-8086-1123a4581c24`；真实服务端completed；准确问题ID在新回答轮 `01a1950f-4816-73d1-8256-f22034dd8cb4` 接受，原生逐题lastSubmission为绿色/方形，最终唯一 `R2-QUESTION-ANSWER:绿色|方形` | `answer-held-http.json`、`answer-completed-native.json` |
| 丢失回答回包 | 仅对上述真实完成回包执行一次传输失败，然后撤回拦截；客户端保留unknown元数据，未再次提交 | `answer-response-lease.json`、`answer-response-restored.json`、`answer-client-after-drop.json` |
| 原缺陷：残留删除锁 | 原生已完成、无待答题/队列，连接正常且普通发送可用，“删除会话”仍禁用；整页刷新后unknown仍在，对原回执GET0次、回答POST0次 | `answered-but-delete-locked.json`、`answered-lock-after-reload.json`；不是断网或运行中导致的禁用 |
| 修复后核对原回执 | 新脚本实际GET同一个a05eca15…回执，取得completed后本地元数据变为`[]`，删除入口恢复；没有新的answer提交 | `original-receipt-recovered.json`、`answered-delete-unlocked.json`、`unlocked-menu-painted.jpg`。仅检查入口，未执行删除 |
| 保护历史与普通草稿 | 完整历史10→12且原前10轮逐字相同，提问/回答各1次，原生bridge send=1/answer=1；M/Q普通草稿均未进入模型输入，Q草稿在回答/恢复后保持 | `before-question-service-history.json`、`after-answer-service-history.json`、`question-answer-final-checks.json`、`control-draft-after-answer.json` |

最初的菜单截图早于弹层绘制，不能证明菜单状态；最终可见菜单使用 `unlocked-menu-painted.jpg`，DOM/命中层同时核对。脚本替换的首次事件批次只有浏览器管理的Document事件，未替换源码；已撤回并保留 `source-script-first-attempt.json`，之后等待准确Script事件才加载修复文件。两类取证问题没有被算作产品失败或补造通过。

### 原因、修复与独立复审

原先 `applyQuestionReceipt` 仅由首次回答调用。HTTP回包丢失后，sessionStorage保留pending/unknown，但刷新、联网和后续连接轮询都没有读取原回执；原生已清题卡并不清理这个客户端锁，`threadDeletionConflict` 因此持续拒绝删除。

新增 `recoverQuestionReceipts` 只GET原ID，在初次连接、前台/在线恢复、提交结束和15秒连接状态轮询触发；同键同ID的并发查询共用一次请求，返回后复核当前授权scope和操作ID。`applyQuestionReceipt` 同时核对响应顶层回执ID及已返回的线程/问题身份；明确成功仅清原锁，明确失败保留答案草稿，outcomeUnknown/404/读取失败继续保留记录。不会重新调用answer或skip，不依赖原生连接；后台不启动查询，晚到终态不会启动后台历史读取。

新增6项长期回归覆盖原ID核对、失败/unknown/404、并发合并、新旧操作替换、换授权、只读/无原生及跨会话隔离。初始红灯记录显示没有查询及旧回执清除新锁的问题；最终 **Node全组116/116、退出0**，`node-final.log`。独立代理按当前源码和本轮原始证据复审，未发现本次新增缺陷；其定向6/6不重复计入116。

`docs/remote-access.md` 已纠正旧的“轮次结束即移除未答题”，并明确页面内未提交答案与普通持久草稿的不同范围，避免后续验收再按过时描述误判。当前72个E2E处理器仍未完整接入本次丢回包人工流程，完整自动化映射仍须补齐。

### 收尾与剩余范围

全部本轮显式动作已有服务端/原生终态；未重放原答案。仅清理了M/Q中本轮标记的普通草稿，测试会话和历史保留。测试登录已退出，Cookie、缓存/SW覆盖及所有拦截已撤回，Chrome测试tab1283922562关闭。服务1439正常退出，临时Rust探针及父模块入口已归档移除。

原安装适配器101已恢复，epoch=`51d8209e-62ca-46b2-a1a0-1b7889537341`，安装器函数摘要保持；进行中/未知写映射均为空。原45222/45221/55277及PID保持，见 `native-restored-inspect.json`、`service-finished.json`、`cleanup.json`。此前被拒的私有文件清理未重试，本轮私有测试目录也暂存；无测试进程残留。

真实failed/404回执、真实换授权/后台、同轮同item更换实体等分支仍未完整执行；已补源码保护不能替代这些运行证据。本次只验证删除入口解锁，不等于永久删除全流程通过。完整三轮、Android/Windows现场条件、原生恢复接管和已发消息安全编辑继续未完成。目标active，未提交、推送、打包、安装或发布。

## 问答恢复正式处理器与验收器二次纠错（11:43—12:16）

本阶段只改验收资产和文档，没有修改生产逻辑或再次发送已完成的测试消息。`recovery.question_receipt` 已加入 runner 2.5.0；文件证据门禁升至1.2.0。处理器数由72增至73，与73条业务基线仍非一一对应。

固定流程包括：原始完整历史→completed多题→同题刷新→单次回答丢回包→原回执unknown→新文档GET同ID→准确逐题接受→原锁与删除入口恢复→两份普通草稿和旧历史保持。原题turn/item/entity、外层服务回执ID、内层问题request ID与实际回答轮分别核对。网络计数由未截断的原始CDP事件重新计算；仅有“零重发=true”、题卡消失、部分历史或驱动自报passed都不够。驱动在提问前开始观察，暂阻断原回执读取直到新文档开始，避免把修复后的即时自动恢复误算为刷新恢复。

| 独立复核轮次 | 发现及处理 | 证据状态 |
| --- | --- | --- |
| 契约复核 | 普通send未知回执和正常问答两项不能合并冒充本缺陷验收；确定单独处理器及固定六阶段 | 已落实至源码和说明 |
| 实现复核 | 将Document推迟到“刷新后截图”之后、但早于解锁截图，旧门禁仍通过 | 已增加Document不晚于reloadedUi的约束；具名反例拒绝原变异 |
| 实现复核 | 原生终态题目保留正确答案但state.turnId错误/sourceTurnStatus失败，旧门禁仍通过 | 已逐题核对原轮及completed；两个字段各有反例 |
| 修复复核 | 上述两处修改与反例 | 独立定向10/10；未发现新的确定缺陷，不重复计入全组 |

完整Node验收/环境/UI组 **126/126、退出0**；`git diff --check`退出0，模块导入确认73个定义均有处理器。首轮124/124仅是增加两个复审反例前的结果，最终以126为准。日志为 `R3-question-contract/node-final.log`。旧Rust/桥接检查未重复执行，本阶段没有对应生产源码变动。

**真实执行仍未完成：** 新处理器依赖本轮精确故障、完整网络和原生读取驱动，本阶段未启动新测试环境。此前 `R2-question/` 的人工红绿证据保留，但不改成2.5.0执行记录。真实failed/404、换授权、前后台、双端竞争等也不能由本例的离线反例替代。目标保持active。

### 原service-R3后台回执归类

按归档 `service-r3-receipts-all.json` 的payloadHash与实际Rust `SHA-256(JSON([action,payload]))`重算，两个unknown均精确匹配Q的`markRead`：`f8c68d56-8c2c-4bf0-ac48-3cf55267459d`、`3a576669-01d3-4545-ace5-f1a4cc9c2325`。原验证器`allow()`不包含markRead，且在service调用之前执行，因此这两条被隔离验证器先行拒绝。显式send5 `cd6797e7-c443-4eea-8ba6-329deadb8126`仍为completed。

分类文件 `R1-followup/service-r3-background-receipts-classification.json` 保存输入文件及验证器哈希；这是只读归档分析，不是新运行。未改写旧unknown、未重放任何动作；这两条不能算已读功能通过，也不能据此否定已单独证实的send5完成。

## 正式处理器真实接入与跨VM边界修复（12:20—13:40）

本轮证据目录为 `R3-question-live/`，计划标记 `RA-20261101-R3-question-contract`。独立源码服务1440/PID50244使用实际原生106业务代码，兼容标记为101，原服务45222/45221/55277未重启。浏览器实际收到的app.js与当前源码逐字一致，SHA-256仍为`cda5c76d…055fe4`；原生实际执行脚本也通过源码比较。运行包目录版本为26.1027.3384.0；内嵌执行程序ProductVersion 156.0.8078.48是另一字段，未将其当作Codex产品版本。

### 真实结果与失败记录

| 案例或步骤 | 实际结果 | 原始证据 |
| --- | --- | --- |
| 首次控制上下文启动 | failed，业务提交0次。浏览器调用不能在工具调用结束后通过悬空Promise继续执行 | `bootstrap-failure.json`、`runner/report.json`；动作账本为空，原生派发文件0个 |
| 两条普通会话创建及首条回复 | passed（本子场景），每条完整历史各1个completed轮、1条原文、1条准确回复 | `runner-sync/report.json`、两个`*-action-proof.json`、`created-*-final-history.json`、`created-conversations-final-checks.json` |
| 创建证据回读 | 首次读取网络缓存时发现截断，未放行；随后只读原生派发记录与服务端payloadHash/回执精确匹配，补读通过 | 创建次数始终2次，未重建或重发；原失败attempt保留 |
| 两会话切换及整页刷新 | passed（本子场景），仍是准确ID和本轮标记 | `runner-sync/`中的DOM/截图；`created-conversation-final.jpg` |
| `recovery.question_receipt` | failed，租约获取前scope比较错误；恢复步骤也报错。尚未启用故障、未发提问或回答 | `runner-sync/report.json`、`driver-scope-realm-check.json`、`service-finished.json`的sends=0/answers=0 |
| JSON观察修复 | 真实插件返回的数组/对象来自另一VM。修复前原型不同导致相同JSON比较失败；归一化后与保存文件逐值一致 | `driver-observation-vm-fixed.json`、`runner-vm-check/`；无新增消息 |
| 租约正常接入复测 | 修复版真实acquire→start-observation→restore→inspect通过，恢复原状态 | `driver-vm-fixed.json`、`driver-vm-preflight/`；只测技术控制接入，未丢弃回答回包 |

两条新测试对象：

| 对象 | thread ID | 创建原回执 | 唯一完成turn |
| --- | --- | --- | --- |
| main | `01a19576-cb27-75a3-a7ef-da43a6543f8c` | `6918bcdd-c1b2-4f97-a2d0-2f453d21763b` | `01a19576-cbe6-70c0-bc16-f7d98e983f61` |
| control | `01a19576-f8e3-75c0-826e-0e22a598a4ad` | `293de31d-2417-45f2-ae59-a0dffb509ced` | `01a19576-f974-79b1-be06-62fd3cba01e7` |

### 本轮修正范围

- 新增长期`remote-access-e2e-receipt-driver.mjs`和对应控制流测试，实际使用准确tab的CDP，保存完整事件窗口，按原回答身份单次丢回包，并在新文档开始后放开原回执GET。
- 驱动scope/request/CDP返回值/客户端元数据统一按模块内JSON解析。实际`jsonIdentical=true/deepEqual=false/samePrototype=false`证明原判断错误来自跨VM原型，并非scope字段变化；保留字段、值、额外键和所有权严格校验。
- runner保存DOM观察时也做JSON归一化；关闭会话菜单使用准确的展开按钮，不向禁用菜单项发送Escape。后者不点击删除，也不修改会话数据。
- 获取前明确无副作用的失败可按原尝试收尾；已登记但取证失败仍恢复原租约。另发现`close()`首次清理失败后再调用可能空返回的缺口，已进入同一驱动的修复和反例复核，不将“调用close”当作“恢复成功”。

最近完整Node组为145/145，退出0，日志`node-final.log`；新增驱动18项和跨VM DOM观察1项计入其中。首次VM测试夹具缺少reload方法，已修正夹具并保存初稿失败日志，不能把该失败当产品反例。后续清理重试分支的最终结果另记，不沿用145作为未检查补丁的通过数。

### 收尾

测试登录已通过页面确认退出，Cookie核对已清除，tab1283922566关闭；更早仅用于准备的tab1283922564也已关闭。服务1440正常退出0，最终计数creates=2、sends=0、answers=0。原生未知/进行中映射均为0后恢复安装适配器101，epoch=`1492ddb7-3461-4397-9367-1a91d317fc80`，fixture=null，原安装器函数摘要保持。原45222/PID46216、45221/PID38204、55277/PID48596仍在监听。

本轮临时Rust入口、原生读取器和接入绑定脚本已逐文件归档并校验后删除；本轮访问密钥副本与Cookie备份已删除，审计原始数据仍保留。旧的清理拒绝未重试。没有提交、推送、打包、安装或发布，也没有改动用户队列和非测试会话内容。

本轮没有完成新问答故障链路，原先R2人工成功仍保留其原范围。下一次必须用稳定驱动及明确账本执行，不把上述技术预检或已知首条回复拼成R3-09通过。Android路径与默认SDK位置再次检查仍未发现可用adb；Windows原生视觉、Android真机、其余失败/恢复分支及完整三轮继续未完成，目标active。

### 异常清理重试复核完成

`close()`原先无论恢复结果如何都会设置closed，再次调用可能空返回；失败recoveryTask也被永久复用。现仅在实际恢复确认且证据落盘后关闭、释放所有权。并发close/restore共享进行中的工作；已终态失败保留在历史中，后续显式调用可仅恢复同一原租约。成功后重复close返回原证据，事件截断或读取失败不能仅凭状态字段相同就假称恢复。

新增6项mock反例覆盖首次清理失败、超时后重试、close/restore并发、同次失败共享、状态相同但读取失败、截断后持续blocked。驱动24/24，最终完整Node组 **151/151、退出0**，日志 `node-cleanup-retry-final.log`；145是该补丁之前的历史检查，不重复计数。异常清理分支本轮没有真实制造，不能用mock结果替代该现场验收。正常租约的真实预检见前述独立源指纹记录，不重新包装成最新故障链路通过。

测试辅助清理曾在目录删除后才关闭驱动，导致收尾JSON重建目录；现已调整为先关闭全部辅助驱动再删除目录，完整复跑未新增残留。以下3个旧mock目录仍存在，自动审批拒绝了递归删除及进一步收窄后的单文件/空目录删除，均仅返回`blocked by policy`，未提供更具体理由。已停止重试，未换工具绕过：

- `temp/receipt-driver-control-BqEzMZ`
- `temp/receipt-driver-control-lZWUk4`
- `temp/receipt-driver-control-oFWPr6`

## R3-live2：真实自动恢复与验收误判修正（14:21—15:29）

独立服务1441/PID28540、Chrome tab1283922568和106业务/101兼容标记完成真实执行。浏览器实际app.js与源码逐字一致，SHA-256为 `cda5c76d…055fe4`。正式2.5.1处理器首先在Fetch启用处受阻：工具要求显式非Document资源类型；此时原题已提出、未提交回答。补齐Fetch/XHR pattern后，沿同一原题只续行回答一次，保留正式报告blocked及续行失败记录。

| 子场景 | 真实结果 | `R3-question-live2/`中的证据 |
| --- | --- | --- |
| 创建与待答发现 | 两条独立新会话各一次创建；原双题所在轮completed，刷新仍是同题 | `runner/report.json`、pendingInitial/pending原始观察 |
| 单次接受与未知回执 | 仅回答绿色/方形一次；原回执 `525d8d20-6dc1-4b54-812e-118bd65861bc` 服务端completed/ok，客户端unknown；准确回答轮 `01a195d3-6730-7520-b40e-333c2cc85e71` | `continuation/answer-ledger.json`、原始驱动事件和回包 |
| unknown菜单验收 | 旧驱动反复使原回执GET网络失败，菜单触发器因连接不可用被禁用；不能证明独立删除锁 | `continuation/continuation-error.json`；本子场景未通过 |
| 撤故障自动恢复 | finally撤回故障后，页面在刷新之前实际GET同一原回执，清除本地记录、解除删除入口；新增answer POST=0 | `continuation/automatic-recovery.json`；GET `50596.402`，未截断事件及正文 |
| 再刷新与草稿/历史 | 已答题不复活；两会话普通草稿保持且未进入模型输入；完整历史1→3，旧前缀不变、提问和回答各一次 | `answer-final-history.json`、`answer-final-snapshot.json`、`answer-final-checks.json`及续行UI证据 |
| 上下文圆环 | 实际9%，对应last=32802/window=353400，未用累计130063 | `context-after-answer.json`；仅该样本通过 |

本轮结论是“真实自动查询原回执恢复”，不是“新文档触发回执恢复”。后来的刷新只证明已答题不复活和普通草稿保持，不能倒改正式处理器结果。`run-result.json`明确记录此边界。原生派发最终create=2/send=1/answer=1，没有重提、重答或删除会话。普通测试草稿验证后经UI明确清空。

验收资产改为runner2.5.2、证据门禁1.3.0：

- 增加answered原生读取，共七阶段；必须在unknown菜单前取得已完成的完整历史。拒绝以运行中/断线禁用冒充独立锁。
- 驱动保留默认network-error变体，刷新专项选pause，仅挂起原ID GET。登记新文档边界后确认旧读撤销，再放行新主文档同loader读取；每个暂停请求唯一控制任务，不并发continue/fail。
- 取消请求只接受CDP确认或匹配的原始Network终态；跨VM观察仍按JSON值比较。

最新完整Node四组 **165/165、退出0**，见 `node-final.log`；其中driver37项。该结果只证明控制流及验收反例，新的pause完整真实链路仍待执行，不继承此前151/151或本轮自动恢复为通过。

15:29收尾：按token摘要确认测试设备 `6e1d1351-d6c7-4c09-8c5f-d6d1e93da79a` 后通过退出确认框撤销授权，远程Cookie消失、其他Cookie名称未变，标签关闭。服务正常退出0；原生写映射全0后恢复101，epoch=`9c42f5d6-5b5e-4dd6-88a1-df63bbdaa756`。原45222/45221/55277及PID保持。临时Rust入口、原生读取器、最新接入脚本已归档校验并删除，密钥/Cookie副本删除；此前被拒目录未重试。见 `browser-cleanup.json`、`service-finished.json`、`native-restored-inspect.json`、`cleanup.json`、`execution-source/manifest.json`。

安装版、Android、完整三轮及全部73条基线仍未完成，目标active。未提交、推送、打包、安装或发布。

## R3-live3：就绪等待缺口和接入错误（15:33—15:48）

本轮服务1442/PID2296、Chrome tab1283922570实际完成两次创建、一次原提问、一次回答。主会话 `01a19609-4b84-7133-a14d-f883d6150607`，控制会话 `01a19609-7ede-7bc3-88bc-0e3849e266c5`。源码指纹 `e5b82c0e…70d843`；本轮后补的输入框等待尚未包含在该指纹中。

这轮没有完成正式刷新恢复，以下失败均保留，不能归为产品失败或改成passed：

1. 主代理把driver输出目录设为`driver/`，runner使用`runner/`，来源校验在租约获取前拒绝。close确认未启用故障。保留初始报告及`driver-configuration-failure.json`，通过准确原创建回执把两条同轮会话接入`runner-configured/`，没有重建。
2. 正式处理器提出原双题并刷新确认completed待答后，切回主会话立即断言输入框可用而失败。稍后真实同题两输入均唯一、enabled；该断言缺少异步界面就绪等待。现增加准确thread、非loading、原题标题、输入唯一/可见/可用的等待，未修改产品问答代码。
3. 主代理沿原题手动续行，已单次回答并捕获unknown；随后取证辅助错误地把只有`evidencePath`的记录交给要求`path`的通用文件读取器，finally撤回了故障。此为接入错误，完整刷新链路再次中断，不能重答补过。

原回答回执 `7c718ce3-8341-4e3a-8392-2654cde7c71d` 已通过实际GET `55248.447`核对completed/ok，准确回答轮 `01a19611-f319-7553-bf2d-18ee03adda54`。客户端记录为空、删除入口解除、两个普通草稿保持；完整历史1→3，旧前缀不变，整个连续网络窗口只有一次send与一次answer。该结果仍仅为自动恢复，见 `continuation/automatic-recovery.json`，不是新文档恢复。

测试草稿核对后经UI清空，退出准确测试设备 `61b4e2a8-cb26-4998-8d57-b2997706f9aa` 并关闭标签。服务退出0，原生写映射为0后恢复101，epoch=`b4720fac-1854-4db1-8c35-1616df1f04cd`。临时三份入口已归档校验删除，密钥和Cookie副本删除。未操作用户原会话或队列。下一轮需使用统一输出目录和补齐就绪等待的正式处理器，不复用本轮已回答问题。

## R3-live4：浏览器导航事件被误判为业务拦截（15:48—15:54）

新服务1443/PID66020、Chrome tab1283922572完成两条创建及切换刷新。主 `01a19616-a095-70e2-9eb9-942c922b682c`、控制 `01a19616-d11f-7991-8204-599836cdd943`。问答处理器在租约获取前停止，send=0/answer=0；不是输入框等待修复后的完整验收。

驱动基线把任意`Fetch.requestPaused`判作未知外部拦截，实际唯一命中为Browser Use管理的Document响应：requestId=`interception-job-20892.0`、networkId=`8D888FDDA4B951D3D8528EF39A8C0108`、sequence=51、HTTP200；同Network在sequence=84已有loadingFinished。Document按工具契约只应观察，不得由业务故障驱动continue/fail。常规事件循环也会把Document错误登记为本驱动待恢复请求；已交由独立代理修正两个路径，仍须保留未知非Document拦截的拒绝。

原始事件见 `R3-question-live4/baseline-fetch-observation.json`。该文件从cursor0补读有`truncated=true`，只能证明保留的准确Document及其终态，不能证明整个历史没有其他事件；不将其当完整问答网络窗口。问答未发出，未制造HTTP故障。

本轮登录/标签和服务已收回，临时入口已归档校验删除，密钥/Cookie副本删除；恢复101，epoch=`6b921cde-6ada-40e2-8049-55c6f0cb6eb7`。原失败报告不改写，完整刷新专项继续未完成。

## R3-live5：Document修复复测与浏览器执行中断（16:12—16:25）

独立代理修正了Document暂停在基线/常规/清理三个阶段的归属：原始事件保存，不进入本驱动pending，不发送continue/fail；未知非Document暂停仍拒绝。新增四项长期反例，包含live4的准确事件形态。驱动41/41、完整Node四组 **169/169**，退出0，日志为 `R3-question-live5/node-final.log`。独立复审未确认已知接入问题之外的新缺陷；静态审查不替代真实验收。

本轮源码指纹 `c442f8f3a0d4516051307ef9bdf805f93d6f6066452cfddeba2aed12f17ae9ab`。服务1444/PID36468，原测试tab1283922574。实际app.js与源码逐字相同；106业务/101兼容fixture实际执行脚本SHA为 `b5cfda10…17a161`，均留原始加载证据。

| 步骤 | 实际结果 | 边界 |
| --- | --- | --- |
| 新建两个主会话及首回复 | 通过：主 `01a1962c-111c-7b12-a7fb-b78eb83c8ad0`，控制 `01a1962c-470d-7612-8614-ad6dcde9640b` | 各一次创建 |
| 侧栏切换/刷新 | 首次blocked，随后同两ID真实显示后只读重试通过 | 发现新建行异步出现时直接count会过早阻塞；后续已补等待，见下方记录；未重建 |
| receipts租约获取 | 通过，Document不再导致获取前误拒 | 未据此宣称完整故障用例通过 |
| 原双题及刷新 | 原题轮 `01a1962d-c259-7481-807a-72177b03db2b` completed，两题仍待答、未skip | 原始before/pendingInitial/pending均留存 |
| 后续流程 | 浏览器执行到240秒超时、内核重置；重新读取原标签报告调试连接未附加 | 最后持久检查点16:15:57；没有answer动作和故障布置动作，不能从无返回推断答案未发，另核原生派发日志 |
| 只读收尾核对 | 原生实际create=2/send=1/answer=0；双题lastSubmission=null/isSkipped=false/isSubmitting=false；新标签可读服务且原题可见 | `after-browser-timeout-snapshot.json`、`pending-question-after-tool-reconnect.*`；没有重提或代答 |

原`runner/report.json`保持中断时原样；另存 `report-interrupted.json`，将本例列为blocked，附原报告摘要及 `interruption.json`，不删除尝试、不补成passed。新的收尾标签1283922575退出准确测试授权并核对Cookie清除后关闭。原标签1283922574显式关闭仍报`Debugger unattached`，保留未关闭状态；未换浏览器或控制方式绕过。

服务正常退出0，进行中原生写映射为0后恢复101，epoch=`7664e8a5-8ebc-482a-a7f6-976c9a3e4a33`。本轮临时入口已归档校验删除，密钥/Cookie副本删除。未回答的测试原题和原始证据保留，用户原会话/队列未改。完整刷新专项、完整三轮及硬件相关验收仍未完成；没有提交、打包、安装或发布。

16:30补齐侧栏等待：`selectOwned`先等准确测试ID的可见行，同步期间出现确认框或非测试草稿则停止，再执行切换。没有自动展开其他项目、清搜索或覆盖输入。新源码指纹 `fa2cf6fc10fde513f58b8543ea3c98d05d0c7af76a28561d47a128ef31c872a9`，完整Node169/169、语法及差异检查通过，日志 `node-sidebar-wait-final.log`。这是live5之后的验收器改动，未把live5的原指纹或真实结果改成新版本执行。

## 既有原题的分段真实刷新恢复（16:39—17:02）

runId=`RA-20261101-R3-original-question-recovery`，证据目录 `R3-original-question-recovery/`。先按原创建回执及哈希核对live5两条测试对象，独立读取完整历史与逐题状态，确认答案从未提交。服务1445/PID68024只允许两条测试对象的读取/已读及原题的一次指定回答，禁止create/send。实际网页与源码逐字一致，106业务/101兼容变体也核对了真实执行脚本。

| 子场景 | 实际结果 | 原始证据 |
| --- | --- | --- |
| 原completed双题刷新发现 | 同原turn/item/entity，未提交且未skip，刷新仍可见 | `before-answer-snapshot.json`、`pending-after-reload-snapshot.json`、`original-pending-after-reload.*` |
| 单次原答案 | 绿色/方形；回执 `7baadac9-8d62-48ae-94a1-fb087f27880f`，回答轮 `01a19648-a0f4-7940-87eb-de6156ffb056` | `answer-original-response.json`、`answer-ledger.json` |
| 成功后的回包丢失 | 服务端completed/ok，回答HTTP加载失败，客户端同ID unknown | `client-unknown.json`、`fault-state.json`、`network.ndjson` |
| 独立unknown删除锁 | 先取得原生回答completed完整历史；UI send按钮和菜单触发器可用，删除入口单独锁定 | `answered-before-menu-history.json`、`answered-unknown-delete-locked.*`；按钮可用不扩成额外发送通过 |
| 新文档查原回执 | 新主loader=`4ABAF0FDD4FF58407C04D43404D42AC7`；GET `62604.336`返回同一原回执200/completed | `reload-boundary.json`、`recovered-original-receipt.json`、`receipt-controls.json` |
| 原锁/草稿/完整历史 | unknown清空、删除入口恢复、原题不复活；两普通草稿保持且未入模型；主历史2→3且旧前缀不变，控制历史仍1轮 | `client-recovered.json`、两端UI、`final-main-history.json`、`final-control-history.json` |

独立审核重读原始文件、哈希、完整历史及702条未截断事件，28项核对通过。`independent-audit.json`只证明该功能子场景，不是旧自动处理器完成记录；服务终态create=0/send=0/answer=1。人工暂停及取证等待不作为产品响应时长验收。

新发现并修复两项验收错误：

1. Fetch暂停seq1130先于对应Network请求seq1133，networkId=`62604.303`，且跨读取批次。驱动现等待准确元数据/Document后重判，不将未知归属当旧loader取消；保留每请求唯一控制、原始取消终态和旧loader拒绝。新增7项反例，定向48/48通过。
2. responseReceived.timestamp=`178535.020689`，loadingFinished.timestamp=`178535.018772`。首次审核据此失败，日志保留。Chromium源码中响应事件取派发时刻，完成事件可携带传入的底层完成时刻，来源及摘要见`timing-contract-source.json`；不声称已调试到本机二进制内部调用。门禁现要求两者不早于原请求，并取较晚时刻核对解锁前完成，保留身份/200/正文/完成事件要求，不引入样本容差。新增两项反例，问答定向16/16通过。

当前runner2.5.3、证据1.3.1，完整Node **178/178**、退出0，见`node-final.log`。新验收代码尚未完成新一轮完整自动处理器，不能将分段结果回填旧报告。

收尾：草稿经UI明确清空且DOM值为空；`final-answered-clean-draft.jpg`捕获到清空前的绘制帧，不作为清空的视觉证明。`reloaded-original-receipt-recovered.jpg`可核对恢复后的回复和保留草稿；菜单状态以同阶段DOM与原生回执核对，不以图片存在冒充通过。授权退出、tab1283922578关闭、Cookie清除；上轮中断tab1283922574也已确认不在列表。服务退出0，原生写映射全0后恢复101，epoch=`2607c4fc-d88e-49e5-82fd-11e3b914bf04`。临时入口、读取器及审核器归档校验删除，密钥/Cookie副本删除；旧拒绝清理未重试。未提交、打包、安装或发布。

另一项只读覆盖审查：当前及HEAD的`changeSidebar`均只有视图切换、会话/项目置顶，没有独立排序菜单。用户已答复“保留并验收现有入口”，各客户端独立保存不变；本轮不新增排序菜单，继续验收最近/项目切换和置顶。既有子智能体生命周期证据14条全部直接属于根任务，不能补成孙级验收。

## 队列保存的真实失败与现场续查（17:52—19:34）

runId=`RA-20261101-R3-queue-edit-recovery`，证据目录 `R3-queue-edit-recovery/`。复用有创建证据的测试 M=`01a1962c-111c-7b12-a7fb-b78eb83c8ad0` 与控制 C=`01a1962c-470d-7612-8614-ad6dcde9640b`。真实网页与当时源码逐字核对，原生为106业务/101兼容变体；没有操作用户会话。服务只允许两条指定发送、取出一次、保存一次。

| 判断 | 当前事实 | 原始证据 |
| --- | --- | --- |
| 保存前置发生变化 | 基础轮18:00:42完成；队列取出后18:01:51保存，此时基础轮已空闲约69秒 | `base-http.json`、`begin-edit-http.json`、`resume-main-writer-detail.json` |
| 保存原响应即失败 | 回执 `158cde0b-7f04-49f8-862a-a25e204fd564` 为 unknown；`server-input`、`mutationAcknowledged=true` | `save-original-response.json` |
| 原消息实际已消费 | 同逻辑 client=`62056d47-ea30-4bfe-9d7e-64fbddb78682`，修改全文含内部空行；turn=`01a1968e-ed38-7132-9277-dcc525254fb7` completed、回复 `RA-QUEUE-EDITED` | `save-unknown-history.json`、`resume-main-history.json`、`resume-main-writer-detail.json` |
| 服务端与本地队列不一致 | 同 writer 的分页队列明确为空；本地仍有 Q=`01a1968c-bf84-7663-832b-b0efaa720522`，暂停原因为重排必须包含全部消息 | `resume-main-writer-detail.json`、`resume-main-snapshot.json` |
| 编辑尚未收敛 | ED=`a5a2acf5-cc54-42b1-b27e-6a7c88d50fa5` 仍 outcomeUnknown，token=null；客户端保原回执、编辑稿及普通稿备份 | 同 writer 详情、`client-after-original-unknown.json` |
| 诊断传输副作用 | 未显式丢弃保存回包；原生先返回 unknown，之后暂停期间浏览器取消请求；故障规则已撤回 | `fault-state.json`、`original-response-delivery-diagnostic.json`、`network.ndjson` |
| 测试服务终态 | 1446/PID36444 到一小时上限退出0；send2/begin1/save1/cancel0/create0 | 私有运行目录的 `finished.json`、`probe-live.log`、终端session93285退出结果 |

本次为 **真实业务失败，正在修复**，不是计划中的“成功回包丢失”验收通过。取证期间原保存未重放、未再次入队、引导或取消。服务到期后的网页断线与本次保存原生失败分别记录；测试进程退出0不能证明功能通过。

19:45—19:55显式收尾：独立审核15项原始材料确认修改输入唯一接受、完整历史与控制会话保持、双普通稿和修改稿保留。首次审核误要求独立CLI含writer逐项内存时间字段而失败；现分别比较全部业务字段及writer接受时间，保留该限制，不舍弃正文/身份。审核结论仍为`businessOutcome=failed`。

随后仅对已证明完成的测试对象使用产品现有入口：结束原ED一次（回执`6a6f280b-8f62-49f6-9791-0f13d2d22729`），主普通稿恢复；残留Q仍可引导/编辑，`cleanup-after-dismiss.png`记录了这一风险。再明确删除该测试影子一次（回执`63bef1c6-2cbb-42e8-bb89-9bd1cac1d600`）；同writer完整5轮逐字段保持，服务端/本地队列均空，C普通稿保持。原保存回执仍为unknown，未伪改为完成。这是测试清理，不是自动恢复或新修复通过。

清理服务1446/PID65216只允许上述两个动作各一次，send/save/restore/steer额度为0；session79920退出0。测试稿经UI清空、授权退出、Cookie消失、tab1283922580关闭；全部原生写映射为0且原ED已通过产品结束后恢复原适配器，epoch=`dc115950-7099-470c-b8dd-d8a69772a379`。本轮六个临时源码及运行记录已逐文件归档校验，38个临时文件删除（密钥/Cookie副本直接删除未归档）；旧拒绝清理目录未动。见`cleanup-summary.json`、`temporary-cleanup.json`和两份`*-closed/finished.json`。源码修复及修复后真实验证仍在进行。

## 队列修复与独立复审（20:00—20:45）

源码冻结：`native.js` SHA-256=`47F56A162D2291C0BA2259696EF3EA364A82819388081806D2E4ADDBFA714166`，`app.js`=`E68492372CE4B4465D3261D1380BAD1D89EF7DE6028AC497F275C1DFCF424FDB`。

- 保存及取消恢复前保留完整历史基线和准确输入；仅原连接对象仍有效、明确恢复回执、同逻辑 ID 唯一新增接受记录、完整输入逐项一致且服务器已无该项时，只读确认完成。旧无证明的 ED 不自动升级，未知操作不重放。
- 已接受的本地暂停副本显示“已发送，待同步”，不计待执行数量，拒绝编辑、引导与重排。保留普通队列操作和用户明确移除入口；不使用无条件自动删除。
- 独立审查发现最终确认 await 期间切换连接可把旧队列包装成更高 revision 覆盖新队列。先红后绿修复最终成功出口、快照及重放出口的连接检查。无有效证明的原始自定义 remote 标记也会先剥离。
- 原始 ASAR 的 restore/add/reorder、普通 send 与暂停门禁提取夹具验证了失败回退和“不唤醒旧暂停副本”；完整桥接179/179、前端状态33/33、Node全组180/180退出0。这些都是源码/夹具证据，不作为真实保存通过。

当前因果链为**部分确认**：现场输入/输出与原始函数复现一致，未取得原事故逐次 add/reorder RPC trace。正式 RPC 未提供带条件的本地副本对账接口；队列残留没有自动清除，不宣称原生 Codex 实现已修好。

第一轮 `R3-queue-idle-save-fixed` 未进入保存路径：120秒基础任务完成前没有点到取出编辑，原队列自然执行。服务1447/PID66692终态send2/begin0/save0，原5轮保持、最终7轮，结果为 `not_run`；不是修复失败或通过。原始事件读取重复了相同source/sequence，已逐值核对后另存唯一事件表，没有重复业务请求。服务退出0、授权退出、21个本轮临时文件归档清理，兼容fixture保留供下一轮。详见`report-not-run.json`。

第二轮 `R3-queue-idle-save-fixed2` 已核对原始7轮与前次终态一致；相同源码/实际脚本哈希吻合。服务1448仅允许新标记的基础/队列/保存后普通消息各一次，取出与保存各一次。基础轮`01a19723-3460-70b2-bd0a-99555352f98f`真实执行300秒等待；在inProgress时取出Q=`01a19723-3ac4-7572-859d-2bd0dc72bea2`、client=`e00acc37-a35b-4764-8c26-55e6fbdb7278`，ED=`d2f0993f-390c-45de-b100-e842be62a2a3`，再等基础轮完成后保存一次。

### fixed2真实结果：保存收敛通过，后续普通发送仍失败

| 子场景 | 实际结果 | 证据 |
| --- | --- | --- |
| 空闲保存 | 回执`123d80d6-a03e-4b6e-8e3f-ac0287e80d7c`为completed/committed/accepted；精确修改input唯一进入`01a19728-ab0a-7a32-9283-8717f56a08b3`并完成，回复`RA-IDLE2-EDITED` | `save-http.json`、`saved-accepted-{history,writer-detail}.json` |
| 自动清锁/草稿/刷新 | 原pending与ED自动清空，主普通稿恢复；刷新保持，控制稿未变 | `save-confirmed-client.json`、`saved-reload-client.json`、`control-after-save.json` |
| 已接受残留防重入口 | 队列显示0待执行、1已发送待同步；编辑/引导/排序禁用，保留“移除已发送记录” | `accepted-ghost-controls.json` |
| 普通新消息 | 回执`c432b964-94b0-461e-a7fa-0e97a8838681`显示queued；logical client=`f21d8937-64a8-4fbe-a286-22f04b8f7fc8`数分钟仍无接受记录，服务器队列空，本地ghost加新queued项两条 | `ordinary-after-save-http.json`、`ordinary-remains-queued-writer-detail.json`、`ordinary-blocked.png` |
| 手工因果对照 | 仅移除已接受ghost一次，回执`a63f7add-2e57-4774-90df-f14e683fbcc0`；原新消息立即转pending并自然执行，turn=`01a1972e-db76-7b50-b43c-dc817dda3f31`完成，未重发或引导 | `manual-remove-accepted-ghost-http.json`、`after-manual-ghost-removal-writer-detail.json`、`manual-recovery-final.png` |

独立原始材料审核11项通过，仅证明上述范围，整例结果仍为 **partial-failed**。完整历史7→10且原前缀不变，修改与后续消息各接受一次；控制会话一轮逐字段保持。普通发送只有在人工移除ghost后恢复，不能据此报告正常主流程通过。

新确认的覆盖错误：旧原生调度夹具只验证新项pending阶段可优先调度，省略了远程完整发送入口的最终准入与queue override。真实 `submitQueuedMessage` 把原始本地队列长度算作active，已接受ghost使send-now转为queue，后续消息被paused head阻塞。继续定向核新版原生保存队列的正式入口，不能只叠加“忽略ghost”的空闲特例而漏掉忙时入队后的自然消费。

收尾：原服务PID58364/session83687退出0，send3/begin1/save1；清理服务PID46200/session38501退出0，仅remove1、send0。41个本轮临时文件逐文件归档校验或删除（密钥副本不归档），授权`f0575b74-d108-4ac7-bb9b-227d3c881272`退出、Cookie消失、tab1283922582关闭；控制测试稿清空，原生映射0且ED为committed后恢复101，epoch=`a2d13c59-734f-4e90-9fec-c15897af6b4b`。原始成功/失败边界和手工恢复均保留，未提交、安装、打包或发布。

## 替换错误保存入口后的真实联动（21:39—21:54）

runId=`RA-20261101-R3-native-save-flow`，目录`R3-native-save-flow/`。新版commit已替换为原生`sendMessage + editPosition`正式保存路径，不再调用Undo的restore；保留完整输入、位置、原操作身份和未知不重放校验。保存前存在基础轮、保存时已空闲的真实路径不会再产生ghost。取消仍走原生Undo恢复，不能随本项结项。

同时补齐原生提交成功、后续队列刷新失败时的ack/result保留。独立实际函数反例证明：先unknown保准确ack，读恢复后仅`queueEditState`可收敛，start始终1、restore0；错误result/身份不能伪成功。该失败恢复分支目前为源码/内存验证。

本轮native源码SHA-256=`49F9BACC940B9A90CE5FD093119FC1473859C5DE6E8E70E24B99CADAA0702B2A`，网页仍为`E684…424FDB`。原生业务106/兼容101实际执行脚本SHA=`130cf727c91ddcea19ba376bd132b3f1be4d327e3a8cf75df1c8ed3c0eb72089`，与待测变体逐字一致；两份源码及原始指纹已归档。

最终源码检查：队列定向14/14、完整`cdp_bridge`186/186，Node相关全组180/180及JS语法、格式、差异检查通过。`cdp_bridge.rs` SHA-256=`16EF19B5B216FF89313F8F6CFD1EE2232EE92E0717ABCA43F0543203C3236A9C`。新增nativeLocal文字/纯图、保图/删图/增图及detail:high、Skill/mention、连接切换和ack防重反例；复杂引用型text_elements与多候选正文在派发前拒绝。这些附件/本地队列例仅为源码夹具，不继承下面真实纯文字联动的通过。

| 联动步骤 | 真实结果 | 原始证据 |
| --- | --- | --- |
| 忙时取出A，等空闲保存 | ED=`e1ea04bc-3b96-4a39-9504-ce995db22a97`；保存回执`d6825407-8021-4e3d-801d-679ae1979a1a` committed/accepted；turn=`01a1975b-c4da-7731-bbac-94e31bcb50fa`唯一完成；队列空，无ghost | `queue-a-before-edit-snapshot.json`、`idle-before-save-a-writer-detail.json`、`save-A-http.json`、`saved-A-client.json` |
| 不清理而普通续发 | 回执`d9a23232-7f08-4287-8982-37fa4a996626`直接start；活动轮`01a1975c-634e-7883-955e-ec9a84c0748b` | `next-active-http.json` |
| 忙时B/C，编辑B保位 | ED=`b27e347f-7a3a-430c-aced-1b380412345b`；保存回执`e5df1ac3-a7a3-404b-bfec-253c1e9df16d` committed；B新physical=`01a1975d-002f-7620-8a45-e8f6e14e3d52`，logical仍`6f044a6d-0322-4b31-bfd4-4484ae6914ff`，位于原C前 | `queue-bc-before-edit-snapshot.json`、`save-B-http.json`、`busy-after-save-b-writer-detail.json` |
| 自然B→C | 活动轮结束后B=`01a1975e-620e-7fb2-ba24-d7614719f4b0`再C=`01a1975e-7686-7180-a267-3ea79ee9595c`；准确全文/内部空行及回复一致，各消费一次，最终本地/服务器/远程队列均空 | `final-main-{snapshot,history}.json`、`final-writer-writer-detail.json`、`final-client.json` |
| 草稿/刷新/控制会话 | 两次保存自动清锁恢复普通稿，刷新保持；控制稿与原一轮历史不变 | `saved-A-client.json`、`saved-B-client.json`、`after-save-reloaded-client.json`、`control-draft-after.json` |

主代理独立材料审核15项通过，原完整10轮逐字段保持、最终15轮；五条原生用户消息及回复逐项核对，未按显示文字推断身份。FIFO采用原生毫秒起点及duration：下一活动轮结束`1793540907409` ≤ B开始`1793540907551`，B结束`1793540912780` ≤ C开始`1793540912793`。网络及服务终态均为send5/begin2/save2，无remove/steer/cancel/重发。`final-linked-success.png`可核对最终B/C回复、空队列和保留普通稿。

证据来源边界：运行期间独立CLI的`thread/read`一度把尚在运行的基础轮重建为interrupted，缺逐项运行记录；同writer同时明确inProgress。保存前采用同writer明确completed，不把二级读取结果当停轮事实；终态两来源全部业务字段一致。`base-status-history.json`与`base-writer-status-writer-detail.json`均保留。

结果为 **通过该纯文字保存联动子场景**。独立子代理已重新核对原始材料、请求指纹、输入和时序，确认该限定结论；next完成到B开始间隔142ms，B完成到C开始间隔13ms。网络另有9次markRead，不作为已读完整回归通过。结果不扩为取消、图片/Skill/复杂隐藏输入、未知回执真实故障、双端并发、完整三轮、安装或Android通过。

收尾：1449/PID46532/session90304退出0；两测试草稿经UI清空，授权`478b8c64-9db0-4479-a45b-91ac48fbab3f`退出、Cookie消失、tab1283922584关闭。原生写映射为0、ED均committed后恢复101，epoch=`e16dc1a9-f74b-45bd-9037-a452715a9a69`。五个临时源码及运行记录逐文件归档校验，34个临时文件删除，密钥不归档；原始会话与全部失败证据保留。未提交、打包、安装或发布，整体目标继续active。

## 下一轮取消恢复准备（2026-11-01）

继续时已重新核对native源码49F9…与测试16EF…指纹、完整脏文件范围；上一目标轮分类为progress。实施代理和独立合同审查分别检查取消能否通过正式submission精确恢复原input、身份及位置，避免原生Undo留下ghost。`R3-native-cancel-flow`已准备5条指定发送、两次取出/取消的隔离入口并编译，尚未启动服务、复制密钥或派发消息；主会话完整15轮与上轮终态逐字段一致，控制1轮。

2026-11-01 22:11:41有界复核Android通道：PATH未找到adb，`ANDROID_HOME/ANDROID_SDK_ROOT`为空，默认SDK及已知`temp/android-build-20261017-050/android-sdk`内没有`platform-tools/adb.exe`，项目`local.properties`不存在。该结果只覆盖这些明确路径，不声称全机没有SDK或手机；真机验收仍缺可用通道。没有重新构建APK或以浏览器测试代替真机。

## 取消恢复的真实联动（2026-11-02 00:19—00:31）

runId=`RA-20261101-R3-native-cancel-flow`。原生源码 `01C468874BE4C7C360E672570643B6D9533DB538BA08BA0F378B79116CCBAE12`，网页 `E684…424FDB`，原生实际执行脚本和浏览器脚本已逐字核对。首次服务 PID64908 在零发送/取出/取消时到期退出，保留为前置未执行；续行服务 1450/PID68188/session25967 才完成本轮操作，测试对象和既有15轮基线不变。

| 操作及必须结果 | 真实结果 | 证据 |
| --- | --- | --- |
| A 忙时取出，基础轮完成后取消 | ED=`cc11478a-eec6-4fbd-9635-e7527d2e3b77`；取消回执=`458a7f91-d29b-4995-b5a4-f42b74cee9c6`，canceled/accepted；原输入进入 turn=`01a197eb-7c23-7031-8e92-b98fc41375f1` 一次 | `idle-confirmed-before-cancel-a-writer-detail.json`、`cancel-A-http.json` |
| 取消后直接发送普通消息 | 未清理或引导，直接 start 新轮 `01a197ec-5d76-78d3-86a5-da59621aad29`；普通稿恢复、无残留 | `canceled-A-client.json`、`next-active-http.json` |
| 忙时取消 B 保位 | ED=`d3de6ae2-9705-4dbe-8eef-f86ec4c477c3`；取消回执=`32426dff-8eeb-435d-bc14-10b13df97f4e`，B 的准确原 input 和逻辑 ID 保留，位于 C 前 | `busy-before-cancel-b-writer-detail.json`、`busy-after-cancel-b-writer-detail.json`、`cancel-B-http.json` |
| 活动轮结束后自然 B→C | B=`01a197ee-61b5-7501-8294-07da1179f992`、C=`01a197ee-77ed-7cf0-a919-2032d136e652` 各完成一次，原生毫秒时序确认先后，最终三处队列为空 | `final-main-snapshot.json`、`final-writer-writer-detail.json` |
| 修改稿丢弃、草稿及刷新 | 两段 discarded 正文和两份普通稿均未进入模型；取消自动恢复主普通稿，控制稿保持，新文档刷新保持 | `final-main-history.json`、`control-draft-after-client.json`、`after-cancel-reloaded-client.json` |

主完整历史15→20，原前缀逐字段保持；控制历史仍1轮。原始网络及服务账本send5/begin2/cancel2/save0，没有人工删除、引导或重发。主代理原始材料审核17项通过；`independent-audit.json`引用的是清理前网络截点，随后追加清理读请求，最终冻结材料以`final-independent-audit.json`的哈希为准。结论只为该纯文字取消联动子场景通过；复杂输入、暂停/send-only、真实未知回执及双端竞争未继承通过。

收尾：主草稿和控制草稿已通过UI清空；`cleanup-control-draft-client.json`捕获于切换过渡阶段，控制会话最终证据为`cleanup-control-confirmed-client.json`。首次退出只打开确认框便关闭了旧标签，未撤销授权；随后另开收尾标签并点击确认，`cleanup-authorized-logout-final.json`记录登录页和设备Cookie消失，两标签均已关闭。服务退出0，原生写映射为空后恢复101，epoch=`86738cfa-f58e-4d9d-92d7-98633b72207e`。临时材料归档清理和独立子代理复核继续进行；未提交、打包、安装或发布，整体目标保持active。

### 独立复核、交付与暂停安排

独立子代理复核支持空闲取消、后续普通发送、忙时取消保位和自然 FIFO 四段通过，原始输入、唯一接受、时序及历史保持一致。另保留两项验收证据缺口：主代理实时浏览器输出观察到已键入待舍弃修改，但归档的 `a-editing-ui.txt` / `b-editing-ui.txt` 没有编辑器值，独立审核无法复核“确实修改后再取消”；控制会话草稿保留，但发送按钮由可用变为不可用的快照缺少后续原因证据。不能将四段通过扩成整条 R1-11 完成。报告见 `R3-native-cancel-flow/independent-review.md`；73条总体复核见 `coverage-audit-20261102.md`。

本轮33份临时材料已逐文件归档校验，37个临时文件（含凭据副本及临时Rust入口）已删除，原测试历史和失败证据保留，见 `temporary-archive-manifest.json` / `temporary-cleanup.json`。未触及旧拒绝清理目录。

用户随后要求优先生成安装包，并明确“编译管理端安装包和安卓包后暂停目标，等重新安装后再继续”。Windows 专用入口和 Android Release 入口均退出0，管理器 `npm run check` 退出0，Android lint及正式签名校验通过。版本保持桌面0.6.0、Android0.1.0/code24；Windows打包二进制与编译产物一致，APK内置app.js、channels.js、recovery-store.js和session-menu.js与当前源码逐项SHA-256一致，记录在 `dist/validation/remote-delivery-20261102/artifact-verification.json`。只完成本地构建交付，没有安装、提交、推送或公开发布；Windows真实窗口及Android真机仍因用户当前不便而受阻。按用户要求在产物交付后暂停整体目标，保留全部未完成验收。
