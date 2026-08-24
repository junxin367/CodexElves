# 任务

## 任务索引

| 任务 | 目标 | 模块 | 波次 | 问题所有权 |
| --- | --- | --- | --- | --- |
| T-001 | schema v1 与安全快照存储 | M1 | W1 | INV-001 |
| T-002 | 创建、revision 与幂等 | M1 | W2 | INV-002 |
| T-003 | 移动与稳定排序 | M1 | W2 | INV-003 |
| T-004 | 跨候选库会话聚合 helper | M2 | W1 | INV-004 |
| T-005 | launcher 真实会话目录 | M2 | W3 | INV-005 |
| T-006 | snapshot/catalog Bridge 读取链路 | M3 | W2 | INV-006 |
| T-007 | Bridge create 真实性编排 | M3 | W3 | INV-007 |
| T-008 | Bridge move 协议 | M3 | W3 | INV-008 |
| T-009 | Renderer 入口、生命周期与只读视图 | M4 | W1 | INV-009 |
| T-010 | modal 与已有会话创建 | M4 | W2 | INV-010 |
| T-011 | 拖拽与状态菜单 | M4 | W3 | INV-011 |
| T-012 | 当前 Codex 宿主特征化 | M5 | W1 | INV-012 |
| T-013 | 原生新会话与恢复 | M5 | W4 | INV-013 |
| T-014 | 关联会话原生导航 | M5 | W5 | INV-014 |

## T-001 建立 schema v1 与安全快照存储基础

- 所属模块：M1
- 波次：W1
- 单一交付目标：系统可用统一 schema/path 规则读取任务文档；缺失、合法、锁忙和损坏文件分别得到确定快照或 typed error。
- 问题所有者：本任务对 INV-001 负唯一责任
- 前置依赖：无

### 不变量

- INV-001：任何由存储返回或接受的任务文档都满足 schema v1、camelCase、UUID/标题/会话/状态/order/时间戳约束和平台词法 cwd 规范；读取者只看到完整合法快照，缺失文件得到 revision 0 空文档，锁忙/损坏得到显式错误且原文件不被重置。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `crates/codex-elves-core/src/task_board/mod.rs`
  - `crates/codex-elves-core/src/task_board/model.rs`
  - `crates/codex-elves-core/src/task_board/validation.rs`
  - `crates/codex-elves-core/src/task_board/store.rs`
  - `crates/codex-elves-core/src/task_board/create.rs`
  - `crates/codex-elves-core/src/task_board/move_task.rs`
  - `crates/codex-elves-core/src/lib.rs`
  - `crates/codex-elves-core/src/paths.rs`
  - `crates/codex-elves-core/tests/task_board_store.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/settings.rs`
  - `crates/codex-elves-core/src/diagnostic_log.rs`
  - `crates/codex-elves-core/src/routes.rs`
  - `crates/codex-elves-core/Cargo.toml`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | Bridge 由 T-006 负责。 |
| 依赖注入 | 否 | BridgeContext 由 T-006 负责。 |
| 配置文件 | 否 | 不增加设置或开关。 |
| 数据库迁移 | 否 | 不修改 Codex SQLite。 |
| 共享结构 | 是 → `task_board/*`、`lib.rs`、`paths.rs` | 定义 CT-002 和 CT-003 DTO。 |
| 构建/依赖声明 | 否 | 复用现有依赖。 |
| 国际化/文案 | 否 | 只定义领域错误分类。 |

### 契约

- 产出：CT-002, CT-003
- 消费：CT-002

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 应用状态目录 | READ | `crates/codex-elves-core/src/paths.rs:18` | `default_app_state_dir()` | 新 JSON/lock path 通过现有状态目录派生。 |
| 原子写入 | READ | `crates/codex-elves-core/src/settings.rs:1568` | `atomic_write(path: &Path, bytes: &[u8])` | 复用或补强既有同目录临时文件替换。 |
| 有界文件锁参考 | READ | `crates/codex-elves-core/src/diagnostic_log.rs:251` | `lock_log_file(file: &std::fs::File)` | 使用现有 `try_lock_exclusive` 重试风格。 |
| 目录 DTO 接入边界 | READ | `crates/codex-elves-core/src/routes.rs:90` | `BridgeDataService` | CT-003 DTO 需供既有 trait 使用。 |
| 现有依赖 | READ | `crates/codex-elves-core/Cargo.toml:15` | `fs2.workspace = true` | 无需增加第三方 crate。 |

### 实现要点

- 新建 `task_board` 模块，定义 schema v1 DTO、五状态枚举、commands/results/errors 和 `TaskBoardStore` trait。
- 在 `paths.rs` 增加 task-board JSON/lock helper；锁文件独立于数据文件。
- 实现词法 cwd 规范化和完整文档校验；不要求目录存在。
- `FileTaskBoardStore::snapshot` 共享锁等待最多 2 秒；缺失文件不创建；损坏/未知 schema 保持原字节。
- 建立独占锁内重读与原子替换 seam，供 T-002/T-003 的独立 mutation 文件使用；占位 mutation 只返回明确 unavailable，不 panic。

### 验收：本任务验收什么

- 验收项：schema、字段约束和 cwd 规范化构成唯一合法边界（INV-001 / CT-002）。
  - 通过判据：schema 1 往返不丢字段；未知 schema、非法 UUID、空/超长标题、临时/重复会话 ID、跨 cwd、断裂 order、负时间均被拒绝；Windows 与 Unix 路径语义符合设计。
  - 必需证据：`cargo test -p codex-elves-core --test task_board_store -- --test-threads=1` 的模型/规范化用例通过。
- 验收项：快照读取和错误分类符合 CT-002（INV-001 / CT-002）。
  - 通过判据：缺失文件返回 revision 0 且不创建文件；合法文件完整返回；锁超时为 `Busy`；损坏/未知 schema 为带 path 的 `InvalidFile` 且字节不变；Windows 覆盖既有目标的原子替换用例通过。
  - 必需证据：同一 test target 的 snapshot/lock/invalid/atomic 用例及 `cargo check -p codex-elves-core`。

### 验收：本任务不验收什么

- 不验收：CT-002 的创建和移动语义。
  - 负责方：T-002, T-003
- 不验收：CT-003 的 launcher 行为与 Bridge 可调用性。
  - 负责方：T-005, T-006
- 不验收：Renderer 展示、mutation 与 native 交互。
  - 负责方：T-009, T-010, T-011, T-013, T-014
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明模型、路径和只读一致性闭包，并保护既有 Bridge 文件行为。
- 通过判据：task_board_store 与既有 bridge_routes test target 均通过。
- 必需证据：`cargo test -p codex-elves-core --test task_board_store -- --test-threads=1`；`cargo test -p codex-elves-core --test bridge_routes`。
- 来源：SPEC-001 Core 单元测试义务 + 本任务自建。

### 回滚边界

- 删除新 `task_board` 模块、path helper、导出和测试即可；不触及 SQLite、Bridge 路由或 Renderer。

### 待关闭未知

- 无（全部接口形态已冻结）。

## T-002 实现任务创建、revision 与幂等语义

