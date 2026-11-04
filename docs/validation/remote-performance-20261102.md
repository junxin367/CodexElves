# 远程会话性能 RCA 与局部验收（2026-11-02）

最新源码为122，增量修改及本轮收尾见文末；最近登记的安装包仍是117阶段，尚不包含118及之后的修改和最新前端。各节保留对应阶段的原始证据，不能跨版本替代运行或产物身份。

状态：**部分确认**。引导 HTTP 样本为 1726.058→570.431 ms；116→117 普通文本发送样本为 12110.400→3759.611 ms；117 同步问答正常回答 HTTP 样本为 243.161 ms。各数值均来自不同真实请求或单次观测，不能视作同轮 A/B、稳定分布或速度承诺，也不能将全部差值归因于代码改动。115 文本和纯图片、116/117 普通文本的正常单次收发已核对。117 起止 Debugger 脚本与登记插桩变体逐字匹配，HTTP、唯一接收、完成回复及旧历史保持均有原件；116 的运行脚本全文缺口作为历史边界保留。后续真实验收补充了同步问答正常回答及成功回包丢失后的收敛、普通发送回执恢复、队列编辑保存和取消回执恢复；这些结论只覆盖报告中的具体样本。整体目标未完成。

## 原因与判定边界

- **现象**：原有排队引导等待包含多次历史核验、原生准备及队列同步，不能用页面 RTT 代替操作耗时。比较对象是专用会话 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` 中两个不同活动轮，不是同一活动轮的 A/B。
- **直接原因**：112 样本在一次 `steerQueued` 调用窗口内执行 6 次 `assertQueuedMessagesUnaccepted`，并在原生发送后继续读取队列；114 样本为 4 次，成功回执允许携带 `queueSyncPending:true`，不再等待队列重订阅完成。
- **局部实现原因**：外层引导与原生准备入口重复做历史核验，已明确发送成功后的队列同步仍延长回执路径；前端另有延迟刷新与主动刷新重叠、编辑重复持久化。本轮仅确认这些具体重复路径，不据此断言所有慢操作均由它们产生。
- **未闭环的上游因素**：两次原生 `execution.prepare` 分别为 971.2 ms、352.8 ms，环境、原生准备及运行时调度存在波动；尚无各项优化单独对照或足够样本分布，不能确定它们对总差值的贡献。后续报告已覆盖特定成功回包丢失和恢复路径，但性能门禁仍需重复测量；完全离线、长期 unknown、明确失败、回执查询异常及并发竞态仍有缺口。

原件根目录为 `dist/validation/remote-performance-20261102/`。`native-before-identity.json`、`native-after-identity.json` 分别登记源码 112、114，均以兼容标记 106 运行，fixture ID 不同；它们不是已安装程序版本已升级的证明。实际运行脚本保留为 `native-before-runtime.js`、`native-after-runtime.js`，网页来源见 `before-web-source.json`、`after-web-source.json`，对应记录均为 `fromServiceWorker:false`。

## 真实证据

### 引导耗时

| 指标 | 基线 112 | 修改后 114 | 证据定位 |
| --- | --- | --- | --- |
| 引导 HTTP 完整往返 | 1726.058 ms | 570.431 ms | `before-http.json` 的 `80000.476`；`after-http.json` 的 `/exchanges` 中 `80000.1084` |
| 适配器 `steerQueued` 调用 | 1670.7 ms | 529.6 ms | 两份 `*-result.json` 的 `/profile/records`，分别为记录 ID 167、161 |
| 调用窗口内历史核验 | 6 次 | 4 次 | 同窗口内 `stage=assertQueuedMessagesUnaccepted` |
| 原生 `execution.prepare` | 971.2 ms | 352.8 ms | 同窗口内记录 ID 176、168；嵌套耗时不能再次相加 |

基线引导目标轮为 `01a19ac1-b09a-7f30-a82f-70df0e17bce3`，回执 ID 为 `6c7a08dc-958a-4f01-bd07-e8b0d549c3c2`；修改后目标轮为 `01a19adb-bf6e-76e2-8f0e-4b30afb7d12b`，回执 ID 为 `c316b879-1c5d-414a-b80e-3f333fd8d490`。修改后 HTTP 回执明确成功并返回 `queueSyncPending:true`，后续原生结果另行核对，未把队列同步待定解释为未发送。

普通首条发送 HTTP 样本反而为 5148.477 ms → 12348.851 ms，排队发送为 838.823 ms → 2963.461 ms；这些不是本次引导指标，亦说明不能将一条引导的改善扩大为全部发送已变快。115 的普通发送预启动精简不使用这些 114 记录冒充验证结果。

### 功能与输入完整性

| 子场景 | 实际结果 | 原件与边界 |
| --- | --- | --- |
| 基线引导 | 准确目标轮中唯一 `accepted`，原输入匹配 | `before-functional.json`：`uniqueAccepted=true/inputMatches=true`；结合 `before-http.json`、`before-result.json` |
| 修改后编辑与保存 | `beginQueuedEdit`、`editQueued` 各派发一次并完成；HTTP 分别为 921.683 ms、117.985 ms | `after-http.json`；编辑事务 `2981e134-8712-4a39-a6d5-8b083b5db5b7` |
| 修改后引导 | 编辑后的消息在原目标轮唯一接受，完整编辑输入匹配，最终队列为空 | `after-functional.json`：`uniqueAccepted=true/editedInputMatches=true`；`after-result.json` 的原生条目及 `/state/queue` |
| 旧历史保持 | 修改后操作前的 8 个已完成轮次逐字保持，新增活动轮最终完成 | `after-steer-before.json` 与 `after-result.json` 的 `/state/turns` 前 8 项一致；`after-functional.json` 的 `oldHistoryUnchanged=true` |

修改后队列物理 ID 由 `01a19adc-2b55-7eb3-bb5d-795215d3e1c4` 变为保存返回的 `01a19add-3a2a-7fb0-bddc-002bf6281844`，逻辑客户端 ID `2ac472ba-1166-4c44-8729-95dc7397642d` 保持；引导核对的是这份恢复对象。不得要求编辑前后物理 ID 不变。基线结果采样时其活动轮仍在执行，不能把该快照与修改后终态直接整段比较，误判正常完成字段为历史改写。

### 界面附项

界面原件根目录为 `dist/validation/openin-fullview-20261102/`。

| 项目 | 当前可证结果 | 未完成范围 |
| --- | --- | --- |
| Open In 完整视图挂载 | `anchor-comparison.json` 中旧 anchor 为 `null`，候选定位同一 titlebar 的 `end` 槽；`explicit-refresh.json` 显示显式刷新后按钮组 0 → 1；`dom-full-rendered.json` 中恰一组、`hidden=false`、46×28 | 仅证明真实 DOM 挂载与布局。Windows 锁屏，真实视图切换和系统打开操作未完成，不能标功能整体通过 |
| Open In 运行身份 | `renderer-installed.json`、`renderer-runtime.json` 登记源码 13、兼容标记 11；DOM 的 `openIn=11` 与此一致，原件保留实际摘要及上下文 | 不能将兼容标记写成源码 11，也不能据该测试变体宣称新安装包已交付；`renderer-restored.json` 记录恢复版本 11 |
| 空子智能体文案 | 当前源码页面的 `empty-subagents-ui-current.txt` 不含“暂无子智能体”，仍有组头与折叠入口；截图为 `empty-subagents-web-current.png` | 只覆盖该页面空态。旧 SW 页面仍显示文案的 `empty-subagents-ui.txt`、`empty-subagents-web.png` 保留为历史，不能作为修改后通过证据 |

## 已排除的解释与保留的反例

- **仅靠回执更快伪造功能成功**：已用准确目标轮的唯一 `accepted`、逻辑消息身份、完整输入与旧历史对照验证；没有只凭按钮解锁或队列行消失判定成功。
- **同轮反复重放得出改善**：前后为不同活动轮与不同请求身份；修改后动作账本为 `send=2/beginQueuedEdit=1/editQueued=1/steerQueued=1`，不将它们合并成同一活动轮对照。
- **源码变了就代表用户端已生效**：性能样本有独立运行脚本和网页来源；Open In 有兼容标记，空态保留旧 SW 反例。上述证据不替代新产物安装及真机验收。
- **原生准备波动已经排除**：尚未排除。不同活动轮、先后次序、缓存和原生准备均可能影响耗时，当前不提供稳定改善比例或性能达标结论。

## 缺口与下一步

1. 在固定版本及相同测量口径下补充多次独立样本，分别报告 HTTP、适配器与原生准备分布；继续区分普通发送、入队、编辑与引导。
2. 同步问答已有两道真实问题记录：正常青色回答 HTTP 为 243.161 ms；橙色回答的 HTTP 200 成功响应被中止后，原生仍只消费一次，页面通过仍可用的状态读取和事件通路收敛。本次该问题窗口没有 GET 原操作回执，故“问答按原回执 ID 恢复”仍未验证；响应与事件流同时不可用、长期 pending/unknown、404、明确失败和迟到回执也未验证。没有问答性能 A/B，不能据 243.161 ms 宣称稳定提速。116 阶段未取得的 Debugger 全文不由 117 原件补造。
3. 普通发送成功回包丢失后 GET 原回执恢复、刷新期间无追加 send；队列 B 保存和 C 取消的成功回包丢失后也分别 GET 原回执恢复，并核对了输入保持及 B→C 自然 FIFO。这些是具体已测分支，不覆盖完全离线、跨进程恢复、长期 unknown、全程初始派发计数、并发编辑或多客户端竞态；worktree 恢复及副作用前身份校验仍须分别验收。
4. 用户 Android 真机、Windows 闪窗、Open In 真实切换及系统打开操作仍待用户或主代理验收。117 Windows 安装包及 Android APK 已构建并核对摘要，但均未安装或发布；当前后续测试运行的恢复状态另见文末，仍待补证。

## 修复与防复发

本轮已实施：已确认的队列引导 `sent` 后不再等待队列重订阅；普通引导减少外层及准备前重复历史核验，worktree 恢复分支保留副作用前校验；`beginQueuedEdit` 只读取一次列表；前端合并延迟与主动刷新，移除编辑重复持久化。117 另将普通发送成功后的队列首帧等待移出回执路径，并移除同步问答的完整历史读取；普通发送正常子场景和两道同步问答均有真实功能证据，但同步问答无性能 A/B，且未知结果与其他恢复分支仍未闭环。对应源码位于 `assets/remote-access/native.js`、`assets/remote-access/app.js`；本文仅整理既有变更，不另改产品。

防复发验收继续同时核对时延与正确性：请求一次派发、目标轮和消息身份准确、完整输入不丢失、旧历史不被改写、未知结果不重发。保留原始失败、运行摘要和各阶段耗时；未确认发送的路径不能采用 `sent` 的快速返回条件，也不能为降低次数移除副作用前必需校验。

## 115普通发送续验与新发现

115仅对已确认的普通直接发送省略本方提前`start`准备，保留原生`steer → start`以及Skill、侧边、排队和旧manager路径。完整桥接回归215/215、前端状态39/39通过。真实纯文本回复`PERF_SEND_FINAL`，随后纯图片回复“蓝色。”；完整历史10→11、旧10轮逐字段保持，图片字节与24×24测试PNG相同，两个独立观测周期各只有一次原生`send`调用。见`send115-text.json`、`send115-image.json`、`send115-functional.json`。

两次发送的浏览器原始网络事件在补读时已被淘汰，`send115-http.json`如实记录`truncated:true`；未补造HTTP回执。上述功能结论依赖原生完整历史和调用观测，不能称为原HTTP账本完整。后续测试在专用网页临时复制实际Fetch回执，保留原请求和Response、不重发，以避免再次丢失证据。

普通文本的原生调用仍耗时约36.13秒，5次准备耗时有明显波动。纯图再次捕获同一错误门禁：`steer → start`准备完成后，本方看到`runtime.type=active`，但活动轮次列表、队列均为空，触发`queueRequired`，又执行`start → steer → start`。见`send115-image-gates.json`。因此“自己的队列行被算作前序消息”已被该样本排除，尚不能把每次准备耗时波动都归到此门禁。

独立源码审查定位到安装版`app-initial-4267e52fe5b0.js`的`fpn`：它在调用本方`assertRequestCurrent`前，先建立`turnId:null/status:inProgress`的本次启动占位并设置`active`，之后才派发`turn/start`。这意味着`active`在该阶段是合法的本次启动状态，本方不能无条件把它当作另一轮忙碌。原生`Imn`的`queue`在空闲时继续启动，在前序消息、活动轮或pending start存在时入队；`submitPrepared`又在最终admission后核对忙碌并返回deferred。116 已据此复用原生决定，源码回归及本次真实采样边界见下节。

用户原工作主会话有`forkedFromId`，但`source=vscode/threadSource=user/ephemeral=false/parentThreadId=null`，仍是普通主会话；此修复不能仅覆盖新建、非fork会话。只读身份原件为`real-main-identities.json`，没有在用户工作会话发送测试消息。

阶段115包已构建并逐项校验，未安装或发布：Windows为2026-11-02 14:27:28的0.6.0，SHA-256 `acb0a87a0251f9a62fcd309fe949e6a0eeeafe444e213d18165ad2191958b7b7`；Android为14:16:41的0.1.0/code24，SHA-256 `bb28be126007074da697f3a44f4915af4cbc7277d9bb56b23ab6640198269d7d`。见`artifacts-115.json`。这些身份属于阶段115，后续管理端构建须重新记录；Android本次确有网页资源变化，不再沿用此前“与11:16包相同”的结论。

## 116 普通发送终审

本节关联 `final116-text.json` 的原始调用、`final116-http.json` 的原请求与 Response 副本，以及 `final116-completed-after-restore.json` 的只读完成历史，与 114/115 原件独立比较。**该普通文本正常子场景已完成核对**，不把“最终”文件名或通过数量当作运行版本和稳定性能证明。

| 核验项 | 可确认结果 | 精确原件位置与边界 |
| --- | --- | --- |
| 适配器发送 | 观测窗口内 `action=send` 恰一次，耗时 12008.5 ms，`ok=true` | `final116-text.json` 的 `/profile/records/18`（ID 19）；不是 HTTP 完整往返 |
| 原生准备 | 恰两次，顺序为 `steer` 5307.2 ms → `start` 5362.6 ms；本窗口未出现 `queueRequired` | `/profile/records/35`（ID 36）、`/profile/records/39`（ID 40）；只证明该样本没有重入准备 |
| 原 HTTP 回执 | 观测副本中 `send` 恰一次，HTTP 200、12110.4 ms；回执 `b71f1219-7178-4b52-9ce2-c084245a545a` 为 `completed/ok=true`，原生结果 `sent/kind=start` | `final116-http.json` 的 `/exchanges/0`，`payload` 与 `body` 分别保存原请求和回执 JSON 字符串；另一项 `/exchanges/1` 是独立 `markRead`，不能算第二次发送 |
| 唯一接收及完整文本 | HTTP、回执和完成历史均绑定客户端 ID `9551a7da-673d-4ec8-8561-ca5ad8759922`、同一会话和目标轮；12 轮快照中仅一条对应 `userMessage` | `final116-http.json /exchanges/0/payload`、`/exchanges/0/body`；`final116-completed-after-restore.json /state/turns/11/items/0`。其 `content` 与同轮 `params.input` 逐字段一致，完整保留原 HTTP `prompt`，仅由原生追加一个末尾换行，没有截字或替换正文 |
| 旧历史 | 原有 11 轮逐字段保持，仅新增目标轮 `01a19b1e-8e81-72f0-b4b1-b801f07c794e`，最终队列为空 | `send115-image.json /state/turns` 与 `final116-completed-after-restore.json /state/turns` 前 11 项相同；新消息为 `01a19b1e-8edc-7901-963f-debcddf4a513` |
| 完整回复终态 | 同一目标轮 `completed`，唯一最终回复精确为 `PERF_116_OK`；会话 `idle`、共 12 轮、队列 0 项 | `final116-completed-after-restore.json` 的 `/state/status`、`/state/turns/11/status`、`/state/turns/11/items/1/text`、`/state/queue`；`final116-completed-ui.txt` 及 `final116-completed-web.png` 补充真实页面显示 |

早期 `final116-text.json` 的 `/state/at=2026-11-02T07:17:22.694Z` 仍为 `inProgress`，该记录保留，未改写为终态。原 HTTP 副本于 `2026-11-02T07:27:18.998Z` 从此前测试页 Fetch 闭包中只读提取，没有重发。完成历史于 `2026-11-02T07:27:30.518Z` 补读，发生在恢复适配器 106 之后，不能称为仍运行 116 时取得的终态快照。完成文件中的 `profile` 是此前 116 测量保留并继续累积的观测，原一次发送和两次准备记录与 `final116-text.json` 完全一致，不是恢复后再次发送或重新计时。

115 文本原件 `/profile/records/37` 的发送耗时为 36125.2 ms，准备序列为 `steer → start → start → steer → start`；116 本次收敛为两次。115 纯图片发送为 9593.3 ms、同样准备五次，比 116 文本样本更短，再次说明准备次数下降不等于所有输入的总延迟必然下降。各样本均为不同真实消息和轮次；原生准备仍约耗 5.3 秒一次，当前没有稳定性能结论。

`cdp-116-final.log` 记录完整桥接回归 **219/219**、零失败；`ui-state-final.log` 记录前端状态 **39/39**、零失败。桥接日志包含自身启动占位不重入、迟到忙碌仍由原生排队、未知结果不重复派发、Skill/侧边/队列输入完整准备、压缩及 manager 变化阻止派发等回归。这证明相应源码夹具通过；真实 HTTP 与终态依据上列独立原件，异常分支仍不能用通过数量替代。

`native-final116-fixture.json`、`native-final116-installed.json` 登记源码116、兼容标记106、fixture `7457cba5-a0eb-49de-a4f4-95c4dcfb6dba` 及预期脚本摘要；源码对应关系另保存在 `native-final116-source.json`。它们仅证明准备与安装回执；`native-final116-expected-runtime.js` 是预期运行文本，不是Debugger重新读取的引擎原件。`fixture-restored-116.json` 记录 `2026-11-02T07:21:36.026Z` 已恢复106。恢复后补取引擎脚本得到0个匹配，见 `native-final116-capture-unavailable.json`；该缺口不由117证据补写为通过。

## 117 普通发送最终实测

本节独立使用 `final117-before.json`、`final117-after.json`、`final117-http.json` 和 `final117-ui.txt`，不沿用 116 的回执或轮次。**普通文本正常子场景通过；性能稳定性仍为部分确认。同步问答真实验收见后续增量，不能把单次问答 HTTP 时延当作稳定改善。**

### 运行身份与原请求

`native-final117-before-identity.json`、`native-final117-after-identity.json` 登记同一 fixture `b5f2e77e-af13-4c52-a390-f61bf70308c5`，源码 117、兼容标记 106，起止均使用实际引擎 `scriptId=69951`。独立读取两份 `native-final117-*-runtime.js` 计算 SHA-256 均为 `bef7338262d3c79a6aa403092923812746cff6f7ceff0fe908dea0a86529f0cc`，与身份原件的 `expectedSha256/actualSha256` 一致，两份文件逐字相同。这补齐本阶段实际引擎脚本身份，不补造 116 的历史证据，也不代表已安装最终包。

真实发送来自新标签 `1283922733`；`final117-http.json /events/30` 为唯一 `Network.requestWillBeSent` 的 `/api/action/send`，sequence 120、网络请求 ID `75400.84`。该事件区间 `truncated=false/hasMore=false`，HTTP 200，按同一请求的 `loadingFinished.timestamp - requestWillBeSent.timestamp` 重算为 **3759.611 ms**，与 `/durationMs` 一致；响应事件不另计为派发。旧标签断连、没有点击发送，其是否关闭由主代理单独收尾，不能写成已经关闭。

`/responseBody/body` 的原回执 ID 为 `a05348ad-453c-4fb4-aed5-68397fcc2148`，`state=completed/result.ok=true`，同时返回 `sent/kind=start`、`deliveryState=accepted` 和 `queueSyncPending=true`。后者表示队列同步仍可继续，不是发送失败或重复提交的理由。

### 耗时与功能逐项核对

| 项目 | 原始结果 | 定位 |
| --- | --- | --- |
| 适配器发送 | `send` 恰一次，3463.6 ms、`ok=true` | `final117-after.json /profile/records/77`（ID 78） |
| 原生准备 | 两次：`steer` 1880.7 ms → `start` 519.7 ms | `/profile/records/87`（ID 88）、`/profile/records/88`（ID 89）；不能与外层耗时重复相加 |
| 消息身份与输入 | HTTP、回执及原生正式消息均匹配客户端 ID `c33e574d-07c2-4e24-b3f3-d68cc85b399b`、会话 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1` 和轮次 `01a19b3e-8b65-77b1-8026-7839ea120da9`；13 轮中只匹配一条正式用户消息 | `final117-http.json /events/30/params/request/postData`、`/responseBody/body`；`final117-after.json /state/turns/12/items/0`，消息 ID `01a19b3e-8c1d-7861-9cf0-396440c5748c` |
| 完整文本 | 原请求 `[remote-e2e:RA-20261102-perf:plain117] 只回复 PERF_117_OK，不调用工具。` 完整保留，原生只追加一个末尾换行；`params.input` 与正式消息 `content` 逐字段相同 | `/state/turns/12/params/input`、`/state/turns/12/items/0/content` 与原 HTTP `prompt` 独立比较 |
| 旧历史 | 操作前 12 轮与完成后前 12 轮逐字段相同，仅新增本轮 | `final117-before.json /state/turns` 对照 `final117-after.json /state/turns` 前 12 项 |
| 完成与界面 | 同轮 `completed`，唯一最终回复为 `PERF_117_OK`，会话 `idle`、共 13 轮、队列为空；实际页面显示相同输入和回复 | `final117-after.json /state/turns/12/items/1/text`、`/state/status`、`/state/queue`；`final117-ui.txt` |

