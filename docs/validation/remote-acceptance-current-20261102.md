# 73 条远程会话功能基线：截至 124 源码的现状与下一步

**2026-11-03 汇报口径更正：**“队列、引导与停止”和“侧边会话与子智能体”不能整类列为未完成。下表各大项的 `partial` 表示仍有明确子场景缺少证据，不否定已验证的正常流程。此前部分行未合并后续 E39/E43/E50/E58/E60/E64 记录，仍重复安排早期动作或要求已暂停的桌面操作；本次按原件复核结果更正这些当前待办，历史独立报告保持原样。

| 已有真实通过记录的子场景 | 证据 | 真正剩余的范围 |
| --- | --- | --- |
| 自然排队承接、编辑保存/取消、多行与图片/Skill保留及增删图 | E03/E07/E09/E29/E60/E63/E64 | 非空隐藏引用等尚无真实样本的输入；服务端原生长期未决或进程重启恢复。已测输入及自然消费不重复列为未做 |
| 普通停止后原队列保留并显式恢复；命令审批取消后恢复；活动轮内引导；引导成功回包丢失并跨刷新查询原回执 | E37/E39/E43/E60 | 原生准备拒绝、真实上游失败和长期未决等独立分支；上游失败隔离仍受环境阻挡。成功回包丢失不等同于原生长期未决 |
| 自然出队后的迟到编辑/删除明确拒绝，原消息唯一消费、草稿保持 | E50 | 该基线允许的迟到拒绝结果已通过；不为补另一种合法结果扩大并发设计 |
| 侧边创建、首条自动命名、原生API改名同步、自定义名保护、正常关闭、迟到创建不抢页面、关闭残留读取拒绝、过期提示、空侧边关闭丢回包恢复 | E16/E40/E42/E46/E47/E58及较早原生创建记录 | 执行中关闭、真实失败/长期未决、特定双草稿与阅读位置组合；桌面菜单操作按用户指示暂停。API双向同步已验，不能概括为命名未验 |
| 子智能体名称、完成列表顺序、直接父关系、active→done及只读布局 | E17的原生/远程集合和生命周期原件 | waiting、真实孙级/两个父范围、发现不完整后的恢复及新只读提示实际展示；22条是样本数，不是固定预期 |

这些结果属于各自归档的真实运行版本，不冒称124安装包全量重新验收。未验证的分支也不自动视为当前已确认缺陷。后续优先核对单客户端问答全通道恢复/旧读隔离、CLI兜底资格及复合草稿恢复的具体缺口；Android和Windows闪窗由用户验收，桌面窗口操作保持停止，文件审批排除，双远程并发从简。除相关代码变化或出现具体反例，不重复已经闭环的子场景。

**E70继续验收（2026-11-03）：** 在独立127.0.0.2服务完成普通文本send的真实超时→未决锁跨刷新→原A GET放行恢复。原A/client及后写草稿保持、唯一接受，M54→55且原54轮/C8完整历史保持，send1、刷新后send0。临时后台观察器失败、原POST正文缺失、成功GET由定时器发起而由人工放行等边界均单列，不冒充预定主动丢包或完全自动恢复。原生123运行、授权及用户服务已恢复，安装包和产品源码未变；见`remote-send-refresh-124-20261103.md`。下一优先转为问答发现/重连的尚缺边界与CLI资格，不重复本次已闭环的普通send刷新恢复。

**2026-11-03 安装重启后的新增事实：** 已核实安装目录三项可执行文件与此前123交付包一致。E68的旧网页由正常关闭重开更新到当前64a074…，没有修改缓存逻辑。E69发现并修复冷会话目标缺失：原生有暂停目标，远程却将未物化会话的目标返回null；现仅对冷的持久主会话补一次目标只读RPC，暖会话零新增RPC。235/235桥接回归通过；同一真实M目标恢复、整页刷新保持、清理后不复活均有原生与浏览器证据。M54/C8全历史保持，唯一业务写是一次准确测试目标清理；旧B回执未重放。产品源码内部升124，真实测试业务文本仅版本标记不同，测试结束原123运行已恢复。见`remote-installed-resume-123-20261103.md`。

**本轮交付与继续指示：** Windows0.6.0安装包已重新生成并内嵌准确124源码，Android0.1.0/code24正式构建及lint通过、APK与此前同资源包一致；版本未递增，未由代理安装、提交或发布。产物与摘要见`remote-delivery-124-20261103.md`。用户随后明确选择“保持目标运行”，撤销此前暂停安排，本轮继续R3-01普通发送未决跨刷新；其后为单客户端问答全通道恢复/旧读及CLI资格边界，不重复E64/E66/E67/E69。目标工具当时仍返回paused且不支持代理改回active，已告知用户可点击继续目标恢复自动续跑；这不阻断当前已授权的验收工作。Android及Windows闪窗保持用户验收，桌面窗口操作仍停止；整体目标不标完成。

**用户最新范围约束（2026-11-02）：文件审批不是原有产品功能，仅记录能力缺失，不纳入本轮开发。** 本轮新增的文件审批展示、辅助函数及6条专用测试已撤回；原有命令审批、问答和既有兼容逻辑保留。历史基线中的“文件审批”条款以本约束为准，不再作为完成目标的必做开发项。待决文件测试请求5已取消，目标文件未创建，完整测试设置恢复；不把这次取消记录为文件审批功能交付。

2026-11-03再次独立核对当前工作树与HEAD：没有文件审批专用UI、辅助函数、样式、能力开关或专用测试残留。原有通用审批按钮、文件请求回复分发、文件修改记录及本轮承接既有分发的RPC方法映射保留，不能因含有文件审批协议名就删除。此次只复核，不再扩大删除范围；文件审批仍为“能力缺失／本轮排除”。

**2026-11-03后续范围调整：双远程客户端同时操作按用户要求简单处理。** 保留基础过期请求拒绝、不自动重发和草稿保护；不继续展开双设备同时提交、原子协调和组合竞态，不为这些低频场景新增协调层。已启动的目标旧请求测试在浏览器等待期间被取消，未得到服务端409，不标通过、不重试该竞争。原件和边界见`remote-goal-stale-20261103.md`。后续优先单客户端问答、重连、队列和草稿恢复。

本轮新增单客户端实际缺陷：13:20:34原生页面重新加载后，实际入口仍在`document.scripts`，预加载列表及Resource Timing却没有initial或允许的根入口；原生模块发现返回503并触发CLI兜底。安装包仍为26.1027.3384.0，初始模块内存差异由既有Fast v3转换精确解释，不归因为安装更新。123源码仅补脚本收集及index根入口；新增行为回归在旧码准确失败、修复后通过，完整桥接232/232通过。此时尚未取得123真实RPC恢复证据，不能把离线模块链或源码测试写成现场恢复；诊断原件在`goal-storage-runtime/module-diagnosis/`。原119恢复契约因文档/epoch/fixture变化失效，不强制覆盖当前页面。

E66补齐侧栏偏好的真实localStorage配额失败：本页切换仍生效、失败提示明确、普通稿保持，窗口内零业务写；移除测试填充后成功保存，最终恢复原0键。图片／IndexedDB配额受浏览器接口限制，仍为not_run；未上传图片或提交消息。见`remote-preference-storage-20261103.md`。此项不新增产品逻辑。

后续E67完成123实际模块解析和M/C读取恢复，原54/8轮完整内容保持，页面显示准确A暂停目标；见`remote-native-entry-123-20261103.md`。用户随后明确要求两端编译后暂停：Windows0.6.0与Android0.1.0/code24均已构建校验成功，未安装或发布，交付见`remote-delivery-20261103.md`。原45430测试服务、9229调试连接和旧测试标签已不可用，未强制运行恢复脚本；测试目标及旧回执原件保留供安装重启后继续核对。整体目标仍未完成，按用户指示交付后暂停。

2026-11-03 10:17收尾：R3-20仅完成同版独立服务准备及M46/C8、空请求/队列的原生基线读取，未发送测试消息、未执行整链。原生桌面操作被用户Esc中止后，桌面及浏览器后续交互均停止；R1-27置顶仅只读探查，无需恢复置顶变更；原生侧边菜单和Open In也未执行，见 `remote-desktop-controls-20261103.md`。121临时运行已准确恢复119（脚本SHA-256仍为`5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`），仅runtimeEpoch更新为`7c35bd15-1ef9-469e-8898-2f975fcf5098`；本轮56212/45430服务已停止，旧1432/45222保持。证据在 `dist/validation/remote-acceptance-20261103/crossflow-121/`。临时服务目录删除被自动审批以`blocked by policy`拒绝，254个文件保留，没有改用其他删除方式；本轮主代理的独立临时驱动已归档并清理。没有安装包构建、安装、提交或发布，目标不标完成。

随后仅源码增量：修复恢复报告可通过删除／修改`required`隐藏`not_run/blocked`的汇总缺口，必测定义和整体状态改为按源码案例编号计算，原执行证据与动作保留；PR工作流补入已有回执恢复驱动测试。四组Node集成275/275通过、22个远程JS资源检查通过，原件在 `dist/validation/remote-acceptance-20261103/regression-ci/`。另完成122队列保存优化：只有原生`sent`跳过额外首帧等待，仍执行完整接受核验；`queued`不变；保持错归属回执拒绝，并修复提交中manager置空时catch再次异常。相关29/29、完整桥接231/231通过，见 `remote-performance-20261102.md` 末章及 `queue-edit-sent-122/verification.json`。本轮没有重新操作界面或部署122，前端仍为64a074…，真实整链与实际耗时未验，不能把源码通过写成R3-20完成。

用户随后明确允许继续浏览器回归、停止桌面窗口操作。E60已在122/106与64a074…同版页面完成同步问答→目标标签本地编辑/删除→队列保存、刷新、自然消费及另一准确队列原轮引导，M46→50/C8、原历史和普通草稿保持；第二保存原HTTP正文采集缺口以单列原A诊断GET及原生状态补证，不冒称自动恢复。见 `remote-crossflow-122-20261103.md`。E61完成测试项目P、会话C的置顶、刷新、另一端同步及取消恢复，见 `remote-project-pin-20261103.md`；链路为1432→45222，另一端仍旧网页。桌面窗口相关入口本轮不操作，Android与闪窗分工不变。122空闲sent保存与独立上游失败正在另项验收，尚未因此标整体验收完成。

本轮后续收尾：E63补齐Q取出编辑、原活动轮结束后空闲保存的真实普通文本分支，原client唯一接受于T52，原50轮/C8和普通稿恢复保持，模式/权限恢复；网络摘录缺完整时长、本页前端执行身份及清稿截图差异仍单列。独立上游失败环境E62在业务派发前受阻，不计产品失败。最终M52/C8、无请求/队列；原生已恢复119基线文本，epoch为`110c83d1-b1bb-4f20-891c-ea9c9f339ca0`，本轮52596/45430服务关闭、旧1432/45222保持。原生起点至after-sent实际脚本一致。新联调目录清理被策略拒绝、258文件保留；业务驱动已归档清理。未提交、推送、打包或安装；目标仍未完成。

2026-11-03 13:07增量：E64完成122同版普通文本故障链：目标标签本地编辑/移除后保存队列，真实成功回包被中止、原回执读取持续中断，刷新仍保留同A/E和普通稿备份；新loader只读GET原A后解锁，回答原同步题后Q自然唯一消费并回复`FAILURE_CHAIN_EDITED`。独立subagent直接核对完整input、752个有界网络事件、原52轮及完整C不变；send/begin/save/answer各一次，没有重存、重发或引导补过。网页刷新前后实际源码一致；原生起止实际全文一致。结束M54/C8、设置和权限恢复，清稿刷新保持、测试页关闭，119基线准确恢复，epoch为`9120600a-4314-44f4-b73a-0a7c115e1893`，74160/45430停止、旧服务保持。未新增产品逻辑、未操作桌面窗口、未打包或安装；临时目录删除被策略拒绝，260文件保留、未绕过。见`remote-queue-save-receipt-122-20261103.md`；服务端真正长期unknown等分支不继承通过，目标整体仍未完成。

2026-11-03增量：已独立复核文件审批撤回范围，并完成E53重连自动补题修复。真实复现了事件通道恢复时HTTP线路仍在退避、补读提前被拒绝且后续在线轮询不补当前会话的链路；最终网页通过119／106原生夹具及独立服务复测，同一请求11自动显示、单次回答成功、普通稿保持。前端状态回归90/90通过。旧PWA资源误加载的尝试已排除；完整R3-08／R3-15、安装版及真机仍未结项，见 `remote-reconnect-question-20261103.md`。

