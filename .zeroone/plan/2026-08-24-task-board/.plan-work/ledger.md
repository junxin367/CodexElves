# 任务看板实施计划工作台账

## 当前阶段

- 阶段：GATE
- plan-id：`2026-08-24-task-board`
- 计划目录：`.zeroone/plan/2026-08-24-task-board/`
- 需求输入：`docs/superpowers/specs/2026-08-24-task-board-design.md`
- 用户授权：用户明确调用 `$zeroone:writing-plans`，授权创建本计划档案；不授权业务代码修改、分支、commit 或任务执行。

## 输入能力判定

| 需要的能力 | 判定 | 来源或补齐方式 |
| --- | --- | --- |
| 需求边界与验收标准 | 已具备 | 设计稿包含目标、非目标、五列状态、既有/新会话流程、响应式规则、错误处理、测试策略和验收标准；用户以调用 writing-plans 确认进入计划阶段。 |
| 代码事实来源 | 部分具备 | 设计稿记录了初步代码边界，但没有满足本 Skill 的逐项事实锚要求；在 GROUND 和 DECOMPOSE 重新读取真实代码取证。 |
| 外部接口契约 | 部分具备 | 设计稿已给出四个 Bridge 路由、请求字段和主要错误码；在 CONTRACT 结合真实 Bridge 形态冻结。 |
| 模块间接口契约 | 缺失 | 在 CONTRACT 基于真实模块边界定义。 |
| 测试义务 | 已具备目标级输入 | 设计稿给出 Core、Data、Bridge、Renderer 和 Debug 验收方向；在 DECOMPOSE 转成任务级正负闭集。 |
| 实施前验证缺口 | 待取证 | 原生新会话创建和会话导航的当前 Codex dispatcher/DOM 能力需要在 GROUND 判断是否形成 `OPEN-*` 或前置探查任务。 |
| 项目工程约定 | 已加载 | 根 `AGENTS.md` 与 `CONTRIBUTING.md`。 |

## 需求边界三问

1. 范围：新增 Renderer 任务看板、Bridge API、本地 JSON 任务存储、SQLite 会话目录、原生会话创建/导航和响应式 UI；第一阶段非目标已在设计稿冻结。
2. 完成：设计稿“验收标准”及三档 Debug 尺寸、持久化、并发、会话创建/导航条件均可判定。
3. 边界：不得修改 Codex SQLite schema，不增加 Manager 页面，不恢复已移除功能，不破坏 CodexElves 品牌、代理和现有注入增强。

结论：需求边界具备，允许进入 GROUND。

## 项目约定

- 保持 `CodexElves` 品牌和 `codex-elves-*` 路径。
- 不恢复已移除功能。
- 不修改用户无关改动。
- 本计划不创建分支、不 commit、不写业务代码。
- `.zeroone` 写入由用户本轮明确指定的 Skill 授权。
- 正式实现涉及 UI 注入时必须保持现有插件入口及增强功能。

## 事实锚

### READ-001 — Bridge 上下文可注入服务边界

- 命令：`rg -n -C 10 "pub struct BridgeContext|pub fn core_with_data_and_app_dir|pub trait BridgeDataService|pub async fn handle_bridge_request|fn failed_from_error" crates/codex-elves-core/src/routes.rs`
- 锚点：`crates/codex-elves-core/src/routes.rs:17`
- 原文：`pub struct BridgeContext {`
- 结论：BridgeContext 当前持有 settings/runtime/data 三类依赖，任务存储依赖需在此扩展或通过现有 data 边界承载。

### READ-002 — Bridge 数据服务扩展点

- 命令：`rg -n -C 10 "pub struct BridgeContext|pub fn core_with_data_and_app_dir|pub trait BridgeDataService|pub async fn handle_bridge_request|fn failed_from_error" crates/codex-elves-core/src/routes.rs`
- 锚点：`crates/codex-elves-core/src/routes.rs:90`
- 原文：`pub trait BridgeDataService: Send + Sync {`
- 结论：launcher 已通过 async trait 提供数据能力；会话目录可以作为该 trait 的新方法，由 launcher 实现。

### READ-003 — Bridge 单入口路由

- 命令：`rg -n -C 10 "pub struct BridgeContext|pub fn core_with_data_and_app_dir|pub trait BridgeDataService|pub async fn handle_bridge_request|fn failed_from_error" crates/codex-elves-core/src/routes.rs`
- 锚点：`crates/codex-elves-core/src/routes.rs:108`
- 原文：`pub async fn handle_bridge_request(`
- 结论：新任务看板 API 应加入现有 path match 路由，不新建另一套传输通道。

### READ-004 — 应用状态目录

- 命令：`rg -n -C 8 "pub fn default_app_state_dir|pub fn default_settings_path|pub\\(crate\\) fn atomic_write|fn lock_log_file|try_lock_exclusive" crates/codex-elves-core/src/paths.rs crates/codex-elves-core/src/settings.rs crates/codex-elves-core/src/diagnostic_log.rs`
- 锚点：`crates/codex-elves-core/src/paths.rs:18`
- 原文：`pub fn default_app_state_dir() -> PathBuf {`
- 结论：`task-board.json` 与稳定锁文件应通过该状态目录增加专用 path helper。

### READ-005 — 现有原子写入能力

- 命令：`rg -n -C 8 "pub fn default_app_state_dir|pub fn default_settings_path|pub\\(crate\\) fn atomic_write|fn lock_log_file|try_lock_exclusive" crates/codex-elves-core/src/paths.rs crates/codex-elves-core/src/settings.rs crates/codex-elves-core/src/diagnostic_log.rs`
- 锚点：`crates/codex-elves-core/src/settings.rs:1568`
- 原文：`pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {`
- 结论：任务存储可复用该 helper，但正式实现必须补充或验证 Windows 覆盖已有目标文件的行为。

### READ-006 — 有界文件锁参考实现

- 命令：`rg -n -C 8 "pub fn default_app_state_dir|pub fn default_settings_path|pub\\(crate\\) fn atomic_write|fn lock_log_file|try_lock_exclusive" crates/codex-elves-core/src/paths.rs crates/codex-elves-core/src/settings.rs crates/codex-elves-core/src/diagnostic_log.rs`
- 锚点：`crates/codex-elves-core/src/diagnostic_log.rs:251`
- 原文：`fn lock_log_file(file: &std::fs::File) -> std::io::Result<()> {`
- 结论：仓库已有 `try_lock_exclusive` 加短等待重试模式，任务存储可按设计实现 2 秒有界锁等待。

### READ-007 — 本地会话数据模型

- 命令：`rg -n -C 10 "pub struct LocalSession|pub fn list_local_sessions|fn list_codex_threads|fn list_codex_automation_runs" crates/codex-elves-data/src/storage.rs`
- 锚点：`crates/codex-elves-data/src/storage.rs:166`
- 原文：`pub struct LocalSession {`
- 结论：现有模型已包含任务目录需要的 id/title/cwd/archived/updated_at_ms，Renderer 不需要接收 db_path 与 rollout_path。

### READ-008 — SQLite 会话枚举能力

- 命令：`rg -n -C 10 "pub struct LocalSession|pub fn list_local_sessions|fn list_codex_threads|fn list_codex_automation_runs" crates/codex-elves-data/src/storage.rs`
- 锚点：`crates/codex-elves-data/src/storage.rs:208`
- 原文：`pub fn list_local_sessions(&self) -> anyhow::Result<Vec<LocalSession>> {`
- 结论：现有 adapter 同时识别 CodexThreads 与 CodexAutomationRuns，可作为任务会话目录的单库读取能力。

### READ-009 — Launcher 阻塞数据任务模式

- 命令：`rg -n -C 10 "impl BridgeDataService for LauncherDataService|fn candidate_db_paths|fn storage_adapter" apps/codex-elves-launcher/src/main.rs`
- 锚点：`apps/codex-elves-launcher/src/main.rs:667`
- 原文：`impl BridgeDataService for LauncherDataService {`
- 结论：launcher 已使用 `tokio::task::spawn_blocking` 包装 SQLite 工作；任务会话目录实现应保持同一 async 边界。

### READ-010 — 候选数据库路径聚合

- 命令：`rg -n -C 10 "impl BridgeDataService for LauncherDataService|fn candidate_db_paths|fn storage_adapter" apps/codex-elves-launcher/src/main.rs`
- 锚点：`apps/codex-elves-launcher/src/main.rs:747`
- 原文：`fn candidate_db_paths(&self) -> Vec<PathBuf> {`
- 结论：任务会话目录应复用所有候选 DB 路径，而不是只读 current_db_path。

### READ-011 — Renderer Bridge 调用入口

- 命令：`rg -n -C 10 "function postJson|function scanRelevantSelectorForDomain|function installObservers|__codexElvesRefreshRuntime|__codexSessionDeleteObservers" assets/inject/renderer-features.js`
- 锚点：`assets/inject/renderer-features.js:4616`
- 原文：`async function postJson(path, payload) {`
- 结论：任务看板 Renderer 应复用该 bridge helper，并遵循其 bridge 就绪等待与失败语义。

### READ-012 — Renderer 分域扫描与重注入生命周期

- 命令：`rg -n -C 10 "function postJson|function scanRelevantSelectorForDomain|function installObservers|__codexElvesRefreshRuntime|__codexSessionDeleteObservers" assets/inject/renderer-features.js`
- 锚点：`assets/inject/renderer-features.js:10630`
- 原文：`function installScanObservers() {`
- 结论：任务入口与 `main` 生命周期应接入现有分域扫描/observer 体系，并在 runtime refresh 时幂等重建。

### READ-013 — 当前会话识别能力

- 命令：`rg -n -C 10 "function normalizeWorkspacePath|function nativeProjectTargets|data-app-action-sidebar-project-row|function currentSessionRef\\(|function sessionRefFromRow" assets/inject/renderer-features.js`
- 锚点：`assets/inject/renderer-features.js:4586`
- 原文：`function currentSessionRef() {`
- 结论：新会话流程可复用当前会话引用解析，并继续忽略/解析临时会话 ID 的现有规则。

### READ-014 — Renderer 项目路径与原生项目目录

- 命令：`rg -n -C 10 "function normalizeWorkspacePath|function nativeProjectTargets|data-app-action-sidebar-project-row|function currentSessionRef\\(|function sessionRefFromRow" assets/inject/renderer-features.js`
- 锚点：`assets/inject/renderer-features.js:6932`
- 原文：`function nativeProjectTargets() {`
- 结论：Renderer 已能从 `data-app-action-sidebar-project-row` 提取项目 path/label，可与后端会话目录合并。

### READ-015 — Renderer 正式资源嵌入

- 命令：`rg -n -C 8 "renderer-features.js|RENDERER_FEATURES|include_str!|bootstrap_injection" crates/codex-elves-core/src/assets.rs crates/codex-elves-core/tests assets | Select-Object -First 500`
- 锚点：`crates/codex-elves-core/src/assets.rs:12`
- 原文：`const RENDERER_FEATURES_SCRIPT: &str = include_str!("../../../assets/inject/renderer-features.js");`
- 结论：正式任务看板 UI 写入现有 renderer-features 资源即可进入 launcher 注入链，不需要增加独立前端构建产物。

### MISS-001 — 正式任务看板领域代码不存在

- 命令：`rg -n -i -e "task-board" -e "task_board" -e "TaskBoard" -e "任务看板" apps crates assets`
- 范围：`apps crates assets`
- 检索词及变体：`task-board`、`task_board`、`TaskBoard`、`任务看板`
- 处置：转新建 `crates/codex-elves-core/src/task_board.rs` 与 Renderer 任务看板运行时；现有 Debug 原型不在正式源码中。

新建 | 任务看板领域模型和文件存储 | 依据：MISS-001

新建 | Renderer 任务看板运行时 | 依据：MISS-001

### READ-016 — Codex 原生模块加载能力

- 命令：`rg -n -C 10 -e "async function loadCodexAppModule" -e "async function findCodexServiceTierDispatcher" -e 'message.type === "start-conversation"' -e "function findCodexConversationManagerInReactTree" assets/inject/renderer-features.js`
- 锚点：`assets/inject/renderer-features.js:1721`
- 原文：`async function loadCodexAppModule(namePart) {`
- 结论：Renderer 已有按 Codex asset 名动态发现/加载内部模块的兼容层，可由原生会话适配器复用。

### READ-017 — Codex dispatcher 发现能力