116→117 的单次普通文本 HTTP 为 12110.4→3759.611 ms，适配器为 12008.5→3463.6 ms，两次准备合计也由 10669.8→2400.4 ms。准备次数均为两次，原生准备本身已明显波动，因此不能把全部差值归于移出队列等待，更不能据两条不同消息、不同轮次给出稳定改善比例。此次回执与后续原生完成分别核对，功能正确性不依赖耗时较短。

### 源码门禁与剩余范围

`cdp-117-final.log` 记录完整桥接 **222/222**、零失败；`cdp-117-result.json` 的 `exitCode=0/sourceStable=true`，起止源码摘要均为 `4ce7129ea8d11da315b622bc2c973837edadd6b4e5a3df4ccff9b2db8e45232a`，与本阶段身份记录的 `sourceInfo.originalSha256` 一致。前端 **39/39** 是本轮此前的 `ui-state-final.log` 检查；117 未改前端，不将其写成 117 新执行的前端测试。

117 同时改进了同步问答的历史读取。后续 `remote-sync-question-20261102.md` 已补两道真实阻塞问答：正常回答 HTTP 为 243.161 ms；成功后丢失回答 HTTP 响应的样本通过仍可用的状态读取/事件通路收敛，已捕获窗口未重复回答，但没有查询原操作 receipt，不能称问答 receipt 恢复通过，也没有 A/B 性能结论。worktree 恢复、Skill/图片的普通发送、并发、完全离线、长期 unknown、Android 真机和 Windows 视觉仍按各自证据报告；最终包已构建并核对摘要，但未安装或发布。用户原工作会话未作为发送对象，本文不宣告整体目标完成。