后续增量：E55补齐指定测试项目的目标创建、准确原生阶段暂缓时刷新后GET同一回执恢复；E56补齐测试会话成功删除丢回包、刷新不复活，并修复已确认删除后仍报迟到网络错误的一行判断，前端状态回归94/94通过。E54保留真实UI操作事实，但旧操作文档的执行源码身份不足，另页新脚本不能回填；折叠跨刷新持久化属于既有能力边界，不作为新增必修功能。121测试夹具已恢复原119，两测试N已删除，P、M44/C8与原草稿保持。目标整体仍未完成。

本轮继续增量：E57在同一已确认64a074…脚本文档中重新执行搜索、主题字号刷新与键盘菜单，补齐E54的证据缺口；E58完成空侧边成功关闭回包丢失、刷新保留unknown及手动GET原A恢复；E59完成准确压缩任务的成功回包丢失与未决跨刷新原生状态收敛，未重压。未新增产品改动。两次独立压缩均真实完成，M原44轮和用户消息保持，新增2条压缩轮为46轮；C8及设置保持。121临时运行已恢复原119，本轮新建快照脚本已清理。原标签1283922800在收尾时已不存在、原因未观测，当前保留1283922838及完整STOP草稿；不虚构旧标签收尾检查通过。

日期：2026-11-02，Asia/Shanghai。初始独立审计截点为内部适配器 **117** 的交付段：Windows包构建于15:56:44，所读进度及性能报告最后更新于16:04:23。主验收随后于18:00补入E27–E30并更新相关行；两份产物和产品源码未变。本文件是跨阶段证据索引和剩余操作表，真实执行细节见各项独立报告。

19:17增量：118及最新前端已完成会话切换、模型／上下文显示、已读目录保持的真实子场景；117补齐文件工具与非Git提示。见E31/E32。桥接223/223、前端42/42通过；本轮未重新打包，原117安装包不含这些最新性能修改。未覆盖的表格分支继续保留，不将历史样本自动升级为118。

20:39增量：E33完成真实命令单次批准及待审期间一图／Skill排队自然续跑；E34补齐Git分区、同长度版本更新、双根正常切换及草稿保护；E35完成119侧栏重复模块解析修复。桥接224/224通过，产品版本未变，未重新打包。测试Git夹具清理被自动审批拒绝，未执行；运行适配器、浏览器缓存开关和调试服务已恢复。

本轮增量（仅源码）：文件审批新增实现和专用测试已撤回，并清理了本表及用例表中要求补做文件审批的旧安排。侧边过期修复保留原生缺失错误并按准确会话身份识别 HTTP 503，主会话暂未加载仍允许 CLI 恢复；新增行为回归覆盖底部过期提示、消息与草稿保留，以及权限、父会话和迟到错误的反例。原生队列删除已确认、随后快照失败时返回成功及 `queueSyncPending`，不能误报删除失败。内部适配器标记升为120，产品版本保持0.6.0；桥接225/225和缺失错误转发3/3通过。这些源码未重新打包或安装，尚无最新服务端与网页联调的真实验收结论，不更新下表真实场景状态。

并行前端修复已合入：普通发送按原授权及操作身份处理迟到结果；问答成功沿现有问题版本阻止旧读取复活题卡；离线时退役旧显示读取，保留已派发写请求和草稿；队列引导／删除复用现有回执保存与查询，未知结果不重复派发、恢复成功不清普通草稿；停止结果的后台终端清理尚未确认时不再显示已清理。前端行为回归60/60通过，覆盖原48条及上述12条时序、回执和提示回归；这些新增测试不含已撤回的文件审批测试。真实断线、跨授权和原生队列竞争仍按下表保留未验收状态。

验收预期修正：`queue.after_stop` 和 `queue.after_failure` 不再要求固定保留一条队列。脚本按原发送回执、client 身份、完整输入和转场前后原生读取区分自动接续与原队列保留；只在后者执行原有显式恢复。未知或证据缺失时停止写入并记录受阻，不能补发同文以通过测试。对应证据门禁102/102、既有回执驱动及环境隔离58/58离线测试通过；这不是两条真实故障场景的验收通过。全部前端修改合入后再次执行桥接回归225/225通过。