- 命令：`rg -n -C 10 -e "async function loadCodexAppModule" -e "async function findCodexServiceTierDispatcher" -e 'message.type === "start-conversation"' -e "function findCodexConversationManagerInReactTree" assets/inject/renderer-features.js`
- 锚点：`assets/inject/renderer-features.js:1810`
- 原文：`async function findCodexServiceTierDispatcher() {`
- 结论：现有代码能跨多个 asset 形态定位带 `dispatchMessage` 的原生 dispatcher，任务看板不得另建不兼容的模块扫描器。

### READ-018 — 原生新会话消息可观测

- 命令：`rg -n -C 10 -e "async function loadCodexAppModule" -e "async function findCodexServiceTierDispatcher" -e 'message.type === "start-conversation"' -e "function findCodexConversationManagerInReactTree" assets/inject/renderer-features.js`
- 锚点：`assets/inject/renderer-features.js:2677`
- 原文：`if (message.type === "start-conversation") {`
- 结论：Renderer 当前会拦截原生新会话消息，但源码中这里只修改请求，不提供可直接调用的任务看板创建函数。

### READ-019 — Renderer source-contract 测试入口

- 命令：`$lines = Get-Content 'crates/codex-elves-core/tests/cdp_bridge.rs'; $lines[154..194] -join "\`n"`
- 锚点：`crates/codex-elves-core/tests/cdp_bridge.rs:157`
- 原文：`fn renderer_features_reuses_scan_observers_when_roots_are_unchanged() {`
- 结论：现有测试通过 `assets::renderer_features_script()` 对注入源码做结构契约断言，任务看板生命周期和响应式规则可在同一测试文件增加长期回归。

### READ-020 — 已有跨 DB 会话去重行为

- 命令：`$lines = Get-Content 'apps/codex-elves-manager/src-tauri/src/commands.rs'; $lines[708..740] -join "\`n"`
- 锚点：`apps/codex-elves-manager/src-tauri/src/commands.rs:708`
- 原文：`pub fn list_local_sessions() -> CommandResult<LocalSessionsPayload> {`
- 结论：Manager 已实现“汇总候选 DB、按 updated_at_ms 排序、按 id 保留最新”的行为；任务会话目录应抽取或复用同等逻辑，避免 launcher 复制另一套规则。

### READ-021 — Data crate 公共导出边界

- 命令：`Get-Content -Raw 'crates/codex-elves-data/src/lib.rs'`
- 锚点：`crates/codex-elves-data/src/lib.rs:10`
- 原文：`pub use storage::{`
- 结论：新的跨路径会话目录聚合函数需要从该公共导出边界暴露给 launcher。

### MISS-002 — 跨候选 DB 的共享会话目录 helper 不存在

- 命令：`rg -n -i -e "list_local_sessions_from_paths" -e "local_session_catalog" -e "session_catalog_from_paths" -e "aggregate_local_sessions" apps crates assets`
- 范围：`apps crates assets`
- 检索词及变体：`list_local_sessions_from_paths`、`local_session_catalog`、`session_catalog_from_paths`、`aggregate_local_sessions`
- 处置：转新建 data crate 聚合 helper，并让 launcher 与可行时的 Manager 复用同一去重规则。

新建 | 跨候选 DB 的会话目录聚合 helper | 依据：MISS-002

### MISS-003 — 可直接复用的任务看板原生会话适配器不存在

- 命令：`rg -n -i -e "startConversation\\(" -e "openSession\\(" -e "navigateToConversation" -e "openConversationById" -e "TaskBoardNativeConversationAdapter" assets/inject`
- 范围：`assets/inject`
- 检索词及变体：`startConversation(`、`openSession(`、`navigateToConversation`、`openConversationById`、`TaskBoardNativeConversationAdapter`
- 处置：转新建 Renderer 原生会话适配器，复用 READ-016/READ-017/READ-018 的底层发现能力。

新建 | Renderer 原生会话创建与导航适配器 | 依据：MISS-003

### MISS-004 — Renderer 全页 `main` host 挂载能力不存在

- 命令：`rg -n -i -e "data-app-shell-main-surface" -e "task-board-main-host" -e "main-host" -e "mount.*main" assets/inject`
- 范围：`assets/inject`
- 检索词及变体：`data-app-shell-main-surface`、`task-board-main-host`、`main-host`、`mount.*main`
- 处置：转新建任务看板专用 `main` host 生命周期；不得用 `body` fixed overlay 替代。

新建 | Renderer 任务看板 `main` host 生命周期 | 依据：MISS-004

### READ-022 — Bridge 无数据服务降级实现

- 命令：`rg -n -C 10 "struct UnavailableDataService|impl BridgeDataService for UnavailableDataService" crates/codex-elves-core/src/routes.rs`
- 锚点：`crates/codex-elves-core/src/routes.rs:502`
- 原文：`impl BridgeDataService for UnavailableDataService {`
- 结论：扩展 BridgeDataService 会话目录方法时必须同步默认不可用实现，保持 core-only BridgeContext 可构造。

### READ-023 — Core 已具备任务存储所需依赖

- 命令：`rg -n -C 5 "fs2.workspace|uuid.workspace|serde.workspace|serde_json.workspace" crates/codex-elves-core/Cargo.toml crates/codex-elves-data/Cargo.toml`
- 锚点：`crates/codex-elves-core/Cargo.toml:15`
- 原文：`fs2.workspace = true`
- 结论：core 已包含 fs2、serde、serde_json、uuid，无需为任务存储引入新的第三方 crate。

### READ-024 — 跨 DB 去重回归测试

- 命令：`rg -n -C 8 "list_local_sessions_deduplicates_threads|list_local_sessions_reads|list_codex_threads" crates/codex-elves-data/src/storage.rs apps/codex-elves-manager/src-tauri/src/commands.rs`
- 锚点：`apps/codex-elves-manager/src-tauri/src/commands.rs:4683`
- 原文：`fn list_local_sessions_deduplicates_threads_across_current_and_legacy_dbs() {`
- 结论：抽取共享聚合 helper 后必须保留“较新 legacy 记录胜出”的既有回归行为。

### READ-025 — 普通 Bridge 路由不受 2 秒后端状态超时限制

- 命令：`rg -n -C 18 "codexBackendBridgeTimeoutMs|bridgeWithBackendTimeout|async function postJson" assets/inject/renderer-features.js`，并补读 `assets/inject/renderer-features.js:4681-4709`
- 锚点：`assets/inject/renderer-features.js:4684`
- 原文：`return await window.__codexSessionDeleteBridge(path, payload);`
- 结论：`codexBackendBridgeTimeoutMs = 2000` 只包裹 `/backend/status` 与 `/backend/repair`；普通任务看板 Bridge 路由直接等待 binding 返回，因此与任务存储最长 2 秒锁等待不冲突，不需要改动全局 `postJson` 超时语义。

### READ-026 — Bridge 线协议包络

- 命令：`rg -n -C 16 "struct Bridge|bridge_payload|bindingCalled|payload.*path|handle\\(path|handler\\(" crates/codex-elves-core/src/bridge.rs`
- 锚点：`crates/codex-elves-core/src/bridge.rs:93`
- 原文：`window.__codexSessionDeleteBridge = (path, payload) => new Promise((resolve) => {{`
- 结论：Renderer 到 launcher 的既有跨进程调用以 `{id, path, payload, generation}` JSON 包络通过 CDP binding 传输；任务看板继续使用 `path` 字符串和 JSON object payload，不新增传输层。

### READ-027 — Bridge 路由无需额外注册表

- 命令：补读 `crates/codex-elves-core/src/launcher.rs:4205-4216` 与 `crates/codex-elves-core/src/routes.rs:108-249`
- 锚点：`crates/codex-elves-core/src/launcher.rs:4211`
- 原文：`async move { Ok(crate::routes::handle_bridge_request(ctx, &path, payload).await) }`
- 结论：已安装的统一 binding 把任意 `path` 交给 `handle_bridge_request`；新增任务路由只需在该函数的 `match path` 认领，不需要维护另一份命令清单或注册表。

### READ-028 — Bridge 返回值编码与 Renderer 解码成对

- 命令：`rg -n -C 14 "resolve_bridge_expression|__codexSessionDeleteResolve|BridgeHandlerCompletion|completion.result" crates/codex-elves-core/src/bridge.rs`
- 锚点：`crates/codex-elves-core/src/bridge.rs:344`
- 原文：`pub fn resolve_bridge_expression(request_id: &str, result: &Value) -> anyhow::Result<String> {`
- 结论：launcher 将 `serde_json::Value` 序列化进 `window.__codexSessionDeleteResolve(id, result)`，Renderer 侧 Promise 原样收到 JSON object；应用错误应通过对象中的 `status/code/message` 表达。

### READ-029 — 任务看板 Bridge 调用值已由设计稿分配

- 命令：补读 `docs/superpowers/specs/2026-08-24-task-board-design.md:245-341`
- 锚点：`docs/superpowers/specs/2026-08-24-task-board-design.md:247`
- 原文：`### \`/task-board/snapshot\``
- 结论：四个新 path 的具体取值、camelCase 请求字段、成功响应及创建/移动语义已由批准设计冻结，可在 CONTRACT 写为自有型跨进程契约。

### READ-030 — BridgeDataService 扩展必须覆盖不可用实现

- 命令：`rg -n -e "pub trait BridgeDataService" -e "struct UnavailableDataService" -e "impl BridgeDataService for UnavailableDataService" crates/codex-elves-core/src/routes.rs`
- 锚点：`crates/codex-elves-core/src/routes.rs:90`
- 原文：`pub trait BridgeDataService: Send + Sync {`
- 结论：会话目录能力是对现有 trait 的引用型扩展；除 launcher 实现外，必须同时提供 `UnavailableDataService` 的失败实现及测试 fake。

### READ-031 — 原生会话适配器的稳定行为边界

- 命令：`rg -n -e "startConversation\\(project, firstInstruction\\)" -e "openSession\\(sessionId\\)" -e "15 秒内" -e "最长 10 秒" -e "24 小时 TTL" -e "优先查找 \`data-app-action-sidebar-thread-id\`" docs/superpowers/specs/2026-08-24-task-board-design.md`
- 锚点：`docs/superpowers/specs/2026-08-24-task-board-design.md:434`
- 原文：`startConversation(project, firstInstruction)`
- 结论：虽然 Codex 私有 dispatcher 细节仍是 OPEN-001，任务看板与适配器之间的能力探测、创建、永久 ID、导航、超时及恢复边界已经足够冻结。

### READ-032 — Workspace 包名与可复核测试入口

- 命令：读取根 `Cargo.toml`、core/data/launcher `Cargo.toml`，并检索 `AGENTS.md`、`CONTRIBUTING.md`、现有 tests。
- 锚点：`Cargo.toml:3`
- 原文：`"crates/codex-elves-core",`
- 结论：任务级证据可使用 `cargo test -p codex-elves-core --test <name>`、`cargo test -p codex-elves-data --test <name>`、`cargo test -p codex-elves-launcher`、`cargo check --workspace`、`cargo fmt --check`、`git diff --check`；Renderer 源码语法可补 `node --check assets/inject/renderer-features.js`。

## OPEN 登记

- OPEN-001
  - 未知内容：当前 Codex 版本中，触发“指定项目新会话 + 提交首条指令”以及按永久 sessionId 导航的最终 dispatcher 消息名、payload 形态和 DOM fallback 组合。
  - 为什么现在取不到：仓库源码只包含原生消息拦截和 dispatcher/module 发现，没有稳定的主动调用 helper；该形态随 Codex asset 版本变化。
  - 已尝试的取证动作：READ-016、READ-017、READ-018；MISS-003 的五变体检索。
  - 影响分级：阻塞任务。
  - 关闭方式：Renderer 任务开工前在当前 Codex Debug 版本做只读/最小调用特征化，冻结适配器内部实现；能力不存在时按设计禁用“创建新会话”，不改变 Bridge 契约。
  - 关联：待 DECOMPOSE 绑定 Renderer 原生会话任务。

## 契约草稿

### 模块边界

#### M1 — 任务看板领域与一致性存储

- 职责：维护 schema v1、字段不变量、路径规范化、稳定锁文件、revision、幂等创建、移动排序、原子替换及文件错误分类。
- 对外接口：CT-002；并为 CT-003 提供共享目录 DTO。
- 依赖：现有应用状态目录、原子写入和诊断能力；不依赖 SQLite 或 Renderer。

#### M2 — 本地会话真实性目录

