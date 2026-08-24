# 实现计划

## 计划目标

在 Codex 原生界面内交付跨项目任务看板：固定五列、已有/新会话建任务、拖拽与状态菜单、关联会话导航、本地持久化、多窗口 revision 冲突保护和按 `main` 实际宽度响应式布局。完成标准引用 SPEC-001 的验收标准；明确不做 NT-001–NT-007。

## 输入与能力判定

| 能力 | 来源 | 状态 |
| --- | --- | --- |
| 需求边界与验收标准 | SPEC-001 | 已具备 |
| 代码事实来源 | 本计划 READ-001–READ-032 / MISS-001–MISS-004 | 已具备 |
| 外部接口契约 | 本计划定义 CT-001、CT-004 | 已具备 |
| 模块间接口契约 | 本计划定义 CT-002、CT-003、CT-005 | 已具备 |
| 测试义务 | SPEC-001 + tasks.md 的任务闭包 | 已具备 |
| 实施前验证缺口 | OPEN-001 | 已登记为阻塞任务，由 T-012 关闭 |
| 项目工程约定 | PROJ-001、PROJ-002 | 已加载 |

## 全局约束

| 约束 | 内容（原文） | 来源 |
| --- | --- | --- |
| 产品形态 | 保留 CodexElves 的本地产品形态。 | PROJ-001 |
| 品牌 | 产品名：`CodexElves`；Rust crate / workspace 包名：`codex-elves-*`。 | PROJ-001 |
| 禁止回流 | 不能恢复已移除功能，不能把项目改回 CodexPlusPlus / Codex++ / codex-plus 命名，不能破坏本地代理和管理器交互改造。 | PROJ-001 |
| 注入增强 | 不得破坏当前 Codex App 注入增强。 | PROJ-001 |
| 既有交互 | 插件入口、工具与插件、会话管理、模型配置等现有交互不能因上游 UI patch 回退。 | PROJ-001 |
| 数据边界 | 不修改 Codex SQLite schema，不向 Codex SQLite 写任务数据。 | SPEC-001 |
| 计划边界 | 本计划不创建分支、不 commit、不执行实现任务；执行期不得修改规划期只读的 `plan.md` / `tasks.md`。 | 用户指定 Skill + 本计划 |
| 用户改动 | 不覆盖或回滚与本计划无关的工作区改动。 | PROJ-001 |

计划非目标：

- NT-001：任务编辑、删除、归档、批量操作。
- NT-002：根据会话内容自动推断或修改任务状态。
- NT-003：跨设备、云端或账号级同步。
- NT-004：修改 Codex SQLite schema 或写入任务字段。
- NT-005：新增 Manager 页面、设置项或功能开关。
- NT-006：一个任务跨项目关联会话。
- NT-007：在看板展示或搜索会话正文。

## 模块划分

### M1 任务看板领域与一致性存储

- 职责：schema v1、字段不变量、cwd 规范化、文件锁、revision、幂等创建、移动排序和原子替换。
- 对外接口：CT-002；为 CT-003 提供共享 DTO。
- 依赖模块：无。
- 涉及任务：T-001、T-002、T-003。

### M2 本地会话真实性目录

- 职责：跨候选 Codex SQLite 聚合、去重、过滤、warning 和 launcher provider。
- 对外接口：CT-003、CT-005。
- 依赖模块：M1。
- 涉及任务：T-004、T-005。

### M3 任务看板 Bridge 编排

- 职责：注入存储、认领四个 path、请求解析、真实会话/项目校验、错误映射和脱敏诊断。
- 对外接口：CT-001、CT-003。
- 依赖模块：M1、M2。
- 涉及任务：T-006、T-007、T-008。

### M4 Renderer 看板运行时

- 职责：入口、`main` host、五列 UI、响应式、搜索/筛选、modal/menu/popover、Bridge 状态机和冲突回滚。
- 对外接口：CT-001、CT-004。
- 依赖模块：M3、M5。
- 涉及任务：T-009、T-010、T-011。

### M5 Codex 原生会话适配

- 职责：当前 Codex 版本能力探测、新会话/composer/永久 ID、恢复队列和分层导航。
- 对外接口：CT-004。
- 依赖模块：M4；宿主事实由 OPEN-001/T-012 提供。
- 涉及任务：T-012、T-013、T-014。

## 接口契约