## 117阶段构建身份与环境恢复

2026-11-02最终Windows专用构建入口退出0，执行会话99390，产物时间15:56:44，版本0.6.0，46,557,532字节，SHA-256为 `cc7960eaf09e3702913a72497bb3aaac8f286cc00505c970d7fb6d904aec6f24`。本次复核重新计算了安装包、launcher、manager及 `target/release-fast`、`target/release`、`dist/windows/app` 各副本的 SHA-256，均与 `artifacts-117.json` 一致。当前源码及构建记录中的内嵌资源摘要也一致：`native.js` 为 `4ce7129ea8d11da315b622bc2c973837edadd6b4e5a3df4ccff9b2db8e45232a`、`app.js` 为 `072afb6e66f350188bf505e9520c8538b64529abca30825c6ccc3b25c2788a7f`、`renderer-features.js` 为 `1f2374eef8eae585ceb485aec795fb19a2a3ade96fdbc55871368a295505d68e`、`session-menu.js` 为 `a7660e057941d512acc9a9582e30313e8206a2b3fefc7546b0bc9fc9df06bb10`；记录均标记 `matches=true`。`artifacts-115/116/117.json` 对 renderer-features.js 也记录相同摘要，与当前源码一致。launcher和manager包含117版native与13版renderer，manager还包含上述最终 `app.js`、`session-menu.js`。见 `windows-build-117.log`、`windows-build-117-result.json`、`artifacts-117.json`。这些证据证明本地构建包及其记录摘要一致，不证明已经安装或发布。