- 所属模块：M1
- 波次：W2
- 单一交付目标：文件存储可原子追加且仅追加一个语义正确的新任务，并正确处理 revision、响应丢失重试和 taskId 冲突。
- 问题所有者：本任务对 INV-002 负唯一责任
- 前置依赖：T-001（事实依赖：必须基于实际领域类型、store seam 与预留 `create.rs` 实现）

### 不变量

- INV-002：一次创建要么原子地产生且仅产生一个新任务并将 revision 加 1，要么返回当前快照/typed error 且不写文件；同 taskId 同语义永不重复创建，同 ID 异语义永不覆盖。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `crates/codex-elves-core/src/task_board/create.rs`
  - `crates/codex-elves-core/tests/task_board_create.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/task_board/model.rs`
  - `crates/codex-elves-core/src/task_board/validation.rs`
  - `crates/codex-elves-core/src/task_board/store.rs`
  - `crates/codex-elves-core/src/settings.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | T-007 负责。 |
| 依赖注入 | 否 | T-006 负责。 |
| 配置文件 | 否 | 无设置项。 |
| 数据库迁移 | 否 | 不写 SQLite。 |
| 共享结构 | 否 | 只实现已冻结 CT-002 行为。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 否 | 错误变体已由 CT-002 定义。 |

### 契约

- 产出：CT-002
- 消费：CT-002

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 幂等与 revision 顺序 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:239` | `task_id_conflict` | 锁内先判 taskId 幂等，再判 expectedRevision。 |
| 原子替换能力 | READ | `crates/codex-elves-core/src/settings.rs:1568` | `atomic_write(path: &Path, bytes: &[u8])` | 真实变化才原子写回。 |

### 实现要点

- 在 T-001 的独占锁 seam 内重读并验证当前文档。
- 以 trim 标题、规范化 cwd、无序 session ID 集合判同语义；标签/会话标题/更新时间不参与幂等比较。
- 新任务进入 `new` 列末尾；真实变化统一生成时间戳、连续 order 和 revision+1。
- 并发测试使用独立线程/进程级 file lock，不用仅进程内 mutex 替代。

### 验收：本任务验收什么

- 验收项：创建 mutation 符合 INV-002 与 CT-002。
  - 通过判据：新任务 order/revision/timestamp 正确；同语义重试 `changed=false,idempotent=true`；异语义 `TaskIdConflict`；旧 revision 带最新文档；两个并发创建无丢失更新。
  - 必需证据：`cargo test -p codex-elves-core --test task_board_create -- --test-threads=1`。

### 验收：本任务不验收什么

- 不验收：会话 ID 是否真实及项目归属。
  - 负责方：T-007
- 不验收：移动和重排。
  - 负责方：T-003
- 不验收：CT-002 的跨任务消费联调。
  - 负责方：W3 波次门
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明成功、冲突、幂等和并发四类创建控制流闭合。
- 通过判据：测试精确断言文件、revision、changed/idempotent 和错误变体。
- 必需证据：`cargo test -p codex-elves-core --test task_board_create -- --test-threads=1`。
- 来源：SPEC-001 Core 创建测试义务。

### 回滚边界

- 只回滚 `create.rs` 与其 test target；T-001 的合法快照读取仍成立。

### 待关闭未知

- 无（创建语义已由 CT-002 冻结）。

## T-003 实现任务移动与稳定排序

- 所属模块：M1
- 波次：W2
- 单一交付目标：跨列移动、列内重排和状态菜单移动以统一索引语义持久化。
- 问题所有者：本任务对 INV-003 负唯一责任
- 前置依赖：T-001（事实依赖：必须基于实际状态类型、store seam 与预留 `move_task.rs` 实现）

### 不变量

- INV-003：移动后源/目标列 order 始终从 0 连续；targetIndex 按移除源任务后的目标列解释；真实变化 revision 恰加 1，无变化不写文件，失败不改变快照。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `crates/codex-elves-core/src/task_board/move_task.rs`
  - `crates/codex-elves-core/tests/task_board_move.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/task_board/model.rs`
  - `crates/codex-elves-core/src/task_board/validation.rs`
  - `crates/codex-elves-core/src/task_board/store.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | T-008 负责。 |
| 依赖注入 | 否 | T-006 负责。 |
| 配置文件 | 否 | 无设置项。 |
| 数据库迁移 | 否 | 不写 SQLite。 |
| 共享结构 | 否 | 只实现 CT-002 move 行为。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 否 | 无 UI 文案。 |

### 契约

- 产出：CT-002
- 消费：CT-002

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| targetIndex 语义 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:341` | `targetIndex` | 先移除源任务，再解释目标列零基插入位。 |

### 实现要点

- 在独占锁内定位任务、移除源项、校验目标 index、插入并只重排受影响列。
- 同状态同位置返回无变化结果；状态菜单传入目标列末尾 index。
- 任何 typed error 前后文档字节与 revision 保持。

### 验收：本任务验收什么

- 验收项：跨列、列内、末尾和无变化移动符合 INV-003（INV-003 / CT-002）。
  - 通过判据：五状态均可到达；越界为 `InvalidInput`、缺失为 `TaskNotFound`、旧 revision 为 `RevisionConflict`；源/目标 order 连续；同位置 `changed=false`。
  - 必需证据：`cargo test -p codex-elves-core --test task_board_move -- --test-threads=1`。

### 验收：本任务不验收什么

- 不验收：Renderer 拖拽命中与菜单交互。
  - 负责方：T-011
- 不验收：Bridge 错误映射。
  - 负责方：T-008
- 不验收：创建幂等。
  - 负责方：T-002
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明每种 move 控制流只产生 CT-002 允许的变化。
- 通过判据：测试逐列断言 status/order/revision 和失败时原文件。
- 必需证据：`cargo test -p codex-elves-core --test task_board_move -- --test-threads=1`。
- 来源：SPEC-001 Core move 测试义务。

### 回滚边界

- 只回滚 `move_task.rs` 与其测试；读取和创建能力保留。

### 待关闭未知

- 无（move 索引和错误语义已冻结）。

## T-004 抽取跨候选库会话聚合 helper

- 所属模块：M2
- 波次：W1
- 单一交付目标：data crate 对任意候选 DB 集合返回唯一 canonical 会话目录或明确全失败。
- 问题所有者：本任务对 INV-004 负唯一责任
- 前置依赖：无

### 不变量

- INV-004：结果只含未归档且 ID/cwd 非空的真实会话，按更新时间保留每个 ID 最新记录；单库失败不丢其余结果且只暴露聚合 warning，全部实际存在库失败才整体失败。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `crates/codex-elves-data/src/storage.rs`
  - `crates/codex-elves-data/src/lib.rs`
  - `crates/codex-elves-data/tests/session_catalog.rs`