| 契约面 | 判定 | 说明 |
| --- | --- | --- |
| 跨端接口 | 有 → CT-001 | Renderer 与 launcher 通过既有 CDP Bridge 调用四个任务看板操作。 |
| 跨模块接口 | 有 → CT-002、CT-003、CT-004、CT-005 | 存储、会话目录、Renderer adapter 和 data helper 跨任务使用。 |
| 对外部系统接口 | 无 | Codex 私有 dispatcher/DOM 被封装在 M5 内；其他任务只消费 CT-004。 |
| 公共能力接口 | 有 → CT-002、CT-003、CT-005 | 多个任务共同依赖这些基础能力。 |

### CT-001 任务看板 Bridge RPC 集

- 契约类型：跨进程调用
- 来源：自有型，复用引用型统一 Bridge transport
- 生产者：M3 → T-006, T-007, T-008；M4 → T-009, T-010, T-011, T-013
- 消费者：M3 → T-006, T-007, T-008；M4 → T-009, T-010, T-011, T-013
- 调用标识：`/task-board/snapshot`、`/task-board/session-catalog`、`/task-board/task-create`、`/task-board/task-move`；Rust 常量与 Renderer `taskBoardBridgeRoutes` 镜像，source-contract 测试逐项比对。
- 取值来源：SPEC-001 分配 `/task-board/*` 值；既有 transport 以 `path` 字符串匹配。
- 两端认领：Renderer 调用 `postJson(path,payload)`；launcher 的 `handle_bridge_request` 通过 `match path` 认领。
- 参数传输：
  - 公共包络：`{id:string,path:string,payload:object,generation:string}`；任务代码只提供 path/payload。
  - snapshot、session-catalog：`{}`。
  - task-create：`taskId:string(UUID)`、`expectedRevision:integer(0..Number.MAX_SAFE_INTEGER)`、`title:string(trim 后 1–120 Unicode 字符)`、`project:{cwd:string,label:string}`、`sessionIds:string[](非空、唯一、永久 ID)`。
  - task-move：`taskId:string(UUID)`、`toStatus:"new"|"planning"|"executing"|"review"|"done"`、`targetIndex:integer>=0`、`expectedRevision:integer(0..Number.MAX_SAFE_INTEGER)`。
- 返回传输：
  - 成功快照：`{status:"ok",schemaVersion:1,revision:integer,tasks:TaskBoardTask[]}`。
  - `TaskBoardTask`：`id,title,project:{cwd,label},status,order,conversations,createdAtMs,updatedAtMs`。
  - conversation：`{sessionId,title,cwd,updatedAtMs:integer|null}`。
  - catalog：`{status:"ok",projects:[{cwd,label,sessionCount}],sessions:[{sessionId,title,cwd,updatedAtMs}],warnings:[{code:"codex_db_read_failed",count}]}`；不得含 DB/rollout path 或正文。
  - conflict：`{status:"conflict",code:"revision_conflict",message,schemaVersion:1,revision,tasks}`。
  - failed：`{status:"failed",code,message,path?}`；path 仅用于任务文件错误。
- 生效登记：不需要额外注册表；四个 `match` 分支通过既有统一 binding 生效。
- 传输类型构成：camelCase JSON；强类型请求拒绝未知字段；时间戳和 revision 使用 JS-safe 非负整数。
- 错误与超时：稳定业务码为 `invalid_input`、`session_not_found`、`project_mismatch`、`revision_conflict`、`task_id_conflict`、`task_not_found`、`task_board_busy`、`task_file_invalid`、`task_board_unavailable`、`session_catalog_unavailable`。普通任务路由不增加 Renderer 总超时，文件锁等待最多 2 秒；transport 无 code 的失败由 Renderer 归一为本地 `bridge_unavailable`。
- 重试：create 的 revision conflict 用同 taskId/最新 revision 自动重试一次；原生新会话的 `session_not_found` 短退避总计最多 10 秒；其他错误不自动重放。
- 幂等：snapshot/catalog 只读幂等；create 以 taskId + trim 标题 + 规范化 cwd + 无序 session ID 集合判同语义；move 无变化不增 revision，旧 revision 重试以 conflict 快照收敛。
- 冻结状态：已冻结
- 锚点：`crates/codex-elves-core/src/bridge.rs:93`、`crates/codex-elves-core/src/routes.rs:108`、`assets/inject/renderer-features.js:4616`、SPEC-001 Bridge API。