- 职责：跨候选 Codex SQLite 聚合 thread/automation 会话、去重、过滤、脱敏 warning、项目分组，并在 launcher 中实现会话目录能力。
- 对外接口：CT-003、CT-005。
- 依赖：M1 的共享目录 DTO、现有 `SQLiteStorageAdapter` 与 launcher `candidate_db_paths()`。

#### M3 — 任务看板 Bridge 编排

- 职责：注入任务存储、认领四个 Bridge path、解析请求、从目录解析真实会话、执行项目校验、切换阻塞任务边界、映射稳定错误并记录脱敏诊断。
- 对外接口：CT-001、CT-003。
- 依赖：M1 的 CT-002、M2 的 CT-003、现有统一 Bridge transport。

#### M4 — Renderer 看板运行时

- 职责：侧边栏入口、`main` host 生命周期、五列 UI、响应式与滚动、搜索/筛选、modal/menu/popover、拖拽/状态移动、Bridge 状态机、冲突回滚与恢复队列。
- 对外接口：CT-001、CT-004。
- 依赖：M3 的 Bridge RPC 行为规格、M5 的原生会话适配器行为规格、现有分域扫描和 `postJson`。

#### M5 — Codex 原生会话适配

- 职责：封装当前 Codex 版本的能力探测、指定项目新会话、原生 composer 提交、永久 session ID 观察和原生会话导航；不承载任务文件业务。
- 对外接口：CT-004。
- 依赖：READ-013/014/016/017/018 的既有能力；OPEN-001 只阻塞本模块实施，不改变 CT-004。

### 四类契约面判定

| 类别 | 判定 |
| --- | --- |
| 跨端接口 | 有：Renderer 与 launcher 通过 CT-001 的四个 Bridge RPC 协作。 |
| 跨模块接口 | 有：M3 调用 M1 的 CT-002、M2/M3 共同实现 CT-003，M4 调用 M5 的 CT-004。 |
| 对外部系统接口 | 无跨任务契约：Codex 私有 dispatcher/DOM 属于 M5 内部被封装的宿主适配细节，其他任务只消费 CT-004；OPEN-001 在 M5 任务内关闭。 |
| 公共能力接口 | 有：CT-002、CT-003 与 CT-005 是多个任务依赖的基础能力；不再建立重复 CT。 |

### CT-001 任务看板 Bridge RPC 集

- 契约类型：跨进程调用
- 来源：自有型，复用引用型统一 Bridge transport
- 生产者：M3, M4 → T-006, T-007, T-008, T-009, T-010, T-011, T-013
- 消费者：M3, M4 → T-006, T-007, T-008, T-009, T-010, T-011, T-013
- 调用标识：
  - Rust 端在 `task_board` 模块定义并由 `routes.rs` 使用四个常量；Renderer 在独立 `taskBoardBridgeRoutes` map 中镜像同一字面量，source-contract 测试逐项比对。
  - 具体取值：`/task-board/snapshot`、`/task-board/session-catalog`、`/task-board/task-create`、`/task-board/task-move`。
  - 现有形态锚点：READ-003（`handle_bridge_request`）、READ-026（`path, payload` 包络）、READ-029（新 path 的批准取值）。
- 取值来源：四个值由 `docs/superpowers/specs/2026-08-24-task-board-design.md:247,268,296,328` 分配在 `/task-board/*` 命名空间；既有路由使用 path 字面量匹配，锚点为 `crates/codex-elves-core/src/routes.rs:124` 的 `let result = match path {`。
- 两端认领：
  - M4 编码端调用 `postJson(taskBoardBridgeRoutes.<operation>, payload)`；既有调用锚点 READ-011、READ-025。
  - M3 解码端在 `handle_bridge_request` 的 `match path` 中以对应 Rust 常量认领，并返回 `serde_json::Value`；锚点 READ-003、READ-027。
- 参数传输：
  - 公共线协议为 `{id: string, path: string, payload: object, generation: string}`；`id` 与 `generation` 由既有 transport 生成，任务代码只提供 `path` 和 `payload`。锚点：`crates/codex-elves-core/src/bridge.rs:93-97` 的 `JSON.stringify({{ id, path, payload, generation }})`。
  - `snapshot`：payload 必须是空 object `{}`。
  - `session-catalog`：payload 必须是空 object `{}`。
  - `task-create`：`taskId: string` 必填且为 UUID；`expectedRevision: integer` 必填且范围 `0..=9_007_199_254_740_991`；`title: string` 必填，trim 后 1–120 个 Unicode 字符；`project: {cwd: string, label: string}` 必填，`cwd` trim 后非空，`label` trim 后允许为空并由后端回退到 cwd basename；`sessionIds: string[]` 必填、至少 1 项、同请求内唯一、每项非空且不得为 `local:client-new-thread:*` 临时 ID。字段依据锚点：`docs/superpowers/specs/2026-08-24-task-board-design.md:300-315` 与 `:164-171`。
  - `task-move`：`taskId: string` 必填 UUID；`toStatus: "new" | "planning" | "executing" | "review" | "done"` 必填；`targetIndex: integer` 必填且非负，语义为从源列移除后目标列的零基插入位，允许等于目标列当前长度；`expectedRevision: integer` 必填且范围同上。字段依据锚点：`docs/superpowers/specs/2026-08-24-task-board-design.md:328-341`。
- 返回传输：
  - launcher 编码端把 `serde_json::Value` 写入 `window.__codexSessionDeleteResolve(id, result)`；Renderer 解码端的 Promise 原样得到 object。锚点：READ-028、`crates/codex-elves-core/src/bridge.rs:81-85`。
  - `TaskBoardSnapshot`：`{status:"ok", schemaVersion:1, revision:integer, tasks:TaskBoardTask[]}`。`revision` 使用 JS-safe 非负整数。
  - `TaskBoardTask`：`id:string(UUID)`、`title:string`、`project:{cwd:string,label:string}`、`status` 为五值枚举、`order:integer>=0`、`conversations:TaskBoardConversation[]`、`createdAtMs:integer>=0`、`updatedAtMs:integer>=0`。
  - `TaskBoardConversation`：`sessionId:string`、`title:string`、`cwd:string`、`updatedAtMs:integer|null`；`null` 表示源会话没有可用更新时间。
  - `snapshot` 成功返回 `TaskBoardSnapshot`。
  - `session-catalog` 成功返回 `{status:"ok", projects:[{cwd:string,label:string,sessionCount:integer>=0}], sessions:[{sessionId:string,title:string,cwd:string,updatedAtMs:integer|null}], warnings:[{code:"codex_db_read_failed",count:integer>=1}]}`。不含 `dbPath`、`rolloutPath`、会话正文或错误路径；空候选库和无有效会话均返回三个空数组。字段依据锚点：`docs/superpowers/specs/2026-08-24-task-board-design.md:268-294`。
  - `task-create` 与 `task-move` 成功均返回完整 `TaskBoardSnapshot`，不只返回被改任务。
  - revision 冲突返回 `{status:"conflict", code:"revision_conflict", message:string, schemaVersion:1, revision:integer, tasks:TaskBoardTask[]}`，其中快照是锁内读到的最新文档。
  - 普通失败返回 `{status:"failed", code:string, message:string, path?:string}`；`path` 只允许用于 `task_file_invalid` 或 `task_board_unavailable`，不得用于 SQLite warning/失败。
- 生效登记：不需要额外注册表；统一 binding 已把所有 path 交给 `handle_bridge_request`，新增四个 match arm 即生效。锚点：READ-027、`crates/codex-elves-core/src/bridge.rs:157-180` 的 `Runtime.addBinding`。
- 传输类型构成：所有应用字段使用 camelCase JSON；Rust DTO 使用 serde camelCase；未知字段拒绝还是忽略由强类型请求 DTO 统一采用 `deny_unknown_fields`，避免误拼字段被静默接受；响应只包含上述字段。
- 错误与超时：
  - 应用错误码：`invalid_input`、`session_not_found`、`project_mismatch`、`revision_conflict`、`task_id_conflict`、`task_not_found`、`task_board_busy`、`task_file_invalid`、`task_board_unavailable`、`session_catalog_unavailable`。
  - `snapshot` 只会产生任务文件相关错误；`session-catalog` 全部实际存在候选库失败时返回 `session_catalog_unavailable`；`task-create` 可产生除 `task_not_found` 外的全部相关创建错误；`task-move` 可产生 `invalid_input/task_not_found/revision_conflict` 及任务文件错误。
  - transport 重启或 binding 缺失可能只给 `{status:"failed",message}`；M4 将无稳定 code 的此类结果归一为本地 `bridge_unavailable`，不把它当成后端业务码。锚点：`crates/codex-elves-core/src/bridge.rs:87-91` 与 `assets/inject/renderer-features.js:4638-4639`。
  - 普通任务路由不增加 Renderer 总超时；文件锁等待自身最多 2 秒。锚点 READ-025 与设计稿 `:195-200`。
  - 自动重试只允许两种：创建发生 `revision_conflict` 时使用相同 `taskId` 和最新 revision 自动重试一次；新原生会话遇到 `session_not_found` 时短退避、总窗口最多 10 秒。其他失败不自动重放。
- 边界：
  - `schemaVersion` 第一阶段只接受 `1`；状态列固定五种；所有时间戳为 Unix 毫秒且不超过 JS safe integer。
  - v1 不设置任务总数、单任务会话数或 catalog 数量的额外协议上限；实现不得分页或截断而不告知 Renderer。
  - 既有 Bridge transport 并发上限 8、排队上限 64，任务看板不得绕过或新建第二通道。锚点：`crates/codex-elves-core/src/bridge.rs:19-20`。
- 幂等：
  - `snapshot`、`session-catalog` 为只读幂等。
  - `task-create` 以 `taskId` 为幂等键；同 ID 且 trim 后标题、规范化 cwd、无序 session ID 集合相同则返回当前快照且不增 revision；同 ID 不同语义返回 `task_id_conflict`。
  - `task-move` 的同状态同顺序请求是无变化成功且不增 revision；已成功但响应丢失后用旧 revision 重试可返回 `revision_conflict`，Renderer 采用其最新快照，不盲目重放。
- 冻结状态：已冻结
- 锚点：自有调用值见 READ-029；引用 transport 见 READ-003、READ-011、READ-025、READ-026、READ-027、READ-028。

### CT-002 任务看板一致性存储能力

- 契约类型：进程内方法调用
- 来源：自有型
- 生产者：M1 → T-001, T-002, T-003
- 消费者：M1, M2, M3 → T-001, T-002, T-003, T-005, T-006, T-007, T-008
- 签名：

  ```rust
  pub trait TaskBoardStore: Send + Sync {
      fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError>;
      fn create_task(
          &self,
          command: TaskBoardCreateCommand,
      ) -> Result<TaskBoardMutationResult, TaskBoardStoreError>;
      fn move_task(
          &self,
          command: TaskBoardMoveCommand,
      ) -> Result<TaskBoardMutationResult, TaskBoardStoreError>;
  }
  ```

  `pub fn normalize_task_project_cwd(raw: &str) -> Result<String, TaskBoardValidationError>`；`TaskBoardCreateCommand { task_id: Uuid, expected_revision: u64, title: String, project: TaskBoardProject, conversations: Vec<TaskBoardConversation> }`；`TaskBoardMoveCommand { task_id: Uuid, expected_revision: u64, to_status: TaskBoardStatus, target_index: usize }`；`TaskBoardMutationResult { document: TaskBoardDocument, changed: bool, idempotent: bool }`。
- 参数约束：
  - `TaskBoardDocument/TaskBoardTask/TaskBoardProject/TaskBoardConversation/TaskBoardStatus` 字段与 CT-001 返回 DTO 一致，serde 使用 camelCase；`TaskBoardStatus` JSON 值固定为五值枚举。
  - `create_task` 接收的 conversations 必须已由 M3 从 CT-003 解析为真实快照，但 M1 仍独立验证 UUID、标题、永久且唯一 session ID、统一规范化 cwd、时间戳和连续 order 不变量。
  - `move_task.target_index` 按“先从源列移除，再对目标列插入”解释；越界为输入错误。
- 返回语义：
  - `snapshot`：文件不存在时返回 schema 1/revision 0/空 tasks，且不创建文件。
  - 真实创建或移动：`changed=true`，revision 恰好加 1，返回写入后的完整文档。
  - 同语义任务 ID 重试：`changed=false,idempotent=true`，返回当前文档，不写文件、不增 revision。
  - 无状态/顺序变化的移动：`changed=false,idempotent=false`，返回当前文档，不写文件、不增 revision。