Android沿用本轮14:16:41的正式Release构建及lint结果，0.1.0/versionCode24，2,632,045字节，SHA-256为 `bb28be126007074da697f3a44f4915af4cbc7277d9bb56b23ab6640198269d7d`。更新清单及26项APK内置网页资源均重新核对通过。116→117只改桌面适配器与桥接测试，不改变Android打包输入。本轮两个产品版本号均未修改，未安装、提交、推送或发布。

117实测结束后，临时适配器已恢复安装版106，epoch为 `9e4d9d5e-a38e-4812-8261-39ea32ec5881`；写入状态表均为空，历史中的队列编辑记录均为已提交或已取消，性能观测器已移除。1432调试服务PID43448已核对命令身份并停止。见 `fixture-restored-117.json`、`runtime-restored-117.json`、`dev117-stopped.json`。

新测试标签1283922733已关闭；旧标签1283922709因浏览器调试连接失效未能确认关闭，清理后的真实库存仍包含它，见 `browser-cleanup-117.json`。没有反复重发或调用其他控制方式关闭用户浏览器。该页只包含测试会话，调试服务现已停止。

本轮20个临时源文件已逐项校验后归档到 `runtime-sources/`。删除操作被自动审批以 `blocked by policy` 拒绝，`temp/remote-performance-20261102/`原件保留，没有改用其他方式删除；详见 `cleanup.json`。既有其他测试目录及用户文件未清理。