### CT-002 任务看板一致性存储能力

- 契约类型：进程内方法调用
- 来源：自有型
- 生产者：M1 → T-001, T-002, T-003
- 消费者：M1 → T-001, T-002, T-003；M2 → T-005；M3 → T-006, T-007, T-008
- 签名：
  - `normalize_task_project_cwd(raw: &str) -> Result<String, TaskBoardValidationError>`
  - `TaskBoardStore::snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError>`
  - `TaskBoardStore::create_task(&self, TaskBoardCreateCommand) -> Result<TaskBoardMutationResult, TaskBoardStoreError>`
  - `TaskBoardStore::move_task(&self, TaskBoardMoveCommand) -> Result<TaskBoardMutationResult, TaskBoardStoreError>`
- 参数约束：DTO 字段与 CT-001 一致；create conversations 已由 Bridge 解析为真实快照但 store 再验证文档不变量；move index 按移除源任务后的目标列解释。
- 返回语义：缺失文件返回 schema 1/revision 0/空 tasks；真实 mutation `changed=true` 且 revision 加 1；create 幂等重试 `changed=false,idempotent=true`；无变化 move 不写文件。
- 异常行为：不 panic；`Busy`、`InvalidFile{path,message}`、`InvalidInput{message}`、`RevisionConflict{current}`、`TaskIdConflict`、`TaskNotFound`、`Unavailable{path,message}`。
- 副作用：snapshot 只取共享锁读取；mutation 在稳定 lock 文件的独占锁内重读、校验、原子替换；M3 用 `spawn_blocking` 调用。
- 冻结状态：已冻结
- 锚点：无，本次新建。

### CT-003 本地任务会话目录能力

- 契约类型：进程内方法调用
- 来源：引用型，扩展现有 `BridgeDataService`
- 生产者：M1 → T-001；M2 → T-005；M3 → T-006
- 消费者：M2 → T-005；M3 → T-006, T-007
- 签名：`BridgeDataService::task_board_session_catalog(&self) -> anyhow::Result<TaskBoardSessionCatalog>`
- 参数约束：无参数；候选库只来自 launcher，不接受 Renderer 路径/标题/cwd。
- 返回语义：`TaskBoardSessionCatalog{projects,sessions,warnings}`；聚合 thread/automation，按更新时间保留每个真实 ID 最新记录，排除 archived/空 ID/空 cwd；单库失败返回 warning，空候选成功空目录，全部实际存在库失败返回 Err。
- 异常行为：真实 provider 只在全部实际存在候选库失败或 blocking join 失败时 Err；`UnavailableDataService` 明确 Err；M3 映射为 `session_catalog_unavailable`。
- 副作用：只读 SQLite，可写脱敏诊断；不得写 SQLite/任务文件或返回 DB/rollout path。
- 冻结状态：已冻结
- 锚点：`crates/codex-elves-core/src/routes.rs:90`、`crates/codex-elves-core/src/routes.rs:502`、`apps/codex-elves-launcher/src/main.rs:667`。

### CT-004 Renderer 原生会话适配器

- 契约类型：进程内方法调用
- 来源：自有型
- 生产者：M5 → T-013, T-014
- 消费者：M4 → T-009, T-010；M5 → T-013, T-014
- 签名：
  - `probe(project) -> Promise<{status:"ok",canStart:boolean,canOpen:boolean,code:string|null,message:string}>`
  - `startConversation(project,firstInstruction) -> Promise<{status:"ok",sessionId,title,cwd}|{status:"failed",code,message}>`
  - `openSession(sessionId) -> Promise<{status:"ok"}|{status:"failed",code,message}>`
- 参数约束：project 为 `{cwd,label}`；instruction trim 后非空且只驻留内存；成功 sessionId 不得是临时 ID。
- 返回语义：probe 无导航副作用；start 仅在原生提交并观察到永久 ID 后成功；open 仅在原生导航已触发后成功；预期兼容性失败返回对象而非 reject。
- 异常行为：稳定码 `project_not_found`、`native_create_unavailable`、`composer_unavailable`、`composer_submit_failed`、`session_id_timeout`、`session_unavailable`、`native_navigation_unavailable`、`runtime_replaced`、`native_adapter_failed`；永久 ID 15 秒，open 总等待 5 秒。
- 副作用：start 会切换原生页面并发送指令，非幂等；open 可安全重复；不得拼未知 URL、写 SQLite 或记录 instruction。
- 冻结状态：已冻结
- 锚点：无，本次新建；宿主内部落点由 OPEN-001/T-012 取证。