- 异常行为：
  - 不 panic；错误只通过 `TaskBoardStoreError` 返回。
  - 稳定变体：`Busy`、`InvalidFile { path, message }`、`InvalidInput { message }`、`RevisionConflict { current }`、`TaskIdConflict`、`TaskNotFound`、`Unavailable { path, message }`。
  - 读取到损坏 JSON、未知 schema 或非法模型返回 `InvalidFile` 并保持原文件；revision 不匹配返回携带最新文档的 `RevisionConflict`。
- 副作用：
  - `snapshot` 只取共享锁并读取，不创建数据文件。
  - `create_task/move_task` 在稳定 lock 文件的独占锁内重读、校验、变更、原子替换；只有 `changed=true` 才写文件。
  - 所有方法是同步阻塞文件操作；M3 必须通过 `spawn_blocking` 调用，不得占用 Tokio worker。
- 冻结状态：已冻结
- 锚点：无，本次新建；行为依据 MISS-001 与批准设计稿的持久化/并发章节。

### CT-003 本地任务会话目录能力

- 契约类型：进程内方法调用
- 来源：引用型，扩展现有 `BridgeDataService`
- 生产者：M1, M2, M3 → T-001, T-005, T-006
- 消费者：M2, M3 → T-005, T-006, T-007
- 签名：

  ```rust
  #[async_trait]
  pub trait BridgeDataService: Send + Sync {
      async fn task_board_session_catalog(
          &self,
      ) -> anyhow::Result<TaskBoardSessionCatalog>;
  }
  ```

  M1 持有返回 DTO；M3 在既有 trait 中声明方法并调用；M2 在 launcher 中实现真实行为。现有 trait 其余方法签名不变。
- 参数约束：无参数；不接受 Renderer 提交的数据库路径、标题或 cwd。候选库只来自 launcher `candidate_db_paths()`。
- 返回语义：
  - `TaskBoardSessionCatalog { projects: Vec<TaskBoardCatalogProject>, sessions: Vec<TaskBoardCatalogSession>, warnings: Vec<TaskBoardCatalogWarning> }`。
  - `TaskBoardCatalogProject { cwd: String, label: String, session_count: u32 }`；`TaskBoardCatalogSession { session_id: String, title: String, cwd: String, updated_at_ms: Option<i64> }`；`TaskBoardCatalogWarning { code: TaskBoardCatalogWarningCode::CodexDbReadFailed, count: u32 }`。
  - 聚合全部候选库，thread 与 automation 均纳入；按 `updated_at_ms` 降序后按真实 ID 保留最新；排除 archived、空 ID、空 cwd；cwd 使用 M1 的规范化规则。
  - 单库失败时返回其余结果和聚合 warning；候选库均不存在或可读但无有效会话时返回成功空目录；全部实际存在候选库均失败时返回 `Err`。
- 异常行为：
  - 真实 launcher 实现仅在“全部实际存在候选库均无法读取”或阻塞任务 join 失败时返回 `anyhow::Error`；M3 统一映射为 `session_catalog_unavailable`。
  - `UnavailableDataService` 返回明确 `Err`；测试 fake 必须可配置成功目录和失败。
- 副作用：只读 SQLite；允许写脱敏诊断日志，但不得写 SQLite、任务文件或暴露 DB path/rollout path。
- 冻结状态：已冻结
- 锚点：READ-002、READ-007、READ-008、READ-009、READ-010、READ-020、READ-021、READ-022、READ-024、READ-030。

### CT-004 Renderer 原生会话适配器

- 契约类型：进程内方法调用
- 来源：自有型
- 生产者：M5 → T-013, T-014
- 消费者：M4, M5 → T-009, T-010, T-013, T-014
- 签名：

  ```text
  TaskBoardNativeConversationAdapter.probe(project)
    -> Promise<{status:"ok", canStart:boolean, canOpen:boolean, code:string|null, message:string}>

  TaskBoardNativeConversationAdapter.startConversation(project, firstInstruction)
    -> Promise<
         {status:"ok", sessionId:string, title:string, cwd:string}
         | {status:"failed", code:string, message:string}
       >

  TaskBoardNativeConversationAdapter.openSession(sessionId)
    -> Promise<
         {status:"ok"}
         | {status:"failed", code:string, message:string}
       >
  ```
- 参数约束：
  - `project` 固定为 `{cwd:string,label:string}`；cwd trim 后非空并按 Renderer 的词法规则规范化，仅用于宿主匹配，最终项目真实性仍由 M3 校验。
  - `firstInstruction` trim 后非空，只在内存中传给原生 composer；不得写入 sessionStorage、任务文件或诊断日志。
  - `sessionId` 必须非空；`local:client-new-thread:*` 只可作为内部观察态，不可作为成功返回值。
- 返回语义：
  - `probe` 不触发导航或发消息；`canStart=false` 只禁用创建新会话模式，不能阻断绑定已有会话；`code=null` 表示能力完整。
  - `startConversation` 只有在首条指令已通过原生路径提交且观察到永久 session ID 后返回成功；返回 cwd 是规范化项目 cwd。
  - `openSession` 只有在原生行点击或已探测 dispatcher 导航已触发后返回成功；会话不可用时任务数据保持不变。
  - 预期失败均归一为返回对象，Promise 不以已知兼容性失败 reject。
- 异常行为：
  - 稳定失败码：`project_not_found`、`native_create_unavailable`、`composer_unavailable`、`composer_submit_failed`、`session_id_timeout`、`session_unavailable`、`native_navigation_unavailable`、`runtime_replaced`、`native_adapter_failed`。
  - 永久 ID 等待上限 15 秒；`openSession` 的展开/导航等待总计不超过 5 秒。未知异常在适配器边界捕获并转成 `native_adapter_failed`。
- 副作用：
  - `probe` 只读 DOM/模块能力。
  - `startConversation` 会卸载或离开看板、启动指定项目原生会话并发送首条指令；重复调用不是幂等的，M4 必须用 busy 状态阻止重复提交。
  - `openSession` 会切换 Codex 原生会话；重复打开同一会话可安全重复。
  - 不自行拼接未经验证 URL，不写 Codex SQLite，不记录指令正文。
- 冻结状态：已冻结
- 锚点：无，本次新建；稳定行为依据 READ-013、READ-014、READ-016、READ-017、READ-018、READ-031。OPEN-001 仅决定 M5 内部采用哪条已验证宿主路径。

### CT-005 跨候选库会话聚合 helper

- 契约类型：进程内方法调用
- 来源：自有型
- 生产者：M2 → T-004
- 消费者：M2 → T-004, T-005
- 签名：

  ```rust
  pub fn aggregate_local_session_catalog(
      candidate_paths: &[PathBuf],
  ) -> Result<LocalSessionCatalog, LocalSessionCatalogError>;
  ```

  `LocalSessionCatalog { sessions: Vec<LocalSession>, warnings: Vec<LocalSessionCatalogWarning> }`；`LocalSessionCatalogWarning { code: LocalSessionCatalogWarningCode::DatabaseReadFailed, count: u32 }`；`LocalSessionCatalogError::AllExistingDatabasesFailed { count: u32 }`。
- 参数约束：输入可为空；路径顺序不具业务语义；重复路径先按平台路径比较去重；只读实际存在的文件，不创建缺失数据库。
- 返回语义：
  - 读取每个候选库的 thread/automation；按 `updated_at_ms` 降序、同时间按 session ID 升序形成确定顺序，再按真实 ID 保留第一条。
  - 排除 archived、空 ID、空 cwd；保留 `LocalSession` 现有字段供 launcher 映射。
  - 单库失败返回其余结果和一条聚合 warning；没有实际存在的库或可读但无有效会话时成功返回空 catalog。
- 异常行为：只有全部实际存在候选库均读取失败时返回 `AllExistingDatabasesFailed`；不 panic，不把具体路径放进公开错误结构。
- 副作用：只读 SQLite；可以通过既有诊断能力记录具体本机路径，但 helper 返回值不得含 DB path warning 细节，也不得写任务文件或 Codex SQLite。
- 冻结状态：已冻结
- 锚点：无，本次新建；去重行为来源 READ-020/READ-024，单库能力来源 READ-007/READ-008。

### CONTRACT 出站自检

1. 四类契约面均已判定；对外部系统没有跨任务契约的理由已记录。
2. CT-001 按跨进程调用完整定义调用值、线协议、两端认领、参数/响应、登记、错误、超时和幂等。
3. CT-002/003/004/005 按进程内调用完整定义签名、参数、返回、异常和副作用。
4. 自有型与引用型已区分；所有引用型事实均有 READ 锚。
5. 生产者/消费者仅绑定到 M1–M5，尚未抢跑写 T-*。
6. 五条契约均已冻结；OPEN-001 为 M5 的阻塞任务事实，不是阻塞契约。
7. 模块按领域职责划分，不按 Controller/Service/UI 技术层机械切分。

## 计划非目标编号

- NT-001：任务编辑、删除、归档、批量操作。
- NT-002：根据会话内容自动推断或修改任务状态。
- NT-003：跨设备、云端或账号级同步。
- NT-004：修改 Codex SQLite schema 或写入任务字段。
- NT-005：新增 Manager 页面、设置项或功能开关。
- NT-006：一个任务跨项目关联会话。
- NT-007：在看板展示或搜索会话正文。

## 任务拆分

### T-001 建立 schema v1 与安全快照存储基础

- 所属模块：M1
- 单一交付目标：提供可导出的任务看板领域模型、路径规范化、文件路径 helper 和可安全读取的 `FileTaskBoardStore`，让缺失/合法/损坏文档都有确定结果。
- 唯一问题所有者：任务文档身份、模型完整性和读取一致性。
- 不变量：
  - INV-001：任何由存储返回或接受的任务文档都满足 schema v1、camelCase、UUID/标题/会话/状态/order/时间戳约束及平台词法 cwd 规范；读取者只看到完整合法快照，文件缺失得到 revision 0 空文档，锁忙或文件损坏得到显式 typed error，原文件不被重置。
- 拆分依据：单一不变量，无需逐对判定；创建幂等和移动排序分别由 T-002、T-003 维护，可独立失败与回滚，故不并入。
- 写入范围：
  - `crates/codex-elves-core/src/task_board/mod.rs`
  - `crates/codex-elves-core/src/task_board/model.rs`
  - `crates/codex-elves-core/src/task_board/validation.rs`
  - `crates/codex-elves-core/src/task_board/store.rs`
  - `crates/codex-elves-core/src/task_board/create.rs`
  - `crates/codex-elves-core/src/task_board/move_task.rs`
  - `crates/codex-elves-core/src/lib.rs`
  - `crates/codex-elves-core/src/paths.rs`
  - `crates/codex-elves-core/tests/task_board_store.rs`
- 只读依赖：READ-004/005/006/023、设计稿持久化模型与路径规范化章节。
- 契约：
  - 产出：CT-002, CT-003
  - 消费：CT-002
- 正面验收：
  - 验收项：schema、字段约束与 cwd 规范化构成唯一合法文档边界（INV-001, CT-002）。
    - 通过判据：schema 1 往返不丢字段；未知 schema、非法 UUID、空/超长标题、临时/重复会话 ID、跨 cwd、断裂 order、负时间均被拒绝；Windows 盘符/分隔符/大小写及 Unix 大小写语义符合设计。
    - 必需证据：`cargo test -p codex-elves-core --test task_board_store -- --test-threads=1` 中对应模型/规范化用例全部通过。
  - 验收项：快照读取和错误分类符合 CT-002（INV-001, CT-002）。
    - 通过判据：缺失文件返回 revision 0 且不创建文件；合法文件返回完整快照；共享锁超过 2 秒返回 `Busy`；损坏 JSON/未知 schema 返回带路径 `InvalidFile` 且文件字节不变；Windows 覆盖既有目标的原子替换测试通过。
    - 必需证据：同一 test target 的 snapshot/lock/invalid/atomic 用例通过，且 `cargo check -p codex-elves-core` 成功。