## 18:00后续真实恢复验收增量

本节记录的是截至18:00左右完成的问答、发送和队列验收及其恢复状态；它不代表之后启动的新一轮测试环境已经恢复。

产品源码与两包未变化，继续使用117业务源码/106兼容标记，另取得三组独立原件及subagent复核：

- 同步问答的正常青色回答HTTP约243.161ms；另一个橙色问题的原生成功响应被真实中止，准确答案仍仅消费一次，旧历史与普通稿保持。没有问答性能A/B，不把243ms写成稳定提速收益。
- 普通发送成功回包丢失后，网页GET原receipt恢复；唯一client/turn接受、刷新零追加send，新草稿未被清空。
- 一图/Skill队列消息保存、另一条纯文字取消分别丢失成功回包，均GET原receipt恢复；图片/Skill/原文保持，随后原阻塞问题被回答，B→C自然各执行一次。

这些子场景均保留事件/其他读取通道，不能代替完全离线、长期unknown、服务重启或所有竞态。详情见 `remote-sync-question-20261102.md`、`remote-send-recovery-20261102.md`、`remote-queue-recovery-20261102.md`。测试会话完整设置已恢复原值，原生适配器恢复106，当前三组写状态表为空，两张新测试页及1432服务已收尾。第二客户端偏好验收因设备上限受阻；没有撤销用户设备。