### CT-005 跨候选库会话聚合 helper

- 契约类型：进程内方法调用
- 来源：自有型
- 生产者：M2 → T-004
- 消费者：M2 → T-004, T-005
- 签名：`aggregate_local_session_catalog(candidate_paths: &[PathBuf]) -> Result<LocalSessionCatalog, LocalSessionCatalogError>`
- 参数约束：输入可空；重复 path 按平台语义去重；只读实际存在文件。
- 返回语义：thread/automation 汇总，更新时间降序、同时间 ID 升序，再按 ID 去重；过滤 archived/空 ID/空 cwd；单库失败聚合 warning，无库成功空 catalog。
- 异常行为：全部实际存在库失败时 `AllExistingDatabasesFailed{count}`；不 panic，不在公开错误中含路径。
- 副作用：只读 SQLite；可记录本机诊断，不写任务文件/SQLite。
- 冻结状态：已冻结
- 锚点：无，本次新建。

## 任务概览

| 任务 | 目标 | 模块 | 波次 | 产出契约 | 消费契约 | 写入范围摘要 |
| --- | --- | --- | --- | --- | --- | --- |
| T-001 | schema v1 与安全快照存储 | M1 | W1 | CT-002, CT-003 | CT-002 | core task_board、paths、lib |
| T-002 | 创建、revision 与幂等 | M1 | W2 | CT-002 | CT-002 | core task_board/create |
| T-003 | 移动与稳定排序 | M1 | W2 | CT-002 | CT-002 | core task_board/move |
| T-004 | 跨 DB 会话聚合 helper | M2 | W1 | CT-005 | CT-005 | data storage/lib |
| T-005 | launcher 真实目录 provider | M2 | W3 | CT-003 | CT-002, CT-003, CT-005 | launcher main |
| T-006 | snapshot/catalog Bridge | M3 | W2 | CT-001, CT-003 | CT-001, CT-002, CT-003 | routes + read handlers |
| T-007 | create Bridge 编排 | M3 | W3 | CT-001 | CT-001, CT-002, CT-003 | create handler |
| T-008 | move Bridge 协议 | M3 | W3 | CT-001 | CT-001, CT-002 | move handler |
| T-009 | Renderer 入口、生命周期、只读视图 | M4 | W1 | CT-001 | CT-001, CT-004 | renderer + cdp tests |
| T-010 | modal 与已有会话创建 | M4 | W2 | CT-001 | CT-001, CT-004 | renderer + cdp tests |
| T-011 | 拖拽与状态菜单 | M4 | W3 | CT-001 | CT-001 | renderer + cdp tests |
| T-012 | 当前 Codex 宿主特征化 | M5 | W1 | 无 | 无 | 无源码写入 |
| T-013 | 原生新会话与恢复 | M5 | W4 | CT-001, CT-004 | CT-001, CT-004 | renderer + cdp tests |
| T-014 | 关联会话原生导航 | M5 | W5 | CT-004 | CT-004 | renderer + cdp tests |

## 执行波次

### W1

- 并行任务：T-001, T-004, T-009, T-012
- 并行依据：
  - 无相互依赖边：四项分别基于既有代码、冻结契约或只读 Debug 事实开工。
  - 写入范围互斥：core task_board、data、Renderer、无源码写入两两分离。
- 进入条件：无（首波次）。

### W2

- 并行任务：T-002, T-003, T-006, T-010
- 并行依据：
  - 无相互依赖边：各自只依赖 W1 产出，不互相等待。
  - 写入范围互斥：create mutation、move mutation、routes、Renderer 文件互斥。
- 进入条件：W1 波次门通过。

### W3

- 并行任务：T-005, T-007, T-008, T-011
- 并行依据：
  - 无相互依赖边：T-005/T-007 只共享冻结 CT-003；T-007/T-008 使用 W2 预留的独立 handler。
  - 写入范围互斥：launcher、create handler、move handler、Renderer 文件互斥。
- 进入条件：W2 波次门通过。

### W4