- 负面验收：
  - 不验收：CT-002 的创建与移动语义 → 负责方：T-002, T-003。
  - 不验收：CT-003 的 launcher 行为与 Bridge 可调用性 → 负责方：T-005, T-006。
  - 不验收：Renderer 展示和交互 → 负责方：T-009, T-010, T-011, T-013, T-014。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明模型、路径和只读一致性闭包成立，并保持现有 settings/diagnostic 文件行为零变更。
  - 通过判据：上述 test target 与 `cargo test -p codex-elves-core --test bridge_routes` 均通过。
  - 必需证据：两条命令的成功输出。
  - 来源：设计稿 Core 单元测试义务 + 本任务自建。
- 独立回滚边界：删除新 `task_board` 模块、path helper、导出和对应测试即可；不触及 SQLite、Bridge 路由或 Renderer。

### T-002 实现任务创建、revision 与幂等语义

- 所属模块：M1
- 单一交付目标：让文件存储在独占锁内可靠追加任务，并正确处理 revision 冲突、响应丢失重试和 task ID 冲突。
- 唯一问题所有者：创建任务的持久化状态机。
- 不变量：
  - INV-002：一次创建要么原子地产生且仅产生一个语义正确的新任务并将 revision 加 1，要么返回当前快照/typed error 且不写文件；同 taskId 同语义重试永不重复创建，同 ID 异语义永不覆盖。
- 拆分依据：单一不变量，无需逐对判定；移动顺序可在创建完全正确时独立失败，故由 T-003 负责。
- 写入范围：
  - `crates/codex-elves-core/src/task_board/create.rs`
  - `crates/codex-elves-core/tests/task_board_create.rs`
- 只读依赖：T-001 产出的 `model/validation/store`、CT-002、设计稿幂等与写入顺序。
- 契约：
  - 产出：CT-002
  - 消费：CT-002
- 正面验收：
  - 验收项：创建 mutation 符合 INV-002 与 CT-002。
    - 通过判据：新任务进入 `new` 列末尾、order 连续、时间戳非负、revision 恰加 1；先检查 taskId 幂等再检查 revision；同语义重试 `changed=false,idempotent=true`；异语义返回 `TaskIdConflict`；旧 revision 返回含最新文档的 `RevisionConflict`；两并发创建无丢失更新。
    - 必需证据：`cargo test -p codex-elves-core --test task_board_create -- --test-threads=1` 全部通过。
- 负面验收：
  - 不验收：会话 ID 是否真实及项目归属 → 负责方：T-007。
  - 不验收：移动与重排 → 负责方：T-003。
  - 不验收：CT-002 消费方联调 → 负责方：W3 波次门。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明创建的成功、冲突、幂等、并发四类控制流闭合。
  - 通过判据：测试精确断言文件内容、revision、changed/idempotent 和错误变体。
  - 必需证据：上述 test target 成功。
  - 来源：设计稿 Core 单元测试义务。
- 独立回滚边界：只回滚 `create.rs` 和其 test target，T-001 的合法快照读取仍成立。

### T-003 实现任务移动与稳定排序

- 所属模块：M1
- 单一交付目标：让跨列移动、列内重排和状态菜单移动以统一索引语义持久化。
- 唯一问题所有者：任务状态与列内顺序 mutation。
- 不变量：
  - INV-003：移动后源/目标列 order 始终从 0 连续；目标索引按移除源任务后的目标列解释；真实变化 revision 恰加 1，无变化不写文件、不增 revision，失败不改变快照。
- 拆分依据：单一不变量，无需逐对判定。
- 写入范围：
  - `crates/codex-elves-core/src/task_board/move_task.rs`
  - `crates/codex-elves-core/tests/task_board_move.rs`
- 只读依赖：T-001 store/model、CT-002、设计稿移动语义。
- 契约：
  - 产出：CT-002
  - 消费：CT-002
- 正面验收：
  - 验收项：跨列、列内、末尾和无变化移动符合 INV-003（INV-003, CT-002）。
    - 通过判据：五状态均可到达；越界 `InvalidInput`、缺失任务 `TaskNotFound`、旧 revision `RevisionConflict`；源/目标 order 连续；同位置移动返回 `changed=false`。
    - 必需证据：`cargo test -p codex-elves-core --test task_board_move -- --test-threads=1` 全部通过。
- 负面验收：
  - 不验收：Renderer 拖拽坐标与菜单交互 → 负责方：T-011。
  - 不验收：Bridge 错误映射与 JSON 响应 → 负责方：T-008。
  - 不验收：创建幂等 → 负责方：T-002。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明每种移动控制流只产生契约允许的文档变化。
  - 通过判据：测试逐列断言 status/order/revision 与原文件保持。
  - 必需证据：上述 test target 成功。
  - 来源：设计稿 Core 单元测试义务。
- 独立回滚边界：只回滚 `move_task.rs` 和其测试，读取与创建仍可保留。

### T-004 抽取跨候选库会话聚合 helper

- 所属模块：M2
- 单一交付目标：在 data crate 提供唯一的跨 DB 会话聚合、去重、过滤和部分失败降级实现。
- 唯一问题所有者：候选 SQLite 集合到 canonical `LocalSessionCatalog` 的转换。
- 不变量：
  - INV-004：对任意候选路径集合，结果只含未归档且 ID/cwd 非空的真实会话，按更新时间保留每个 ID 最新记录；单库失败不丢其余结果且只暴露聚合 warning，全部实际存在库失败才整体失败。
- 拆分依据：单一不变量，无需逐对判定；launcher async/DTO 映射即使 helper 正确也可独立失败，故拆为 T-005。
- 写入范围：
  - `crates/codex-elves-data/src/storage.rs`
  - `crates/codex-elves-data/src/lib.rs`
  - `crates/codex-elves-data/tests/session_catalog.rs`
- 只读依赖：READ-007/008/020/021/024、现有 `storage_adapter.rs` fixtures。
- 契约：
  - 产出：CT-005
  - 消费：CT-005
- 正面验收：
  - 验收项：聚合 helper 完整实现 CT-005（INV-004, CT-005）。
    - 通过判据：thread/automation、跨 current/legacy DB、较新记录胜出、确定排序、过滤 archived/空字段、重复 path、无库空成功、单库 warning、全失败 typed error 均有断言；返回 warning 无路径。
    - 必需证据：`cargo test -p codex-elves-data --test session_catalog -- --test-threads=1` 全部通过。
  - 验收项：现有单库 adapter 行为零变更（INV-004）。
    - 通过判据：现有 storage adapter 测试保持通过。
    - 必需证据：`cargo test -p codex-elves-data --test storage_adapter -- --test-threads=1`。
- 负面验收：
  - 不验收：launcher `candidate_db_paths()` 与 spawn_blocking 接入 → 负责方：T-005。
  - 不验收：Bridge catalog JSON 与项目分组 → 负责方：T-006。
  - 不验收：Manager UI 或设置 → 负责方：非目标 NT-005。
  - 不验收：Codex SQLite 写入/schema 改动 → 负责方：非目标 NT-004。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明 shared helper 的数据真实性和退化策略，并保护既有 adapter。
  - 通过判据：两个 data test target 成功。
  - 必需证据：上述命令输出。
  - 来源：设计稿 Data 测试义务 + READ-024 回归。
- 独立回滚边界：回滚 helper、导出和新测试，不修改 launcher 或 core 接口。

### T-005 在 launcher 提供真实会话目录

- 所属模块：M2
- 单一交付目标：让生产 launcher 的 `BridgeDataService` 从所有候选 DB 异步返回 CT-003 目录，并保持隐私和 async worker 安全。
- 唯一问题所有者：launcher 候选库选择、阻塞边界和 core DTO 映射。
- 不变量：
  - INV-005：每次目录请求只使用 launcher 自己的 candidate paths，在 `spawn_blocking` 中调用 CT-005，并通过 CT-002 规范化 cwd/形成项目；成功/部分失败/全失败均映射为 CT-003，Renderer 永远看不到 DB/rollout path。
- 拆分依据：单一不变量，无需逐对判定。
- 写入范围：
  - `apps/codex-elves-launcher/src/main.rs`
- 只读依赖：T-001 的目录 DTO/normalize、T-004 的 helper、T-006 声明后的 `BridgeDataService`、READ-009/010。
- 契约：
  - 产出：CT-003
  - 消费：CT-002, CT-003, CT-005
- 正面验收：
  - 验收项：launcher provider 完整实现 CT-003（INV-005, CT-002, CT-003, CT-005）。
    - 通过判据：所有 candidate paths 传入 helper；阻塞工作不在 async worker 直接执行；projects 按规范化 cwd 聚合且 sessionCount 正确；空目录成功；全失败返回 Err；warning 和响应对象均不含本机 DB/rollout path。
    - 必需证据：`cargo test -p codex-elves-launcher task_board_session_catalog -- --test-threads=1` 全部通过。
- 负面验收：
  - 不验收：CT-003 的 Bridge 路由序列化与错误码 → 负责方：T-006。
  - 不验收：任务创建时按 ID 重新解析会话 → 负责方：T-007。
  - 不验收：CT-005 聚合算法正确性 → 负责方：T-004。
  - 不验收：NT-004/NT-005/NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明真实 launcher 接线符合冻结方法契约且不泄露路径。
  - 通过判据：launcher 定向测试与 `cargo check -p codex-elves-launcher` 成功。
  - 必需证据：两条命令输出。
  - 来源：设计稿 Data 与 launcher 测试义务。
- 独立回滚边界：只回滚 launcher trait 实现和局部测试；data helper 与 core DTO 保留。

### T-006 接通快照与会话目录 Bridge 读取链路

- 所属模块：M3
- 单一交付目标：让看板激活所需的 snapshot/catalog 两个只读 RPC 可调用，并为后续 mutation 路由提供统一上下文、错误包络和 handler 扩展点。
- 唯一问题所有者：任务看板 Bridge 读取协议与依赖注入。
- 不变量：
  - INV-006：snapshot 只依赖存储且在目录失败时仍可成功；catalog 只依赖目录能力；两者严格按 CT-001 编解码、阻塞工作有边界、错误码稳定、诊断脱敏，且 BridgeContext 可注入 store/fake data。
- 拆分依据：单一不变量，无需逐对判定；创建和移动是独立 mutation，可在读取链路成功时分别失败，故拆为 T-007/T-008。
- 写入范围：
  - `crates/codex-elves-core/src/routes.rs`
  - `crates/codex-elves-core/src/routes/task_board/mod.rs`
  - `crates/codex-elves-core/src/routes/task_board/snapshot.rs`
  - `crates/codex-elves-core/src/routes/task_board/catalog.rs`
  - `crates/codex-elves-core/src/routes/task_board/create.rs`
  - `crates/codex-elves-core/src/routes/task_board/move_task.rs`
  - `crates/codex-elves-core/tests/task_board_read_routes.rs`
- 只读依赖：T-001 CT-002/CT-003 DTO，READ-001/002/003/022/026/027/028。
- 契约：
  - 产出：CT-001, CT-003
  - 消费：CT-001, CT-002, CT-003
- 正面验收：
  - 验收项：读取 RPC 的两端服务形态符合 CT-001（INV-006, CT-001）。
    - 通过判据：四个 path 常量和 match 认领存在；snapshot/catalog 请求拒绝未知字段；成功/failed 响应字段精确；catalog 不含 `dbPath/rolloutPath`；transport 无额外注册；诊断只记 path/status/耗时/计数。
    - 必需证据：`cargo test -p codex-elves-core --test task_board_read_routes -- --test-threads=1`。
  - 验收项：依赖隔离符合 INV-006 与 CT-003。
    - 通过判据：FakeData 失败时 snapshot 仍成功；损坏任务文件不阻止独立 catalog；`UnavailableDataService` 返回目录不可用；BridgeContext 测试可注入临时 store。
    - 必需证据：同一 test target 的独立失败用例全部通过。
- 负面验收：
  - 不验收：create/move handler 的最终业务行为 → 负责方：T-007, T-008。
  - 不验收：真实 launcher 目录实现 → 负责方：T-005。
  - 不验收：Renderer 是否能调通 → 负责方：W2 波次门。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明服务端读取协议符合冻结契约且 snapshot/catalog 故障域分离。
  - 通过判据：定向路由测试和既有 `bridge_routes` 回归通过。
  - 必需证据：上述 test target 加 `cargo test -p codex-elves-core --test bridge_routes`。
  - 来源：设计稿 Bridge 路由测试义务 + 既有行为零变更。
- 独立回滚边界：回滚 task-board route module、match arms、BridgeContext/store 注入和测试；核心存储与 data helper不回滚。

### T-007 实现 Bridge 任务创建与真实性校验