## 后续测试运行状态（18:34阶段记录）

该阶段另有一轮工具测试正在运行：临时 fixture 已启用，PID 47464 正在提供1432服务，Chrome验收页为 `1283922789`。当时恢复证据未齐；这轮的实际收尾记录已在下节补齐。此前关闭的 `1283922757`/`1283922758` 仍只属于上一轮。

## 118：相邻交互的最小性能修复与真实验收

本轮由两个subagent分别复核调用链及历史证据，主代理处理前端合并读取、真实浏览器验收和运行身份核对。生产修改限于 `native.js`、`app.js`；没有增加权限门禁、重试框架或跨请求元数据缓存。

| 操作 | 证实的冗余或等待 | 最小处理 | 保留的正确性要求 |
| --- | --- | --- | --- |
| 打开／切换会话、显示模型和上下文 | `thread` 返回前同步等待队列订阅首帧，队列上限为10秒 | 建立订阅后返回已有详情；队列由原订阅更新 | 未取得队列时省略字段，不伪造空列表；复用订阅、异常隔离及旧manager事件拒绝保持 |
| 标记已读后刷新列表 | 已读只修改现有摘要，却令完整目录失效，下一次重扫所有分页 | 保留原生已读确认，单会话刷新不读完整历史，不清空目录加载状态 | 原生拒绝仍失败，新轮未读仍生效，其他会话状态保持 |
| 完整侧栏加载 | 配置组先读一次置顶ID且未使用，后面又调用原生置顶列表 | 删除未使用的配置读取，每次完整侧栏只读一次实际置顶列表 | 最新置顶数据、返回结构校验、降级和目录generation检查不变 |
| 并发普通列表读取 | 相同cursor的普通调用无条件要求完成后再读一遍 | 普通调用共享在途请求 | 置顶、删除、目标终态、编辑／发送成功及删除通知明确要求fresh；不会吞掉写操作后的刷新 |

新增／加强的长期回归位于既有 `cdp_bridge.rs` 和 `remote-access-ui-state.test.mjs`。完整桥接223/223、前端42/42通过，JS语法、Rust格式和diff检查通过。回退到旧行为时，队列首帧阻塞、重复置顶读取、普通并发补读的测试均按预期失败；这是源码行为验证，不代替全部远程E2E。

真实原件根为 `dist/validation/remote-performance-20261102/followup-118/`，最终运行在其 `final/`：