- 只读依赖：
  - `crates/codex-elves-data/tests/storage_adapter.rs`
  - `apps/codex-elves-manager/src-tauri/src/commands.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | 无 Bridge 修改。 |
| 依赖注入 | 否 | launcher 由 T-005 接线。 |
| 配置文件 | 否 | 候选 path 由调用方提供。 |
| 数据库迁移 | 否 | 只读现有 schema。 |
| 共享结构 | 是 → `storage.rs`、`lib.rs` | 定义并导出 CT-005。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 否 | warning 使用稳定 code。 |

### 契约

- 产出：CT-005
- 消费：CT-005

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 会话模型 | READ | `crates/codex-elves-data/src/storage.rs:166` | `LocalSession` | 已含目录需要的 id/title/cwd/archive/time。 |
| 单库枚举 | READ | `crates/codex-elves-data/src/storage.rs:208` | `list_local_sessions(&self)` | helper 聚合现有单库读取。 |
| 既有跨 DB 去重 | READ | `apps/codex-elves-manager/src-tauri/src/commands.rs:708` | `list_local_sessions()` | 保持按更新时间、ID 去重行为。 |
| data 导出边界 | READ | `crates/codex-elves-data/src/lib.rs:10` | `pub use storage` | 新 helper 从现有公共入口导出。 |

### 实现要点

- 输入 path 先按平台语义去重，跳过不存在文件且不创建 DB。
- 每库复用 `SQLiteStorageAdapter::list_local_sessions()`；汇总后确定排序并按 ID 去重。
- 单库错误聚合成 `DatabaseReadFailed` count，具体 path 只进诊断；全实际存在库失败返回 typed error。
- 不修改 Manager；用现有 Manager 回归事实校准 helper 语义。

### 验收：本任务验收什么

- 验收项：聚合 helper 完整实现 CT-005（INV-004 / CT-005）。
  - 通过判据：thread/automation、current/legacy、较新胜出、确定排序、过滤、重复 path、无库、单库 warning、全失败均有精确断言且公开 warning 无路径。
  - 必需证据：`cargo test -p codex-elves-data --test session_catalog -- --test-threads=1`。
- 验收项：现有单库 adapter 行为零变更（INV-004）。
  - 通过判据：既有 storage_adapter test target 保持通过。
  - 必需证据：`cargo test -p codex-elves-data --test storage_adapter -- --test-threads=1`。

### 验收：本任务不验收什么

- 不验收：launcher candidate paths 和 spawn_blocking。
  - 负责方：T-005
- 不验收：Bridge catalog JSON 与项目分组。
  - 负责方：T-006
- 不验收：Manager UI/设置或 SQLite 写入。
  - 负责方：非目标（NT-004, NT-005）
- 不验收：Renderer catalog 展示。
  - 负责方：T-009

### 测试义务

- 层级：任务交付验收
- 目标：证明 shared helper 的真实性/退化策略并保护既有 adapter。
- 通过判据：session_catalog 与 storage_adapter 两个 test target 通过。
- 必需证据：上述两条 cargo test。
- 来源：SPEC-001 Data 测试义务 + 既有去重回归。

### 回滚边界

- 回滚 helper、导出和新 test target；不修改 launcher、core 或 Manager。

### 待关闭未知

- 无（单库读取与聚合语义均已取证）。

## T-005 在 launcher 提供真实会话目录

- 所属模块：M2
- 波次：W3
- 单一交付目标：生产 launcher 从所有 candidate DB 异步返回 CT-003 目录，并保持路径隐私和 Tokio worker 安全。
- 问题所有者：本任务对 INV-005 负唯一责任
- 前置依赖：T-001（契约实现依赖 CT-002：需要真实 normalize/DTO）；T-004（契约实现依赖 CT-005：需要真实 helper）；T-006（事实依赖：需要实际 `BridgeDataService` 方法声明）

### 不变量

- INV-005：每次目录请求只使用 launcher 的 candidate paths，在 `spawn_blocking` 中调用 CT-005，并通过 CT-002 规范化 cwd/形成项目；成功、部分失败、全失败均映射为 CT-003，Renderer 永远看不到 DB/rollout path。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `apps/codex-elves-launcher/src/main.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/routes.rs`
  - `crates/codex-elves-core/src/task_board/mod.rs`
  - `crates/codex-elves-data/src/lib.rs`
  - `crates/codex-elves-data/src/storage.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | T-006 注册。 |
| 依赖注入 | 是 → `apps/codex-elves-launcher/src/main.rs` | 实现现有 Bridge data provider。 |
| 配置文件 | 否 | 复用 candidate paths。 |
| 数据库迁移 | 否 | 只读 SQLite。 |
| 共享结构 | 否 | 消费 CT-002/003/005。 |
| 构建/依赖声明 | 否 | 现有依赖足够。 |
| 国际化/文案 | 否 | 错误由 Bridge 映射。 |

### 契约

- 产出：CT-003
- 消费：CT-002, CT-003, CT-005

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| launcher provider | READ | `apps/codex-elves-launcher/src/main.rs:667` | `impl BridgeDataService for LauncherDataService` | 在现有 async trait impl 增加目录方法。 |
| 候选 DB | READ | `apps/codex-elves-launcher/src/main.rs:747` | `candidate_db_paths(&self)` | 必须扫描全部兼容 DB。 |
| trait 边界 | READ | `crates/codex-elves-core/src/routes.rs:90` | `BridgeDataService` | 实现 T-006 增加的 CT-003 方法。 |

### 实现要点

- 克隆 candidate paths 后进入 `tokio::task::spawn_blocking` 调用 CT-005。
- 用 CT-002 normalize 形成 projects；label 回退 cwd basename，sessionCount 精确。
- join failure 或 helper 全失败返回 Err；部分 warning 保留 count，不返回 path。
- 单元测试通过临时 SQLite/可替换路径输入覆盖空、部分、全部失败。

### 验收：本任务验收什么

- 验收项：launcher provider 实现 CT-003（INV-005 / CT-002 / CT-003 / CT-005）。
  - 通过判据：全部 candidate paths 进入 helper；阻塞工作不直接占 async worker；projects/counters 正确；空目录成功；全失败 Err；返回和 warning 无 DB/rollout path。
  - 必需证据：`cargo test -p codex-elves-launcher task_board_session_catalog -- --test-threads=1`；`cargo check -p codex-elves-launcher`。

### 验收：本任务不验收什么

- 不验收：CT-003 的 Bridge JSON 和错误码。
  - 负责方：T-006
- 不验收：task-create 按 ID 重新解析。
  - 负责方：T-007
- 不验收：CT-005 算法。
  - 负责方：T-004
- 不验收：SQLite 写入、Manager 设置和正文。
  - 负责方：非目标（NT-004, NT-005, NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明生产 launcher 接线符合冻结方法契约且不泄露路径。
- 通过判据：定向 launcher tests 和 package check 通过。
- 必需证据：上述两条命令。
- 来源：SPEC-001 Data/launcher 测试义务。

### 回滚边界

- 只回滚 launcher trait 实现和局部 tests；data helper 与 core DTO 保留。

### 待关闭未知

- 无（candidate path 与 async 边界已取证）。

## T-006 接通快照与会话目录 Bridge 读取链路

- 所属模块：M3
- 波次：W2
- 单一交付目标：看板激活所需的 snapshot/catalog 两个只读 RPC 可调用，并为 mutation 路由提供统一 context、错误包络和独立 handler seam。
- 问题所有者：本任务对 INV-006 负唯一责任
- 前置依赖：T-001（契约实现依赖 CT-002/CT-003：BridgeContext 需要真实 store trait、默认 file store 和 DTO）

### 不变量