- 所属模块：M3
- 单一交付目标：让 `/task-board/task-create` 只用后端目录中的真实会话创建任务，并稳定处理输入、项目、revision 与幂等错误。
- 唯一问题所有者：跨会话目录与任务存储的创建编排。
- 不变量：
  - INV-007：Renderer 只提交 taskId/title/project/sessionIds；Bridge 每次重新取 CT-003、按 ID 解析快照、规范化并验证同项目后才调用 CT-002；任一校验/目录/存储失败都不写任务，所有响应符合 CT-001。
- 拆分依据：单一不变量，无需逐对判定。
- 写入范围：
  - `crates/codex-elves-core/src/routes/task_board/create.rs`
  - `crates/codex-elves-core/tests/task_board_create_routes.rs`
- 只读依赖：T-002 create store、T-005/T-006 的 CT-003、T-006 route/error helpers。
- 契约：
  - 产出：CT-001
  - 消费：CT-001, CT-002, CT-003
- 正面验收：
  - 验收项：create RPC 的真实性边界与错误矩阵闭合（INV-007, CT-001, CT-002, CT-003）。
    - 通过判据：成功返回完整快照；客户端伪造标题/cwd 不进入文件；空/重复/临时 ID、缺失会话、跨项目、目录全失败、revision conflict、taskId conflict、busy/invalid/unavailable 均映射为冻结 code；conflict 携带最新快照。
    - 必需证据：`cargo test -p codex-elves-core --test task_board_create_routes -- --test-threads=1` 全部通过。
- 负面验收：
  - 不验收：store 内部幂等与并发实现 → 负责方：T-002。
  - 不验收：Renderer 自动重试与 modal 状态 → 负责方：T-010, T-013。
  - 不验收：真实跨进程联调 → 负责方：W3/W4 波次门。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明 Bridge 不信任 Renderer 会话元数据，并按 CT-001 映射每条可达错误路径。
  - 通过判据：测试 fake 同时覆盖成功、校验失败、目录失败、store conflict/error。
  - 必需证据：上述 test target 成功。
  - 来源：设计稿 Bridge create 测试义务。
- 独立回滚边界：回滚 create handler 与 test target，snapshot/catalog/move handler 和底层 store 不受影响。

### T-008 实现 Bridge 任务移动协议

- 所属模块：M3
- 单一交付目标：让 `/task-board/task-move` 精确解析目标状态/索引/revision，调用 CT-002 并返回稳定快照或错误。
- 唯一问题所有者：Bridge 层移动命令的协议校验和错误映射。
- 不变量：
  - INV-008：任何 move 请求只能按 CT-001 的字段与枚举进入 CT-002；成功/无变化均返回服务端完整快照，越界、缺失任务、revision 冲突和文件错误不产生额外变更且使用冻结错误包络。
- 拆分依据：单一不变量，无需逐对判定。
- 写入范围：
  - `crates/codex-elves-core/src/routes/task_board/move_task.rs`
  - `crates/codex-elves-core/tests/task_board_move_routes.rs`
- 只读依赖：T-003 move store、T-006 route/error helpers、CT-001/CT-002。
- 契约：
  - 产出：CT-001
  - 消费：CT-001, CT-002
- 正面验收：
  - 验收项：move RPC 完整符合 INV-008 与 CT-001。
    - 通过判据：五状态、零/末尾索引、无变化成功均返回完整快照；未知字段/非法枚举/负数或越界索引为 `invalid_input`；缺失任务为 `task_not_found`；冲突携带最新快照；busy/invalid/unavailable 映射稳定。
    - 必需证据：`cargo test -p codex-elves-core --test task_board_move_routes -- --test-threads=1` 全部通过。
- 负面验收：
  - 不验收：列重排算法本身 → 负责方：T-003。
  - 不验收：拖拽命中、乐观更新和菜单键盘交互 → 负责方：T-011。
  - 不验收：真实 Renderer 联调 → 负责方：W3 波次门。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明每种请求形态和 store 结果只映射到 CT-001 允许的响应。
  - 通过判据：定向 route test 精确断言 status/code/snapshot 和 fake 调用参数。
  - 必需证据：上述 test target 成功。
  - 来源：设计稿 Bridge move 测试义务。
- 独立回滚边界：回滚 move handler 与 test target，不影响 create/read 路由。

### T-009 实现 Renderer 看板入口、生命周期与只读视图

- 所属模块：M4
- 单一交付目标：用户点击“插件”下方的“任务看板”后，在当前原生 `main` 内看到可搜索、可筛选、可滚动的五列任务快照；离开或 reinjection 时原生内容完整恢复。
- 唯一问题所有者：看板激活态的 DOM 所有权、只读投影和响应式可用性。
- 不变量：
  - INV-009：看板激活时只有一个入口和一个直接挂在当前 `main` 的根节点，原生顶栏不被覆盖；最新 snapshot/catalog 被投影为固定五列、计数、卡片、搜索和项目筛选，容器宽度/高度变化按设计重排并允许访问全部列；退出、main 替换和 runtime refresh 后无重复节点、监听器、observer 或残留隐藏状态。
- 拆分依据：单一“激活态只读投影”不变量；入口、挂载、渲染和恢复由同一 runtime 状态机维护，不能单独发布。创建与移动会在该只读闭包成立时独立失败，故拆为 T-010/T-011。
- 写入范围：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：READ-011/012/014/015/019/025、CT-001、CT-004、批准 Debug 布局。
- 契约：
  - 产出：CT-001
  - 消费：CT-001, CT-004
- 正面验收：
  - 验收项：入口、main host 与清理生命周期满足 INV-009。
    - 通过判据：入口紧随原生“插件”，重复扫描/refresh 不复制；根节点是当前 `main` 直接子节点；只在 host class 下隐藏其他直接子节点；原生导航、main 替换和 destroy 均恢复内容并清掉外部 popover/监听器。
    - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_lifecycle -- --test-threads=1` 与 Debug DOM 断言。
  - 验收项：只读视图按 CT-001 编码/解码并在三档尺寸可用（INV-009, CT-001, CT-004）。
    - 通过判据：snapshot 失败不伪装保存；catalog 失败不遮蔽已有任务；五列固定；搜索覆盖标题/项目/会话，筛选以 cwd 为键；关联会话显示快照/最新目录标题与不可用标记并调用 mock `openSession`；1922×1034、996×785、780×400 的工具栏关系、图标 aria-label、横纵 scroll range 均符合设计。
    - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_view -- --test-threads=1`、`node --check assets/inject/renderer-features.js`，以及 Debug 使用冻结 mock 响应的三档 DOM/截图记录。
- 负面验收：
  - 不验收：CT-001 服务端实现正确性 → 负责方：T-006, T-007, T-008。
  - 不验收：新建 modal 与创建请求 → 负责方：T-010。
  - 不验收：拖拽/状态菜单 mutation → 负责方：T-011。
  - 不验收：CT-004 的真实宿主创建/导航 → 负责方：T-013, T-014。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：用 mock Bridge/adapter 证明 Renderer 单方符合冻结契约并保持现有扫描生命周期零回归。
  - 通过判据：两个 filtered cdp tests、JS syntax、既有 observer 回归测试通过。
  - 必需证据：上述命令及 `cargo test -p codex-elves-core --test cdp_bridge renderer_features_reuses_scan_observers_when_roots_are_unchanged`。
  - 来源：设计稿 Renderer 自动化 + Debug 验收义务。
- 独立回滚边界：回滚 task-board runtime 块和新增 source-contract tests，现有 renderer 增强与原生内容保持。

### T-010 实现新建任务 modal 与已有会话流程

- 所属模块：M4
- 单一交付目标：用户可在响应式 modal 中选择项目和同项目一个或多个已有会话，提交创建并正确处理 busy、失败、冲突与刷新。
- 唯一问题所有者：看板创建表单状态和既有会话创建交互。
- 不变量：
  - INV-010：modal 的所有可提交状态都包含合法标题、一个项目和至少一个该项目目录会话；提交只发送 CT-001 允许字段并以客户端 UUID/当前 revision 建立幂等，任何结果都解除 busy，成功采用完整服务端快照，失败保留可恢复输入，冲突只自动重试一次。
- 拆分依据：单一表单提交状态机；原生新会话会切换页面并需要不同恢复边界，可在已有会话流程正确时独立失败，故由 T-013 负责。
- 写入范围：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：T-009 runtime/state、CT-001/CT-004、设计稿 modal 与关联已有会话流程。
- 契约：
  - 产出：CT-001
  - 消费：CT-001, CT-004
- 正面验收：
  - 验收项：modal 可访问性、同项目选择与 create 状态机满足 INV-010（INV-010, CT-001, CT-004）。
    - 通过判据：650px/窄屏宽度、`role=dialog`、焦点约束、Escape、焦点恢复、模式按钮左对齐；项目变化清空跨项目选择；提交 payload 无会话标题/cwd；成功/invalid/session_not_found/project_mismatch/conflict/task_id_conflict/bridge failure 均按设计反馈并解除 busy；revision conflict 同 taskId 最多重试一次。
    - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_create -- --test-threads=1`、`node --check assets/inject/renderer-features.js`，以及 mock Debug 表单记录。
- 负面验收：
  - 不验收：CT-001 create 服务端真实性与幂等实现 → 负责方：T-002, T-007。
  - 不验收：`startConversation` 的真实宿主调用、永久 ID 与恢复队列 → 负责方：T-013。
  - 不验收：拖拽/状态菜单 → 负责方：T-011。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明前端单方严格按冻结 create/adapter 契约构造请求并闭合 UI 状态。
  - 通过判据：mock 测试覆盖成功、每类稳定错误、冲突一次重试、busy/focus 清理。
  - 必需证据：上述 filtered cdp test 和 JS syntax。
  - 来源：设计稿 Renderer create/modal 测试义务。
- 独立回滚边界：回滚 modal/create 状态机和对应 tests，只读看板仍可使用。

### T-011 实现拖拽与状态菜单移动

- 所属模块：M4
- 单一交付目标：用户可用鼠标拖拽或可访问状态菜单修改状态和列内顺序，并在后端结果、失败和并发冲突下保持一致。
- 唯一问题所有者：Renderer move 意图计算、乐观状态与回滚。
- 不变量：
  - INV-011：每次用户移动只产生一个符合 CT-001 的 `toStatus/targetIndex/expectedRevision`；乐观视图最终必须被服务端完整快照校正，失败恢复最近服务端快照，revision 冲突采用最新快照且不自动覆盖；菜单键盘和鼠标路径语义一致。
- 拆分依据：单一移动交互状态机，无需逐对判定。
- 写入范围：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：T-009/T-010 最新 runtime、CT-001、设计稿状态修改。
- 契约：
  - 产出：CT-001
  - 消费：CT-001
- 正面验收：
  - 验收项：拖拽、列内重排与五项状态菜单满足 INV-011（INV-011, CT-001）。
    - 通过判据：目标索引按移除源后计算；状态菜单移动到目标列末尾；方向键/Enter/Escape 可操作；成功使用响应快照；普通失败回滚；conflict 使用冲突快照并提示重试；所有路径解除 busy/drag 状态；筛选态不改持久化 order。
    - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_move -- --test-threads=1`、`node --check assets/inject/renderer-features.js`，以及 mock Debug 的跨列/列内/菜单记录。
- 负面验收：
  - 不验收：后端目标索引和重排算法 → 负责方：T-003, T-008。
  - 不验收：新建任务 modal → 负责方：T-010。
  - 不验收：原生会话适配器 → 负责方：T-013, T-014。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明前端对每类 move 响应只有契约允许的状态转移。
  - 通过判据：mock 断言请求 index/revision、成功校正、失败回滚、冲突刷新与菜单可访问性。
  - 必需证据：上述 filtered cdp test 和 JS syntax。
  - 来源：设计稿 Renderer move 测试义务。
- 独立回滚边界：回滚 move 交互块与 tests，读取和创建流程保留。

### T-012 特征化当前 Codex 原生会话宿主能力

- 所属模块：M5
- 单一交付目标：在当前 Codex Debug 版本取得“指定项目新会话 + 原生提交 + 永久 ID”以及“按永久 ID 导航”的真实可达路径，或可复核地判定对应能力不可用。
- 唯一问题所有者：OPEN-001 的宿主事实。
- 不变量：
  - INV-012：后续适配器只基于本任务观察到的当前版本 dispatcher/DOM/module 行为实施；证据包含版本、触发入口、消息/字段名或 DOM 层级、成功/失败/临时到永久 ID 信号，且不记录首条指令、标题正文或会话正文。