- 并行任务：T-013
- 并行依据：
  - 无相互依赖边：单任务。
  - 写入范围互斥：单任务；因 `renderer-features.js` 与 T-014 冲突而主动串行。
- 进入条件：W3 波次门通过，且 T-012 已关闭 OPEN-001 的 create 事实。

### W5

- 并行任务：T-014
- 并行依据：
  - 无相互依赖边：单任务。
  - 写入范围互斥：单任务；必须基于 T-013 修改后的同一 runtime。
- 进入条件：W4 波次门通过，且 T-012 已关闭 OPEN-001 的 open 事实。

## 波次门

### W1 波次门

- 验收项：core/data 公共能力可在同一 workspace 被下游引用。
  - 涉及任务：T-001, T-004
  - 涉及契约：CT-002, CT-003, CT-005
  - 通过判据：core/data 同时 check，无依赖环或导出冲突，公共 DTO/helper 可导入。
  - 必需证据：`cargo check -p codex-elves-core -p codex-elves-data` 和两个任务 test target 的成功记录。
- 验收项：Renderer 扩展点与宿主事实可同时交接。
  - 涉及任务：T-009, T-012
  - 涉及契约：CT-004
  - 通过判据：Renderer 只依赖 CT-004，不内嵌未验证 payload；OPEN-001 有当前 Codex 绝对版本/build 的 supported/unsupported 结论。
  - 必需证据：source-contract test + 脱敏特征化记录。
- 不验收：真实 Bridge mutation 与 native adapter → 负责方：W2、W3、W4、W5 波次门。

### W2 波次门

- 验收项：只读看板通过真实 Bridge snapshot/catalog 工作且故障域分离。
  - 涉及任务：T-001, T-006, T-009
  - 涉及契约：CT-001, CT-002, CT-003
  - 通过判据：空/合法/损坏文件与 catalog failure 各自产生约定 UI；一方失败不遮蔽另一方。
  - 必需证据：合并后的 read-route tests 与 Debug 真实 binding 记录。
- 验收项：创建 modal 可在真实只读 runtime 中保持状态。
  - 涉及任务：T-009, T-010
  - 涉及契约：CT-001, CT-004
  - 通过判据：modal 使用真实 catalog state、mock create response，退出/refresh 无残留。
  - 必需证据：filtered cdp tests + Debug DOM/交互记录。
- 不验收：真实 create/move 和原生会话 → 负责方：W3、W4、W5 波次门。

### W3 波次门

- 验收项：已有会话创建从真实 candidate DB 经 launcher/Bridge/store 到 JSON 文件闭合。
  - 涉及任务：T-001, T-002, T-004, T-005, T-006, T-007, T-009, T-010
  - 涉及契约：CT-001, CT-002, CT-003, CT-005
  - 通过判据：同项目一/多会话创建成功；伪造/跨项目/消失会话不写文件；部分 DB failure 可退化；全失败只阻断新建；同 taskId 不重复。
  - 必需证据：`cargo check --workspace`、workspace task_board 定向测试、Debug 真实已有会话创建与脱敏 JSON 检查。
- 验收项：move 在成功、失败和多窗口冲突下闭合。
  - 涉及任务：T-003, T-006, T-008, T-009, T-011
  - 涉及契约：CT-001, CT-002
  - 通过判据：跨列/列内/菜单持久化；失败回滚；同 revision 的第二窗口收到最新快照且无静默覆盖。
  - 必需证据：Debug 拖拽/菜单/双窗口记录和 refresh 后 order/revision。
- 不验收：原生 create/open → 负责方：W4、W5 波次门。

### W4 波次门

- 验收项：新会话模式与真实 Codex 宿主及 create RPC 协同。
  - 涉及任务：T-005, T-007, T-010, T-012, T-013
  - 涉及契约：CT-001, CT-003, CT-004
  - 通过判据：supported 时真实创建/发送/永久 ID 后落盘并验证延迟与恢复；unsupported 时只禁用该模式，已有会话创建不受影响。
  - 必需证据：当前 Codex Debug、sessionStorage 脱敏检查、task JSON、native-create tests。
- 不验收：openSession → 负责方：W5 波次门。

### W5 波次门