- 最终源码 `native.js` SHA-256 为 `63f99482a1d0a3fa44e9d327b56c013a8f4e18da731bd2490c34012e8ade10b3`，`app.js` 为 `13474bf0ceef7bf89750c104275af854f64593f361c1a5f97a57c87919d60a22`。最终起止引擎文本摘要均为 `ca220d3eb35daa73068d70f55bdfedf4630728cd68a14f04b0905893c3303fba`；仍为118业务源码／106兼容标记，不能称用户程序已经升级。
- 首次网页重载仍执行旧缓存app.js，Debugger摘要为 `072afb…`，该轮不作为最新前端验收。只在测试标签启用缓存旁路后重新读取引擎文本，摘要与最新源码逐字一致，见 `browser-app-fresh-identity.json`。最终测试后已恢复旁路开关。
- 最终两次切换详情HTTP分别为138.801ms、80.495ms；对应首次历史读取99.445ms、98.170ms，已读104.911ms、112.185ms。全部相关窗口未截断，写请求只有测试会话的markRead，没有发送／重发消息。见 `final/verification.json` 及四份switch HTTP账本。
- 页面B展示模型 `gpt-6.1-terra`、37,107/353,400（11%）；切回A展示 `gpt-6.1-sol`、59,274/475,000（12%）。分别与同一会话的原生快照一致，模型菜单可正常展开和关闭。测试覆盖显示与切换，不冒充修改模型后真实发送的协议验收。
- 已读专门复核中，完整目录161项、`catalogLoaded=true` 在原生已读成功后保持，目录成员不变，目标已读状态正确。该样本使用删除重复置顶读取之前的118文本，原件为 `mark-read-catalog.json`；最终文本另由浏览器两次markRead及 `final/verification.json` 核对。

**仍需保留的边界：** 原生列表读取仍有2–4秒单样本波动；其耗时包含侧边状态、全局配置、一次`thread/list`首页和实际置顶读取。尚缺分段计时，不能将它们都当作过度保护或宣称已消除全部性能问题。公共入口与具体写操作的元数据重复读取、菜单失效后的补读已检查，前者本次引导样本约4ms，后者可能承担新状态刷新；未为此扩展缓存接口或删除身份检查。停止后的后台终端清理也未因只有静态重复线索而修改。

最终文本下另有三个完整侧栏HTTP样本2319.667／2344.438／2329.463ms，已补入 `final/verification.json.sidebarReads`。因此会话详情80–139ms不能代替整份列表加载耗时，剩余侧栏等待仍需单独定位。

19:17已恢复原生适配器106（epoch `79b850ba-d8f0-42b6-ab46-377f7030ada5`），关闭测试标签1283922789、恢复浏览器缓存开关，并核对命令身份后停止1432调试进程47464。本轮9个临时文件已逐项校验归档后删除，既有被拒绝清理的目录未再操作。依据见 `final/fixture-restored.json`、`browser-cleanup.json`、`service-cleanup.json`、`temporary-cleanup.json`。

本轮没有重新构建、安装、提交或发布。117阶段的15:56 Windows包和14:16 Android包**不含本节最新修改**；app.js属于APK内置资源，后续提供完整新安装包时应一并重构建Android。产品版本号未修改，整体73条验收目标仍为active。

## 119：侧栏剩余等待的分段定位与修复

原件位于 `dist/validation/remote-performance-20261102/sidebar-breakdown/`。118与119采用同一14阶段插桩，父段已包含子段，不将嵌套耗时重复相加。测量期间没有重试业务写入。

**已确认的局部原因**：六项配置分别经过 `requestClientGlobalState → nativeModuleSnapshot("initial") → nativeResource("initial")`。既有模块缓存按资源URL查询，但查询前仍重新定位资源、识别读取函数。现场缓存已经包含initial模块；直接取缓存仅0.1ms，而一次完整模块快照调用370.2ms。当前DOM的4个modulepreload和250个Resource Timing记录没有initial入口，定位会进入可信入口的依赖查找。原生读取函数的六项真实配置读取合计仅20ms，导出扫描12.2ms，耗时主要在重复解析路径，不能归因于配置本身、侧边状态或目录分页。

依据分别为 `samples.json`、`global-reader-source.json`、`cached-module.json`、`direct-global-reads.json`、`resource-observation.json`。原生导出 `W3n/RVt` 根据键读取项目目录、项目展示和成员关系；没有证据表明本次六键都读取整份globalState，不采用这一未证实解释。尝试一次传输10.8MB源码的诊断调用超时，未作为业务读取耗时证据；后续仅返回耗时和类型的只读对照成功。

**最小修复**：只缓存原生全局读取函数的解析Promise，立即登记以合并首次并发解析；解析失败只清除自身Promise，后续可重试。每个键仍执行本次原生 `read(key)`，不缓存配置值；业务读取失败不丢弃已解析函数。未改通用资源查找、权限、发送或队列接口。既有回归补充了解析次数、并发合并、失败重试和新值读取，旧实现准确失败 `6 !== 1`，最终桥接224/224通过；前端未再改动，沿用42/42结果。