- 拆分依据：单一探查不变量，无需逐对判定；探查不实现产品行为，可独立失败/重跑，故与 T-013/T-014 分离。
- 写入范围：无源码写入；只读当前 Codex Debug 页面和已注入 Renderer，证据进入任务执行报告。
- 只读依赖：READ-013/014/016/017/018、OPEN-001、当前 Codex Debug build/version。
- 契约：
  - 产出：无
  - 消费：无
- 正面验收：
  - 验收项：OPEN-001 被真实证据关闭（INV-012）。
    - 通过判据：对 create/open 分别记录优先原生路径、fallback、可观察成功信号、可观察失败；至少执行成功、能力缺失/元素缺失、临时 ID 三类特征化。若某能力确实不存在，明确记录 `unsupported` 和 probe 判据，而不是猜测 payload。
    - 必需证据：带 Codex 绝对版本/build 与时间戳的脱敏 Debug 记录；字段只列 key/类型，不含 instruction/title/body。
- 负面验收：
  - 不验收：CT-004 adapter 实现 → 负责方：T-013, T-014。
  - 不验收：任务创建 Bridge 或恢复队列 → 负责方：T-007, T-013。
  - 不验收：UI 视觉与响应式 → 负责方：T-009, T-010。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：以实机证据取代对易变 Codex 私有实现的推测。
  - 通过判据：create/open 两条事实均得到“支持的具体路径”或“可判定 unsupported”结果。
  - 必需证据：脱敏特征化记录。
  - 来源：OPEN-001 关闭动作。
- 独立回滚边界：无源码变化；丢弃本次证据即可，不影响其他任务。

### T-013 实现原生新会话与任务恢复流程

- 所属模块：M5
- 单一交付目标：在支持的 Codex 版本中按项目创建原生会话、提交首条指令、等待永久 ID 并创建任务；不支持或中途失败时按设计禁用/恢复且不泄露指令。
- 唯一问题所有者：`probe/startConversation` 与新会话后的任务恢复状态机。
- 不变量：
  - INV-013：只有原生提交成功且得到永久 sessionId 才调用 create RPC；SQLite 延迟仅以相同 taskId 在 10 秒窗口重试，15 秒无永久 ID 不写任务；切页后失败只保存 24 小时、不含 instruction 的恢复元数据并最多自动重试一次；能力不足只禁用新会话模式。
- 拆分依据：单一原生创建/恢复不变量；打开既有会话不发送指令且可独立失败，故拆为 T-014。
- 写入范围：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：T-010/T-011 最新 runtime、T-012 事实、CT-001/CT-004、READ-016/017/018。
- 契约：
  - 产出：CT-001, CT-004
  - 消费：CT-001, CT-004