- INV-006：snapshot 只依赖存储且在目录失败时仍可成功；catalog 只依赖目录能力；两者严格按 CT-001 编解码、阻塞工作有边界、错误码稳定、诊断脱敏，BridgeContext 可注入 store/fake data。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `crates/codex-elves-core/src/routes.rs`
  - `crates/codex-elves-core/src/routes/task_board/mod.rs`
  - `crates/codex-elves-core/src/routes/task_board/snapshot.rs`
  - `crates/codex-elves-core/src/routes/task_board/catalog.rs`
  - `crates/codex-elves-core/src/routes/task_board/create.rs`
  - `crates/codex-elves-core/src/routes/task_board/move_task.rs`
  - `crates/codex-elves-core/tests/task_board_read_routes.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/bridge.rs`
  - `crates/codex-elves-core/src/launcher.rs`
  - `crates/codex-elves-core/src/task_board/mod.rs`
  - `crates/codex-elves-core/tests/bridge_routes.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 是 → `routes.rs` | 一次认领四个 path，create/move 先接独立 handler seam。 |
| 依赖注入 | 是 → `routes.rs` | BridgeContext 增加 store，trait 增加 CT-003。 |
| 配置文件 | 否 | 无开关。 |
| 数据库迁移 | 否 | catalog 通过 service 读取。 |
| 共享结构 | 是 → `routes.rs` | 修改引用型 `BridgeDataService`。 |
| 构建/依赖声明 | 否 | 不新增 transport/dependency。 |
| 国际化/文案 | 是 → `routes/task_board/*` | 稳定错误 message/code。 |

### 契约

- 产出：CT-001, CT-003
- 消费：CT-001, CT-002, CT-003

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| BridgeContext | READ | `crates/codex-elves-core/src/routes.rs:17` | `BridgeContext` | 增加可注入 TaskBoardStore。 |
| data trait | READ | `crates/codex-elves-core/src/routes.rs:90` | `BridgeDataService` | 增加 CT-003 方法和默认 unavailable。 |
| 路由入口 | READ | `crates/codex-elves-core/src/routes.rs:108` | `handle_bridge_request(` | 四个 path 进入现有 match。 |
| wire 包络 | READ | `crates/codex-elves-core/src/bridge.rs:93` | `__codexSessionDeleteBridge = (path, payload)` | 不新建传输层。 |
| 生效接线 | READ | `crates/codex-elves-core/src/launcher.rs:4211` | `handle_bridge_request(ctx, &path, payload)` | 无额外注册表。 |

### 实现要点

- 扩展 BridgeContext 构造器，默认使用 app state dir 的 FileTaskBoardStore；tests 可注入 fake/temp store。
- 在 `BridgeDataService` 增加带默认 unavailable 的 CT-003 方法，保持现有实现可分阶段编译。
- 强类型解析空请求，拒绝未知字段；snapshot 文件 I/O 用 `spawn_blocking`。
- 新 route module 提供共享成功/conflict/failed 编码和 typed error 映射；预留 create/move handler 文件供 T-007/T-008 独占修改。
- 诊断只记录 path/status/code/耗时/计数，不记录 payload 文本。

### 验收：本任务验收什么

- 验收项：snapshot/catalog 服务端形态符合 CT-001（INV-006 / CT-001）。
  - 通过判据：四 path 常量/match 存在；读取请求拒绝未知字段；响应字段精确；catalog 无 DB/rollout path；无需第二注册；诊断脱敏。
  - 必需证据：`cargo test -p codex-elves-core --test task_board_read_routes -- --test-threads=1`。
- 验收项：依赖故障域分离（INV-006 / CT-002 / CT-003）。
  - 通过判据：FakeData 失败时 snapshot 成功；损坏 task file 时 catalog 独立成功；UnavailableDataService 返回目录不可用；temp store 可注入。
  - 必需证据：同一 test target 的独立失败用例。

### 验收：本任务不验收什么

- 不验收：create/move handler 的最终业务行为。
  - 负责方：T-007, T-008
- 不验收：真实 launcher provider。
  - 负责方：T-005
- 不验收：Renderer 跨进程联调。
  - 负责方：W2 波次门
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明读取协议符合冻结契约且 snapshot/catalog 故障域分离。
- 通过判据：task_board_read_routes 与既有 bridge_routes 通过。
- 必需证据：`cargo test -p codex-elves-core --test task_board_read_routes -- --test-threads=1`；`cargo test -p codex-elves-core --test bridge_routes`。
- 来源：SPEC-001 Bridge 读取测试义务 + 既有行为零变更。

### 回滚边界

- 回滚 task-board route module、match arms、context/store 注入和 tests；core store 与 data helper 保留。

### 待关闭未知

- 无（Bridge transport 和 route 生效方式已取证）。

## T-007 实现 Bridge 任务创建与真实性校验

- 所属模块：M3
- 波次：W3
- 单一交付目标：`/task-board/task-create` 只用后端目录中的真实会话创建任务，并稳定处理输入、项目、revision 与幂等错误。
- 问题所有者：本任务对 INV-007 负唯一责任
- 前置依赖：T-006（事实依赖：必须在实际预留 create handler、context 和错误 helper 上实现）

### 不变量

- INV-007：Renderer 只提交 taskId/title/project/sessionIds；Bridge 每次重新取 CT-003、按 ID 解析快照、规范化并验证同项目后才调用 CT-002；任一校验/目录/存储失败都不写任务，响应符合 CT-001。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `crates/codex-elves-core/src/routes/task_board/create.rs`
  - `crates/codex-elves-core/tests/task_board_create_routes.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/routes.rs`
  - `crates/codex-elves-core/src/routes/task_board/mod.rs`
  - `crates/codex-elves-core/src/task_board/mod.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | T-006 已注册；本任务只实现 handler。 |
| 依赖注入 | 否 | 复用 T-006 context。 |
| 配置文件 | 否 | 无设置项。 |
| 数据库迁移 | 否 | 只调用目录 service。 |
| 共享结构 | 否 | 消费冻结 CT。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 是 → create handler | 映射冻结 code/message。 |

### 契约

- 产出：CT-001
- 消费：CT-001, CT-002, CT-003

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 统一 path match | READ | `crates/codex-elves-core/src/routes.rs:124` | `match path` | 在 T-006 分发后的独立 handler 内实现。 |
| create 请求 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:296` | `/task-board/task-create` | 只接受 taskId/revision/title/project/sessionIds。 |
| 错误矩阵 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:317` | `invalid_input` | 按冻结 code 映射可达失败。 |

### 实现要点

- 用 deny-unknown-fields DTO 解析并先做 UUID/title/sessionIds 基础校验。
- 调 CT-003 后按 sessionId 建索引；重新构造会话 snapshot，不信任客户端标题/cwd。
- 用 CT-002 normalize 比较项目；任一缺失/跨项目在 store 调用前失败。
- 调 create_task 后把 mutation result 编为完整 snapshot/conflict/failed。

### 验收：本任务验收什么

- 验收项：create RPC 的真实性边界与错误矩阵闭合（INV-007 / CT-001 / CT-002 / CT-003）。
  - 通过判据：成功返回完整快照；伪造标题/cwd 不入文件；空/重复/临时 ID、缺失、跨项目、目录全失败、revision/taskId conflict、busy/invalid/unavailable 都映射冻结 code；conflict 含最新快照。
  - 必需证据：`cargo test -p codex-elves-core --test task_board_create_routes -- --test-threads=1`。

### 验收：本任务不验收什么

- 不验收：store 幂等与并发内部实现。
  - 负责方：T-002
- 不验收：Renderer 自动重试和 modal 状态。
  - 负责方：T-010, T-013
- 不验收：真实跨进程联调。
  - 负责方：W3 波次门
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明 Bridge 不信任 Renderer 元数据，并覆盖每条可达错误路径。
- 通过判据：fake 覆盖成功、校验失败、目录失败、store conflict/error。
- 必需证据：`cargo test -p codex-elves-core --test task_board_create_routes -- --test-threads=1`。
- 来源：SPEC-001 Bridge create 测试义务。

### 回滚边界

- 回滚 create handler 与 test target；read/move 路由和底层 store 不受影响。

### 待关闭未知

- 无（真实会话校验边界已冻结）。

## T-008 实现 Bridge 任务移动协议

- 所属模块：M3
- 波次：W3
- 单一交付目标：`/task-board/task-move` 精确解析状态/index/revision，调用 CT-002 并返回稳定快照或错误。
- 问题所有者：本任务对 INV-008 负唯一责任
- 前置依赖：T-006（事实依赖：必须在实际预留 move handler、context 和错误 helper 上实现）

### 不变量

- INV-008：任何 move 请求只能按 CT-001 字段/枚举进入 CT-002；成功/无变化返回完整快照，越界、缺失、revision 冲突和文件错误不产生额外变更并使用冻结包络。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `crates/codex-elves-core/src/routes/task_board/move_task.rs`
  - `crates/codex-elves-core/tests/task_board_move_routes.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/routes.rs`
  - `crates/codex-elves-core/src/routes/task_board/mod.rs`
  - `crates/codex-elves-core/src/task_board/mod.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | T-006 已注册。 |
| 依赖注入 | 否 | 复用 context。 |
| 配置文件 | 否 | 无设置项。 |
| 数据库迁移 | 否 | 不访问 SQLite。 |
| 共享结构 | 否 | 消费 CT-001/002。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 是 → move handler | 映射冻结 code/message。 |

### 契约

- 产出：CT-001
- 消费：CT-001, CT-002

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 统一 path match | READ | `crates/codex-elves-core/src/routes.rs:124` | `match path` | 在 T-006 分发后的独立 handler 实现。 |
| move 请求 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:328` | `/task-board/task-move` | 请求字段固定。 |
| index 语义 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:341` | `targetIndex` | 后端拒绝越界并重排。 |

### 实现要点

- 强类型解析 UUID、五状态、非负 index 与 JS-safe expectedRevision。
- 调 CT-002 move_task；不在 Bridge 复制排序算法。
- 将 no-op 成功、真实成功、conflict 和 typed error 编成 CT-001 精确对象。

### 验收：本任务验收什么

- 验收项：move RPC 完整符合 INV-008 与 CT-001。
  - 通过判据：五状态、零/末尾 index、no-op 成功；未知字段/非法枚举/负或越界 index 为 `invalid_input`；缺失为 `task_not_found`；conflict 含最新快照；文件错误稳定映射。
  - 必需证据：`cargo test -p codex-elves-core --test task_board_move_routes -- --test-threads=1`。

### 验收：本任务不验收什么

- 不验收：列重排算法。
  - 负责方：T-003
- 不验收：拖拽、乐观状态和菜单键盘交互。
  - 负责方：T-011
- 不验收：真实 Renderer 联调。
  - 负责方：W3 波次门
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明每种请求和 store 结果只映射到 CT-001 允许的响应。
- 通过判据：测试精确断言 status/code/snapshot 和 fake 调用参数。
- 必需证据：`cargo test -p codex-elves-core --test task_board_move_routes -- --test-threads=1`。
- 来源：SPEC-001 Bridge move 测试义务。

### 回滚边界

- 回滚 move handler 与 test target；create/read 路由保留。

### 待关闭未知

- 无（move 协议已冻结）。

## T-009 实现 Renderer 看板入口、生命周期与只读视图

- 所属模块：M4
- 波次：W1
- 单一交付目标：用户点击“插件”下方的“任务看板”后，在当前原生 `main` 内看到可搜索、可筛选、可滚动的五列快照；离开或 reinjection 时原生内容完整恢复。
- 问题所有者：本任务对 INV-009 负唯一责任
- 前置依赖：无（只依赖冻结 CT-001/CT-004，可用 mock 单方验收）

### 不变量

- INV-009：看板激活时只有一个入口和一个 `main` 直接子根节点，顶栏不被覆盖；最新 snapshot/catalog 投影为固定五列、计数、卡片、搜索/筛选并按容器尺寸可访问；退出、main 替换和 runtime refresh 后无重复节点、监听器、observer 或残留隐藏状态。

### 拆分依据

- 单一“激活态只读投影”不变量；入口、挂载、渲染和恢复由同一 runtime 状态机维护，不能单独发布。

### 写入范围

- 允许修改：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：
  - `crates/codex-elves-core/src/assets.rs`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | 只调用 CT-001。 |
| 依赖注入 | 否 | 使用现有 binding。 |
| 配置文件 | 否 | 功能随 Renderer 增强启用。 |
| 数据库迁移 | 否 | 不直接访问 DB。 |
| 共享结构 | 否 | JS 内部 runtime，不改跨任务契约。 |
| 构建/依赖声明 | 否 | 现有 asset 自动嵌入。 |
| 国际化/文案 | 是 → `renderer-features.js` | 新看板中文文案与 aria-label。 |

### 契约

- 产出：CT-001
- 消费：CT-001, CT-004

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| Bridge helper | READ | `assets/inject/renderer-features.js:4616` | `postJson(path, payload)` | 复用现有 binding 就绪与错误语义。 |
| scan 生命周期 | READ | `assets/inject/renderer-features.js:10630` | `installScanObservers()` | 接入分域 observer，不加全页 observer。 |
| 项目目录 | READ | `assets/inject/renderer-features.js:6932` | `nativeProjectTargets()` | 合并原生项目 label/创建能力。 |
| asset 嵌入 | READ | `crates/codex-elves-core/src/assets.rs:12` | `RENDERER_FEATURES_SCRIPT` | 修改现有 source 即进入注入链。 |
| source-contract 测试 | READ | `crates/codex-elves-core/tests/cdp_bridge.rs:157` | `renderer_features_reuses_scan_observers_when_roots_are_unchanged()` | 扩展现有长期回归入口。 |

### 实现要点

- 在“插件”原生入口后幂等插入任务看板入口，复用原生 row 结构/class/选中态。
- 激活时定位 `main[data-app-shell-main-surface]`，回退 main/role main；根节点直接挂载并通过 host class 隐藏其他直接子项。
- runtime 统一持有 listeners/observer/menu/modal/popover/ResizeObserver，destroy/reinjection 先清理。
- 用 CT-001 mock/真实响应维护最新服务端 snapshot/catalog；catalog failure 不遮蔽任务。
- 实现五列、卡片/会话摘要、搜索/项目筛选、container-width 响应式、低高度和双轴 scroll；会话点击只调用 CT-004。

### 验收：本任务验收什么

- 验收项：入口、main host 与清理生命周期满足 INV-009。
  - 通过判据：入口紧随“插件”且不重复；根为当前 main 直接子；只在 host class 隐藏 siblings；原生导航/main 替换/destroy 恢复内容并清外部节点。
  - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_lifecycle -- --test-threads=1` 和 Debug DOM 记录。
- 验收项：只读投影按 CT-001/CT-004 在三档尺寸可用（INV-009 / CT-001 / CT-004）。
  - 通过判据：五列固定；搜索/筛选准确；会话快照/最新标题/不可用标记正确；1922×1034、996×785、780×400 的工具栏关系、aria-label 与横纵 scroll range 符合设计。
  - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_view -- --test-threads=1`；`node --check assets/inject/renderer-features.js`；mock Debug 三档记录。

### 验收：本任务不验收什么

- 不验收：CT-001 服务端实现。
  - 负责方：T-006, T-007, T-008
- 不验收：modal/create 与 move mutation。
  - 负责方：T-010, T-011
- 不验收：CT-004 真实宿主实现。
  - 负责方：T-013, T-014
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：用 mock Bridge/adapter 证明 Renderer 单方符合冻结契约并保持扫描生命周期零回归。
- 通过判据：两个 filtered cdp tests、JS syntax 和既有 observer 回归通过。
- 必需证据：上述命令及 `cargo test -p codex-elves-core --test cdp_bridge renderer_features_reuses_scan_observers_when_roots_are_unchanged`。
- 来源：SPEC-001 Renderer 自动化/Debug 义务。

### 回滚边界

- 回滚 task-board runtime 块和新增 tests；现有 renderer 增强与原生内容保持。

### 待关闭未知

- 无（此任务只消费冻结 adapter 行为，不需要宿主内部事实）。

## T-010 实现新建任务 modal 与已有会话流程

- 所属模块：M4
- 波次：W2
- 单一交付目标：用户可在响应式 modal 中选择项目和同项目一/多个已有会话，提交创建并正确处理 busy、失败、冲突与刷新。
- 问题所有者：本任务对 INV-010 负唯一责任
- 前置依赖：T-009（事实依赖：必须在实际 runtime state、mount/cleanup 与 render hooks 上扩展）

### 不变量

- INV-010：所有可提交状态都包含合法标题、一个项目和至少一个该项目目录会话；提交只发送 CT-001 字段并以客户端 UUID/当前 revision 建立幂等，任何结果解除 busy，成功采用完整服务端快照，失败保留输入，冲突最多自动重试一次。

### 拆分依据

- 单一表单提交状态机；原生新会话会切页并需要不同恢复边界，故由 T-013 独立负责。

### 写入范围

- 允许修改：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | 只消费 CT-001。 |
| 依赖注入 | 否 | 复用 runtime。 |
| 配置文件 | 否 | 无开关。 |
| 数据库迁移 | 否 | 不直接访问 DB。 |
| 共享结构 | 否 | 扩展同一 Renderer runtime。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 是 → `renderer-features.js` | modal、validation、错误文案。 |

### 契约

- 产出：CT-001
- 消费：CT-001, CT-004

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 原生项目候选 | READ | `assets/inject/renderer-features.js:6932` | `nativeProjectTargets()` | 补充 label/空项目/可创建能力。 |
| modal 可访问性 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:377` | `role="dialog"` | 焦点约束、Escape 和恢复是必需行为。 |
| 工具栏位置 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:371` | `新建任务` | 按钮紧邻项目筛选右侧。 |

### 实现要点

- modal 挂在 main 外，宽 650px，窄屏为 viewport-32px；模式按钮图标+文字左对齐。
- 项目变化清空选择；已有会话列表只展示同规范化 cwd、未归档目录会话。
- taskId 在提交前生成并在 conflict retry 中复用；payload 不含会话 title/cwd。
- 对 create 每个稳定 code 给明确状态；session_not_found 刷新目录；conflict 采用快照并最多重试一次。
- 预留 CT-004 probe/start 调用点；在 T-013 前用 mock/unsupported 结果验收。

### 验收：本任务验收什么

- 验收项：modal 可访问性、同项目选择与 create 状态机满足 INV-010（INV-010 / CT-001 / CT-004）。
  - 通过判据：宽度、dialog/focus/Escape/restore 正确；项目变化不残留跨项目选择；payload 精确；成功及 invalid/session_not_found/project_mismatch/conflict/task_id_conflict/bridge failure 都反馈并解除 busy；conflict 同 ID 最多一次。
  - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_create -- --test-threads=1`；`node --check assets/inject/renderer-features.js`；mock Debug 记录。

### 验收：本任务不验收什么

- 不验收：create 服务端真实性/幂等。
  - 负责方：T-002, T-007
- 不验收：startConversation、永久 ID 与恢复队列。
  - 负责方：T-013
- 不验收：拖拽和状态菜单。
  - 负责方：T-011
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明前端单方按冻结 create/adapter 契约构造请求并闭合 UI 状态。
- 通过判据：mock 覆盖成功、稳定错误、一次冲突重试、busy/focus 清理。
- 必需证据：filtered cdp test + JS syntax。
- 来源：SPEC-001 Renderer create/modal 义务。

### 回滚边界

- 回滚 modal/create 状态机和 tests；只读看板仍可使用。

### 待关闭未知

- 无（真实 native start 由 T-013 在 OPEN-001 关闭后实现）。

## T-011 实现拖拽与状态菜单移动

- 所属模块：M4
- 波次：W3
- 单一交付目标：用户可用拖拽或可访问状态菜单修改任务状态/列内顺序，并在成功、失败和并发冲突下保持一致。
- 问题所有者：本任务对 INV-011 负唯一责任
- 前置依赖：T-010（事实依赖：必须基于含 modal/board state 的最新 Renderer runtime 实现）

### 不变量

- INV-011：每次用户移动只产生一个符合 CT-001 的 `toStatus/targetIndex/expectedRevision`；乐观视图最终由服务端完整快照校正，失败恢复最近服务端快照，revision 冲突采用最新快照且不自动覆盖；菜单键盘和鼠标路径语义一致。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | 只消费 CT-001。 |
| 依赖注入 | 否 | 复用 runtime。 |
| 配置文件 | 否 | 无设置项。 |
| 数据库迁移 | 否 | 不直接访问 DB。 |
| 共享结构 | 否 | 扩展同一 Renderer runtime。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 是 → `renderer-features.js` | 状态菜单、冲突和回滚提示。 |

### 契约

- 产出：CT-001
- 消费：CT-001

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| Bridge 调用 | READ | `assets/inject/renderer-features.js:4616` | `postJson(path, payload)` | move 使用现有 binding。 |
| 拖拽要求 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:421` | `拖拽支持跨列移动和列内重排` | 支持跨列和列内。 |
| 冲突行为 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:425` | `revision` | 采用最新快照并提示重试。 |

### 实现要点

- 从完整服务端 snapshot 维护 draggable task 身份；筛选只改变可见项，不重写持久化 order。
- 目标 index 按移除源任务后的目标列计算；状态菜单默认目标列末尾。
- 乐观更新前保存最近服务端 snapshot；response 总是替换本地，普通失败回滚，conflict 采用返回快照。
- 自定义深色菜单挂 main 外，五项，方向键/Enter/Escape；cleanup 统一移除 drag/menu 状态。

### 验收：本任务验收什么

- 验收项：拖拽、列内重排与五项菜单满足 INV-011（INV-011 / CT-001）。
  - 通过判据：请求 index/revision 精确；菜单到列末尾；键盘完整；成功用响应校正；失败回滚；conflict 用最新快照并提示；所有路径解除 busy/drag；筛选态不改变持久化 order。
  - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_move -- --test-threads=1`；`node --check assets/inject/renderer-features.js`；mock Debug 跨列/列内/菜单记录。

### 验收：本任务不验收什么

- 不验收：后端 index/排序算法。
  - 负责方：T-003, T-008
- 不验收：新建 modal。
  - 负责方：T-010
- 不验收：原生会话适配。
  - 负责方：T-013, T-014
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明前端对每类 move 响应只有 CT-001 允许的状态转移。
- 通过判据：mock 断言请求、成功校正、失败回滚、冲突刷新和菜单可访问性。
- 必需证据：filtered cdp test + JS syntax。
- 来源：SPEC-001 Renderer move 义务。

### 回滚边界

- 回滚 move 交互和 tests；读取与创建流程保留。

### 待关闭未知

- 无（move 前后端契约已冻结）。

## T-012 特征化当前 Codex 原生会话宿主能力

- 所属模块：M5
- 波次：W1
- 单一交付目标：取得当前 Codex Debug 版本“指定项目新会话 + 原生提交 + 永久 ID”及“按永久 ID 导航”的真实可达路径，或可复核地判定能力不可用。
- 问题所有者：本任务对 INV-012 负唯一责任
- 前置依赖：无

### 不变量

- INV-012：后续 adapter 只基于本任务观察到的当前版本 dispatcher/DOM/module 行为实施；证据含版本、触发入口、字段 key/类型、成功/失败/临时到永久 ID 信号，且不记录 instruction、完整标题或会话正文。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - 无（只读取证）
- 只读依赖：
  - `assets/inject/renderer-features.js`
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | 只读宿主。 |
| 依赖注入 | 否 | 不改源码。 |
| 配置文件 | 否 | 不改设置。 |
| 数据库迁移 | 否 | 不写 SQLite。 |
| 共享结构 | 否 | 不改 CT-004。 |
| 构建/依赖声明 | 否 | 无源码写入。 |
| 国际化/文案 | 否 | 只产脱敏证据。 |

### 契约

- 产出：无
- 消费：无

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 原生模块加载 | READ | `assets/inject/renderer-features.js:1721` | `loadCodexAppModule(namePart)` | 复用现有 asset 发现，不另建扫描器。 |
| dispatcher 发现 | READ | `assets/inject/renderer-features.js:1810` | `findCodexServiceTierDispatcher()` | 特征化主动调用能力。 |
| start message 观测 | READ | `assets/inject/renderer-features.js:2677` | `message.type === "start-conversation"` | 已能观测但尚无主动 helper。 |
| 永久会话观察 | READ | `assets/inject/renderer-features.js:4586` | `currentSessionRef()` | 观察临时到永久 ID。 |
| 项目 DOM | READ | `assets/inject/renderer-features.js:6932` | `nativeProjectTargets()` | 指定项目上下文来源。 |

### 实现要点

- 在当前 Codex Debug 记录 app version/build、native module/dispatcher 可达性、项目 start 入口和 composer 路径。
- 对 create 观察从触发到永久 ID 的事件/DOM 序列；对 open 观察 thread row、项目展开和 dispatcher fallback。
- 只记录消息/对象 key 与类型，不记录真实 instruction/title/body；用空敏感文本或测试短语。
- 每条能力输出 supported 的具体路径或 unsupported 的 probe 判据，不猜测未知 payload。

### 验收：本任务验收什么

- 验收项：OPEN-001 被真实证据关闭（INV-012）。
  - 通过判据：create/open 分别记录优先路径、fallback、成功信号和失败；覆盖成功、关键能力/元素缺失、临时 ID 三类。若能力不存在，明确 unsupported 与判定证据。
  - 必需证据：带绝对 Codex version/build 和 2026-08-24 时间戳的脱敏 Debug 记录；字段只列 key/类型。

### 验收：本任务不验收什么

- 不验收：CT-004 adapter 实现。
  - 负责方：T-013, T-014
- 不验收：任务 create Bridge 与恢复队列。
  - 负责方：T-007, T-013
- 不验收：UI 视觉与响应式。
  - 负责方：T-009, T-010
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：以当前实机证据替代对易变 Codex 私有实现的推测。
- 通过判据：create/open 均得到“支持的具体路径”或“可判定 unsupported”。
- 必需证据：脱敏特征化记录。
- 来源：OPEN-001 关闭动作。

### 回滚边界

- 无源码变化；丢弃本次证据即可，不影响其他任务。

### 待关闭未知

- 无（OPEN-001 是本任务要关闭的交付对象，不是本任务开工前提）。

## T-013 实现原生新会话与任务恢复流程

- 所属模块：M5
- 波次：W4
- 单一交付目标：支持时按项目创建原生会话、提交 instruction、等待永久 ID 并创建任务；不支持或中途失败时按设计禁用/恢复且不泄露 instruction。
- 问题所有者：本任务对 INV-013 负唯一责任
- 前置依赖：T-011（事实依赖：必须基于最新完整 Renderer runtime 接入切页/恢复）；T-012（事实依赖：必须使用当前 Codex 真实宿主路径）

### 不变量

- INV-013：只有原生提交成功且得到永久 sessionId 才调用 create RPC；SQLite 延迟仅以相同 taskId 在 10 秒窗口重试，15 秒无永久 ID 不写任务；切页后失败只保存 24 小时、不含 instruction 的恢复元数据并最多自动重试一次；能力不足只禁用新会话模式。

### 拆分依据

- 单一原生创建/恢复不变量；openSession 不发送 instruction 且可独立失败，故由 T-014 负责。

### 写入范围

- 允许修改：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | 调用 CT-001 create。 |
| 依赖注入 | 否 | 复用 Renderer runtime。 |
| 配置文件 | 否 | sessionStorage 是窗口恢复元数据，不是设置。 |
| 数据库迁移 | 否 | 只等待目录可见，不写 DB。 |
| 共享结构 | 否 | 实现冻结 CT-004。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 是 → `renderer-features.js` | 能力不足、阶段失败、恢复提示。 |

### 契约

- 产出：CT-001, CT-004
- 消费：CT-001, CT-004

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| 模块加载 | READ | `assets/inject/renderer-features.js:1721` | `loadCodexAppModule(namePart)` | adapter 复用现有 loader。 |
| dispatcher | READ | `assets/inject/renderer-features.js:1810` | `findCodexServiceTierDispatcher()` | 只采用 T-012 验证的调用形态。 |
| start message | READ | `assets/inject/renderer-features.js:2677` | `message.type === "start-conversation"` | 与既有消息拦截兼容。 |
| session ref | READ | `assets/inject/renderer-features.js:4586` | `currentSessionRef()` | 忽略临时 ID 并等待永久 ID。 |
| 恢复隐私 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:456` | `sessionStorage` | 只保存不含 instruction 的元数据。 |

### 实现要点

- 在 adapter 内实现 probe/start，已知兼容性失败全部返回 CT-004 failed union。
- M4 提交前生成 taskId/revision、锁 busy，在内存保存 instruction；切出看板后触发 T-012 验证的原生流程。
- 等 composer、通过原生输入/发送路径提交；永久 ID phase 最多 15 秒并忽略 `local:client-new-thread:*`。
- 调 create；`session_not_found` 短退避总计 10 秒，revision conflict 同 ID 一次；成功清恢复记录。
- 切页后仍失败时仅保存 taskId/title/project/sessionId/createdAtMs，TTL 24h；下次激活自动重试一次。

### 验收：本任务验收什么

- 验收项：probe/startConversation 实现 CT-004 和 INV-013。
  - 通过判据：unsupported 只禁用新模式；supported 时指定项目、原生提交、忽略临时 ID、永久 ID 后 create；15 秒 timeout、10 秒退避、一次 revision retry、幂等重试可判定。
  - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_native_create -- --test-threads=1`；`node --check assets/inject/renderer-features.js`；当前 Debug supported/unsupported 记录。
- 验收项：恢复和隐私满足 INV-013。
  - 通过判据：sessionStorage 无 firstInstruction；24h 过期清理；下次激活同 ID 一次；诊断无 instruction/完整标题。
  - 必需证据：source-contract 否定断言 + Debug storage/log 检查。

### 验收：本任务不验收什么

- 不验收：create 服务端。
  - 负责方：T-002, T-005, T-007
- 不验收：openSession。
  - 负责方：T-014
- 不验收：宿主事实真实性。
  - 负责方：T-012
- 不验收：NT-001–NT-007。
  - 负责方：非目标（NT-001–NT-007）

### 测试义务

- 层级：任务交付验收
- 目标：证明新会话状态机只在永久 ID 后持久化任务并保护 instruction。
- 通过判据：自动化覆盖 supported/unsupported、临时 ID、timeout、SQLite 延迟、Bridge failure、恢复 TTL；实机覆盖当前版本路径。
- 必需证据：filtered cdp test、JS syntax、Debug 记录。
- 来源：SPEC-001 native create/recovery 义务。

### 回滚边界

- 回滚 adapter create/probe/recovery 与 tests；已有会话创建、移动和只读看板保留，新模式回到 disabled。

### 待关闭未知

- OPEN-001：当前 Codex create/composer/permanent-ID 路径 → 开工前由执行方确认 T-012 的 supported/unsupported 证据已通过 W1 波次门关闭。

## T-014 实现关联会话原生导航

- 所属模块：M5
- 波次：W5
- 单一交付目标：点击关联会话时按分层策略进入正确的 Codex 原生会话，无法导航时保留任务并给出显式错误。
- 问题所有者：本任务对 INV-014 负唯一责任
- 前置依赖：T-013（事实依赖：必须基于其 adapter probe/error/cleanup 形态修改同一 runtime）；T-012（事实依赖：必须使用当前 Codex 真实导航路径）

### 不变量

- INV-014：openSession 依次尝试已挂载 thread 行、展开项目后行、已特征化 dispatcher；任何成功只触发原生导航，所有失败在 5 秒内返回 CT-004 code，不构造未知 URL、不写 SQLite、不删除关联。

### 拆分依据

- 单一不变量，无需逐对判定。

### 写入范围

- 允许修改：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：
  - `docs/superpowers/specs/2026-08-24-task-board-design.md`

### 公共文件核对

| 类别 | 本任务是否涉及 | 说明 |
| --- | --- | --- |
| 路由/端点注册 | 否 | 不调用新 Bridge route。 |
| 依赖注入 | 否 | 扩展 adapter。 |
| 配置文件 | 否 | 无设置。 |
| 数据库迁移 | 否 | 禁止写 SQLite。 |
| 共享结构 | 否 | 实现 CT-004 open。 |
| 构建/依赖声明 | 否 | 无新依赖。 |
| 国际化/文案 | 是 → `renderer-features.js` | session unavailable/navigation failure。 |

### 契约

- 产出：CT-004
- 消费：CT-004

### 事实锚

| 引用对象 | 取证方式 | 锚点 | 原文 token | 结论 |
| --- | --- | --- | --- | --- |
| thread selector | READ | `assets/inject/renderer-features.js:446` | `sidebarThread` | 优先匹配原生 thread 行。 |
| session ID 解析 | READ | `assets/inject/renderer-features.js:4466` | `data-app-action-sidebar-thread-id` | 使用真实永久 ID。 |
| 项目行 | READ | `assets/inject/renderer-features.js:6932` | `nativeProjectTargets()` | 会话行缺失时定位并展开项目。 |
| 导航层级 | READ | `docs/superpowers/specs/2026-08-24-task-board-design.md:481` | `data-app-action-sidebar-thread-id` | 顺序为行、展开、dispatcher fallback。 |

### 实现要点

- 从 board catalog/task snapshot 找 session 对应项目；先查询已挂载 thread row 并原生 click。
- 未挂载时定位原生项目 row、展开并有界等待；仍无行时只用 T-012 验证的 dispatcher。
- 统一 5 秒 deadline 和 CT-004 failure codes；runtime replacement 中止并返回 `runtime_replaced`。
- 不拼 URL、不写 SQLite、不修改或删除 task conversations。

### 验收：本任务验收什么

- 验收项：三层导航和显式失败符合 INV-014/CT-004。
  - 通过判据：直接 row、展开后 row、dispatcher fallback 分别有覆盖；缺失/归档/能力不足返回稳定 code/message；重复打开安全；失败后 task/card/snapshot 仍存在。
  - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_open_session -- --test-threads=1`；`node --check assets/inject/renderer-features.js`；当前 Debug 真实 sessionId 导航。

### 验收：本任务不验收什么

- 不验收：目录中是否仍存在该 ID。
  - 负责方：T-004, T-005
- 不验收：新会话创建/恢复。
  - 负责方：T-013
- 不验收：任务数据删除或自动修复关联。
  - 负责方：非目标（NT-001, NT-002）
- 不验收：CT-004 的 UI 调用联调。
  - 负责方：W5 波次门

### 测试义务

- 层级：任务交付验收
- 目标：证明所有可达导航层级有界、原生且失败不破坏任务。
- 通过判据：自动化覆盖三层和失败，实机点击进入期望 session。
- 必需证据：filtered cdp test、JS syntax、Debug 导航记录。
- 来源：SPEC-001 native navigation 义务。

### 回滚边界

- 回滚 openSession 与 tests；卡片仍显示会话并返回 adapter unavailable，其他看板功能保持。

### 待关闭未知

- OPEN-001：当前 Codex thread-row/project-expand/dispatcher 导航路径 → 开工前由执行方确认 T-012 的 supported/unsupported 证据已通过 W1 波次门关闭。