| 同口径真实样本 | 118 | 119 |
| --- | ---: | ---: |
| 主动侧栏读取1 | 3277.6ms | 111.7ms |
| 主动侧栏读取2 | 1800.6ms | 97.6ms |
| 主动侧栏读取3 | 1748.3ms | 85.9ms |
| 对应配置组耗时 | 2952.0／1717.6／1687.5ms | 24.4／39.3／18.1ms |
| 返回数量 | recent156／pinned5／projects19 | recent156／pinned5／projects19 |

上述主动样本来自同一已安装桌面、不同时间的独立读取，属于模块已解析后的短时样本，不是稳定分布或所有机器的速度承诺。实际网页点击刷新又记录到三次侧栏HTTP分别415.987／162.966／172.932ms，包含本地接入和同时发生的其他读取；线程72.148ms、模型143.140ms、Skill364.811ms、历史407.862ms。全部相关网络窗口保留在 `browser-refresh-start.json`、`browser-refresh-end.json`，不能把原生调用85–112ms当成完整HTTP或模型推理耗时。

**首次初始化仍单列**：119重建适配器后的首个侧栏样本4341.4ms，其中首次侧边关系发现3733.4ms、配置组401.1ms。该首次模块发现没有被本修复消除；随后缓存命中的样本不能替代首次加载验收。未继续扩大为通用模块缓存或改变侧边关系校验。

119最终源码SHA-256为 `d8c635875c50622194fdfebcede393afb540c9263bc44c6a3809231ceca00944`；插桩文本为 `28e24ac1dcbfe88bf426df8451fd26b4ab955221d491544da74053cb9f3100cd`。起止引擎文本均为 `14a61c24b2dbe6cda75fc96aabd2db0faa0732f06fc3b079f31966b996a6dc9d`，与实际兼容变体逐字匹配，原件在 `source-119/`。118对照原件独立保存在 `source-118/`，34个诊断文件逐项校验归档，未覆盖旧证据。

20:24已恢复安装版106，epoch为 `a3b983ad-ec7d-4d5a-9f35-5a4475cf92e8`；20:26已关闭测试页1283922798、恢复浏览器缓存开关并停止已核对身份的1432进程84288。网页实际app.js摘要仍为 `13474bf0ceef7bf89750c104275af854f64593f361c1a5f97a57c87919d60a22`，见审批原件目录的 `browser-identity.json`。

本轮没有构建、安装、提交、推送或发布。两个专用Git夹具的清理被自动审批以 `blocked by policy` 拒绝，整条清理命令未执行，原文件及Git元数据保留，未换命令重试；本轮临时诊断源码也保留供未完成验收复用。详见 `workspace-git-final/fixture-cleanup.json`。当前整体目标仍未完成。

## 122 队列保存的已发送确认路径（2026-11-03，仅源码回归）

**已复现并修复额外等待；真实页面耗时改善尚未测量。** `submitNativeQueuedEdit` 原来没有传入适配器已有的完成选项，原生 `sendMessage` 返回 `sent` 后仍等待新队列订阅首帧，随后才进入正式历史接受核验。首帧没有返回时，保存停在该等待，而正式历史本已具备可核验的准确消息。这是离线调用路径证据，不等于已证明用户现场的全部延迟来源。

现在只为该提交复用 `deferQueueSnapshotOnSent:true`。`sent` 直接进入原有接受核验，仍检查 thread/client、完整输入、唯一新接受记录、事务及连接归属；`queued` 继续等待真实队列快照，保存和取消恢复均保留原来的输入保护。独立复核发现成功返回分支还必须保留原 catch 中的可选回执归属条件，因此在设置 `mutationAcknowledged` 前核对返回的 `threadId`、`conversationId`、`clientUserMessageId`；缺省字段仍允许，不新增协议必填字段。

另用真实 `resetStaleManager` 函数在提交等待中退役连接，复现 catch 再次访问空 manager 而抛出 TypeError。只将这一处读取改为空值安全访问，使原事务保留 `outcomeUnknown`，重复核对不再派发输入，没有新增状态层、重试或全历史扫描。

相关回归29/29、完整桥接231/231通过；语法、格式与差异检查通过。新增和调整的测试覆盖首帧不返回但正式历史可核验、历史未确认不能报成功、`queued` 仍等待、错归属回执拒绝，以及断连后原事务不重发。原始红绿输出保留在执行工具记录中，归档摘要 `dist/validation/remote-acceptance-20261103/queue-edit-sent-122/verification.json` 明确不是原始日志。产品版本仍0.6.0、Android版本未改；内部适配器缓存标记升122，源码SHA-256为 `b4e92bc7c760c59fdb50d8940e0efbf96c0c3a5710dfbfeec0cb1d2e2c5eb04e`。

原先要求“`sent`仍必须等待首帧”的测试已替换为“正式历史确认前不能成功”；等待首帧的正确约束单独保留在`queued`测试。另补历史未确认与管理器退役两个场景，测试数量由228净增3项至231，没有通过删除输入完整性或不重发断言消除失败。

本次没有操作浏览器、注入运行代码、启动服务、编译安装包或安装。真实整链、保存延迟、断连恢复及122运行身份仍需后续验收。另一项“详情和历史可能重复读取元数据”的候选缺少现场重叠证据，本轮未改。