- 正面验收：
  - 验收项：probe/startConversation 严格实现 CT-004 和 INV-013。
    - 通过判据：unsupported 时只禁用新模式；支持时定位指定项目、走已特征化原生新会话/composer、忽略临时 ID、永久 ID 后才调用 create；15 秒 timeout、10 秒 session_not_found 退避、revision 一次重试、响应丢失幂等均可判定。
    - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_native_create -- --test-threads=1`、`node --check assets/inject/renderer-features.js`，以及当前 Debug 的成功或 unsupported 实机记录。
  - 验收项：恢复与隐私边界满足 INV-013。
    - 通过判据：sessionStorage 仅含 taskId/title/project/sessionId/createdAtMs，绝无 firstInstruction；24h 过期清理；下次看板激活同 ID 自动重试一次；诊断不含指令/完整标题。
    - 必需证据：filtered source-contract test 对 storage/log payload 做否定断言，Debug 检查恢复记录。
- 负面验收：
  - 不验收：CT-001 create 服务端实现 → 负责方：T-002, T-005, T-007。
  - 不验收：openSession 导航 → 负责方：T-014。
  - 不验收：宿主事实是否真实 → 负责方：T-012。
  - 不验收：NT-001–NT-007 → 负责方：非目标。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明新会话状态机只在永久 ID 后持久化任务，并保护 instruction 隐私。
  - 通过判据：自动化覆盖 supported/unsupported、临时 ID、timeout、SQLite 延迟、Bridge 失败、恢复 TTL；实机覆盖当前版本真实路径。
  - 必需证据：上述自动化和 Debug 记录。
  - 来源：设计稿原生新会话与恢复测试义务。
- 独立回滚边界：回滚 adapter create/probe/recovery 块和 tests，已有会话创建、移动与只读看板保留；新会话模式回到 disabled。

### T-014 实现关联会话原生导航

- 所属模块：M5
- 单一交付目标：点击任务关联会话时按分层策略进入正确的 Codex 原生会话，无法导航时保留任务并给出显式错误。
- 唯一问题所有者：`openSession(sessionId)` 的宿主导航与失败分类。
- 不变量：
  - INV-014：openSession 依次尝试已挂载 thread 行、展开项目后行、已特征化 dispatcher；任何成功只触发原生导航，所有失败在 5 秒内返回 CT-004 code，不构造未知 URL、不写 SQLite、不删除关联。
- 拆分依据：单一导航不变量，无需逐对判定。
- 写入范围：
  - `assets/inject/renderer-features.js`
  - `crates/codex-elves-core/tests/cdp_bridge.rs`
- 只读依赖：T-009 card/popover、T-012 宿主事实、T-013 最新 adapter runtime、CT-004。
- 契约：
  - 产出：CT-004
  - 消费：CT-004
- 正面验收：
  - 验收项：三层导航和显式失败符合 INV-014 与 CT-004。
    - 通过判据：直接 thread row 点击、折叠项目展开后点击、虚拟列表 dispatcher fallback 分别有覆盖；缺失/归档/能力不足返回稳定 code/message；重复打开安全；失败后任务卡和关联快照仍存在。
    - 必需证据：`cargo test -p codex-elves-core --test cdp_bridge renderer_task_board_open_session -- --test-threads=1`、`node --check assets/inject/renderer-features.js`，以及当前 Debug 对至少一个真实永久 sessionId 的导航记录。
- 负面验收：
  - 不验收：会话目录是否仍存在该 ID → 负责方：T-004, T-005。
  - 不验收：新会话创建/恢复 → 负责方：T-013。
  - 不验收：任务数据删除或自动修复关联 → 负责方：非目标 NT-001/NT-002。
  - 不验收：CT-004 的 UI 调用位置正确性 → 负责方：W5 波次门。
- 测试义务：
  - 层级：任务交付验收
  - 目标：证明所有可达导航层级有界、原生且失败不破坏任务。
  - 通过判据：自动化覆盖三层和失败，实机点击进入期望 session。
  - 必需证据：上述测试与 Debug 记录。
  - 来源：设计稿原生会话导航测试义务。
- 独立回滚边界：回滚 openSession 实现和 tests，卡片仍可显示会话但给出 adapter unavailable；其他任务看板功能保持。

## DECOMPOSE 公共文件核对

| 类别 | 结论 |
| --- | --- |
| 路由/端点注册 | `routes.rs` 只有 T-006 修改；T-007/T-008 只改预留的独立 handler 文件。 |
| 依赖注入 | BridgeContext/store 注入与 `BridgeDataService` 声明只有 T-006；launcher 实现只有 T-005。 |
| 配置文件 | 不涉及设置、开关或环境配置。 |
| 数据库迁移 | 不涉及；NT-004 明确禁止 schema/write 变更。 |
| 共享结构 | core DTO/enum/normalize 只有 T-001；data helper DTO 只有 T-004。 |
| 构建/依赖声明 | 不增加第三方依赖，不修改 Cargo manifest 或前端构建。 |
| 国际化/文案 | 无语言包；任务看板内联中文文案由串行 Renderer 任务 T-009/T-010/T-011/T-013/T-014 各自维护其闭包。 |
| 高冲突 Renderer 文件 | `renderer-features.js` 与 `cdp_bridge.rs` 由 T-009 → T-010 → T-011 → T-013 → T-014 串行，不放同一波次。 |
| 高冲突 Core 存储 | T-001 先建立模块；T-002/T-003 只改不同 mutation 文件，可同波次。 |

## DECOMPOSE 出站自检

1. T-001–T-014 均具备单一目标、唯一所有者、编号不变量、证据和独立回滚边界。
2. 每个任务只有一个聚合不变量；会独立失败/回滚的创建、移动、目录接线、Bridge 操作、Renderer 操作和 native create/open 已拆开。
3. 任务以验收闭包命名，不以 DTO/Controller/CSS 文件层命名。
4. 每个 INV-001–INV-014 只有一个问题所有者。
5. 所有计划修改文件已列入写入范围；公共文件和同文件串行约束已核对。
6. 下一步把 CT-001–CT-005 的模块级生产/消费关系绑定到上述任务，并在 SEQUENCE 校验逆索引。

## 依赖边

- DEP-001：T-002 依赖 T-001。
  - 类型：事实依赖。
  - 理由：T-002 必须基于 T-001 实际创建的 `task_board/create.rs` 扩展点、领域类型和 store 锁内 mutation seam 实现；不是只读冻结规格。
- DEP-002：T-003 依赖 T-001。
  - 类型：事实依赖。
  - 理由：T-003 必须基于 T-001 实际创建的 `move_task.rs` 扩展点、状态类型和 store mutation seam 实现。
- DEP-003：T-006 依赖 T-001。
  - 类型：契约实现依赖。
  - 理由：BridgeContext 必须注入真实存在的 `TaskBoardStore` trait/默认 file store，并序列化 T-001 提供的 DTO；仅有计划规格无法编译。
- DEP-004：T-010 依赖 T-009。
  - 类型：事实依赖。
  - 理由：两任务写同一 Renderer 文件，T-010 必须在 T-009 已建立的 runtime state、mount/cleanup 与 render hooks 上扩展 modal。
- DEP-005：T-005 依赖 T-004。
  - 类型：契约实现依赖。
  - 理由：launcher 必须调用实际存在的 CT-005 helper 才能工作，不能仅凭签名完成交付验收。
- DEP-006：T-005 依赖 T-006。
  - 类型：事实依赖。
  - 理由：T-006 在既有 `BridgeDataService` 中实际加入 CT-003 方法声明；Rust impl 在该声明存在前无法编译。
- DEP-007：T-005 依赖 T-001。
  - 类型：契约实现依赖。
  - 理由：launcher 映射必须调用真实的 cwd normalize 并构造真实目录 DTO。
- DEP-008：T-007 依赖 T-006。
  - 类型：事实依赖。
  - 理由：T-007 只改 T-006 预留的 create handler，并复用其请求解析、错误包络和 context seam。
- DEP-009：T-008 依赖 T-006。
  - 类型：事实依赖。
  - 理由：T-008 只改 T-006 预留的 move handler，并复用其请求解析、错误包络和 context seam。
- DEP-010：T-011 依赖 T-010。
  - 类型：事实依赖。
  - 理由：两任务写同一 Renderer 文件，T-011 必须基于包含 modal/board state 的最新 runtime 增加 move 状态机。
- DEP-011：T-013 依赖 T-011。
  - 类型：事实依赖。
  - 理由：T-013 写同一 Renderer runtime，必须基于已有完整看板、modal 与 move 清理逻辑接入切页/恢复状态。
- DEP-012：T-013 依赖 T-012。
  - 类型：事实依赖。
  - 理由：T-013 的宿主调用路径必须使用 T-012 在当前 Codex 版本取得的真实消息/DOM 事实；OPEN-001 不能凭规格关闭。
- DEP-013：T-014 依赖 T-013。
  - 类型：事实依赖。
  - 理由：T-014 修改同一 adapter/runtime 文件，必须读取 T-013 完成后的 probe/error/cleanup 形态，避免覆盖。
- DEP-014：T-014 依赖 T-012。
  - 类型：事实依赖。
  - 理由：dispatcher fallback 和项目展开策略必须使用 T-012 的真实宿主导航事实。

明确不建立的伪依赖：

- T-009 不依赖 T-006；Renderer 只依赖已冻结 CT-001，可用 mock 单方验收。
- T-010/T-011/T-013 不依赖 T-007/T-008 的实现；前端按 CT-001 编码/解码，真实联调放波次门。
- T-007 不依赖 T-005 的实现；用 CT-003 fake 验证单方真实性编排，真实 launcher provider 在同波次完成后联调。
- T-008 不依赖 T-003 的实现；用 CT-002 fake 验证协议映射，真实 store 在波次门联调。

## 执行波次

### W1 — 并行建立四个独立基础

- T-001：schema v1 与安全快照存储基础。
- T-004：跨候选库会话聚合 helper。
- T-009：Renderer 入口、生命周期与只读视图。
- T-012：当前 Codex 原生宿主能力特征化。
- 同波次写入校验：core task-board、data、Renderer、只读取证四个范围两两不相交。

### W2 — 并行补齐 mutation、Bridge 读取和创建 UI

- T-002：任务创建存储语义。
- T-003：任务移动存储语义。
- T-006：Bridge snapshot/catalog 读取链路与公共 handler seam。
- T-010：新建任务 modal 与已有会话流程。
- 前置依赖：DEP-001、DEP-002、DEP-003、DEP-004。
- 同波次写入校验：core `task_board/create.rs`、`move_task.rs`、core `routes*`、Renderer 文件两两不相交。

### W3 — 并行接通真实目录、create/move 路由与移动 UI

- T-005：launcher 真实会话目录。
- T-007：Bridge create 真实性编排。
- T-008：Bridge move 协议。
- T-011：Renderer 拖拽与状态菜单。
- 前置依赖：DEP-005、DEP-006、DEP-007、DEP-008、DEP-009、DEP-010。
- 同波次写入校验：launcher main、create handler/test、move handler/test、Renderer 文件两两不相交。

### W4 — 原生新会话与恢复

- T-013：probe/startConversation、永久 ID、SQLite 延迟重试和恢复队列。
- 前置依赖：DEP-011、DEP-012。
- 同波次写入校验：单任务，无冲突。

### W5 — 原生会话导航与最终闭环

- T-014：openSession 分层导航。
- 前置依赖：DEP-013、DEP-014。
- 同波次写入校验：单任务，无冲突。

## 波次门

### W1 波次门

- 验收项：core/data 两个公共能力可以在同一 workspace 中被下游引用。
  - 涉及任务：T-001, T-004
  - 涉及契约：CT-002, CT-003, CT-005
  - 通过判据：core 与 data 同时 check 无依赖环或导出冲突；T-001 的 DTO/normalize 与 T-004 helper 可从各自 crate 公共入口导入。
  - 必需证据：`cargo check -p codex-elves-core -p codex-elves-data`，加两个任务 test target 的一次成功记录。
- 验收项：Renderer 后续扩展点与宿主事实可以同时交接。
  - 涉及任务：T-009, T-012
  - 涉及契约：CT-004
  - 通过判据：T-009 只调用 CT-004 行为边界、不内嵌未知 dispatcher payload；OPEN-001 已以当前 Codex 绝对版本/build 关闭为 supported 或 unsupported。
  - 必需证据：Renderer source-contract 断言和 T-012 脱敏特征化记录。
- 不验收：Bridge 实际读取联调 → 负责方：W2 波次门。
- 不验收：任务创建/移动及真实 native adapter → 负责方：W3、W4、W5 波次门。

### W2 波次门

- 验收项：Renderer 只读看板通过真实 Bridge snapshot/catalog 链路工作，且两类后端故障域分离。
  - 涉及任务：T-001, T-006, T-009
  - 涉及契约：CT-001, CT-002, CT-003
  - 通过判据：空文件显示 revision 0 五列；合法临时文件显示任务；catalog provider fake 失败时任务仍显示并给目录错误；损坏任务文件时 catalog 仍可独立返回；请求/响应字段与 CT-001 完全一致。
  - 必需证据：合并后 `cargo test -p codex-elves-core --test task_board_read_routes -- --test-threads=1`，以及 Debug 通过真实 binding 的空/合法/两类独立失败记录。
- 验收项：创建 modal 可在真实只读 runtime 中保持状态，但不要求服务端 create 已存在。
  - 涉及任务：T-009, T-010
  - 涉及契约：CT-001, CT-004
  - 通过判据：modal 读取真实 catalog state，使用 mock create 响应完成成功/失败/冲突状态；退出/refresh 后无残留。
  - 必需证据：Debug DOM/交互记录和 filtered cdp tests。
- 不验收：真实 create/move mutation 联调 → 负责方：W3 波次门。
- 不验收：原生新会话和导航 → 负责方：W4、W5 波次门。

### W3 波次门

- 验收项：已有会话创建从真实 candidate DB 经 launcher/Bridge/store 到 JSON 文件全链路闭合。
  - 涉及任务：T-001, T-002, T-004, T-005, T-006, T-007, T-009, T-010
  - 涉及契约：CT-001, CT-002, CT-003, CT-005
  - 通过判据：选择同项目一个/多个真实未归档会话后创建一个任务；文件只含后端目录快照；跨项目/消失会话不写文件；单库失败显示 warning 且可创建；全库失败已有任务仍可看但创建受阻；响应丢失同 taskId 不重复。
  - 必需证据：`cargo check --workspace`，全 workspace `task_board` 定向测试，以及当前 Debug 的真实已有会话创建记录与 `task-board.json` 脱敏结构检查。
- 验收项：move 从 Renderer 到 Bridge/store 在成功、失败和多窗口冲突下闭合。
  - 涉及任务：T-003, T-006, T-008, T-009, T-011
  - 涉及契约：CT-001, CT-002
  - 通过判据：跨列、列内和菜单移动重启后保持；失败回滚；两个窗口用同 revision 修改时一个成功、另一个收到最新快照且无静默覆盖。
  - 必需证据：Debug 拖拽/菜单/双窗口记录，随后重启或 runtime refresh 再读同一 revision/order。
- 不验收：原生新会话创建与恢复 → 负责方：W4 波次门。
- 不验收：关联会话原生导航与最终全量回归 → 负责方：W5 波次门。

### W4 波次门

- 验收项：新会话模式通过 CT-004 与真实 Codex 宿主、CT-001 create 链路协同。
  - 涉及任务：T-005, T-007, T-010, T-012, T-013
  - 涉及契约：CT-001, CT-003, CT-004
  - 通过判据：若 T-012 判定 supported，则指定项目真实创建会话、首条指令真实发送、临时 ID 被忽略、永久 ID 后任务落盘且用户停留在新会话；模拟 SQLite 延迟/Bridge 失败时 10 秒退避和 24h 无指令恢复记录符合设计。若判定 unsupported，则只禁用该模式并保留已有会话创建全功能。
  - 必需证据：当前 Codex Debug 实机记录、sessionStorage 脱敏检查、对应 task JSON 和 filtered native-create tests。
- 不验收：openSession 导航 → 负责方：W5 波次门。
- 不验收：任务编辑/删除、自动状态、同步、Manager 设置 → 负责方：非目标 NT-001–NT-005。

### W5 波次门

- 验收项：关联会话点击通过 CT-004 进入正确原生会话，失败不破坏任务。
  - 涉及任务：T-009, T-012, T-014
  - 涉及契约：CT-004
  - 通过判据：单会话和多会话 popover 均可打开真实永久 session；直接行/展开项目/dispatcher fallback 中当前可达层级成功；不可用会话显示错误且任务仍在。
  - 必需证据：当前 Debug 的真实 session 导航记录与 filtered open-session tests。
- 验收项：完整任务看板在发布边界内通过最终回归。
  - 涉及任务：T-001, T-002, T-003, T-004, T-005, T-006, T-007, T-008, T-009, T-010, T-011, T-013, T-014
  - 涉及契约：CT-001, CT-002, CT-003, CT-004, CT-005
  - 通过判据：1922×1034、996×785、780×400 均满足布局/滚动；已有会话创建、可用时的新会话创建、拖拽、菜单、导航、restart persistence、双窗口 conflict、main 替换和 reinjection 均通过；无新依赖、无 SQLite schema/write、无重复 observer。
  - 必需证据：`cargo fmt --check`、`cargo check --workspace`、`cargo test --workspace`、`node --check assets/inject/renderer-features.js`、`git diff --check`，加三档 Debug 验收记录。
- 不验收：NT-001–NT-007 → 负责方：非目标。

## Review 检查点

### Review 检查点 R1

- 时机：W1 波次结束后。
- 覆盖范围：T-001、T-004、T-009 的写入范围；T-012 只审证据隐私。
- 关注点：schema/路径语义是否单一；文件锁/原子替换是否跨平台；catalog 是否泄露路径；Renderer host 清理是否会破坏既有增强；特征化是否记录敏感文本。
- 不关注：create/move 业务、Bridge 路由联调、native adapter 实现 → 理由：对应任务尚未进入波次。

### Review 检查点 R2

- 时机：W3 波次结束后。
- 覆盖范围：T-002、T-003、T-005、T-006、T-007、T-008、T-010、T-011 的写入范围。
- 关注点：CT-001/002/003/005 双端一致性；revision/锁/幂等顺序；真实会话校验是否不信任 Renderer；async blocking 边界；乐观更新/冲突恢复是否只采用服务端快照。
- 不关注：Codex 私有 dispatcher 细节和 openSession → 理由：由 T-012/T-013/T-014 与后续门负责。

### Review 检查点 R3

- 时机：W5 波次结束后，兼作存在跨波次公共契约风险的最终整体 Review。
- 覆盖范围：T-001–T-014 全部写入范围，但只复查跨波次契约和 W4/W5 新增 native 代码。
- 关注点：CT-001–CT-005 逆索引与实际字段一致；runtime reinjection 资源释放；指令/标题/DB path 隐私；宿主适配无未知 URL/SQLite 写入；非目标未被带入。
- 不关注：未改动的既有功能内部实现、纯风格偏好、已由 R1/R2 关闭且未受影响的局部实现 → 理由：避免开放式重复 Review。

修复循环约束：每个 Review 检查点先汇总并校准发现，再批量修复；只对受影响范围复查一次。超出关注点的问题进入偏差流程，不扩张当前检查点。

## SEQUENCE 出站自检

1. DEP-001–DEP-014 均标明事实依赖或契约实现依赖及可复核理由。
2. 仅依赖冻结规格的前后端关系未建立伪依赖，改由波次门联调。
3. W1–W5 内所有写入范围两两不相交；共享 Renderer 文件严格串行。
4. 所有事实/实现依赖的消费者都晚于生产者；T-005 与 T-007 同波次只依赖冻结 CT-003，真实协同由 W3 门验收。
5. 每个波次的产出均被后续消费或涉及跨任务协同，因此 W1–W5 均设置独立波次门。
6. 因 `renderer-features.js`/`cdp_bridge.rs` 同文件冲突导致 T-009→T-010→T-011→T-013→T-014 串行，原因已显式记录。

## GATE 结果

- 正式产物：
  - `.zeroone/plan/2026-08-24-task-board/plan.md`
  - `.zeroone/plan/2026-08-24-task-board/tasks.md`
  - `.zeroone/plan/2026-08-24-task-board/deviations.md`
- 审计命令：`node C:\Users\junes\.codex\plugins\cache\zeroone\zeroone\0.1.5-alpha.33\skills\writing-plans\scripts\plan-kit.mjs audit --root E:\code\junes\github\CodexPlusPlus --plan E:\code\junes\github\CodexPlusPlus\.zeroone\plan\2026-08-24-task-board\plan.md --tasks E:\code\junes\github\CodexPlusPlus\.zeroone\plan\2026-08-24-task-board\tasks.md --ledger E:\code\junes\github\CodexPlusPlus\.zeroone\plan\2026-08-24-task-board\.plan-work\ledger.md`
- 审计结果：退出码 0；A1–A14 全部 PASS。共解析 14 个任务、51 个事实锚、5 条契约、5 个波次和 14 条依赖边。
- 覆盖核对：四类契约面均判定；五条契约均冻结；OPEN-001 为阻塞任务并由 T-012 前置关闭；任务正负验收、五个波次门、三个 Review 检查点和偏差五项绑定均已写入。
- 工作区核对：本轮只新增本计划目录的 `plan.md`、`tasks.md`、`deviations.md` 与过程 ledger；未修改业务代码、未建分支、未 commit。

## 退回记录

- 2026-08-24：CONTRACT 预检发现任务存储设计要求最长等待文件锁 2 秒，而 Renderer 现有 Bridge 请求可能同样使用 2 秒总超时。为避免冻结一个必然产生竞态超时的契约，退回 GROUND，补读请求超时实现；补证完成后重新进入 CONTRACT。
- 2026-08-24：READ-025 证实 2 秒超时仅适用于后端状态/修复路由，任务看板普通 Bridge 调用没有该限制；事实缺口关闭，重新进入 CONTRACT。