- 验收项：关联会话点击进入正确原生会话，失败不破坏任务。
  - 涉及任务：T-009, T-012, T-014
  - 涉及契约：CT-004
  - 通过判据：当前可达导航层级成功；不可用会话显示错误且任务仍在。
  - 必需证据：Debug 真实 session 导航与 open-session tests。
- 验收项：完整发布边界回归。
  - 涉及任务：T-001, T-002, T-003, T-004, T-005, T-006, T-007, T-008, T-009, T-010, T-011, T-013, T-014
  - 涉及契约：CT-001, CT-002, CT-003, CT-004, CT-005
  - 通过判据：三档尺寸、创建、移动、导航、restart persistence、双窗口 conflict、main 替换、reinjection 通过；无新依赖、SQLite 写入或重复 observer。
  - 必需证据：`cargo fmt --check`、`cargo check --workspace`、`cargo test --workspace`、`node --check assets/inject/renderer-features.js`、`git diff --check`、三档 Debug 记录。
- 不验收：NT-001–NT-007 → 负责方：非目标。

## Review 检查点

### R1

- 时机：W1 波次结束后。
- 覆盖范围：T-001、T-004、T-009 的写入范围；T-012 的证据隐私。
- 关注点：
  - schema/cwd 语义、跨平台锁与原子替换。
  - catalog path 隐私和 Renderer host 清理。
  - 特征化不记录敏感文本。
- 不关注：create/move、Bridge 联调、native adapter 实现 → 理由：对应任务尚未进入后续波次。

### R2

- 时机：W3 波次结束后。
- 覆盖范围：T-002、T-003、T-005、T-006、T-007、T-008、T-010、T-011 的写入范围。
- 关注点：
  - CT-001/002/003/005 双端一致性。
  - revision/锁/幂等顺序和 async blocking 边界。
  - Bridge 不信任 Renderer 元数据，冲突只采用服务端快照。
- 不关注：Codex 私有 dispatcher 与 openSession → 理由：T-012–T-014 和后续波次门负责。

### R3

- 时机：W5 波次结束后，兼作最终整体 Review。
- 覆盖范围：T-001–T-014 的写入范围，但只复查跨波次契约与 W4/W5 native 代码。
- 关注点：
  - CT-001–CT-005 实际字段与生产/消费关系。
  - reinjection 资源释放及 instruction/title/DB path 隐私。
  - 无未知 URL、SQLite 写入或非目标回流。
- 不关注：未改动既有实现、纯风格偏好、R1/R2 已关闭且未受影响的局部实现 → 理由：防止重复开放式 Review。

每个检查点先汇总发现后批量修复，只对受影响范围复查一次；超出关注点的事项走偏差处置。

## 未知登记

| ID | 未知内容 | 影响分级 | 关闭方式 | 状态 | 关联 |
| --- | --- | --- | --- | --- | --- |
| OPEN-001 | 当前 Codex 版本中指定项目新会话/首条提交/永久 ID，以及按永久 ID 导航的实际 dispatcher 消息、payload 与 DOM fallback | 阻塞任务 | 前置特征化任务 T-012 | 待关闭；允许结论为 supported 或 unsupported，均不改变 CT-004 | T-012, T-013, T-014 |

## 实施前验证

无（没有不阻塞的 OPEN 或上游 VERIFY；OPEN-001 由独立前置任务 T-012 关闭）。

## 偏差处置

- 计划持有方：主控会话，L2 以上转用户
- 偏差登记落点：`.zeroone/plan/2026-08-24-task-board/deviations.md`
- 适用分级：L0~L3
- 分级规则出处：执行期加载 `zeroone:writing-plans` 的 `DEVIATION-RULES.md`，或由执行层 Skill 提供
- 本计划特殊约定：T-012 得出 supported/unsupported 均属计划内结果；若需要改变 CT-004 的签名、错误码或隐私边界，必须按 L1 处置

## 上游引用

| ID | 类型 | 摘要 | 在本计划中的作用 |
| --- | --- | --- | --- |
| SPEC-001 | 工程设计 | `docs/superpowers/specs/2026-08-24-task-board-design.md` | 交付范围、非目标、Bridge/存储/Renderer/native 设计及验收标准 |
| PROJ-001 | 项目约定 | 根 `AGENTS.md` | 品牌、禁止回流、注入增强、代理与检查边界 |
| PROJ-002 | 贡献约定 | 根 `CONTRIBUTING.md` | Rust 格式、测试和 clippy 约定 |