真实网页读取检查：在既有 Chrome 测试页 `1432 → 45222` 刷新主测试会话 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`；刷新前输入框为空且无待确认回执，未发送测试消息。实际加载 `app.js` SHA-256 为 `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`，与磁盘一致。session/sidebar/thread/history/relatedThreads 均返回200；项目分组、历史、输入框和模型入口可见，无审批卡片或页面错误文案。session 显示 `readOnly=false`、`nativeConnected=true`、`allowApprovals=true`。旧服务的 channels 和 thumbnail 请求仍有404，原图后续200；不将这些记录称为全部接口通过。本次只证明最新前端能够加载既有服务，未替换服务端或原生适配器，不证明120写操作、过期联调或失败恢复已经真实验收。

2026-11-03 00:10增量：E39补普通停止带队列的真实子场景；E40补最新Rust与网页的过期错误透传及底部提示。E40使用独立 `127.0.0.1:45420`、单独状态目录，正常权限与产品路由保持，只跳过隧道启动；源码仅在构建副本调整两处原生兼容标记以保留正在运行的119/106适配器，未替换现有安装。PID5148已通过本实例停止标记正常退出，45420不再监听；原45222仍由PID13568监听。浏览器拦截及离线设置已清除，两个本轮临时页已关闭，原1432测试页保留。结束时M完整30轮、C持久8轮，均idle、无队列或待答请求；当前产品源码与本轮开始摘要一致。另发现关闭侧边后短暂残留 `ephemeral=true` 元数据被标为main的窗口，保留为下一项最小修复调查，不称永久复活或已解决。

随后121源码修复：在共享 `readThread` 入口仅核对 request-client 的未确认临时会话候选；raw元数据为ephemeral、没有准确原生标签/confirmed关系、真实原生 `getConversation` 为null时返回既有410过期错误，读取和发送均在产生业务副作用前拒绝。持久主会话、已确认侧边及子智能体不新增RPC，不改成全局只读；连接失败与原生对象不匹配仍保留原错误。新增3项行为回归先红后绿，桥接228/228、语法/格式/差异检查通过。native.js SHA-256为`54b79c2095b7486ce21a580fe61a571a90c6f2cf95d88f0de9350ec722e54c73`；产品版本仍0.6.0。上述源码修复完成时尚未注入或打包；后续121真实验证见下段E42/E43，不继承E40为121全部通过。

2026-11-03 01:19增量：121已临时部署并完成E42残留窗口与E43引导丢回包的真实验证。实际原生起止文本SHA-256均为`cb14de43cb993c5690e74e8a9cd855bd84719060bbca3bb45773b8f13a15a181`，业务版本121/兼容标记106；实际网页SHA-256仍为`53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`。Q在原第31轮内唯一接受，旧30轮和C的8轮保持，刷新后的新文档GET准确原回执后解锁，未发送草稿与旧客户端草稿保持；旧GET `62868.383` 的网络终态未归档，保留该清退证据缺口，不扩大为所有unknown分支通过。临时服务74324已正常停止，45420不监听，原45222仍为13568；原生准确恢复119脚本，fixture仅runtimeEpoch更新为实际新值，owned名单恢复空。E41另外补齐不同origin偏好隔离和同origin双页传播/刷新；均已恢复原视图。本轮没有新增产品代码或新安装包，也未提交、安装或发布。

2026-11-03 02:30增量：E44补两组异步问答的离线禁用、切换保持、准确回答与另一实体跳过；第一段HTTP淘汰缺口保留。E45补目标标签与Skill、双图共存及重复选择、只删标签不丢草稿。E46补121真实创建HTTP完整往返32.913秒（含受控延迟），返回后切到C，原响应到达不抢页面且两会话草稿保持，回M可见正确侧边。创建与关闭各一次、无send，M34/C8完整历史、设置和权限保持；空测试侧边已关闭，原生准确恢复119，独立45430服务正常退出。未点击返回而由通知直接切换的分支仍需独立验证。子代理另与HEAD对比复核，新增文件审批UI、辅助函数、样式和专用测试均已撤回，原有文件修改记录与兼容分发保留。本增量没有新增产品代码、构建、安装、提交或发布。

E46临时目录 `temp/remote-side-late-native-121-20261103/` 在完成归档摘要比对及原生恢复后尝试清理，被自动审批以 `blocked by policy` 拒绝。目录原样保留，未换命令重试；这一清理未完成不影响已确认的服务停止和原生恢复，不得记录为临时文件已全部删除。

2026-11-03后续增量：E47实际复现通知导航与侧边创建竞争。调用当前网页的原 `openNotificationThread(C)` 时，旧创建面板仍使C不可交互；迟到成功回包借该面板执行历史后退，页面回到M。已在 `selectThread` 离开创建父会话时关闭面板、禁用该记录自动打开，并清除当前记录所属的历史创建标记；正常 `preparedView` 创建过渡保留。新增5项回归先红后绿，前端65/65通过。真实网页同场景复测确认回包未到时C已可输入、迟到成功不回退或覆盖新稿；普通创建仍自动进入侧边、返回保留父稿。实际app摘要由`53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`变为`641b08ac6f4571ba59dd71e8c94df279c0ae5ece85a22f4885e83ea34c00881b`；原生全程119，未更换。三个空测试侧边已关闭，M34/C8历史保持，临时1434/45430服务已停。该验证直接调用网页原通知函数，不包含Android事件派发或真机点击通知；首次刷新命中旧SW后，对本测试页绕过缓存核对新源码，最后已恢复缓存设置，亦不算PWA更新流程通过。尚未重新打包。

同轮验收逻辑修正：`queueRace`不再强制原生出现未承诺的`starting/consuming`字段，改为原Q可见、原POST在Request阶段暂缓、准确原client自然唯一接受、放行同一POST后明确过期拒绝的证据顺序。门禁核对完整输入、原回执、原生历史和队列；unknown及成功清理已接受残留不能算阻止出队。主代理另以E47真实CDP事件发现，同一请求在Network/Fetch中的普通请求头及可选字段不同，原整对象深比较会误判；已改为准确比较URL、POST正文、唯一幂等键及Network/Fetch ID。差异原件为`notification-side-create/cdp-request-shape-observation.json`，它来自Response暂停，只支持协议形态结论，不是R3-04的Request暂停通过。证据门禁108/108通过；缺少真实暂缓/放行驱动时runner在提交测试数据前明确阻止执行。R3-04的真实竞争仍未验，R3-06也仍缺独立上游故障条件。

2026-11-03继续验收增量：上述R3-04未验状态已有E50补充。期间实际发现并修复两处交互缺陷：E48问答卡遮挡队列按钮，E49未编辑的旧页面在卸载时覆盖共享草稿。两处均沿用现有定位／草稿比较逻辑，没有新增轮询、跨页锁或存储协议；最终前端74/74通过，app实际摘要为`a4d9c02b2b222608cd73df59ccea6d99b6e2c99d7130fb035df9f074f3a37c55`。E50的删除及编辑第二轮均取得真实Request暂停、自然消费、原A放行及409拒绝；首轮编辑因操作间隔超过91秒被浏览器取消，保留为未通过，不改写成服务端拒绝。事实门禁随后按真实server队列＋native组合投影修正，不能虚构独立物理local队列；两个真实原件经透明适配均通过事实函数，完整runner报告仍未执行。E51补当前版本既有会话目标创建并完成，只有一次setGoal，两个原生目标工具调用归属同G。E52另记录重连后已有问题未自动显示的待查缺口，原因未确认，不能算重连全通过。

本轮结束时，M持久41轮（loaded42含既有虚拟目标记录）、C持久8轮；均无待答请求或队列，M原default配置已恢复。实际121运行脚本起止摘要均为`5b97e84abf7933129193a3bb0230bc9fc3feb39fd0705fef65ff1c7af930cf78`，随后准确恢复进入本轮前的119文本，仅runtimeEpoch改变。独立1435开发进程和45430测试服务已停止，原1432／45222保持；两张测试页空稿、缓存覆盖及拦截已恢复并关闭，原1432草稿仍为`STOP_LIVE_2_UNSENT_DRAFT_保留`。没有重新打包、安装、提交或发布。

**整体尚未完成。** 旧覆盖表的 `not_run` 是其冻结截点，不能忽略随后已经取得的子场景证据；另一方面，105—116 的局部通过、222 项桥接回归、39 项前端回归或某份报告中的 `passed`，也不能证明 117 已完成全部 73 条及三轮要求。本表不将任何整条基线标为 `passed`。

初始独立审计只读取下列五份报告及其直接指向的必要原件，只新建本文件，没有执行业务动作。E27–E30为主验收后续真实操作及另一subagent独立复核所得；下表“下一项操作”仍是待执行步骤。

## 1. 版本和判定口径

| 对象 | 截点可证事实 | 使用边界 |
| --- | --- | --- |
| 117 真实运行 | E01 的起止引擎脚本逐字相同，SHA-256 为 `bef7338262d3c79a6aa403092923812746cff6f7ceff0fe908dea0a86529f0cc`；普通文本只有一次 HTTP 请求和一次原生接受，完整历史 12→13、旧前缀不变 | 是受控 117 业务源码/106 兼容标记的一个正常子场景；不代表安装包已安装，不覆盖同步问答、复杂输入、故障或并发 |
| Windows 最终产物 | `CodexElves-0.6.0-windows-x64-setup.exe`，15:56:44，46,557,532 字节，SHA-256 `cc7960eaf09e3702913a72497bb3aaac8f286cc00505c970d7fb6d904aec6f24`；E25 登记含 native 117、renderer 13 | 构建和内嵌源码校验属于交付证据；截至所读记录，尚未安装、公开发布。本次未重新计算二进制摘要 |
| Android 最终产物 | `CodexElves-0.1.0-android.apk`，versionCode 24，14:16:41，2,632,045 字节，SHA-256 `bb28be126007074da697f3a44f4915af4cbc7277d9bb56b23ab6640198269d7d` | 与 11:16 包不同；116→117 没有改变 APK 输入，沿用 14:16 正式包。资源/签名/lint 记录不是手机实际激活版本和生命周期验收 |
| 运行收尾 | 性能报告记录临时适配器已恢复安装版 106，调试服务停止；新测试标签已关闭，旧断连标签未确认关闭 | 这是归档收尾状态，本次未查当前进程、Cookie 或浏览器，不把“交付 117”写成“现场已持续运行 117” |
| 历史证据 | 按每份原件的源码、兼容标记、服务和对象保留 | 同一版本数字不足以证明代码身份；旧原件不会因新构建自动变成 117 复测 |

状态含义：

- `partial`：已有可归属的真实子场景或真实缺陷及后续修复证据，但整条要求仍缺必要分支。表中会标明版本和本次复核深度。
- `not_run`：该条核心操作序列尚无可确认执行证据；相关按钮、普通成功流程或源码测试不能代替。
- `failed`：准确场景出现可复核不符，且没有同一缺陷的后续修复/复测证据将其替代。历史失败单列保存，不冒充当前 117 仍失败。
- `待用户验收`：仅指用户已经接管的 Android 真机及 Windows 闪窗视觉项目；不转成通过，不阻塞本方其余可执行工作。

“原件已复核”只指本次对归档文件进行读取、身份/状态或字段比较，不代表再次执行原操作。“仅报告定位，原件未复核”会明确标出，不能作为本次新增通过结论。表中 `E` 编号展开后均有具体目录、文件和核对对象。

## 2. 原始证据索引与本次复核深度

路径缩写：`A = dist/validation/remote-acceptance-20261101/`；`B = dist/validation/remote-acceptance-20261102/`；`P = dist/validation/remote-performance-20261102/`。这些都是本仓库内路径，不是三个可互换的运行版本。

| 证据 | 原件位置及对象 | 本次可独立复核的内容／限制 |
| --- | --- | --- |
| E01：117 普通发送 | `P/final117-before.json`、`final117-after.json` 的 `/state/turns`；`final117-http.json /events/30`、`/responseBody/body`；`native-final117-before-runtime.js`、`native-final117-after-runtime.js` 及两份 identity JSON | 已重算起止脚本摘要；只有一次 `/api/action/send` 请求，回执 `a05348ad-453c-4fb4-aed5-68397fcc2148` completed；client `c33e574d-07c2-4e24-b3f3-d68cc85b399b` 在原生 `/state/turns/12` 唯一接受，turnId `01a19b3e-8b65-77b1-8026-7839ea120da9` completed、回复 `PERF_117_OK`。旧 12 轮逐字段相同，队列 0。单样本 HTTP 3759.611ms 不是稳定性能结论 |
| E02：双图/Skill 的已发编辑 | `B/input-integrity/before-edit-server-queue.json`、`after-edit-server-queue.json` 的 `/history/thread/turns`；`cancel-editor.json`、`image-edited-http.json`、`sent-edit-final-server-queue.json` | 已比较第二轮正式用户输入的非 text 项逐字段相同，顺序均为 text/image/image/skill；第一轮完全不变。已读取消前 actual original/edited 两段不同正文。单次提交、刷新及后续发送完整链另见该目录原件及 `sent-edit-verification.json`，本次未重算全部 HTTP 计数 |
| E03：双图/Skill 队列正文及增删图 | `B/input-integrity/queue-original-server-queue.json`、`queue-text-saved-server-queue.json`、`queue-images-saved-server-queue.json` 的 `/raw/data`；`final-server-queue.json` | 已比较仅改正文时非 text 输入不变、后继 C 对象不变，增删图后 Skill 名称/路径保持。B 的三代物理 ID 为 `01a19a0d…`→`01a19a0e…`→`01a19a0f…`，同 client `41188cf4-bdac-4dad-8168-c478b8d4e7ff`；终态历史 7 轮、队列空。红/蓝→蓝/绿的原文件字节复算见历史独立核验，本次未再次解码图片 |
| E04：107 活动异步问答 | `B/installed-mainflow/native-during-active-question.json`、`session-during-active-question.json`；`active-question-fixed-proof.json`；`active-answer-http.json`、`question-fixed-final-history.json` | 已读修复后准确会话仍可写、`allowApprovals/nativeWriteAvailable/nativeConnected=true`，原生 requests 非空；原答复回执 completed，绑定活动轮 `01a1982c-0363-7370-8bbf-4b14bcea3945`。原异常及最终完整历史的逐项核验见进度和 `question-verification.json`；不覆盖 117 同步问答 |
| E05：completed 问答与跳过 | `A/R1/question-pending-native.json`、`question-after-fix-native.json`、`question-answer-http.json`、`question-desktop-sync-native.json`；`question-skip-http.json`、`question-skip-complete-native.json` | 进度明确关联原失败题、准确新回答轮及两题 skipped/lastSubmission 状态。本次仅报告定位，未重新逐题读取这些原件；不新增整条通过结论 |
| E06：异步回答丢回包及新文档恢复 | `A/R3-original-question-recovery/client-unknown.json`、`recovered-original-receipt.json`、`client-recovered.json`；`reload-boundary.json`、`final-main-history.json` | 已读同回执 `7baadac9-8d62-48ae-94a1-fb087f27880f` 的客户端 unknown→原回执 completed→客户端记录空；设备身份相同。原题、GET 所属新 Document、一次回答及完整历史的全链审核见同目录 `independent-audit.json`；本次未重新逐条审核 702 条事件 |
| E07：正式保存后的空闲/忙时联动 | `A/R3-native-save-flow/save-A-http.json`、`next-active-http.json`、`save-B-http.json`、`busy-after-save-b-writer-detail.json`、`final-main-history.json`、`final-writer-writer-detail.json` | 已读原保存回执及 15 轮完整历史；进度逐项关联空闲保存、无人工清理续发、B 在 C 前和自然 B→C。其他文件本次仅定位；不把这些正常结果当成真实丢回包通过 |
| E08：早期纯文本取消联动 | `A/R3-native-cancel-flow/cancel-A-http.json`、`cancel-B-http.json`、`final-main-history.json`、`final-independent-audit.json`、`independent-review.md` | 本次仅报告定位，原件未重读。只保留空闲取消、续发、忙时保位和 FIFO 四段证据；早期归档没有取消前实际编辑器值、控制按钮变化缺解释，不能借审核数量补齐 |
| E09：108 纯图补文字修复 | `B/media-mutations/pure-http.json`；`B/media-mutations-fixed/busy-http.json`、`idle-save-http.json`、`final-main-history.json`、`verification.json` | 已读修复后忙/闲两组原回执，保存均有 completed/ok。原失败的输入构造及两张图字节、完整 33→37 轮比较以历史原件核验为依据，本次未全部重算；没有把手工结束旧 unknown 计作修复通过 |
| E10：109 编辑终态广播 | `B/queue-status-before/red-test.json`；`B/queue-status-fixed/after-save-events.json /result/events`、`a-save-completed-network.json`、`after-cancel-events.json`、`b-after-save-ui.json`、`b-after-cancel-ui.json` | 已读保存原回执和 `committing:33/34 → queuedEdits=[]:35` 事件；UI 自动收敛、取消 revision49、无刷新联动见相应原件。本次未重做全部 UI 对比；它不自动解释此前每次暂态提示 |
| E11：108 双授权旧编辑请求 | `B/queue-race/b-paused-request.json`、`b-released-network.json`、`saved-before-b-release-server-queue.json`、`after-b-release-server-queue.json`、`actions.ndjson` | 已读原请求明确 failed/409；放行前后 `/raw/data` 完全相同，各一项。没有静默覆盖甲端。暂态“待确认”所引用 ED 的瞬间证据仍缺，尾段浏览器网络截断也保留 |
| E12：110 排序/删除及正式引导 | `B/queue-order-steer/steer-plan.json`、`b-released-reorder.json`、`before-stale-release-server-queue.json`、`after-stale-release-server-queue.json`；`B/steer-formal-r2/3-queue-steer-before.observation.json`、`handler-steer-http.json`、`handler-accepted-server-queue.json` | 已读正式处理器点击前唯一 processing 轮 `01a199ee-efba-7f71-a364-5b484070d911`、准确可引导行；回执 turnId 与 `/nativeConversation/turns/0/items/4` 的 accepted targetTurnId 一致。turnHistory 是同一项的投影，不能再算一次接受。排序竞争原件本次仅报告定位 |
| E13：109 目标生命周期 | `B/goal-lifecycle/paused-server-queue.json`、`after-update-server-queue.json`、`completed-server-queue.json`、`goal-tool-records.json`、`stale-conflict-state.json`、编辑器原件 | 已比较保存前后 createdAt、paused、null 预算、tokensUsed1783、timeUsedSeconds9 不变，正文变化；最终当前目标 null。工具 complete 回执及 UI 保护链有具体原件；本次未重审每项网络/编辑器动作 |
| E14：106 两轮主会话已读 | `B/main-read/cycle1-unread-snapshot.json`、`cycle2-unread-snapshot.json`、`cycle1-mark-read-http.json`、`cycle2-mark-read-http.json`、`cycle1-reloaded-snapshot.json`、`cycle2-reloaded-snapshot.json` | 已读两轮远程 hasUnreadTurn 均 true→刷新后 false，两份 markRead 原回执；快照的历史不完整，不能拿它们证明完整旧历史保持，完整历史另见 `final-main-history.json` |
| E15：106 双侧边聚合 | `B/side-read/before-reading-a-snapshot.json`、`after-reading-a-snapshot.json`、`after-reading-b-snapshot.json`、`read-a-http.json`、`read-b-http.json`、`after-close-a-native-tab.json`、`after-close-b-native-tab.json` | 已定位并局部读取材料结构；逐个清除、聚合、关闭和完整历史比较本次仅依据进度定位，未重审全部原件。没有桌面 UI 读 S2 的通过证据 |
| E16：107 侧边创建/模型/命名 | `B/installed-mainflow/side-create-http.json`、`side-created-native-tab.json`、`side-native-rename-first-native-tab.json`、`side-native-rename-second-native-tab.json`、`side-close-http.json`、`side-model-before-first-native-tab.json` | 已读同一侧边创建和关闭的 completed/ok 原回执；准确父关系、两次命名、自定义保护及空白模型的原生/UI 对比见所列原件，本次未逐项重做。原生 API 改名不等于原生桌面菜单验收 |
| E17：子智能体集合/只读 | `B/subagents-current/comparison-stable.json`；`A/R2-lifecycle/native-subagents-during-acceptance-review.json`、`native-subagents-after-acceptance-review.json`、`subagent-active-menu.json`、`subagent-completed-menu.json` | 已比较稳定截面 beforeRows=afterRows，原生 discovery22、远程完成22；准确详情 viewKind=subagent/readOnly=true/parent 正确。active→done 为较早同 ID 原件，本次仅定位；无孙级样本，数量不成为固定产品预期 |
| E18：108 native→service→native | `B/transport-takeover-fixed/native-http.json`、`fallback-http.json`、`restored-http.json`、三段 history、`send-trace.ndjson` | 已读三组原回执；兜底 send completed，但同组 markRead 明确 failed/503、未派发。完整 39→42 历史及准确传输次数见对应原件/核验，本次未重算。普通发送恢复与兜底已读能力必须分开 |
| E19：慢读/草稿/独立视图 | `A/R2-lifecycle/slow-read-network.json`、`slow-read-after-release.json`、`draft-reloaded.json`、`preferences-first-refresh.json`、`preferences-second-refresh.json`、`preferences-client-identities.json` | 已读两轮偏好归档的不同 localhost/127.0.0.1 客户端及 project/recent 对调结构；未逐条重审 DOM 与 ERR_ABORTED 事件。慢读被取消不能代表迟到成功响应已送达但被忽略 |
| E20：独立优化 | `A/R3-optimizer/optimize-1-http.json`、`optimize-1-result.json`、`optimize-1-restored.json`、`late-3-held-completed.json`、`late-3-after-release.json`、`cancel-4-before-release.json`、`cancel-4-final.json`、`provider-evidence.json` | 本次仅报告定位，原件未复核。历史证据区分真实供应商完成、恢复原文、完成结果迟到保新稿、取消先于 start；运行中上游取消曾错失窗口，仍未执行 |
| E21：只读欢迎页草稿修复 | `A/R3-draft-recovery/draft-before-readonly.json`、`readonly-first.json`、`writable-recovered.json`、`permission-identity-comparison.json`、`explicit-clear-after-reload.json` | 已读比较记录的同 device/createdAt 及旧空稿、新两行稿；原始 UI 文件只定位、未逐一复核。不能据此推导坏缓存、真实存储失败或撤权已验 |
| E22：停止/上下文/置顶 | `A/R1-followup/stop-active-before-native.json`、`stop-active-after-native.json`、`stop-control-before-native.json`、`stop-control-after-native.json`、`context-current.json`、`pin-before-native.json`、`pin-restored-native.json`、`pin-thread-http.json`、`unpin-thread-http.json` | 已比较停止 inProgress→interrupted、控制历史不变；last35499/window353400 与 UI10%对应，累计499207不是分子；已比较取消置顶后的完整 nativePins 与原值相同。项目置顶、终端停止、零/缺失值未因此通过 |
| E23：工具与正常压缩 | `A/R1-followup/service-r3-active-native.json`、`service-r3-final-native.json`、`service-r3-unknown-tool-ui.json`；`B/compaction/verification.json`、`B/side-read/compact-started-snapshot.json`、`compact-completed-snapshot.json` | 本次仅报告定位，原件未复核。进度绑定同一 sleep 的开始/完成和 unknown；正常压缩同一 turn/item 开始→完成；媒体阅读、压缩未知和压缩中入队不继承 |
| E24：初始新建/目标标签 | `A/R1/create-main-http.json`、`create-main-native.json`、`goal-draft-local.json`、`goal-draft-typed.json`、`goal-draft-removed.json`、`goal-create-http.json`、`goal-complete-native.json` | 本次仅报告定位，原件未复核。无项目新建、目标两入口本地编辑及新建目标完成有准确原件；指定项目、复合标签和部分成功仍缺 |
| E25：产物/性能附项 | `P/artifacts-117.json`；`P/after-http.json`、`after-result.json`；`dist/validation/desktop-compat-20261102/final-functional-verification.json`；`dist/validation/openin-fullview-20261102/dom-full-rendered.json` | 已读产物记录结构和 114 原 HTTP/状态材料；112/114 引导分别只有一条样本，不能算稳定改善。桌面侧边圆环、Open In 的 DOM/原生对照本次仅报告定位，不升级最终安装包按钮的端到端验收 |
| E26：授权隔离的真实环境失败 | `B/queue-race/local-access-before-recovery.json`、`local-access-recovered.json`、`preflight-user-service-identities.json` | 已读401→200，但 `originalCookieValueRestored=false`；原本机 ID 与复用远程 ID 不同。后续等价访问不能覆盖原 Cookie 身份未恢复的事实；本次未查询当前 Cookie |
| E27：117真实同步问答及回包丢失 | `B/sync-question-final/`；`docs/validation/remote-sync-question-20261102.md` | 原生Plan轮产生真实请求0/1，网页准确答青色/橙色；后者真实成功响应被中止。原13/14轮保持、普通草稿刷新保持，捕获窗口各answer一次。事件流仍可用，刷新后完整网络窗口缺失，不覆盖所有unknown |
| E28：117普通发送丢回包 | `B/send-recovery-final/`；`docs/validation/remote-send-recovery-20261102.md` | 真实sent回包中止后GET同一原receipt恢复，原15轮保持且仅新增一轮；刷新追加send为0，新草稿保留。未覆盖完全离线、复杂输入或长期未知 |
| E29：117队列保存/取消丢回包及自然FIFO | `B/queue-edit-recovery-final/`；`docs/validation/remote-queue-recovery-20261102.md` | 保存B与取消C各中止成功回包，各GET原receipt恢复；B图片/Skill及C原文保持，gate后B→C各一次，原16轮保持，最终19轮/idle/queue0。最早入队HTTP账本不完整，不补成全程请求计数或出队竞争 |
| E30：第二客户端授权条件 | `B/client-preferences-final/second-client-blocked.txt`、`second-client-blocked.png` | localhost客户端登录被设备数量上限拒绝，输入已清空、测试页关闭；没有撤销已有设备。跨客户端偏好仍受阻，不能以单客户端已登录代替 |
| E31：117文件工具页 | `B/workspace-tools-final/workspace-verification.json`、各文件UI记录、起止native identity | Markdown、图片、源码、超限及二进制提示正常；非Git变更返回422并可返回会话，5个测试文件摘要不变。未覆盖Git差异、跨根缓存、非空草稿和Android浏览器 |
| E32：118交互性能增量 | `P/followup-118/final/verification.json`、模型菜单UI、起止native identity；`browser-app-fresh-identity.json` | 最新native和app实际运行文本已核对，两次切换详情138.801/80.495ms，模型／用量与各自原生快照一致；已读保留目录，普通读取去重与fresh更新由源码回归覆盖。仍非稳定性能分布或完整三轮验收 |
| E33：真实命令批准与审批中排队 | `B/approval-final/`；独立报告 `remote-approval-20261102.md` | 118兼容夹具中request3单次accept，准确命令item输出标记/exit0；待审一图加Skill未抢跑，之后唯一消费。旧19轮逐字段保持，最终21轮；完整设置及权限恢复，最终截图/AX一致。拒绝和文件审批未测 |
| E34：Git与双根缓存 | `B/workspace-git-final/`；独立报告 `remote-workspace-tools-20261102.md` | Git独立diff与接口/UI一致；详情notModified、同长度更新、A/B/A同名内容/筛选隔离、非空草稿保持。阶段106/118/119分别记录，不合并为同版全回归；20:38结束摘要已补齐，夹具清理仍blocked |
| E35：119侧栏分段优化 | `P/sidebar-breakdown/samples.json`、`samples-119.json`、`direct-global-reads.json`、浏览器HTTP及两阶段起止引擎文本 | 重复解析前六键组1.7–2.95s，真实原生读本身20ms；复用解析后组18–39ms、侧栏85.9–111.7ms，完整网页HTTP另录。首次侧边发现仍约3.73s，不作稳定时延承诺 |
| E36：119双会话三轮模型实发 | `B/models-final/`；独立报告 `remote-model-settings-20261102.md` | M1=terra/high、C2=sol/medium、M3回M=terra/high；HTTP/原生轮次/实际代理Responses WS一致，各client唯一。未发送选择不改变历史，旧M21/C7保持，最终M23/C8。C从notLoaded恢复，权限路径集合等价但原始拼写有重复，未掩盖 |
| E37：119命令取消与原队列恢复 | `B/approval-cases-119/command-cancel/`；独立报告 `remote-cancel-queue-20261102.md` | request4单次cancel，准确命令item declined且无输出，turn interrupted；Q原位保留，单次steer原消息后唯一接受，23→25轮旧前缀保持、设置恢复。不是停止按钮或所有停止策略通过 |
| E38：删除预览取消 | `B/delete-preview-final/`、`B/models-final/delete-preview-before.json`及after | 确认框准确展示C及永久删除说明，取消窗口零delete请求；完整persisted8轮不变，loaded6轮另列。初始草稿为空，不冒称非空草稿保护 |
| E39：普通停止与原队列恢复 | `B/stop-queue-live/`；独立报告 `remote-stop-queue-20261102.md` | 第二次测试窗口内准确T正在执行时单次stop，T interrupted，原Q/client/input保持；单次引导后唯一接受并完成。原28轮及C持久8轮、设置权限不变，草稿保留。第一次自然结束未命中停止窗口。运行原生仍119/106、网页为120阶段最新文本，不包含120原生删除修复验收 |
| E40：侧边过期真实读取 | `B/side-expiry-live/`；独立报告 `remote-side-expiry-20261103.md` | 120阶段服务/网页对同一真实过期S保留准确错误，旧服务返回通用连接指引。S2经真实UI创建、发送、关闭后，独立确认native缺失；在隔离关闭广播/关系刷新竞争后，真实thread/history HTTP503触发底部过期态，正文保持、输入框隐藏；解除限制后仍保持。草稿终态非空有证，归档非空前基线不足，不扩大为全过程或跨刷新保持。原生119/106；S1只覆盖正常返回父会话，不计过期提示通过 |
| E41：客户端偏好隔离及同客户端双页 | `dist/validation/remote-acceptance-20261103/client-preferences/` | 不同origin：45420切到最近并刷新后保持，1432仍为项目且原草稿不变。同origin：1432第二页切换后原页同步为最近，刷新后选择与草稿保持。两窗口均零 `/api/action/`，已通过UI恢复原项目视图。原生为119/106；不是两台设备或换授权/撤权验收 |
| E42：121关闭残留窗口 | `dist/validation/remote-acceptance-20261103/side-lifecycle-121/`及`runtime-121/`；独立报告 `remote-side-lifecycle-121-20261103.md` | 真实121/106下新侧边正常创建、发送、单次关闭；原生标签/metadata已空而raw仍成功返回ephemeral时，产品thread/history均410。M30/C8及设置权限未变；实际脚本起止匹配，之后准确恢复119。仅实际验证读取拒绝，未负面发送或重跑121底部过期UI |
| E43：121引导成功回包丢失与刷新恢复 | `dist/validation/remote-acceptance-20261103/steer-receipt-121/`；独立报告 `remote-steer-receipt-20261103.md` | 原成功steer响应被中止，原A及草稿保留；刷新后新loader的GET原A返回同一成功回执并解锁。原client/input仅在原第31轮接受一次；准备send2，随后steer1、无重发，旧M30/C8保持。旧GET383终态证据缺失，不能宣称全部旧读清退或原生长期unknown已验 |
| E44：异步问答离线与第二实体跳过 | `dist/validation/remote-acceptance-20261103/question-offline/`及`question-runtime/`；独立报告 `remote-question-offline-20261103.md` | 离线控件禁用、页面内选择和普通草稿保持；恢复后准确回答原两题，第二组同名不同实体不继承答案，单次skip不带答案且不新增回答轮。M31→34、C持久8轮保持；硬刷新重置未提交选项符合原范围。第一段网络账本有淘汰，不能称全程HTTP唯一或迟到旧事件已验 |
| E45：目标标签复合草稿与重复选择 | `dist/validation/remote-acceptance-20261103/goal-composite/`及`two-images/`；独立报告 `remote-goal-composite-20261103.md` | Skill与两图共存时连续选目标仍为单标签；标签后输入更新Skill偏移，只删标签保持正文、Skill身份和两图名称、顺序、字节。动作窗口零action，恢复刷新各有自动markRead，未创建服务器目标；能力不足和跨端复合草稿恢复未验 |
| E46：121侧边创建回包迟到 | `dist/validation/remote-acceptance-20261103/side-create-late-121/`；独立报告 `remote-side-create-late-20261103.md` | 原创建HTTP完整往返32.913秒，成功响应暂停期间点击返回并切C，放行后仍在C且草稿不变；回M见准确S且M草稿保持。创建/关闭各1、send0，原生父子关系与关闭一致，M34/C8完整历史及设置权限保持；清理刷新另有markRead。未覆盖不经返回的通知切换、unknown或运行中关闭 |
| E47：通知导航与创建面板竞争修复 | `dist/validation/remote-acceptance-20261103/notification-side-create/`；独立报告 `remote-notification-side-create-20261103.md` | 同一119原生与真实产品HTTP下，旧网页通过原通知函数切C仍被创建面板遮挡、回包后退M；修复网页立即恢复C交互，回包前继续输入且回包后保稿、不回退，M关系正确。另验普通创建自动进入S和返回保M稿；三S均关闭、M34/C8不变。网页边界集成通过，不计Android真机或PWA更新通过 |
| E48：问答卡遮挡队列操作修复 | `dist/validation/remote-acceptance-20261103/queue-dequeue-race/question-queue-overlap-*`；独立报告 `remote-queue-question-draft-20261103.md` | 原Q仍待执行时，按钮中心实际命中question-card；定位逻辑避让前置内容后，卡片上移34px、中心命中原队列按钮，真实菜单可打开。原Q未重发；未扩大为所有窗口尺寸或Android键盘验收 |
| E49：同客户端旧页面覆盖新草稿修复 | 同目录`draft-*`及`stale-blank-reload-reproduction.json`；独立报告 `remote-queue-question-draft-20261103.md` | 同owner/generation、无在途写时，仅刷新旧空B页就删除A持久记录，A内存稿仍在。未变化草稿不再写入后，旧空／旧非空刷新均保留A新稿，主动清空仍删除且不被旧页复活。真实范围是普通文本；B网络窗口截断，不宣称双页全程零HTTP或所有旧丢稿同因 |
| E50：自然出队后的迟到编辑／删除 | 同目录`remove-*`、`edit-r2-*`及`edit-r2/`；独立报告 `remote-queue-dequeue-race-20261103.md` | 两个独立Q各自然唯一完成，再放行其原POST，原A均failed/409；完整输入、历史及普通草稿保持，未重复派发。两份`*-real-verification.json`列明实际字段来源并通过事实函数；nativeQueue是组合投影。首轮edit超时取消且显式结束编辑标记，仅作独立恢复记录，不算迟到拒绝通过 |
| E51：既有会话目标创建并完成 | `dist/validation/remote-acceptance-20261103/goal-create-existing/`；独立报告 `remote-goal-create-existing-20261103.md` | M原无目标；单次setGoal的原A返回active G（createdAt=1793653033），原生get_goal及update_goal complete的同call_id结果确认同G完成。原38个持久turn ID顺序保持，只有一个新增执行轮；loaded多一条turnId=null/items=[]的目标记录，不计第二轮执行。无普通send/create/排队，不覆盖未知或新建目标部分成功 |
| E52：服务空档中的问题未自动补齐（历史缺口，后续见E53） | 同目录`edit-r2-native-waiting.json`、`edit-r2-before-refresh-ui.json/jpg`、`edit-r2-refresh-thread-responses.json`及服务起止记录 | 第二服务05:14退出、gate05:25创建、第三服务05:26启动；05:29原生有请求8，05:30页面online却无题卡。手动刷新获得准确200/原题后显示。当时完整自动读取链缺失；E53另以新问题连续复现、定位并修复同类缺口，不补造这次历史故障的瞬间数据 |
| E53：重连自动补题修复 | `dist/validation/remote-acceptance-20261103/reconnect-question/`；独立报告 `remote-reconnect-question-20261103.md` | 真实复现HTTP线路仍在退避导致自动读取在发出前失败；顺序恢复及既有轮询离线转在线补读后，同一请求11自动显示，一次回答completed，普通稿保持。后续5次在线轮询无全文重读，原41轮历史及设置保持。运行原生119／106，最终网页哈希c8bcf0…；旧PWA缓存尝试不计修复通过 |
| E54：搜索／外观／菜单与验收口径 | `dist/validation/remote-acceptance-20261103/existing-ui-controls/`；独立报告 `remote-existing-ui-controls-20261103.md` | 标题/路径搜索、主题字号刷新与键盘菜单有真实操作记录；执行脚本身份只在后补新文档确认64a074…，不能回填旧操作，因此当前版本整项仍partial。折叠刷新展开在HEAD已存在，不新增持久化。1437启动被策略拒绝、localhost授权达上限，改复用既有授权，未撤销设备 |
| E55：指定项目目标创建／阶段恢复 | `dist/validation/remote-acceptance-20261103/goal-partial/`；独立报告 `remote-goal-project-recovery-20261103.md` | 两事务各单次create及同目标完成；第二事务在首条accepted而goal尚未委托时刷新，7次GET同A pending后completed，0重建/重发。原生121／106及实际网页c8bcf0…；第一事务错失刷新窗口不算恢复。无pending窗口新稿、真实原生拒绝或派发后unknown证据 |
| E56：删除丢回包／迟到错误提示 | `dist/validation/remote-acceptance-20261103/delete-receipt/`；独立报告 `remote-delete-receipt-20261103.md` | 两个准确N各一次删除完成回包被丢弃，原文件缺失、刷新不复活。第二事务在丢包前已由另一来源持久确认deleted，最终64a074…网页不再弹迟到错误；M原草稿、M44/C8历史/设置/队列保持。未确认时保留unknown和错误提示的行为回归保留；长期全部来源unknown仍未验 |
| E57：同一执行源码下UI复测 | `dist/validation/remote-acceptance-20261103/existing-ui-controls-r2/`；独立报告 `remote-existing-ui-controls-20261103.md`末章 | 新标签1283922837实际函数所属脚本前后64a074…一致；在该页重做准确标题/路径搜索、清空、主题字号刷新保持、键盘菜单，截图与时间线留档，设置恢复。C空稿不能代表非空稿保护；独立origin受授权设备上限限制未验 |
| E58：侧边关闭丢回包 | `dist/validation/remote-acceptance-20261103/side-close-compaction/`中side/close前缀；独立报告 `remote-side-close-receipt-20261103.md` | 空S准确父M，create1/close1/send0；成功关闭回包Aborted后unknown原A跨刷新保持，手动GET同A completed后清理待确认，原生tab/registry消失、菜单不复活、原STOP稿及M44/C8保持。不宣称自动查询或运行中关闭通过 |
| E59：压缩丢回包与刷新恢复 | 同目录compaction前缀及compaction-r2/；独立报告 `remote-compaction-receipt-20261103.md` | 两原A各compact1；第二次原成功running回包Aborted后unknown跨新文档，同T/I原生状态收敛完成，0重压、原稿/用户消息保持。恢复时GET原A为0，末尾GET仅诊断；第一次未命中未决刷新，真实失败及非空队列未验 |
| E60：122同版正常整链 | `dist/validation/remote-acceptance-20261103/crossflow-122/`；独立报告 `remote-crossflow-122-20261103.md` | 原题12单次回答、目标标签只改本地稿；Q1保存后自然消费，Q2保存/刷新后单次引导入原T。M46→50/C8，完整旧历史和普通稿保持。第二保存原响应正文未取得，后续诊断GET同A及原生结果补证；不计整链unknown、空闲sent保存或Android通过 |
| E61：项目及会话置顶跨端 | `dist/validation/remote-acceptance-20261103/project-pin/`；独立报告 `remote-project-pin-20261103.md` | 准确P/C经菜单置顶、刷新、另一端显示、取消和原状态恢复；项目完整顺序及C邻项保持，零发送。操作端1432当前脚本、观察端45222旧脚本；不计桌面UI、故障/unknown或45430独立服务验收 |
| E62：独立上游失败环境启动受阻 | `dist/validation/remote-acceptance-20261103/upstream-appserver/`；`remote-upstream-isolation-20261103.md`末章 | 独立home的真实CLI initialize成功；监视器发现子进程连接系统代理7897后终止。thread/start未发送、provider请求0，无turn/队列结果，不能判产品失败或上游失败已验。进程与监听已关；两个新临时目录清理被策略拒绝、未绕过 |
| E63：122空闲队列保存正式接受 | `dist/validation/remote-acceptance-20261103/queue-sent-live-122/`；独立报告 `remote-queue-sent-live-122-20261103.md` | Q在原问答等待时进入编辑，T51完成后保存，原client及完整修改输入唯一接受于T52并完成；M50→52/C8、原稿恢复/设置保持。只验证普通文本；网络摘要无完整时长，前端执行身份未独立捕获；清稿JSON/截图差异和停服后复开不能补证均保留 |
| E64：122队列保存丢回包、读取中断及刷新恢复 | `dist/validation/remote-acceptance-20261103/failure-chain-122/`；独立报告 `remote-queue-save-receipt-122-20261103.md` | 真实completed/committed后丢HTTP回包，刷新保留同A/E及双草稿，新loader GET同A解锁；原题回答后client自然唯一消费于T54，完整input、原52轮/C8保持。有界send/begin/save/answer各一次；实际网页及原生起止源码均已捕获。不计服务端长期unknown、复杂输入或全部回归通过 |
| E65：目标旧请求迟到尝试及范围调整 | `dist/validation/remote-acceptance-20261103/goal-stale/`；`remote-goal-stale-20261103.md` | 甲原HTTP保存completed；乙Request暂停后被浏览器取消，未实际放行、无服务端409，乙稿仍保留。期间原生文档重载导致旧运行身份失效；不判竞争通过，按用户要求不继续扩展双远程端并发 |
| E66：单客户端侧栏偏好存储失败 | `dist/validation/remote-acceptance-20261103/storage-failure/`；`remote-preference-storage-20261103.md` | 浏览器原setItem实际QuotaExceededError，视图本页生效且明确告知未落盘，普通稿保持；释放测试填充后保存成功并恢复原0键。18项独立核对通过；不扩为图片、IndexedDB、撤权或全量存储故障通过 |
| E67：123重载后入口发现恢复 | `dist/validation/remote-acceptance-20261103/native-entry-123-live/`；`remote-native-entry-123-20261103.md` | 当前准确新文档安装123/106后，initial及关联模块真实解析成功，M54/C8读取及A暂停目标恢复，业务写0、原完整历史不变。环境随后变化，未冒称旧脚本恢复成功；不扩为所有写入口或安装后全回归 |

同一证据行内省略目录的文件名沿用该行最近一次完整目录；涉及两个根目录时已重新写明路径。

## 3. 第一轮 32 条现状

| 基线／功能 | 已有可定位原件／通过的子场景 | 仍缺分支 | 当前整体状态 | 下一项具体操作 |
| --- | --- | --- | --- | --- |
| R1-01 登录、只读与写权限 | E04 证明原生权限正常；E21 有同授权只读→可写草稿修复；E20 定位真实只读页禁优化 | 失效/重新授权、各业务后端拒绝、旧私有状态隔离；E26 的登录隔离失败不能忽略 | partial | 在隔离浏览器存储中以专用正常/只读授权各打开同一测试会话；先只查看控件，再对允许验证的拒绝路径留原回执 |
| R1-02 项目／无项目新建 | E24 定位无项目创建、唯一首轮；后续测试会话也有创建原回执 | 指定项目的 projectId/workspaceKind/cwd；缺能力和部分创建 | partial | 从已有获准测试项目入口新建一次，记录创建 ID 和原生项目归属；没有项目时先由桌面 UI 准备 |
| R1-03 普通发送与流式 | E01为117单次文字与唯一接受；E28已补普通发送成功回包丢失、原回执恢复及草稿保护；E39普通主会话和E42侧边正常发送有真实回执 | 明确失败保稿、双击；完整多次流式变化 | partial | 只补明确失败或流式中间态，不重新将E28已过的丢回包场景列为未执行 |
| R1-04 模型／思考深度 | E36两个实际模型M→C→M三轮，effort/原生/wire协议准确，旧历史保持；未加载C正常恢复 | 拒绝等级／失败保稿；跨协议、侧边和并发等其他设置场景 | partial | 单独验证不支持等级与明确失败时保稿，不再重复三轮正常切换替代失败分支 |
| R1-05 停止当前轮 | E22 原轮 interrupted、控制历史未变 | 真正运行的后台终端；过期 T/新轮竞争；停止 unknown；117 停止链 | partial | 让专用 M 的准确 T 有一条可安全停止的终端任务，空草稿点击停止一次；核 T、终端和 C |
| R1-06 多行与 Skill | E02/E03 真实 text/image/image/skill；较早 Skill 队列保存/取消见当天进度 | 多光标位置、多个 Skill 交错；停用/跨项目拒绝；117 准备精简后的同场景 | partial | 在正文开头、中间分别选择真实可用 Skill，保留中文/内部空行后发送一次，对照原生输入顺序 |
| R1-07 图片普通发送 | E02 原始普通消息确有实际选择的双图；性能115有纯图原生接受，原HTTP已截断 | 两图预览→移一图再普通发送；取消/非法格式/读取未完；117 图片 | partial | 用真实文件选择器选两图、预览、移除第一图后发送；记录保留图原字节和最终原生输入 |
| R1-08 媒体／工具／历史阅读 | E23 定位同 sleep 的进行中/完成/unknown 展示修复 | 图片视频实际预览、点播前不下载/关闭止流；完整分页、阅读锚点、失效媒体 | partial | 打开已有媒体和多页历史的测试会话，翻页并展开准确工具；预览/关闭视频同时记录媒体请求 |
| R1-09 编辑最新已发消息 | E02 双图/Skill 正常取消和仅改正文保存；原前轮不变；110 正式恢复普通编辑 | 非空 text_elements/其他隐藏输入；实际跨端过期竞态；失败/unknown；117 | partial | 在带真实非空引用元数据的最新已完成消息上先取原输入，再改正文一次；没有该样本时先保留缺口，不能造引用 |
| R1-10 自动排队承接 | E07自然B→C；E29在117的保存/取消丢回包恢复后自然FIFO；E60/E64正常及故障链自然唯一消费；E37/E39停止/取消后原队列保留并显式恢复 | 真实上游失败后的原生策略及迟到忙碌等独立分支，见R3-06 | partial，正常承接及停止恢复已验 | 不重做已过的自然FIFO和停止恢复；仅补未验的真实失败等分支，按原生实际策略判定，不能强制要求每种终止都自动消费 |
| R1-11 队列编辑取消 | E08早期联动、E10双页终态；E29取消成功回包丢失、GET原回执、原文恢复和自然FIFO；E50补自然出队后的迟到编辑409拒绝 | paused/send-only、复杂隐藏输入、原生长期未决或进程重启；不将已过出队冲突重复列为缺口 | partial，正常取消及已测恢复/冲突通过 | 不重复E29/E50；后续只针对准确未验输入或恢复条件，保留原编辑身份和草稿，不用重发补过 |
| R1-12 多行队列保存 | E07忙闲联动、E03复合正文、E10双页收敛；E29图/Skill丢回包恢复；E60同版忙时保存/刷新，E63空闲保存；E64补成功丢回包和持续读取中断、刷新保留/恢复及自然唯一消费 | 服务端真正长期unknown及进程重启；E63自身采集边界保留，不能用E64回填旧原件 | partial | 正常忙闲和E64网络中断不重复；后续仅在准确原生未决或进程恢复场景核原ED/输入，不扩大为所有复杂输入已验 |
| R1-13 队列原图与上下文 | E03 仅改正文时双图/Skill 非 text 项保持，后继 C 不变 | 非空隐藏引用/text_elements/文件等；无法识别输入拒绝、unknown；117 | partial | 在自然形成的真实隐藏输入队列上只改正文，比较所有非 text 项；缺真实样本就注明尚未复核 |
| R1-14 增图／移图／纯图 | E03 双图+Skill 红蓝→蓝绿；E09 108 修复忙/闲纯图补文字误报 unknown | 选图取消/无效附件/旧能力、纯图追加图等组合；真实失败/unknown；117 | partial | 在本轮纯图队列先打开选择器后取消，确认无变化；再追加一图保存一次，核实际顺序、输入和后续消费 |
| R1-15 排序与删除 | E12 定位键盘 B/A/D、C 删除及旧排序请求409不覆盖 | 排序/删除丢回包；出队竞争；鼠标拖动/触控不由键盘结果代替 | partial | 对新的 Q1/Q2/Q3 键盘重排，再仅丢一次准确删除回包，核原对象终态且不第二次删除 |
| R1-16 队列引导 | E12/E25正常活动轮引导；E43成功回包丢失、刷新GET原A及原T内唯一接受；E60同版正常整链；E37/E39停止后原Q显式另起新轮恢复 | 压缩/过期拒绝、准备不兼容、原生长期未决；稳定耗时分布尚未完整建立 | partial，正常引导及丢回包恢复已验 | 不重复正常引导和E43；仅补原生准备拒绝或长期未决等独立分支，区分活动轮内引导与停止后另起新轮 |
| R1-17 阻塞问答／命令审批 | E27真实阻塞问答；E33命令单次批准；E37实际可用cancel及同item declined/interrupted闭环 | 桌面同步、明确失败及通道中断；文件审批为用户确认缺失能力，排除开发 | partial | 按原生availableDecisions验证剩余既有命令/问答失败与恢复；不开发文件审批 |
| R1-18 异步问答发现 | E04 107 活动题权限正常；E05 原 completed 丢卡修复；E06 同题刷新 | 读取模块失败时如实未知、事件中断恢复、117 活动/完成题发现 | partial | 在当前117会话保留一张未答题，等提问轮自然 completed 后刷新，对照原 turn/item/question/entity |
| R1-19 异步多题回答／跳过 | E04 单次活动回答；E05 定位双题提交和 skip；E06 原 completed 双题回答恢复 | 桌面部分处理、真实拒绝/unknown 的 skip、跨实体竞争；117 | partial | 对两题先改选择不提交，记录零派发，再只提交一次；另一张新题填写草稿后跳过，核草稿未发送 |
| R1-20 目标标签／取消草稿 | E24两入口、真实键盘；E45最新网页Skill/双图共存、重复选择单标签及仅删标签保持输入，零目标写入 | 能力不足、跨端/刷新非空复合草稿恢复 | partial | 仅在现有权限或真实能力不足条件下核入口及草稿，不人为增加新产品能力；不重复已过的双图/Skill正常序列 |
| R1-21 既有会话创建目标 | E13旧轮active→paused；E51当前121/a4d9单次setGoal、准确同G active→complete、普通send/create零次 | 原生明确失败/unknown、非空附件仍未验；入口无tokenBudget字段 | partial | 下一步只补原A未知恢复或合法复杂输入；不重复已过的纯文本正常创建/完成，不将HTTP回包丢失等同原生保存失败，不加预算字段 |
| R1-22 新建目标／部分成功 | E24无项目正常创建；E55指定P的真实ID/rootPaths、单次create与目标完成，第二事务在准确阶段暂缓时刷新后恢复同A | 真实原生拒绝、目标RPC派发后unknown、pending窗口新稿及复杂输入 | partial，指定项目及阶段暂缓已验 | 继续真实失败或新稿保持分支；不重复正常指定P创建，也不把阶段暂缓通过扩大为所有部分失败 |
| R1-23 目标更新／结束／取消 | E13 改稿取消/还原、保存 paused/预算/非零累计、complete后当前目标空 | 原生 clear→远程同步、非空预算、运行中更新、失败/unknown、服务端旧基线 | partial | 桌面将专用目标 clear 一次，远程只读观察同 G 消失并刷新；预算分支另用合法非空预算目标 |
| R1-24 手动压缩 | E23正常完成与后续发送；E59补121/106同T/I真实完成、成功回包丢失及原稿保持，原用户消息不变 | 拒绝、真实失败、并发旧完成、非空队列及本轮后续实际发送 | partial，丢回包及保稿已验 | 下一步仅补真实失败或其他独立分支，不重复E59正常及未决跨刷新；受理回执running不是最终执行状态 |
| R1-25 优化／恢复／取消 | E20 定位真实供应商、逐字恢复、迟到保新稿、cancel先于start | 供应商运行中取消、明确失败/unknown；完整跨会话 | partial | 在真实 pending 时取消原优化ID，分别记录本次上游与服务终态；窗口已过就保持该分支未执行 |
| R1-26 上下文口径 | E22 last35499/window353400→10%，区别累计499207 | 新轮降低值、真实0/missing、准确兜底来源、117 | partial | 查看两个已有不同用量会话，再完成一条短请求；将 UI 原数值与同轮 last/window 对比，0/missing 只用真实样本 |
| R1-27 项目／会话置顶 | E22 nativePins恢复；E61准确P/C置顶、刷新、另一端显示及取消恢复，项目顺序和C邻项保持 | 失败/unknown、桌面UI及准确原生写回执独立归档仍未全覆盖 | partial，正常跨端子场景已验 | 不重复正常置顶；后续仅针对原写回执失败/unknown核对，保留原对象及原状态 |
| R1-28 子代理列表／只读 | E17 稳定22条/准确父关系和只读；较早 active→done；E25 当前空态文案 | waiting、孙级/多父、发现不完整/恢复；新只读提示安装后实际展示 | partial | 打开一个自然处于 waiting 的已有子代理，核原生分类与详情所有写入口；没有样本时不制造通过 |
| R1-29 侧边新建／双向 | E16网页创建、默认/后续名、原生API改名、零消息模型一致、关闭；较早原生API创建后远程发现/改名/刷新；E46/E47迟到创建 | 桌面菜单创建入口、真实创建失败/原生长期未决、临时历史分页 | partial，创建及API双向同步已验；桌面入口暂停 | 按用户指示不操作桌面菜单；网页创建和API同步不重复。远程手工改名入口是既有能力缺失，不追加为本轮开发 |
| R1-30 变更／文件／浏览器 | E31文件预览/非Git；E34 Git暂存／未暂存／未跟踪详情、双根读取、筛选与非空草稿保持 | Git图片/二进制差异及其他状态、非零滚动、错误与路径边界；Android浏览器 | partial；Android部分待用户验收 | 覆盖仍缺的Git详情类型、长列表返回、失败缓存和路径边界；不重复正常文件及非Git样本 |
| R1-31 设置／搜索／菜单 | E19视图保持；E57在同一已确认64a074…脚本文档实际重做搜索、主题字号刷新、键盘菜单并恢复，补齐E54身份缺口 | 非空草稿、大字边界、存储失败；Open In系统打开未验 | partial，现有正常UI流程已验 | 保留E57已过路径，后续仅补存储失败/非空稿或Open In；折叠跨刷新持久化未属原有能力，不主动开发 |
| R1-32 删除预览／取消 | E38准确C标题及不可恢复说明，取消零delete；完整8轮历史及设置不变 | 非空草稿、确认期间身份变化及其他生命周期 | partial | 在明确测试对象上补非空草稿及身份变化，不把预览取消等同永久删除验收 |

## 4. 第二轮 21 条现状

| 基线／功能 | 已有可定位原件／通过的子场景 | 仍缺分支 | 当前整体状态 | 下一项具体操作 |
| --- | --- | --- | --- | --- |
| R2-01 缓存与慢读取 | E19旧thread/history被ERR_ABORTED；E32在118切换两条测试会话，模型和用量准确，队列不阻塞详情返回 | 成功送达的迟到旧读、滚动／失败保旧及重试；慢队列首帧仅源码回归覆盖 | partial | 暂缓旧读取后让其实际成功送达，核当前ID/正文/模型/用量不被覆盖 |
| R2-02 复合草稿恢复 | E19 两行切换/刷新；E21 只读往返；E02/E03 编辑后普通草稿保持 | Skill位置/双图顺序/目标模式的同份复合草稿；项目/新建键、重开及旧回执 | partial | 在M/C及项目新建页各保存不同的文字+Skill+图片+目标稿，切换/刷新/重开，逐项比较且不发送 |
| R2-03 侧边返回／刷新 | E16同侧边名称/ID刷新和返回原父；E25网页正常来回；E40正常关闭后回父与独立过期提示；E46/E47迟到创建切换保稿 | M/S各自非空草稿和阅读位置组合、浏览器后退、陈旧history owner、缺父；过期页返回按钮及草稿跨刷新 | partial，正常返回/刷新已验 | 仅补明确的双草稿/阅读位置、后退或缺父组合；不重复正常刷新返回，也不将E40终态非空稿扩大为跨刷新保持 |
| R2-04 侧边名称 | E16原生API两次改名、刷新和后续发送不改自定义名；首条自动命名同步原生；较早原生创建后改名同步 | 桌面菜单改名入口、迟到旧名、名称能力失败；固定十字符假设已撤销 | partial，API同步与自定义名保护已验；桌面入口暂停 | 按用户指示停止桌面窗口操作；不重复已有命名同步，也不新增远程手工改名功能 |
| R2-05 迟到创建／关闭 | E16/E25正常创建/关闭；E15两侧互不误删；E40过期提示；E42残留窗口410；E46/E47两类迟到创建不抢页面且保稿；E58空侧边关闭丢回包、跨刷新手动GET原A恢复 | 运行中关闭、真实失败/原生长期未决或服务重启；Android真实通知触达由用户验收 | partial，普通关闭及成功丢回包恢复已验 | 仅补执行中关闭或真实失败/长期未决等独立分支，不重做E58。通知网页导航不替代Android系统派发 |
| R2-06 子代理多父／孙级 | E17 同一直接子级active→done、详情刷新和稳定集合 | 两父及真实孙链、waiting、旧父迟到、不完整/恢复 | partial | 在已有真实两父/孙级样本出现后只读往返菜单和详情，按source/parent逐级核对；不能拿22个直接子级补孙级 |
| R2-07 主会话两轮已读 | E14 两轮未读→阅读→刷新保持；E18 原生恢复后可已读 | 原生桌面阅读、读失败/unknown、不在底部、新旧轮；CLI兜底markRead仍failed/notSent | partial | 在117对M完成两轮离开/未读/读底部/刷新；另单独让markRead读取失败，不误清新轮 |
| R2-08 多侧边未读聚合 | E15读A后B仍未读且父未读，读B后父聚合清，最终联网刷新保持；正常关闭互不误删 | 桌面UI读S2、中间刷新、新轮复现、旧已读/失败/原生长期未决 | partial，逐个已读与最终刷新已验；桌面入口暂停 | 不重复E15；桌面阅读按用户指示暂停，仅保留明确未验的中间刷新或失败分支 |
| R2-09 客户端独立偏好 | E19独立视图/离线保存；E41不同origin隔离和同origin传播；E61置顶；E66真实localStorage配额失败、提示/保稿、恢复后保存及原状态恢复 | 桌面原顺序保持未验；不新增排序菜单、不扩展低频双远程端组合并发 | partial，既有入口及本次存储失败已验 | 不重复E66；按用户范围优先单客户端主流程，桌面窗口继续停止 |
| R2-10 同scope多tab／授权隔离 | E10同授权双页队列同步；E41已补同origin双页偏好传播/刷新和不同origin隔离；E26暴露过Cookie隔离错误 | 换授权/撤权、坏记录、旧响应不泄私有稿 | partial | 仅用独立专用授权补撤销和晚响应，不碰用户原Cookie；不重复已过的视图同步 |
| R2-11 问答切换／桌面部分处理 | 前日R2-question定位未提交选择/文字M→Q→M保持；E06同题刷新和原回执恢复 | 桌面答一题/跳过、其余题保留及迟到旧读；117 | partial | 保留多题未提交选择后切C再回M；桌面只处理其中一题，远程核剩余准确题不被整卡误清 |
| R2-12 问答／审批跨轮实体 | E44已形成两个不同turn/item/entity的同名问题，第二组不继承旧答案，跳过不提交答案或新建轮次 | 迟到旧事件到达新卡期间、旧提交锁与桌面并发 | partial | 只补真实旧事件迟到或并发；保留两组准确身份，不重复同名题正常先后与跳过 |
| R2-13 目标终态重开 | E13同G complete、当前目标null、刷新不复活的原件 | 原生clear、切换/重开、独立队列、迟到旧目标和单独编辑回执 | partial | 专用G完成后切C再回M并重开页，核目标条/时钟不存在且新稿/队列保持；clear另验 |
| R2-14 目标计时／编辑保持 | E13暂停目标累计值及编辑稿保持；E69修复重启后冷会话目标缺失，同G暂停状态/计数恢复、刷新保持，清除后不复活 | active计时、缺时间、通知不重建输入、后台/断线再校准 | partial；冷会话读取子场景通过，手机部分待用户验收 | 后续只补未验计时/编辑窗口，不重做E69。冷会话仍notLoaded，不能把补读目标当作已恢复执行或已知问答 |
| R2-15 上下文通知竞态 | E22一个真实口径；E19切换旧读取消后的值保持 | 真零/missing/降低、读取与新通知反序、展开提示同步 | partial | 在两个实际用量不同会话间切换，延迟旧读并让新请求通知先到；比较最后一次原始用量及UI |
| R2-16 文件／变更缓存隔离 | E34两真实Git根正常往返同名文件/差异、详情notModified、同长度新revision及筛选隔离 | 链接worktree、迟到竞争、DOM/图片复用、非零滚动、错误缓存、路径/授权失效 | partial | 下一步测试迟到旧响应与错误缓存；普通双根切换不能代替竞争或授权失效 |
| R2-17 Android安装／资源身份 | E25产物身份、26项资源是构建记录 | 实机包名/签名/激活资源、实际连接同M；旧资源及更新后重新核验 | 待用户验收 | 用户安装14:16正式APK后记录手机/App版本与资源来源，再打开同一测试会话；不能沿用11:16包结果 |
| R2-18 Android后台恢复 | 历史桌面尝试均未产生真实visibilitychange，不能代替真机 | 短后台、超过读取期限、系统回收、图文草稿、无旧槽卡死/自动重发 | 待用户验收 | 用户在同M保留图文稿，依次短后台、超过一分钟返回、系统回收重开，记录ID/草稿/超时及时间 |
| R2-19 Android相册／键盘／返回 | 真实网页选择器结果只作桌面局部证据 | 相册选择/取消、键盘遮挡、逐层返回、焦点滚动及连接失败 | 待用户验收 | 用户实际选/取消相册图片，展开模型/问答并输入多行后逐级返回，确认不误退出/误发 |
| R2-20 Android通知／资源更新 | E25只证明APK和网页资源被打包 | 精确通知跳转/已读、成功更新、坏更新回退、撤权隐私及未决写保持 | 待用户验收 | 用户点击真实测试通知；仅在确有更新时核激活摘要和草稿，不存在更新夹具的分支继续未验 |
| R2-21 Windows启动窗口 | 有源码/受控子进程历史记录；无最终包完整视觉验收 | 启动/托盘唤回/位置、真实探测的闪窗及失败恢复 | 待用户验收 | 用户安装15:56包，在允许重启时启动一次、托盘唤回一次并刷新远程/模型，记录额外窗口和位置 |

## 5. 第三轮 20 条现状

| 基线／功能 | 已有可定位原件／通过的子场景 | 仍缺分支 | 当前整体状态 | 下一项具体操作 |
| --- | --- | --- | --- | --- |
| R3-01 普通发送丢回包 | E28成功回包中止后GET即时恢复；E70补124普通send原回包挂起超时、同A/client及新稿跨真实刷新保持，人工放行新文档原A GET后解锁；send1、刷新后0 | 真正后端长期unknown、真实failed/404、全来源中断、跨授权及完全自动恢复仍未验；E70未采集原POST正文 | partial，普通文本未决跨刷新子场景通过 | 不重复E28/E70；后续仅按具体剩余失败或授权边界执行，不把人为挂起91秒当正常发送性能，也不把定时GET的成功归给按钮发出的另一个GET |
| R3-02 队列保存未知 | E29一图/Skill保存成功丢回包及GET恢复；E64补122单条文本真实成功回包丢失、持续读取中断和跨刷新恢复，准确原client自然唯一消费 | 服务端真正长期unknown、原生重启、复杂隐藏输入；出队竞争另见E50 | partial，成功丢回包及读取中断恢复已验 | 不重复E64；只在独立原生未决或重启场景继续核原ED、返回physical、原client及完整输入，不把网络丢回包说成原生未完成 |
| R3-03 引导准备／接受失败 | E12正常契约、E25 accepted进入原T；E43补121真实成功回包丢失、刷新后GET原A恢复及唯一消费 | 真实准备结构不兼容、压缩/过期拒绝、原生派发结果长期unknown、worktree恢复；旧GET383清退证据缺口 | partial | 下一步只补原生准备拒绝或长期unknown；不重复已过的HTTP成功回包丢失恢复 |
| R3-04 编辑／删除与出队竞争 | E50已完成两个独立事务：真实Request暂停→原Q唯一自然完成→放行原begin/remove→原A明确409，普通稿和历史保持；真实原件通过事实函数 | 内部临界区及另一种接受结果未执行；首轮编辑超时取消仍不计通过，但这些不是新增产品或全自动runner门禁 | passed（原基线允许的迟到拒绝结果） | 不重复E50，也不为凑另一种合法结果新增并发逻辑；后续只有出现具体反例或相关代码变化才复测。其他未执行时序不据此宣称通过 |
| R3-05 双客户端改队列／目标 | E11旧请求409且原队列不变；E10正常终态广播；E13新目标到达后禁旧稿；E65原乙请求取消、未取得服务端过期拒绝 | E11暂态提示来源和E65服务端409仍无完整证据；用户要求双远程并发从简 | partial，低优先级 | 保留现有过期拒绝、不重发及保稿；不再展开同时提交、双物理设备或组合竞态，不新增协调层 |
| R3-06 停止／失败后的队列 | E37取消后Q保留；E39普通停止及显式引导；源码/本机schema支持线程配置。E62独立app-server仅完成initialize，实际业务未发 | 测试进程网络隔离未建立；真实上游失败、失败后原生自动／保留策略及未知回执仍缺 | partial；上游失败子场景blocked/not_run | 先取得匹配安装版的隔离网络依据或已验证的独立环境；不修改用户系统代理/共享供应商，不把初始化TCP连接或派发前拒绝当模型失败 |
| R3-07 审批／压缩中普通入队 | E33真实waitingOnApproval时一图加Skill入队，批准后自然唯一消费、输入完整 | 压缩窗口、审批取消／失败后的队列策略、旧能力和unknown | partial | 单独验收压缩等待及取消审批后的原生队列行为，不重加已存在消息 |
| R3-08 问答发现中断恢复 | E44已有题离线恢复；E52原缺题保留；E53新增真实停服期间生成原题、恢复后自动读取显示及单次回答成功 | 全通道中断、长期未知与后台；早期HTTP淘汰；正式121原生及真机 | partial，单线路停服补题已修 | 不重复E53已闭环分支；继续核长期未知或其他通道恢复，保持准确原题／原回执身份。旧PWA资源尝试不计通过 |
| R3-09 问答／审批未知与竞争 | E06异步回答原回执恢复；E27同步回答已接受后响应中止，原生状态收敛且已捕获窗口不重答 | 真实failed/404、全部通道中断、换授权/后台、桌面竞争与下一实体 | partial | 在相似问题跨实体或桌面竞争时延迟旧结果，核旧题终态不清新题；不因当前卡消失就认定原答复成功 |
| R3-10 新目标部分成功／未知 | E55第二事务证明原生创建及首条accepted、goal=null时刷新，7次GET原A pending后completed，0重复create/send/setGoal | 明确原生失败、RPC已派发后的unknown、长期未知及pending窗口新稿 | partial，阶段暂缓恢复通过 | 下一步针对真实失败或准确原A长期未知，不重做已过阶段窗口；35秒测试gate定时释放须与生产超时及人工放行区分 |
| R3-11 目标旧基线／完成保存竞争 | E13更新到达后UI禁旧稿、保留输入；E65生成旧POST但被浏览器取消，不能算409 | 真过期POST拒绝无现场证明；complete/clear组合未执行 | partial，按用户要求从简 | 保留三字段旧基线检查及不自动重发；不追加原子CAS、跨端锁或完成/clear组合竞态，优先单端目标恢复 |
| R3-12 压缩未知／断线／失败 | E59第二事务真实成功回包Aborted后unknown跨刷新，按同T/I原生状态收敛；0重复compact、原稿/原用户消息保持 | 全断线/长期unknown、原生真实失败、原回执驱动恢复、非空队列、后续实发 | partial，未决跨刷新已验 | 后续针对真实失败或非空队列；不要把末尾诊断GET当自动恢复，也不把原受理running回执当当前压缩未完成 |
| R3-13 优化取消／迟到／无注入 | E20真实独立供应商、恢复、迟到保新稿、cancel先于start；E21权限草稿修复 | 运行中上游取消、实际失败/unknown、跨真实会话、完整只读生命周期 | partial | 让真实上游仍pending时取消，保留原优化ID；若已完成则记录错失窗口，不把晚取消当中断成功 |
| R3-14 原生失效／CLI兜底 | E18正常native→派发前连接拒绝→service→恢复native，普通主会话可写 | 复杂输入、目标/队列/审批、不合资格侧边/子代理拒绝、owner变化、派发后unknown；CLI已读不支持 | partial | 在117专用实例重复文字接管，再单独核不合资格对象明确拒绝；原生可能已派发的未知动作绝不转CLI重送 |
| R3-15 前台重连／双路／旧读 | E19标签离线保稿；E53在visible页面复现线路冷却拒绝提前补读，修复后既有轮询恢复时自动补当前会话，后续在线轮询不重复全文读 | 双路、旧epoch、其他前台恢复时序未全验；手机由用户验收 | partial，已闭环E53单线路分支 | 继续核双路或旧读迟到，不重置原有epoch／代次保护。服务channel epoch与原生requestsEpoch生命周期仍分开记录 |
| R3-16 Android断网／系统回收 | 桌面慢读、tab离线及未成立visibilitychange均不能替代 | 待确认写+图文稿+待答题复合状态、后台断网/回收、恢复槽、无超时卡死/重发 | 待用户验收 | 用户在专用会话保留三类对象后后台断网，再前台联网；另做系统回收重开，分别记录原身份和回执 |
| R3-17 坏缓存／存储失败／撤权 | E21只读欢迎页覆稿已修；E49同授权旧页卸载覆稿已修；E66真实localStorage偏好配额失败、提示与重试保存通过 | 图片/IndexedDB配额未执行；损坏缓存、撤权API及旧响应隔离仍缺 | partial，localStorage偏好失败已验 | E49/E66不重复；后续仅针对独立授权或图文恢复的实际缺口，浏览器配额接口不支持不能算图片保存保护通过 |
| R3-18 关闭／删除unknown | E56删除成功回包丢失、确认后不报迟到错误；E58空S关闭成功回包丢失、unknown跨刷新、手动GET原A恢复及父会话保稿 | 全部确认来源长期unknown、服务重启、运行中关闭及真实失败 | partial，关闭与删除丢回包均已验 | 后续仅补全来源unknown或运行中关闭；原回执只读核对，不重复关闭/删除，不将空S扩大为运行中或含未保存内容 |
| R3-19 启动／探测失败窗口 | 源码/受控子进程检查不代实际窗口视觉 | 最终包实际失败退出码、无闪窗/重试风暴、恢复不续旧循环 | 待用户验收 | 用户仅在实际遇到不可用探测时记录窗口和时间，正常恢复后复查；无需重启其正在工作的服务制造故障 |
| R3-20 五项事故交叉复测 | E60同版122正常整链；E64在同版同M完成原题等待→标签本地编辑/移除→队列保存真实成功丢回包→刷新保留→GET原A恢复→回答原题→自然唯一消费，历史与普通稿保持 | 原生真正长期unknown、全通道中断与真机复合恢复仍缺；不把不同版本原件拼接成同版整链 | partial，正常及本次故障连续子链已验；真机待用户验收 | 正常链和E64故障链不重复，剩余分支按具体条件执行；Android按用户实际安装版本单列 |

目标创建验收口径（R1-21/R1-22/R3-10）：一次 `create` HTTP 动作使用稳定的 `Idempotency-Key=A`，内部创建、首条接受和设目标分别以原生 `startConversation` 的 `status/conversationId/firstTurn.status/firstTurn.turnId`、`setThreadGoal` 记录及独立 `thread/read`、`thread/goal/get` 证明，不是三个网页 POST。`GET /api/receipts/A` 只读原回执，不重跑；HTTP 成功回包丢失不等于原生目标保存失败。回执 `completed` 不等于目标 `complete`，现有创建入口无 `tokenBudget` 字段，不为测试加字段或入口。E45 只验证本地标签草稿，不能计为服务器目标创建通过；指定 P 和隔离条件未具备的分支保持未执行。

## 6. 旧 failed 的替代关系与仍然真实的失败

以下“替代”只改变现在应如何描述同一缺陷，不修改旧失败文件、回执或当时结果，也不把关联的其他分支一并标通过。

| 旧失败／误判 | 后续同路径证据 | 截至117应如何记录 |
| --- | --- | --- |
| completed 后未答异步卡消失 | E05原失败题重新发现、准确答复和刷新；E06同题恢复 | 正常发现缺陷已有后续修复证据；R1-18仍partial，发现中断/未知不能继承 |
| 活动异步问答错误显示无权限、events ReferenceError | E04同一类活动题下权限正常和原答复completed | 107受控真实修复已证；不能继续说源码仍调用该残留函数，也不能说117同步问答已验 |
| 空闲保存返回unknown、暂停ghost及后续普通发送阻塞 | E07正式sendMessage/editPosition路径不清理续发、忙时保位和自然FIFO | 旧失败已被对应正常联动替代；原unknown回执未变，R3-02真正成功回包丢失仍未执行 |
| 纯图片补文字执行成功却unknown | E09 108忙时与闲时保存completed/解锁、后续普通发送；严格输入比较保留 | 是同触发条件的修复证据，不能把该失败继续列为当前未修；但附件校验/unknown分支仍缺 |
| 甲保存后乙长期残留编辑待确认 | E10 109成功终态revision广播和双页正常收敛 | 特定终态通知缺陷已修；E11更早暂态提示没有瞬间ED，不能强行认定是同因或已解释 |
| 异步答案已接受但客户端unknown永不查原回执，删除长期禁用 | E06原回执GET后清锁，未重答；前日R2-question也有旧/新对照 | 原回执恢复缺陷已有真实修复子场景；404/失败、换授权、后台、新实体竞争仍缺 |
| 只读欢迎页空编辑器覆盖新建草稿 | E21同授权两行稿恢复、真实主动清空仍生效 | 已有同场景修复；不扩大为存储失败、撤权或所有复合草稿通过 |
| service新CLI无法抢占桌面writer／后续主会话资格误拒绝 | 同writer IPC后产品HTTP发送；E18 native→service→native完整正常接管 | 普通空闲文字兜底已有真实后续成功；不能继续列为完全不能发送。复杂输入/unknown仍保留 |
| 单项业务503使全线路冷却、工具未完成就显示成功 | 两日前日进度有真实service自动回复与同item完成前后对照，E23定位 | 对应源码修复/真实局部复测已存在，本次未重审E23全部原件，不据测试数追加整条通过 |
| 已发送编辑被整体禁用 | 用户选择最小恢复后110正式合入；E02真实双图/Skill取消与正文保存 | “普通编辑未恢复”已过时；部分空input过期目标拒绝与非空text_elements仍是兼容边界／待验风险 |
| 最近列表纯索引遮蔽active、侧边active不汇总 | 当天桌面兼容章节及E25定位最终112原生主idle/侧active、DOM圆环、自然结束 | 该正常路径有修复后材料；多侧边waiting/读取中关闭等只有契约回归，不能补为真实并发通过 |

**仍有原件支持、不能用后续数量抹去的失败或缺口：**

1. **G0.7 登录环境隔离真实失败，未恢复原Cookie身份。** E26直接记录原入口401，后恢复200，但原本机设备 `00136a3e…` 改用既有远程设备 `763c716f…`，`originalCookieValueRestored=false`。最后可复核收尾只有等价访问恢复；本次未读取当前Cookie。它是验收环境副作用，不能据此判产品73条全失败，也不能写成原环境完全未变。
2. **CLI兜底期间markRead明确failed/notSent仍是已知能力边界。** E18的fallback原件同时有成功send和失败503回执；不能因发送或原生恢复成功，把该失败动作说成已读成功。正常原生两轮已读E14与它并不矛盾。
3. **E11暂态恢复提示未闭环。** 原请求409、队列未覆盖是事实；提示瞬间准确ED/事件缺失，仍为partial，不足以定为新的错误unknown分类，也不足以宣布完全无问题。
4. **R1-09的原生过期目标/引用保留边界尚未真实穷尽。** 非空text_elements丢失为源码候选风险，跨端设置等待竞态未现场复现；不得伪装为已确认的117运行失败或已修复。
5. **Open In最终安装包按钮的真实视图切换和系统打开未完成。** 原路径失败→仅规范化path成功、真实DOM挂载分别有材料；空的生产点击记录不能替代完整按钮操作。
6. **真实前后台与性能稳定性仍无完整证据。** 真实tab多次一直visible；首段断网恢复被主动刷新打断；112/114及116/117为不同消息的单样本，准备时间明显波动。它们是未闭环项，不能随意标failed，也不能宣称性能达标。

所读材料中，未发现可以直接归属于“实际运行117并仍未修复”的新关键业务失败原件。这个结论只说明证据归属，**不表示不存在缺陷**。后续E27–E29已补同步问答、普通send及队列保存/取消的成功回包丢失子场景；完全离线、长期未知和竞态仍是独立缺口。

## 7. 初始5项优先清单与后续进展

执行前复用已获准的专用对象和授权隔离，先核实际网页/原生脚本身份；不在用户工作会话和原待确认队列试验。下面保留最初的优先操作定义。**截至18:00，第1项的同步问答正常与丢回包、第2项的纯文字发送丢回包、第3项的一图/Skill保存及纯文字取消丢回包已完成相应子场景，见E27–E29；不再重复将其当成完全未执行。** 第3项的真实出队竞争、第4/5项，以及各行当前“下一项操作”仍须继续。

截至20:39，第1项的真实命令批准、第4项的审批等待期间图/Skill入队与批准后自然续跑、第5项的正常Git分区/双根缓存已补齐相应子场景，见E33/E34。后续命令取消和模型实际请求核对已有E36/E37子场景证据，继续处理自然出队竞争及失败恢复；不要重新把上述正常序列列为未执行。文件审批按用户要求仅登记能力缺失，已移出本轮开发与必做验收清单。

| 顺序 | 对应基线／价值 | 可执行的下一步 | 成功边界与必须保留的原件 |
| --- | --- | --- | --- |
| 1 | R1-17、R3-09：117新改同步问答路径尚无真实验收 | 在117专用会话产生阻塞问答/安全审批；正常答一次，再用另一个请求做成功回包丢失恢复 | 精确thread/turn/request/entity、提交前UI、唯一原回执、实际resolved、旧历史和新稿；失败/unknown只查原ID，不能用异步题代替 |
| 2 | R3-01：117已确认sent后提早返回，普通send未知恢复还空缺 | 发送一次短文本，只丢这次已执行回包；切换/刷新后恢复原回执读取 | 原client/turn唯一接受、unknown期间稿/锁保持、准确成功后解锁、全程send1；E06的answer故障不能替代 |
| 3 | R1-11/12/13、R3-02/04：正常编辑很多，真正unknown/自然出队竞争仍缺 | 对带图/Skill的准确Q保存或取消一次并丢对应回包；另一个Q不暂停自动调度，覆盖出队临界编辑 | 原输入→ED→返回physical→真实接受、位置/后继对象、双草稿、本地/服务端队列及后续续发；只接受明确成功/过期/仍unknown三类真实结果 |
| 4 | R3-06/07/12：停止/失败/审批或压缩期间队列未真实闭环 | 先在真实审批等待时入队，再处理原审批；独立场景停止带队列活动轮，随后按原生实际策略恢复 | 阻塞原请求、Q身份及完整输入、状态时间、一次消费；窗口错过就not_run，不能用后续普通发送补过或强制规定必须引导 |
| 5 | R1-30、R2-16：可见“变更/文件”核心流程尚无本轮可用证据，且可先只读执行 | 用两个已准备worktree，依次变更列表→详情→返回、文件→预览→返回、切换工作区再回原处 | 每阶段准确根路径/版本/内容、筛选与滚动、未变缓存/失败提示；Android浏览器入口另交用户，不以菜单存在算工具页通过 |

R1-04两个模型三轮实际协议、R2-12相似题跨实体、R3-10目标部分成功、R3-17真实存储失败/撤权是后续紧接的独立缺口，不因没有进入本批前5而退出73条范围。

## 8. 原生Windows UI与用户已接管项目

| 分类 | 具体项目 | 当前责任与下一步 |
| --- | --- | --- |
| 原生桌面UI暂停执行 | R1-02测试项目准备；R1-17桌面审批双向；R1-23原生clear；R1-29/R2-04原生菜单创建/改名；R2-08桌面阅读S2 | 用户2026-11-03明确要求停止桌面窗口操作，本方不再操作或探查桌面窗口；这些入口保持未执行。原生API回执可作独立状态，不能替代桌面菜单操作本身 |
| 原生桌面交互暂停执行，非73条完整替代项 | Open In完整/压缩视图切换、最终按钮真实系统打开；多个侧边运行/等待的真实UI | 服从同一停止指示，E25局部材料不升级为完整操作通过，也不自动转成用户已接管整项 |
| 用户已接管Android | G0.5、R2-17—20、R3-16，及R1-30/R2-14/R3-20的真机部分 | `待用户验收`。使用14:16 APK并记录实机激活身份；按 `remote-user-acceptance-20261102.md` 执行前后台、系统回收、选图键盘返回、通知/真实资源更新 |
| 用户已接管Windows闪窗视觉 | R2-21、R3-19窗口视觉部分 | `待用户验收`。使用15:56管理端包，正常启动/托盘唤回/探测时留视觉与时间；源码、进程树和失败退出码仍与视觉结果分开 |

## 9. 来源快照

本报告采用文件当前内容中的后续追加段，不把文件标题中的20261101当成最终证据日期。

| 来源 | SHA-256 |
| --- | --- |
| `docs/validation/remote-acceptance-cases-20261101.md` | `d0bae55d0d3a59b9c06f320d8babc98f263b0e79b44aecfed7cf34499ced78e6` |
| `docs/validation/remote-acceptance-coverage-20261101.md` | `ba8a69db886577d79d0c5b54b313896f2dd64e50e29e38b651bd98a1d51de5cf` |
| `docs/validation/remote-acceptance-progress-20261101.md` | `9ca468967a35f44210af930522d3a3a590025956ec6d6764cdc98b1f8e91f645` |
| `docs/validation/remote-acceptance-progress-20261102.md` | `362b9507fc516636d9681236babeaf5731024c7567fe93e0660494d308decf11` |
| `docs/validation/remote-performance-20261102.md` | `64516920b2a29321d08041bd9f955052e3cd742ad162f94f7438cb9e8185e608` |

用户分工与安装操作只读取上述报告直接引用的 `docs/validation/remote-user-acceptance-20261102.md`。没有读取其他会话、外部网页或未被报告指向的产品/测试源码，也没有将未读取的原件写成本次已复核。
