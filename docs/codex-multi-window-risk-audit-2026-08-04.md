# CodexElves 多窗口支持与功能风险审计

> 审计日期：2026-08-04
> 审计状态：静态代码、现场证据、第二轮产品功能复核和功能入口反向盘点已完成；动态并发验证待执行
> 范围：Codex 桌面应用通过“在新窗口打开”创建多个主窗口后，CodexElves 注入、Bridge、功能状态、共享数据、Manager 控制面和协议代理的正确性与一致性；同时复核与多窗口无直接依赖、但会影响方案上线安全性的现存产品风险。
> 本文档记录已经完成证据闭环的结论；未闭环判断标记为“待验证”，不能直接作为实现依据。
> 修订说明：此前把 `AGENTS.md` 的 subagent 推理等级约束误当成产品 thinking depth 规格，该结论已经撤销，不再作为产品缺陷或发布阻断项。
> 2026-08-19 更新：会话删除已改为永久删除，撤销路由与 `BackupStore` 已移除；本文涉及 delete/undo 的段落仅记录审计时的历史实现。

## 1. 背景与目标

当前 CodexElves 只向一个 CDP 页面 target 安装 renderer 脚本和 Bridge。Codex 新建第二个主窗口后，原窗口仍然健康，因此 watchdog 不会发现新窗口未注入。

本次审计目标不是只让第二个窗口出现 CodexElves 菜单，而是明确多窗口条件下每个功能的：

- 页面作用域、进程作用域和持久化状态所有权。
- 来源窗口执行、全窗口广播和后端串行化语义。
- 并发、幂等、状态收敛、失败隔离和退出清理风险。
- 上线前必须完成的自动化测试、真实烟测和可观测性门禁。

## 2. RCA 总结

### 2.1 现象

Codex 使用“在新窗口打开”后，新窗口没有 CodexElves 注入；旧窗口功能仍然可用。

### 2.2 直接原因

watchdog 只检查一个缓存 WebSocket。只要旧窗口 Bridge 健康，就直接返回 `Healthy`，不再枚举全部可注入 target，因此无法发现新窗口。

代码证据：

- `crates/codex-elves-core/src/launcher.rs` 的 Bridge 健康检查只维护单个缓存 WebSocket。
- `crates/codex-elves-core/src/cdp.rs` 的 `pick_injectable_codex_page_target` 只返回一个最高分 target。

### 2.3 根因

当前 Bridge 和运行时服务采用单 target 模型：

- launcher 只保存 `Option<BridgeRuntime>`。
- 再次注入前会关闭旧 runtime。
- `LauncherRuntimeService` 只保存一个 `websocket_url`。
- renderer feature 安装、用户脚本重载和 DevTools 打开均依赖该单值目标。

因此，直接循环现有注入函数并不能实现多窗口：新窗口安装会关闭旧窗口 Bridge，或者由 A 窗口发起的请求错误执行到 B 窗口。

### 2.4 系统性根因

- Bridge 路由没有携带不可伪造的请求来源 `target_id`、runtime generation 和 execution context。
- 健康状态是进程级单值，不能表达“部分窗口健康、部分窗口失败”。
- 设置、抑制列表和部分 localStorage 状态没有按多窗口并发写入设计。
- 现有测试主要验证“选中一个 target”和“一个 WebSocket runtime”，没有覆盖 target 集合收敛。
- 近期 Bridge 性能和恢复优化把“旧 target 健康即无需枚举”固化成了单窗口不变量。

## 3. 已确认的运行环境事实

### 3.1 Codex 原生支持多个主窗口

对当前 Codex Windows App 主进程 bundle 的只读检查显示：

- Codex 使用 `primaryWindows` 集合维护多个 primary 窗口。
- `getPrimaryWindow()` 返回最近活跃窗口。
- `getPrimaryWindows()` 返回全部存活主窗口。
- primary、avatar overlay、快捷窗口和 secondary/debug 窗口共享同一窗口创建工厂，但 `appearance` 只存在于 Electron 主进程内部。

因此，多主窗口是 Codex 的正式生命周期，不是偶发边缘情况。

### 3.2 一个调试端口对应多个 target

当前现场进程使用同一个 `--remote-debugging-port=51111`。主窗口、avatar overlay 和应用内浏览器页面都出现在同一 CDP target 列表。

现场已观察到：

- Codex 主页面：`app://-/index.html`
- avatar overlay：`app://-/index.html?initialRoute=%2Favatar-overlay`
- 第三方应用内浏览器页面：普通 HTTP URL，target 类型同样可能是 `page`

安全结论：多窗口实现不能遍历所有 `type == page` 的 target 直接注入，必须严格识别 Codex 主页面并拒绝第三方页面、overlay、DevTools 和未知辅助页面。

### 3.3 新窗口可能短时存在

2026-08-04 的 Codex 日志显示，第二个 primary 窗口在北京时间约 `10:36:59` 完成加载，约 13 秒后从窗口路由中移除。无论这是用户关闭还是窗口流程结束，都说明 target 安装不能由单个卡死目标串行阻塞，也不能依赖较长轮询周期。

## 4. 总体设计决策

### 4.1 第一阶段推荐架构

第一阶段继续使用每个页面 target 一个独立 WebSocket runtime，避免立即重写 browser WebSocket 的多 session 消息分发。

```text
BridgeCoordinator
├─ registry: HashMap<TargetId, TargetEntry>
├─ reconcile single-flight
├─ global route semaphore
├─ settings/suppressed write locks
└─ broadcast service

TargetEntry
├─ target_id / websocket_url
├─ generation
├─ execution_context_id
├─ state: discovered → installing → healthy → draining/failed
├─ BridgeRuntime
└─ last_error / retry_backoff

RequestOrigin
├─ target_id
├─ generation
└─ execution_context_id
```

### 4.2 target 集合收敛

每次 reconcile 都比较：

```text
CDP 可注入 target 集合
        与
当前 managed runtime 集合
```

产生以下动作：

- `Ensure`：发现新 target，安装独立 runtime。
- `Replace`：同一 target ID 的 WebSocket 或 generation 已变化。
- `Stop`：target 已消失，只关闭对应 runtime。
- `Noop`：目标和 runtime 均健康，不重复安装。

不能再用“任一缓存 target 健康”短路整个 target inventory。

### 4.3 目标发现

- 周期性 `/json` 全量 reconcile 是正确性兜底。
- CDP Target 事件可用于加速发现，但不能成为唯一正确性来源。
- target 分类未知时进入 `unsupported/unmanaged` 并记录拒绝原因，禁止试探性注入。

### 4.4 锁与异步边界

- 全局 registry 锁内只交换状态，不执行 WebSocket install、health 或 shutdown。
- 同一 target 只允许一个安装/替换/关闭流程。
- 不同 target 允许有界并行，单个卡死窗口不能阻塞其他窗口。
- 进程退出时并发关闭全部 runtime，并设置总清理期限。
- runtime transport 关闭不等于业务副作用取消；删除、移动等写操作必须有独立幂等与完成语义。

## 5. 功能状态语义

以下分类是多窗口实现的基础契约。

### 5.1 target-local

只应作用于发起请求的窗口：

- renderer features 初始安装和当前窗口刷新。
- CodexElves 菜单、弹窗、DOM 增强和 MutationObserver。
- 当前窗口的 service-tier 请求 patch。
- 打开当前窗口 DevTools。
- 当前窗口导出流程中的用户交互反馈。
- `sessionStorage` 中的临时项目、分支和 worktree 选择。

### 5.2 全窗口广播

状态变化后所有 managed target 都应收敛：

- 后端设置变更。
- 用户脚本清单变更和显式 reload。
- 皮肤、背景和 overlay 配置变更。
- 模型目录、模型能力和 service-tier 能力缓存失效。
- 删除、撤销、项目移动后的会话列表和项目投影刷新。
- Bridge build、launch cycle 或 runtime generation 升级。

广播必须返回每个 target 的结果，允许 `partial`，不能用一个成功掩盖其他窗口失败。

### 5.3 后端共享并串行化

以下状态不能由多个 renderer 独立全量覆盖：

- `BackendSettings`
- 用户脚本配置
- 持久会话抑制列表
- 同一 `session_id` 的删除、撤销和工作区移动
- provider/relay 配置
- 皮肤清单和激活皮肤

需要后端单一事实源、revision，以及全局或资源键级别的写入串行化。

## 6. 注入与 Bridge 风险矩阵

| 风险 | 等级 | 触发条件 | 后果 | 设计约束 |
|---|---|---|---|---|
| 旧窗口健康短路 target 枚举 | P0 | 打开第二窗口 | 新窗口永久无注入 | 每轮对 target 集合 reconcile |
| 新 target 安装关闭旧 runtime | P0 | 复用现有注入函数 | 注入在窗口间转移 | runtime registry 按 `target_id` 管理 |
| 请求来源窗口丢失 | P0 | 多 runtime 共用单 `websocket_url` | A 请求执行到 B | handler 捕获 `RequestOrigin` |
| 误向第三方页面注入 | P0 | 遍历所有 page target | 高权限 Bridge 暴露给第三方页面 | 主页面 allowlist、辅助页面拒绝 |
| 同 target 并发重装 | P1 | watchdog、事件、repair 同时触发 | binding 相互拆除、回调悬挂 | 每 target single-flight |
| 持锁跨网络 await | P1 | target 卡死或批量退出 | 死锁、退出耗时线性增长 | 锁内换状态、锁外 await |
| binding 可被非主 execution context 调用 | P1 | 页面包含 iframe/其他 context | 非预期上下文调用高权限路由 | 校验主 frame context |
| 并发额度随窗口线性增长 | P1 | 多窗口各自拥有 8 并发 | SQLite、文件和阻塞线程压力失控 | 全局 semaphore + target 公平配额 |
| 状态缺少 target 维度 | P1 | 一窗口健康、一窗口失败 | UI 和日志显示整体正常 | target 级状态和 `partial/degraded` |

## 7. 功能域风险审计

静态代码审计已完成。每项覆盖完整调用链、状态所有权、并发语义和失败恢复；需要真实 Codex、Windows 文件锁或 Electron storage 行为才能确认的部分保留为待验证项。

### 7.1 会话管理与共享数据

#### 7.1.1 总结

总体风险为高，删除和项目移动达到 P0/P1 边界。核心问题不是单个 SQL，而是一个业务操作横跨多个独立状态介质：

```text
renderer 内存与 DOM
        ↓
Codex global-state
        ↓
CodexElves 抑制文件
        ↓
多个 SQLite 数据库
        ↓
rollout 文件
        ↓
localStorage 项目投影
```

当前没有统一事务、会话级锁、共享 revision、operation ID、补偿日志或跨窗口失效通知。

#### 7.1.2 功能调用链

| 功能 | 调用链和状态所有权 |
|---|---|
| 注入窗口会话列表 | renderer 扫描当前窗口 DOM；不是后端权威列表 |
| Manager 会话列表 | `App.tsx → Tauri commands.rs → codex-elves-data`，与 renderer 列表独立 |
| 删除 | renderer `/delete` → routes → launcher `spawn_blocking` → `delete_local_from_paths` |
| 撤销 | renderer `/undo` → routes → launcher → `undo_local_from_backup` |
| 抑制/取消抑制 | renderer `/session/suppress` → routes → `suppressed_threads.rs` |
| Markdown 导出 | renderer `/export-markdown` → routes → launcher → `export_markdown_from_paths` |
| 移动到项目 | renderer `/move-thread-workspace` → SQLite + rollout → renderer 再修改 global-state |
| 移动到普通对话 | renderer 直接顺序修改多个 Codex global-state key |
| 排序和项目投影 | renderer localStorage/DOM → `/thread-sort-keys` → launcher → SQLite |
| Provider 同步 | Manager 或 launcher → provider_sync → config、多个 SQLite、rollout、global-state |

#### 7.1.3 功能风险矩阵

| 功能 | 等级 | 已确认风险 | 设计约束 |
|---|---|---|---|
| 会话列表与搜索投影 | P1 | renderer、Manager 和 Codex 原生列表是不同时间点的独立快照；删除和移动后其他窗口不会主动失效，旧窗口可继续展示并操作后端已删除的会话 | 引入 `session_revision`、广播失效事件和 `not_found` 自动恢复 |
| 删除 | P0 | 备份、多个 SQLite 和 rollout 文件不在同一事务；同会话并发删除可能分别删除不同数据库副本，各自只拿到不完整单库撤销 token | 按 `session_id` 排他；引入 operation ID 和部分提交恢复记录 |
| 撤销 | P1 | 多库预检和逐库恢复之间存在 TOCTOU；数据库先提交、文件后恢复；token 可重复使用 | token 状态化并消费；恢复流程按会话串行并记录阶段 |
| 抑制 | P0 | 每窗口独立 Set；文件读改写无锁；固定 `.tmp` 路径；写错误被忽略；其他窗口撤销后仍会继续隐藏已恢复会话，形成持续的表观删除 | 后端单一事实源、revision、可靠错误返回和广播替换快照 |
| Markdown 导出 | P2 | SQLite 路径和 rollout 内容不是一致快照；导出时删除、移动或追加会得到旧内容、缺尾或失败 | 明确 best-effort 快照语义；必要时复制稳定快照再导出 |
| 项目移动 | P0 | SQLite、rollout、global-state 和 localStorage 分步提交；rollout 全文件重写可能覆盖并发追加；失败后 UI 与真实状态可能相反 | 后端统一编排或增加可恢复状态机；同会话排他 |
| 排序与投影 | P1 | localStorage 整对象读改写会丢并发更新；storage 监听未处理项目投影；排序数据源只读首选数据库 | 后端 revision；投影只作短期视觉提示，不作为权威状态 |
| Token/用量历史 | P2 | 每个可见窗口独立轮询，窗口数增加会放大 SQLite 和 rollout 读取 | 全局按 session 单飞和短期缓存 |
| Provider 同步 | P1 | 手动同步可与活动窗口写入并发；进程崩溃会留下永久锁；多个 SQLite 按库提交，后续数据库/global-state/备份清理失败时只恢复 rollout，可能以 `Skipped` 状态留下部分提交 | 共享资源协调器；可回收租约锁；跨库恢复日志；活动 Codex 场景需明确暂停、快照或拒绝策略 |

#### 7.1.4 删除专项

已确认：

- 单库删除在事务前读取数据并创建备份，事务只覆盖 SQLite 行。
- rollout 文件在数据库提交后单独删除。
- 数据库删除成功但 rollout 删除失败时返回 `failed`；renderer 不会移除或抑制当前行，但后端已经发生删除。
- 多数据库删除逐库执行。第一个数据库删除成功后，后续数据库失败可能不会覆盖前一个成功结果。
- 并发删除同一 thread 时，两个操作可能分别删除不同数据库副本，无法生成完整的 multi-database undo manifest。
- `DELETE` 没有用受影响行数确认事务执行时记录仍属于本操作。

因此，“同一会话所有副本删除 + 生成一个完整撤销点”必须被建模为一个会话级业务操作，而不是多个独立 adapter 调用的聚合。

#### 7.1.5 撤销专项

已确认：

- 多数据库撤销先对所有目标执行预检查，再逐个恢复，预检查和恢复之间存在竞态窗口。
- 前一个数据库恢复后，后一个数据库失败不会回滚前一个数据库。
- 单数据库恢复先提交 SQLite，再恢复 rollout 文件。
- undo token 不会被标记为已消费，可在未来再次重放旧备份。
- 发起窗口只从本窗口抑制 Set 移除 ID；其他窗口不会移除旧 ID。

撤销应具备 operation 状态，例如：

```text
prepared → restoring-db → restoring-files → committed
                         ↘ failed-partial
```

失败时必须能够报告已经完成的阶段，而不是只返回笼统 `failed`。

#### 7.1.6 抑制专项

已确认：

- `suppress_thread` 和 `unsuppress_thread` 都是读取整个文件、修改、覆盖。
- `atomic_write` 使用固定 `<name>.tmp`，只保证单写者替换，不提供并发互斥。
- 抑制写错误被忽略，路由仍返回完整列表和成功响应。
- renderer 初始化时只把后端 ID 合并进本地 Set，不会删除本地已经失效的 ID。

因此其他窗口执行撤销后，旧窗口仍可能永久通过 MutationObserver 删除已经恢复的会话行，直到页面完整刷新。

#### 7.1.7 项目移动专项

移动到项目的实际顺序：

1. 更新 SQLite `threads.cwd`。
2. 全文件重写 rollout 中的 session meta cwd。
3. renderer 修改 Codex global-state。
4. renderer 写 localStorage 项目投影并刷新 DOM。

移动到普通对话则顺序修改：

- `projectless-thread-ids`
- `thread-workspace-root-hints`
- `thread-writable-roots`
- `thread-projectless-output-directories`

这些 key 都采用“读取完整值、修改、完整写回”，两个窗口移动不同会话也可能互相覆盖。任一步失败都会留下部分状态。

数据层当前即使 rollout 更新失败，也可能返回 `status: moved` 并只提供 `rollout_error`；renderer 会继续执行后续 global-state 变更。

#### 7.1.8 Provider 同步专项

Provider 同步已经创建完整备份，但当前错误恢复链并没有真正消费这些备份：

1. `tmp/provider-sync.lock` 以目录存在性作为互斥，`owner.json` 虽记录 PID 和时间，但获取锁时不检查 owner 是否仍存活，也没有租约超时。进程崩溃后，同步会持续返回 `Skipped`，直到人工删除锁目录。
2. rollout 文件先逐个覆盖；后续失败时只调用 `restore_session_changes()`。
3. SQLite 在每个数据库内部使用事务，但多个数据库按顺序分别提交。第二个数据库、global-state 写入或 `prune_backups()` 失败时，前面已经提交的数据库不会从备份恢复。
4. `.codex-global-state.json` 采用整文件读改写；写入完成后如果后续备份清理失败，也不会自动恢复。
5. 所有内部错误最终都包装为 `ProviderSyncStatus::Skipped`，该状态不能表达“未执行”和“已经发生部分写入后失败”的差异。

因此，“已经生成备份”不是恢复闭环。需要引入：

- 带 owner PID、启动时间和租约的可回收锁。
- `operation_id`、阶段日志和 `prepared/applying/committed/failed-partial` 状态。
- 对每个已提交 SQLite、global-state 和 rollout 的补偿记录。
- Manager 中可见的恢复入口，且恢复操作本身需要同一资源锁。

#### 7.1.9 本域必测场景

1. 两窗口同时删除同一 thread，验证响应、备份 token、所有 SQLite 和 rollout 最终状态。
2. 两窗口同时删除不同 thread，验证抑制集合不丢更新。
3. 一窗口删除、另一窗口立即撤销，验证所有窗口都重新显示。
4. 同一 token 并发撤销，验证 token 消费和恢复幂等。
5. 第二个数据库故意锁冲突，验证删除不能错误报告整体成功。
6. 活跃会话持续追加 rollout 时执行删除、移动和导出。
7. 同一 thread 同时移动到项目 A、项目 B、普通对话。
8. 两个不同 thread 同时移动到普通对话，验证 global-state 不丢条目。
9. SQLite 更新成功后让 global-state 写入失败，验证可恢复状态和 UI 提示。
10. thread 只存在于非首选数据库时，验证列表、删除、移动、导出和排序选择一致。
11. Manager 手动 Provider 同步期间持续创建消息、移动项目和修改 global-state。
12. Provider 同步在第一个 SQLite 提交后让第二个 SQLite 失败，验证第一个数据库、rollout 和 global-state 全部恢复。
13. Provider 同步在 global-state 写入后让备份清理失败，验证不能以 `Skipped` 掩盖部分提交。
14. 同步进程在持锁期间被终止，下一次启动能够验证 owner 并安全回收陈旧锁。

#### 7.1.10 待验证

- Codex 原生 global-state API 是否包含内部锁、CAS 或跨窗口广播。
- `refresh-recent-conversations-for-host` 的作用域是当前 renderer 还是整个 host。
- Codex SQLite 的实际 journal mode 和 busy timeout。
- Windows 活动 rollout 文件句柄的共享删除、替换和覆盖行为。
- Codex 是否会从另一个活动窗口内存重新创建已删除会话。

### 7.2 模型、协议、本地代理与设置

#### 7.2.1 总结

协议转换本身已有较完整的单请求测试，但“全局配置、窗口缓存、请求快照和实际代理监听状态”之间没有一致性协议。多窗口后主要风险是：

- Manager 整体保存与 renderer 字段更新互相覆盖。
- 一个窗口修改配置后，其他窗口继续使用旧设置和旧模型能力。
- provider/relay 是全局状态，但 UI 没有明确提示影响全部窗口的新请求。
- Fast 的 per-thread 和 draft 状态依赖 localStorage 整对象读改写。
- 本地代理的 desired、listening 和 effective port 可能处于半生效状态。

#### 7.2.2 当前协议语义

| 功能 | 当前语义 |
|---|---|
| 模型协议归属 | 优先使用 `modelMappings`；否则读取 Responses、Chat Completions、Anthropic 三个模型列表 |
| 未映射模型 | 后端 fail-closed，明确返回“没有协议归属” |
| Responses | 直接请求 Responses 上游，不翻译为 Chat |
| Chat Completions | 将 Responses 请求转换为 Chat 请求 |
| Anthropic | 转换为 Messages API，并按模型能力处理 thinking |
| Context window | 主要来自 model mapping；仅部分必须模型有定向能力回填 |
| active relay/provider | 全局 `settings.json` 单值，新请求读取最新全局配置 |
| 请求中途切换 relay | 已开始请求持有自己的设置快照，不应中途迁移 |
| service tier/Fast | 每个 renderer 独立 patch 请求；状态来自窗口缓存和 localStorage |

#### 7.2.3 风险矩阵

| 风险 | 等级 | 已确认结论 | 设计约束 |
|---|---|---|---|
| 跨进程 settings 丢更新 | P0 | Manager 进程内锁不覆盖 launcher；Manager 整体 `save` 可覆盖 renderer `/settings/set` 的新字段 | 所有写入统一经过 revision/CAS 事务入口 |
| settings 写临时文件竞争 | P1 | 所有进程使用同一个固定 `.tmp` 路径 | 唯一临时名 + OS 级跨进程锁 |
| 其他窗口设置不收敛 | P1 | renderer 只在启动/refresh 拉后端设置，没有配置广播 | 设置成功后广播 revision，窗口拉取最新快照 |
| 模型目录和能力缓存过期 | P1 | 每窗口缓存只有最短刷新间隔，没有周期失效；Manager 改映射后旧窗口可能继续展示旧模型 | 广播 `model-catalog-invalidated`，每 target 主动刷新 |
| 本地代理半生效 | P1 | helper 监听端口和是否启动在 launcher 启动时决定；Manager 保存不会启动、停止或重绑定 helper | 显式建模 desired/listening/effectivePort，原子切换或明确要求重启 |
| Fast per-thread 状态丢更新 | P1 | localStorage entries 整对象读改写；多窗口并发写不同 thread 可能互相覆盖 | 后端 revision 或至少合并/CAS |
| 新会话 Fast draft 串绑 | P1 | storage area 只有一个 60 秒 draft；首个获得 thread ID 的窗口会消费 | draft 携带 target/window identity |
| provider/relay 作用域不明确 | P2 | 当前为进程全局，所有窗口新请求共同使用新 relay | 产品和 UI 明确“影响所有窗口的新请求” |
| Anthropic 能力缓存跨供应商污染 | P1 | 兼容缓存只以模型名为 key，相同模型名的不同 relay 共用上限 | key 至少包含 relay ID、endpoint 和 model |
| service tier 上游兼容性 | P2 | Fast 主要按模型能力判断；Chat 上游是否支持 `service_tier=priority` 取决于供应商 | 能力按协议和 provider 建模，不能只看模型名 |
| UI 与后端 fail-closed 文案冲突 | P1 | UI 声称“未列入模型按当前协议转发”，后端明确拒绝未映射模型 | 保持后端 fail-closed，修正 UI 文案 |

#### 7.2.4 settings 写入因果链

Manager：

```text
加载 settings V1
→ React 表单长期持有完整快照
→ 修改部分字段
→ 提交完整 BackendSettings
→ SettingsStore.save 全量覆盖
```

renderer：

```text
/settings/set
→ SettingsStore.update
→ 重新读取当前文件
→ 合并一个或少数字段
→ 全文件覆盖
```

Manager 的 `settings_write_mutex` 只在 Manager 进程内有效，不能串行 launcher 进程中的 `SettingsStore.update`。因此典型竞态为：

1. Manager 加载 V1。
2. 窗口 A 写入 Fast 开关，生成 V2。
3. Manager 用仍基于 V1 的表单保存 relay。
4. V2 的 Fast 开关被恢复成旧值。

`atomic_write` 只能保证单次替换过程，不具备版本比较和跨进程事务能力。

#### 7.2.5 窗口缓存收敛

每个 renderer 独立维护：

- `codexElvesBackendSettings`
- `codexModelCatalog`
- service-tier patch 安装状态
- per-thread Fast 状态缓存
- UI 中的模型与能力展示

当前 `storage` 监听只处理 `codexElvesSettings` 和 `codexThreadServiceTierOverrides`，不感知 Manager 或其他进程对 `settings.json` 的修改。

后果：

- Manager 关闭 Fast 控制后，旧窗口可能继续 patch 请求。
- Manager 删除或修改模型映射后，代理立即按新设置路由，但旧窗口仍可能展示和发送旧模型。
- 旧窗口发送已删除模型时，后端会按照 fail-closed 规则拒绝，形成 UI 和后端不一致。

需要统一事件：

```text
settings-changed(revision, changed_fields)
model-catalog-invalidated(revision)
relay-changed(revision, relay_id)
```

窗口收到事件后应拉取后端快照，而不是直接相信事件携带的部分字段。

#### 7.2.6 本地代理动态启停

helper 是否启动和实际监听端口在 launcher 启动阶段确定。Manager 保存设置后会同步部分配置文件，但不会启动、停止或重绑定 helper。

高风险条件：

- launcher 启动时增强和协议代理均关闭，helper 未启动；运行中打开本地代理。
- launcher 以非默认 helper 端口启动；运行中打开本地代理后配置改指向固定代理端口。

这可能形成：

```text
desiredEnabled = true
config base_url = 127.0.0.1:45221
listening = false 或监听在其他端口
```

正确状态模型至少包含：

- `desired_enabled`
- `process_running`
- `listening`
- `effective_port`
- `config_points_to_effective_port`
- `restart_required`

在监听未确认前不能提前把 live Base URL 指向不可用端口。

#### 7.2.7 Fast 多窗口状态

per-thread entry 虽然按 thread ID 区分，但整个 entries 对象仍采用读取、修改、完整写回。两个窗口同时修改不同 thread，也可能丢失一个更新。

新会话在获得正式 thread ID 前使用单个 `draft`：

- draft 有效期 60 秒。
- 首个获得正式 thread ID 的窗口会绑定并清空 draft。
- 若两个窗口同时创建新会话并选择不同模式，存在串绑风险。

如果实机确认多个 Codex 窗口共享 storage area，draft 必须增加 `target_id` 或随机 `draft_id`；如果 storage area 不共享，也仍应把作用域语义写入测试。

#### 7.2.8 Anthropic reasoning 兼容缓存

当前进程级缓存 key 只有标准化模型名。例如：

```text
relay A / claude-x → 上游仅支持 high
relay B / claude-x → 上游支持 max
```

一旦 relay A 的兼容重试把 `claude-x` 缓存为 `high`，relay B 的后续请求也可能继续被钳制到 `high`。

缓存必须至少按以下组合隔离：

```text
(relay_id, normalized_endpoint, model)
```

relay 配置 revision 变化时应清理对应缓存。

#### 7.2.9 审计边界更正：thinking depth

此前版本把 `AGENTS.md` 中“派发 subagent 时的推理等级限制”误当成 CodexElves 产品的模型能力规格，因此错误地将产品支持 `max` / `ultra` 定性为 P0 功能缺陷。该判断已撤销。

本审计只检查 thinking depth 在产品内部是否满足以下一致性要求：

- UI 展示、模型能力表、请求转换和上游降级行为一致。
- 不同 provider、relay、endpoint 的兼容性缓存互不污染。
- 用户选择的等级在不受支持时有明确、可恢复的降级或错误反馈。

除非另有独立的产品需求或用户可见规格明确限制等级，否则 `AGENTS.md` 不作为产品功能缺陷的判定依据。

#### 7.2.10 本域必测场景

1. 两窗口同时修改不同 settings 字段，最终两个字段均保留。
2. Manager 持有旧表单时，renderer 修改 Fast，再保存 relay，Fast 不得回滚。
3. Manager 修改模型映射后，所有窗口自动刷新模型列表和 Fast 能力。
4. 删除旧模型后，旧窗口不得继续展示或发送该模型。
5. 请求执行中切换 relay：旧请求使用旧快照，新请求统一使用新 relay。
6. 运行中启用/关闭本地代理，验证 desired、listening、effectivePort 和 Base URL 原子一致。
7. 两窗口同时为不同已有 thread 设置 Fast，两个 entry 均保留。
8. 两窗口同时创建新会话并选择不同 Fast，draft 不串绑。
9. 相同 Claude 模型名在两个供应商上具有不同 thinking 上限，缓存不互相污染。
10. UI、能力表、协议转换和上游兼容降级对 thinking depth 的语义一致。
11. UI 和代理对未映射模型采用一致的 fail-closed 语义。
12. 聚合 relay 的轮转在多窗口下符合明确的全局语义。

#### 7.2.11 待验证

- 多 Codex 主窗口是否共享同一 localStorage storage area。
- 动态 import 得到的 dispatcher/prototype 是否可能跨 BrowserWindow 共享对象。
- Chat Completions 上游对 `service_tier=priority` 的实际兼容范围。
- Windows 两个进程同时操作同一个 `settings.json.tmp` 时的具体错误模式。

### 7.3 用户脚本、插件、皮肤和 renderer UI

#### 7.3.1 总结

renderer 内的 DOM、模块 patch、observer、timer 和 `sessionStorage` 大部分已经按 `window` 作用域设计。只要每个 target 获得独立注入，这些状态天然隔离。

主要风险不是“两个窗口互相污染”，而是：

- 本应作用于全部窗口的变更只执行到一个 target。
- 后端共享状态没有 revision 或广播。
- 用户脚本没有 dispose 生命周期。
- 部分全局配置修复会和其他配置写入竞争。
- upstream worktree 等高成本操作没有资源级调度。

#### 7.3.2 功能分类

| 功能 | 正确作用域 | 当前主要风险 |
|---|---|---|
| CodexElves 菜单和弹窗 | target-local | 第二窗口没有独立安装 |
| 工具与插件 Tab | target-local UI + 全局设置 | 设置写入和广播不一致 |
| 插件市场 request patch | target-local | 每个窗口都需独立完成动态模块 patch |
| 用户脚本配置 | 全局 | Manager/launcher 跨进程配置竞争 |
| 用户脚本执行 | 每 target 实例 | reload 单 target、无 dispose |
| 皮肤和 overlay 配置 | 全局 | 即时应用只推一个 target |
| service-tier request patch | target-local | 状态同步依赖 localStorage 假设 |
| 会话侧边栏 DOM | target-local | 共享抑制和项目投影不收敛 |
| DevTools | 来源 target-local | 当前重新挑选任意页面 |
| 打开 Manager | 进程级 | 无明显多窗口风险 |
| upstream worktree 草稿 | target-local `sessionStorage` | 正确隔离 |
| upstream worktree 执行 | repo/global 资源 | 无 per-repo 调度，阻塞式 Git 命令 |
| MutationObserver、timer | target-local | 多窗口资源线性增长，需要容量门禁 |

#### 7.3.3 风险矩阵

| 功能 | 等级 | 已确认风险 | 设计约束 |
|---|---|---|---|
| 菜单、弹窗和 DOM 增强 | P3 | 单窗口内有版本标记和重复菜单清理，幂等基础较好 | 每个 target 独立安装和 dispose |
| 用户脚本 reload | P0 | launcher 和 Manager 都只执行到一个 target | 明确全 target 广播，返回 per-target 结果 |
| 用户脚本生命周期 | P1 | wrapper 只记录 loaded/failed，没有 dispose、revision 或实例身份 | 引入脚本实例协议；旧脚本不能安全重载时提示刷新 |
| 用户脚本配置并发 | P1 | `UserScriptManager` 锁只属于单实例；Manager 每个命令重新创建 manager，且与 launcher 跨进程不共享锁 | 统一后端配置服务和跨进程锁 |
| 脚本市场安装原子性 | P1 | 先写脚本文件，再写安装配置；配置失败会留下默认启用的孤立脚本 | staging + 配置事务；失败时回滚脚本文件 |
| 皮肤和 overlay | P0 | Manager 即时应用只选一个 target | coordinator 全 target 广播，不重装整套 Bridge |
| 插件市场配置修复 | P1 | 修复会 read-modify-write `config.toml`，与 relay/provider/config 保存没有共享跨进程锁 | Codex home 配置写统一事务入口 |
| 插件市场 runtime patch | P2 | patch 为 target-local；每个窗口可能独立等待动态模块最长 60 秒 | 安装过程可观测、有界重试，不阻塞 Bridge ready |
| service-tier badge | P1 | 请求 patch 是 target-local，但业务状态同步依赖未验证的 storage partition | 后端 revision 广播；每窗口刷新自身 patch |
| DevTools | P1 | `/devtools/open` 与请求来源 target 无绑定，可能打开另一窗口 | 使用 `RequestOrigin.target_id` |
| 打开 Manager | 信息 | 只启动/唤醒 Manager 单例 | 保持现有行为 |
| upstream worktree | P1 | prepare/create 直接执行阻塞式 fetch、SSH、Git 命令；无 per-repo single-flight | 按 repo/project 资源键串行；spawn_blocking；operation ID |
| observer、timer 和扫描 | P2 | 单窗口清理较完整，但窗口数量会线性增加扫描、心跳和动态 patch | 8 窗口资源门禁和可观测计数 |

#### 7.3.4 renderer 幂等与清理

正向结论：

- 菜单使用版本标记并主动删除重复节点。
- 入口顶部会清理大量 timeout、observer、resize/storage listener。
- 同 build、helper 和 manager discovery version 下走 `__codexElvesRefreshRuntime`，避免整套重复安装。
- plugin request ID Map、service-tier patch 标记、statsig patch 和 DOM 状态均挂在当前 `window`，不同 target 不共享。
- upstream worktree 草稿使用 `sessionStorage`，符合 target-local 语义。

仍需补齐：

- 部分匿名监听器没有可引用 handler，无法对称移除。
- 插件市场的 `message`/bridge patch 和第三方用户脚本没有统一 dispose。
- launch cycle 已注入但未完整参与 runtime identity 判断。
- runtime 替换期间的 pending UI 操作没有统一取消错误。

建议每个 target 维护：

```text
RendererRuntime
├─ build
├─ launch_cycle
├─ bridge_generation
├─ installed_features
├─ disposers[]
├─ user_script_revision
└─ timers/observers/listeners metrics
```

#### 7.3.5 用户脚本

当前 wrapper 每次执行都会覆盖状态记录并再次执行脚本正文：

```text
status = loading
→ 执行脚本
→ status = loaded/failed
```

没有：

- `instance_id`
- `bundle_revision`
- `dispose()`
- reload 前清理
- 每 target 已应用 revision

多窗口语义应固定为：

- 新 target 初次加入：执行一次当前 bundle revision。
- 配置启停：更新全局清单 revision，并广播 inventory refresh。
- 显式 reload：对全部 target 执行新 revision。
- 同一 target 同一 revision：不得重复执行。
- 无 dispose 的旧脚本：默认不热重载，提示刷新对应窗口；如继续兼容重载，必须明确可能重复副作用。

配置层还有一项明确竞态：

- launcher 长期持有一个 `UserScriptManager`。
- Manager 的每个 Tauri 命令都会重新构造 `UserScriptManager`。
- 每个 manager 实例拥有独立 `Arc<Mutex<()>>`。
- launcher 和 Manager 更是不同进程。

因此现有锁不能保护跨命令和跨进程 read-modify-write。

脚本市场安装还分两步：

1. 原子写脚本文件。
2. 更新 `user_scripts.json` 安装记录。

如果第 2 步失败，第 1 步不会回滚；扫描脚本时未配置的新文件默认启用，可能出现“安装命令报告失败，但脚本下次注入仍执行”。

#### 7.3.6 插件市场

renderer 侧插件市场 patch 是 target-local，多个窗口各自 patch 没有共享 Map 冲突。

Manager 的市场初始化和修复会：

- 下载或释放 marketplace 目录。
- staging 后替换目录。
- read-modify-write `config.toml` 注册 marketplace。

目录替换有 staging/backup 基础，但 `config.toml` 写入没有与 relay 切换、context 配置和 provider sync 共用的跨进程事务锁。多窗口不是唯一触发源，Manager 并发命令和 launcher 启动同步也可能互相覆盖配置段。

修复成功后应广播：

```text
plugin-marketplace-invalidated(revision)
```

每个 target 自行失效 native query cache 和刷新插件 UI，不能重新安装整个 renderer runtime。

#### 7.3.7 皮肤、背景和 overlay

配置是全局的，DOM overlay 是 target-local 的。

正确流程：

```text
保存全局皮肤 revision
→ coordinator 广播 revision
→ 每个 target 执行 apply overlay
→ 汇总 per-target 结果
```

当前 Manager 只挑一个 target 执行脚本，其他窗口只能等待导航、重注入或重启。

广播脚本应只更新：

- `__CODEX_ELVES_IMAGE_OVERLAY__`
- `__codexElvesApplySkinAppearance()`
- `__codexElvesApplyImageOverlay()`

不应借皮肤变化重新安装 Bridge、用户脚本和所有 feature。

#### 7.3.8 DevTools

当前真实 launcher 路径会重新枚举 target 并调用宽松 `pick_page_target`，与发起请求的窗口无关。

多窗口后可能出现：

```text
用户在窗口 B 点击“打开 DevTools”
→ 后端重新选择 target
→ 打开窗口 A 或其他最高分页面的 DevTools
```

该路由必须使用 Bridge handler 捕获的 `RequestOrigin.target_id`。目标不存在时明确返回“来源窗口已关闭”，不能退化为任意选择另一个 target。

#### 7.3.9 upstream worktree

renderer 的选择草稿使用 `sessionStorage`，多窗口隔离正确；但执行层需要单独治理。

当前 prepare/create 会同步执行：

- `git fetch`
- branch/path 预检查
- `git worktree add -b`
- 远端项目场景下 SSH 命令

风险：

- renderer 选择一个 upstream 分支时会立即调用 `/upstream-worktree/prepare`，并固定传入 `fetch: true`；用户尚未确认创建 worktree，仓库就已经发生网络访问和 remote-tracking ref 更新。
- 在 async Bridge handler 中执行阻塞式 `Command::output`，会占用 Tokio worker。
- Bridge handler 会等待 Git/SSH 完成，当前 target 的后续 Bridge 请求也会被该长操作拖延。
- 两窗口同时操作同一 repo 时，预检查到真正执行之间存在 TOCTOU。
- 相同 branch/path 的第二个操作通常会被 Git 拒绝，但错误取决于执行时序。
- 不同 branch 并发 fetch 同一 ref 可能争用 Git lock。
- 无 operation ID，用户超时重试可能重复执行 fetch/create。
- 路由允许对用户选择的任意本地仓库路径或远端 project 执行 fetch/create。该能力本身属于产品功能，但在当前过宽 Bridge 权限下会显著放大 renderer 脚本失陷的影响范围。

需要：

- `(repo_root 或 project_id)` 资源键锁。
- Git/SSH 操作放入 `spawn_blocking`。
- `prepare` 默认只做只读校验；真正 fetch 必须绑定显式确认和短期 operation token。
- create 使用 operation ID 幂等。
- 返回 `already-created`、`conflict`、`failed-partial` 等明确状态。

#### 7.3.10 本域必测场景

1. 两窗口都出现唯一 CodexElves 菜单，不重复。
2. 同一 target 重注入、刷新、runtime generation 变化后 listener/observer/timer 数量不增长。
3. 用户脚本配置并发启停不同脚本，不丢字段。
4. 脚本市场在“文件已写、配置写失败”时不留下可执行孤立脚本。
5. reload 时一个 target 关闭，其他 target 成功且 Manager 显示 partial。
6. 用户脚本分别覆盖有 dispose、无 dispose、抛错、删除、禁用和连续 reload。
7. 皮肤切换在全部窗口即时生效；一个窗口失败不影响其余。
8. 插件市场修复与 relay/config 保存并发时不丢配置段。
9. A/B 分别打开 DevTools，目标与来源窗口一致。
10. 两窗口同时对同一 repo 创建相同 branch/path，只有一个成功且无残留。
11. 两窗口对同一 repo 创建不同 branch，并发受到 per-repo 调度。
12. 8 个窗口同时运行 observer、心跳、Token 和动态 patch，资源使用有界。
13. 只选择 upstream 分支但取消创建时，不得执行 fetch 或修改 remote-tracking ref。
14. renderer 中非 worktree 功能脚本不能直接调用 fetch/create 能力。

#### 7.3.11 待验证

- 多窗口实际是否共享同一 localStorage partition。
- Codex 动态 import 的模块实例是否存在跨 BrowserWindow 共享情况。
- 插件市场原生 query-cache invalidate 是否可能由 host 广播。
- 远端 worktree SSH 服务是否具备自己的 operation 幂等协议。

### 7.4 Manager 与 launcher 控制面

#### 7.4.1 总结

Manager 和 launcher 的单实例、托盘隐藏与进程分离整体可复用；主要风险集中在：

- Manager 命令静默只作用于一个 target。
- 状态模型只有一个进程级快照，不能表达 target 集合和部分失败。
- “重启 Codex”按进程扫描终止所有匹配进程，缺少启动归属。
- launcher 陈旧实例恢复依赖启发式外部信号，存在启动竞态。
- 持久配置全局生效，但运行时热更新只作用于一个窗口。

#### 7.4.2 进程与窗口模型

当前模型是：

```text
一个 Manager 单例
        +
一个 launcher 单例
        +
一个 helper / protocol proxy
        +
一个 Codex 应用进程
        +
同一 debug_port 下多个 page target
```

因此多窗口不应创建多个 launcher 或 helper，而应由单个 `BridgeCoordinator` 管理同一 debug port 下的 target 集合。

#### 7.4.3 风险矩阵

| 风险 | 等级 | 已确认结论 | 设计约束 |
|---|---|---|---|
| Manager 热更新静默单选 target | P0 | 用户脚本 reload、皮肤即时应用和部分 DevTools 路径只挑一个最高分 target | Manager 调用 coordinator 广播或指定来源 target，返回 per-target 结果 |
| 状态被健康窗口掩盖 | P1 | watchdog 和 `LaunchStatus` 没有 target 维度 | 状态包含 observed/managed/healthy/installing/failed 数量 |
| `LaunchStatus` 退出后陈旧 | P1 | 状态只有 running/running_degraded/failed；Codex 退出后不会主动写 stopped | launcher 生命周期结束写 stopped，并记录结束原因 |
| “重启 Codex”终止所有匹配进程 | P1 | Windows 下按可执行路径枚举并终止所有匹配 Codex/ChatGPT 主进程 | 跟踪 launcher 拥有的主 PID；不终止无归属实例 |
| 持久配置与运行时状态分裂 | P1 | relay/config 写盘后影响全部窗口新请求，但皮肤/脚本等运行时刷新只有单 target | 明确持久化成功与 runtime broadcast 两阶段结果 |
| 单实例陈旧判断竞态 | P2 | 旧 launcher 初始化中可能暂时没有 Codex 进程和 CDP 监听，被启发式判断为 stale | 使用 OS 级进程身份/互斥；恢复前验证 owner PID 和启动代次 |
| Manager 关闭误停 launcher | 信息 | 关闭按钮只隐藏到托盘，不终止 launcher/helper | 保持现有行为并增加回归测试 |

#### 7.4.4 Manager 命令语义

必须明确区分：

| 命令 | 目标语义 |
|---|---|
| 打开 Manager | 进程级；复用 Manager 单实例 |
| 打开 DevTools | 来源 target-local |
| 重载用户脚本 | 全 target 广播并返回每 target 结果 |
| 即时应用皮肤 | 全 target 广播；不能重新安装整个 Bridge |
| 保存 settings | 全局 revision 更新；随后广播配置失效 |
| 切换 relay/provider | 全局；影响所有窗口后续新请求 |
| 插件市场修复 | 全局配置修复；随后通知所有 target 刷新插件状态 |
| repair backend | 触发 coordinator 全量 reconcile，不只是返回 status |
| restart Codex | 只终止当前 launcher 拥有的 Codex 主进程，并明确会关闭该进程的全部窗口 |

广播返回建议：

```json
{
  "status": "partial",
  "revision": 42,
  "targets": [
    { "targetId": "A", "status": "ok" },
    { "targetId": "B", "status": "failed", "message": "target closed" }
  ]
}
```

不能因一个窗口成功就向 Manager 显示“全部即时生效”。

#### 7.4.5 重启与进程归属

当前 Windows 重启流程：

```text
stop_launcher_processes_and_wait
→ find_codex_processes
→ 终止所有匹配的 Store/便携 Codex/ChatGPT 主进程
→ 启动新 launcher
```

同一 Codex 进程内的所有窗口随进程退出是预期行为；真正风险是扫描范围可能包含并非当前 launcher 拉起的其他 Codex/ChatGPT 实例。

应在 launcher 启动后持久化：

- owner launcher PID
- Codex 主 PID
- debug port
- launch cycle
- executable identity

重启只操作匹配 owner 记录且仍通过身份校验的主 PID。

#### 7.4.6 状态模型

当前 `latest-status.json` 只包含：

- `status`
- `message`
- `started_at_ms`
- `debug_port`
- `helper_port`
- `codex_app`

多窗口后至少扩展：

```text
launcher_status
codex_process_alive
helper_status
observed_target_count
eligible_target_count
managed_target_count
healthy_target_count
installing_target_count
failed_target_count
last_reconcile_at_ms
last_reconcile_outcome
stopped_at_ms
stop_reason
```

Manager 概览应区分：

- 全部健康
- 部分窗口未注入
- 正在收敛
- helper 未监听
- Codex 已退出但保留历史启动记录

#### 7.4.7 单实例与陈旧恢复

launcher 当前以 loopback guard、文件 fallback、Codex 进程存在性和 CDP 监听状态组合判断旧实例是否陈旧。

待防御竞态：

1. 旧 launcher 已存活但 Codex 正在初始化。
2. 暂时没有可观察 Codex 主进程或 CDP listener。
3. 新 launcher 将旧实例判断为 stale。
4. 旧实例刚完成初始化时被终止或与新实例短暂并行。

改造 target coordinator 时不能进一步依赖“当前是否找到 target”来判断 launcher 所有权；target 暂时为空是合法生命周期状态。

#### 7.4.8 正向结论

以下行为已确认不需要因多窗口改变：

- Manager 关闭按钮只隐藏到托盘。
- 再次启动 Manager 会请求现有实例显示。
- Manager 与 launcher 生命周期独立。
- 关闭 Manager 不会停止 helper、watchdog 或 Codex。

#### 7.4.9 本域必测场景

1. 双窗口下从 Manager reload 用户脚本，验证 per-target 结果和部分失败提示。
2. 双窗口下即时切换皮肤，所有窗口更新且不重复安装 Bridge。
3. 从 A/B 分别打开 DevTools，只打开来源窗口。
4. 关闭被选中的旧 target，watchdog 只替换该 target，其他窗口不中断。
5. 手动关闭 Codex 后，Manager 状态变为 stopped，不继续显示 running。
6. 同时存在其他安装来源的 Codex/ChatGPT 时执行重启，不终止无归属进程。
7. launcher 初始化期间并发启动第二实例，最终只保留一个 owner。
8. watcher 开机启动与手动启动并发，收敛为一个 launcher/helper。
9. 关闭 Manager 到托盘，验证 launcher/helper/watchdog 不受影响。
10. relay/config 持久化成功但一个 target 广播失败时，Manager 明确显示 partial。

#### 7.4.10 待验证

- `find_codex_processes` 在 Store 版和便携版并存时的实际匹配边界。
- launcher stale 判定竞态的真实复现概率。
- 更新过程对 Codex、launcher、helper 和多个 target 的实际终止顺序。

### 7.5 总体风险优先级与实施顺序

本节只排序“多窗口方案本身”的风险。第二轮复核发现的现存安全和独立产品风险见第 8 节；其中 P0-S 同样会阻断发布，但不能错误归因于多窗口。

#### 7.5.1 P0-MW：不解决不能开始多窗口功能开放

1. **target registry 与严格 target 分类**
   - 管理全部合格主窗口。
   - 禁止第三方页面、overlay、DevTools 和未知 target 获得 Bridge。
2. **RequestOrigin**
   - target-local 路由必须携带 `target_id`、generation 和 execution context。
3. **共享写协调器**
   - settings 使用跨进程 revision/CAS。
   - 删除、撤销、移动按 `session_id` 排他。
   - Codex home 配置写使用统一事务入口。
4. **全窗口广播协议**
   - 设置、模型目录、用户脚本、皮肤、插件状态和会话变更使用显式 revision。

#### 7.5.2 P1：功能开放前必须完成

- 抑制列表从窗口本地权威状态改为后端 revision 快照。
- 项目移动建立可恢复状态机，避免 SQLite、rollout、global-state 分裂。
- 用户脚本增加 bundle revision、实例身份和 dispose/刷新兼容策略。
- 本地代理建模 desired、listening、effective port 和 restart-required。
- Anthropic reasoning 兼容缓存按 relay/endpoint/model 隔离。
- Manager 状态增加 target 集合、partial 和 stopped。
- restart 只终止 launcher 拥有的 Codex 主 PID。
- upstream worktree 使用 per-repo 调度、阻塞任务隔离和 operation ID。

#### 7.5.3 P2：稳定性和性能门禁

- Token 用量和排序请求按 session 全局单飞。
- 插件动态模块 patch 有界重试和 target 级耗时指标。
- 8 窗口 observer、timer、heartbeat 和路由并发容量验证。
- target fixture、stable/beta 兼容快照和未知 target 拒绝测试。

#### 7.5.4 推荐实施阶段

```text
阶段 0：动态证据补齐
  ├─ 双窗口 CDP fixture
  ├─ localStorage partition
  ├─ target reload/recreate
  └─ webview/辅助窗口分类

阶段 1：基础设施
  ├─ BridgeCoordinator
  ├─ TargetEntry registry
  ├─ RequestOrigin
  ├─ broadcast revision
  └─ shared write coordinator

阶段 2：功能迁移
  ├─ settings/model/relay
  ├─ skin/user scripts/plugins
  ├─ session delete/undo/move
  ├─ DevTools
  └─ Manager status

阶段 3：并发与恢复
  ├─ operation ID
  ├─ partial recovery
  ├─ resource locks
  └─ process ownership

阶段 4：自动化与真实发布门禁
```

不能先把 `Option<BridgeRuntime>` 机械替换为 `HashMap`，再逐个修功能；来源窗口、共享状态和广播协议必须与 registry 同阶段设计，否则会产生更隐蔽的串窗和数据一致性问题。

## 8. 第二轮复核：此前未覆盖的产品功能与安全边界

### 8.1 审计边界与结论分类

本轮复核不再把项目开发约束直接映射成产品功能约束，而是只使用以下证据判断产品风险：

- 用户可见规格和项目内明确产品约束。
- 实际代码调用链、状态流、数据流和持久化行为。
- 现场进程、端口、HTTP 响应和本地文件证据。
- 已存在的测试断言。

风险按与多窗口的关系分为三类：

| 类型 | 含义 | 实施关系 |
|---|---|---|
| 直接放大 | 原本已存在的风险会因 target 数量、renderer 数量或并发请求增加而扩大 | 必须与多窗口基础设施同阶段处理 |
| 独立现存 | 单窗口下已经成立，多窗口不是根因 | 单独立项修复，不能塞进 BridgeCoordinator 方案 |
| 待验证缺口 | 暂未证实存在缺陷，但现有测试不足以证明多窗口下仍正确 | 先补动态或并发证据，再决定是否改代码 |

### 8.2 功能覆盖复核表

| 功能域 | 此前覆盖情况 | 第二轮结论 | 与多窗口关系 |
|---|---|---|---|
| 本地 Helper HTTP / WebSocket | 未覆盖信任边界 | 已确认缺少请求认证、Origin/Host 限制，HTTP 使用通配 CORS | 直接放大 |
| renderer Bridge 设置读写 | 只覆盖来源 target 和并发写 | 已确认返回完整设置并允许广泛设置变更，包含敏感字段和高权限能力 | 直接放大 |
| Bridge binding 鉴权 | 只把 generation 当生命周期字段 | 已确认 generation 可省略，`executionContextId` 未参与校验，同页面脚本可直接调用全路由 | 直接放大 |
| 诊断报告与诊断日志 | 只覆盖可观测性字段 | 已确认诊断报告可包含 API key、auth/config 原文；日志无统一脱敏治理 | 独立现存 |
| 脚本市场 | 只覆盖安装原子性、reload 和孤儿脚本 | 已确认下载地址不受限、声明的 SHA-256 不校验、安装后默认启用 | 直接放大 |
| 插件缓存刷新 | 只覆盖配置并发和 reload | 已确认 plugin ID/source 缺少路径边界，递归复制/删除可作用于计算出的越界目录 | 独立现存 |
| GitHub Release 更新 | 只覆盖进程终止顺序 | 已确认前端可提交任意 Release 下载地址，后端下载后直接执行且不校验签名/摘要 | 独立现存 |
| 自更新生命周期 | 未覆盖 | 已确认安装器强杀 Manager/launcher，完成后只重启 Manager；launcher/Bridge 不自动恢复 | 独立现存 |
| CDP 控制面和额外启动参数 | 只覆盖 target 选择 | 当前监听为 loopback，但额外参数可覆盖/追加调试相关参数，缺少 reserved flag 策略 | 独立现存 |
| 远程 SSH destination | 只覆盖远程命令 shell quote | host/user 未拒绝 option-like 值；实际 OpenSSH 参数解释待动态验证 | 独立现存 |
| 本地代理日志 | 只覆盖 Manager 状态和并发容量 | 已确认明文保留请求/响应正文，缺少内容脱敏；清理语义仍有异步写回窗口 | 直接放大 |
| 分层压缩 / Continue Thinking | 未覆盖输入信任边界 | 已确认结构化压缩标记可从普通 user 文本触发协议历史展开 | 独立现存；并发部分待验证 |
| CLI wrapper | 未覆盖 | 已确认密钥嵌入 C# 源码/产物，禁用不撤销旧凭据，参数日志和 quoting 存在风险 | 独立现存 |
| 环境变量冲突清理 | 未覆盖 | 已确认把全部 `OPENAI_*` 视为冲突，备份不保存原值，删除失败无事务回滚 | 独立现存 |
| Computer Use Guard | 未覆盖 | 已确认会修改 Codex 配置、marketplace 和外部 `@oai/sky/package.json`，缺少完整恢复链 | 独立现存 |
| Manager Tauri command 权限面 | 未覆盖整体能力边界 | 已确认主窗口和允许的 localhost dev origin共享同一组高权限命令；CSP 为空且 asset scope 为 `**`，前端失陷会影响更新、会话、脚本、环境变量、watcher 和 relay 等全部功能 | 独立现存；条件型高危 |
| cc-switch 导入 | 未覆盖 | 已确认未进入 Manager 设置写锁，可能与保存/切换并发覆盖 | 直接放大 |
| 模型目录生成与同步 | 只覆盖窗口缓存失效 | 已确认设置保存与模型目录同步是两阶段非事务链，可能出现 settings 成功而目录失败 | 直接放大 |
| Provider 同步恢复与锁 | 只覆盖共享写并发 | 已确认陈旧锁不会自动回收；跨多个 SQLite/global-state 的后续失败只恢复 rollout，并以 `Skipped` 表达错误 | 直接放大 |
| 聚合 relay 轮转 | 只覆盖全局/窗口语义 | 已确认对话分配表无淘汰；多窗口会增加增长速度 | 直接放大 |
| upstream worktree | 只覆盖并发调度 | 已确认选择分支阶段即执行 fetch；高影响 fetch/create 路由缺少确认 token 和能力隔离 | 直接放大 |
| 会话撤销备份 | 只覆盖 token 重放和多库一致性 | 已确认备份包含完整表行和 rollout 原文，长期明文保留且成功撤销后不消费、不清理 | 独立现存 |
| 皮肤导入、导出和删除 | 只覆盖广播刷新 | 已确认写失败被忽略、导入图片无大小上限、删除皮肤不清理导入图片 | 直接放大 |
| 安装、修复与卸载 | 只覆盖 launcher 所有权和重启 | 已确认数据删除失败被忽略，且“托管数据”实际清理范围不完整 | 独立现存 |
| watcher 与安装器所有权 | 未覆盖 | 已确认标准卸载不移除 watcher 自启动项，安装/卸载按镜像名强杀进程 | 独立现存 |
| Codex Radar | 未覆盖 | 已确认缺少 singleflight 和失败时 stale fallback；属于 P2 稳定性 | 独立现存 |
| 会话删除、撤销、移动 | 已覆盖 | 第二轮未发现可替代原结论的新根因 | 直接放大 |
| 协议转换、模型归属、Fast | 已覆盖 | thinking depth 只保留产品内部一致性检查，不再引用 AGENTS 约束 | 直接放大 |
| watcher、launcher 单实例、进程归属 | 已覆盖 | 第二轮新增安装/卸载和 Guard 资源修改风险，不改变原多窗口 RCA | 部分直接放大 |

### 8.3 P0-S：本地 Helper 缺少调用者身份与来源边界

#### 8.3.1 事实

`crates/codex-elves-core/src/launcher.rs` 的 `handle_helper_connection`：

- 只解析 method、path、body 和 user-agent。
- 没有校验 bearer token、一次性 capability、Origin、Referer 或 Host。
- `/v1/responses`、`/v1/chat/completions` 会使用本机已保存的 relay 配置请求上游。
- `/backend/status`、`/backend/repair`、`/diagnostics/log`、`/overlay/image`、`/inject/renderer-features.js`、`/inject/user-scripts.js` 均由同一端口提供。
- 所有普通 HTTP 响应和预检响应均返回 `Access-Control-Allow-Origin: *`。

`crates/codex-elves-core/src/responses_websocket_bridge.rs` 的 `handle_responses_websocket_connection`：

- 校验了路径和 Upgrade 头。
- 在建立上游连接前后均未校验 WebSocket `Origin` 或 capability token。

现场 `Get-NetTCPConnection` 同时确认当前 CDP `51111` 和 Helper `45221` 均监听在 `127.0.0.1`，没有发现静态或当前运行时监听 `0.0.0.0` 的证据。这是有效的正向控制，但不能替代应用层调用者身份。

2026-08-04 对当前本地 Helper `127.0.0.1:45221` 做了无副作用现场检查，并携带 `Origin: https://evil.example`：

1. `OPTIONS /v1/responses` 返回 `204` 和通配 CORS。
2. `GET /inject/user-scripts.js` 返回 `200` 和通配 CORS。
3. `GET /backend/status` 返回 `200` 和通配 CORS。
4. `GET /overlay/image` 返回 `200 image/png`、通配 CORS，响应长度为 `10396643` 字节。

#### 8.3.2 RCA

- **现象**：任意网页可跨源读取 Helper 资源，并可向本地模型代理发起请求。
- **直接原因**：Helper 对 loopback 监听端口实行通配 CORS，且没有调用者认证。
- **根因**：把“仅监听 `127.0.0.1`”错误等同于“只有 Codex renderer 能访问”。
- **系统性根因**：HTTP、WebSocket、注入资源、图片资源、诊断入口和模型代理共用一个未分级的信任边界，缺少本地能力令牌和路由权限模型。

#### 8.3.3 影响

- 恶意网页可使用本机配置的上游 API key 消耗额度并读取模型响应。
- 可读取用户脚本 bundle，暴露本地自定义逻辑和内部实现。
- 可读取当前图片覆盖层，泄露用户本地选用的私人图片。
- 可向诊断日志写入大量或误导性内容。
- 多窗口会增加 Helper 的使用频率和连接数，但该风险在单窗口下已经成立。

需要严格区分两层结论：

- **已确认的服务端缺陷**：服务端没有认证和来源边界，并明确返回通配 CORS。
- **待验证的浏览器完整利用链**：Chrome/WebView2 的 Private Network Access、HTTPS 到 loopback 的 mixed-content 策略和 WebSocket 限制，可能阻止部分恶意网页场景；HTTP 恶意页面、本地 WebView 或本机原生进程不应依赖这些浏览器侧防线。

#### 8.3.4 修复约束

- 每次 launcher 启动生成高熵 capability token，不写入 URL 查询日志。
- Codex renderer 通过受控注入获得 token；Manager 使用独立能力或直接 Tauri command。
- 对 HTTP 校验 token、Host 和允许的 Origin；非必要路由不返回 CORS。
- WebSocket 校验 Origin，并通过受控 header 或 subprotocol 携带短期 token。
- 模型代理、注入静态资源、图片和诊断入口拆分权限，不能继续共用“端口可达即授权”。
- 增加请求速率、正文大小和诊断日志配额。

### 8.4 P0-S：Bridge 暴露完整设置和过宽写能力

#### 8.4.1 事实

- `crates/codex-elves-core/src/routes.rs` 的 `/settings/get` 通过 `serde_json::to_value(settings)` 返回完整 `BackendSettings`。
- `/settings/set` 把 renderer payload 交给 `SettingsStore.update`。
- `BackendSettings` 中 `relayApiKey`、`cliWrapperApiKey` 会序列化。
- `RelayProfile.api_key` 虽然使用 `skip_serializing`，但 `configContents`、`authContents` 会序列化，内容可包含 API key、访问令牌或完整认证 JSON。
- `SettingsStore.update` 允许修改应用路径、Codex home、额外启动参数、relay、CLI wrapper、overlay 等大量字段。
- 用户脚本与 CodexElves 注入运行在同一个 renderer JavaScript 上下文，可访问相同全局 Bridge 对象。
- Bridge 函数和底层 CDP binding 都挂在 `window` 全局对象上。
- `bridge_payload_matches_generation` 对缺失 generation 使用 `is_none_or`，因此调用者完全省略 generation 也会通过。
- `Runtime.bindingCalled` 事件包含的 `executionContextId` 没有进入 `prepare_binding_call` 校验。
- 当前 target 评分中，只要是非 avatar overlay 的 `app://-/` page，即使标题和 URL 不含 Codex/ChatGPT 也会获得 1 分并可能被选中。

#### 8.4.2 RCA

- **直接原因**：页面 Bridge 使用持久化实体 `BackendSettings` 直接充当 renderer DTO 和写入命令。
- **根因**：Bridge 按“页面是否已注入”授权，而不是按具体 UI 功能和 execution context 发放最小能力；generation 实际是生命周期标识，不是鉴权。
- **系统性根因**：缺少敏感字段分级、只读视图 DTO、命令 allowlist 和 capability ownership。

#### 8.4.3 影响与修复

任意在合格 renderer 中执行的脚本都可能读取凭据或修改高权限配置。多窗口实现若把同一完整 Bridge 注入更多 target，会直接扩大暴露面。

必须：

- 新建面向 renderer 的脱敏 `RendererSettingsView`，禁止返回原始 key、auth/config 内容和敏感路径。
- 将设置写操作拆成具体命令，只允许 UI 实际需要的字段。
- relay 凭据、CLI wrapper、路径和进程控制保留在 Manager 或需要显式挑战确认的能力中。
- 用户脚本运行上下文与高权限 Bridge 进行能力隔离；不能只依赖命名约定。
- generation 必须强制存在并匹配，但仍不能把它当 secret；同时校验主 frame/预期 execution context。
- target 分类必须使用主窗口必要条件 allowlist，禁止“任意 `app://-/` 页面兜底加分”。

### 8.5 P0-S：诊断报告和日志缺少敏感信息治理

#### 8.5.1 事实

- `apps/codex-elves-manager/src-tauri/src/commands.rs` 的 `diagnostics_report()` 直接写入 `"settings": settings`。
- `copy_diagnostics()` 把完整报告返回前端，正常使用路径是复制后提交或分享。
- 设置中可能存在 `relayApiKey`、`cliWrapperApiKey`、`authContents` 和 `configContents`。
- `write_diagnostic_event` 和 Helper `/diagnostics/log` 接受调用者提供的通用 JSON detail；当前只有事件名清洗，没有统一字段级脱敏。

#### 8.5.2 RCA

- **直接原因**：诊断导出复用了完整设置对象，日志入口接受无 schema 的任意 detail。
- **根因**：诊断系统只考虑“信息是否足够”，没有定义“哪些信息禁止离开进程或落盘”。
- **系统性根因**：缺少全局 secret classifier、redaction 层和带 canary 的回归测试。

#### 8.5.3 修复约束

- 使用显式 `RedactedDiagnostics` DTO，字段默认不进入报告，只有 allowlist 字段可导出。
- 对 key、token、Authorization、Cookie、auth/config 内容和疑似密钥模式统一脱敏。
- 诊断日志入口按事件定义 schema 和最大正文，拒绝任意深层 JSON。
- 回归测试写入多种 canary secret，断言报告、日志、错误和剪贴板结果均不包含原值。

### 8.6 P0-S：脚本市场缺少供应链完整性与权限边界

#### 8.6.1 事实

- `MarketScript` 包含 `script_url` 和 `sha256`。
- `download_script` 对 manifest 提供的 URL 直接执行 `reqwest::get`，没有域名或协议 allowlist。
- `install_market_script_content` 直接写入脚本文件，没有使用 `sha256`。
- 测试 `install_market_script_ignores_checksum_mismatch_and_replaces_existing_file` 明确固化了“忽略 checksum mismatch”的当前行为。
- `record_market_install` 对新脚本执行 `or_insert(true)`，安装后默认启用。
- `sanitize_market_id` 把多种标点统一替换为 `-`，不同市场 ID 可能映射到同一 `market-*.js` 文件。
- 市场脚本随后在拥有 Bridge 能力的 Codex renderer 中执行。

#### 8.6.2 RCA

- **直接原因**：市场索引同时控制下载 URL、文件身份和执行内容，客户端不验证内容摘要。
- **根因**：把脚本市场当成普通文件下载，而不是高权限代码供应链。
- **系统性根因**：缺少发布签名、内容寻址、权限声明、安装预览、默认禁用和回滚治理。

#### 8.6.3 修复约束

- 至少强制校验 SHA-256；更稳妥方案是对 manifest 和脚本建立签名链。
- 限制 HTTPS、可信 host、仓库和固定 revision，禁止任意跳转到未知源。
- 文件名使用原始 ID 的安全编码加哈希，消除 sanitize collision。
- 安装后默认禁用，展示来源、hash、权限和差异，用户确认后再启用。
- 市场脚本不得继承完整 Bridge；按声明授予最小能力。
- 安装和升级保留旧版本并支持原子回滚。

### 8.7 P0-S：更新安装包信任链由前端数据驱动

#### 8.7.1 事实

- `check_for_update` 从固定 latest JSON 地址读取 Release。
- Manager 前端保存 `assetUrl`，调用 `perform_update` 时重新构造完整 `Release` 并传回 Tauri。
- 后端命令 `perform_update(release: Option<Release>)` 直接信任前端提供的 `asset_url` 和 `asset_name`。
- `update::perform_update` 下载全部 bytes，写入本地文件后直接 `spawn` 安装包。
- 当前没有重新拉取可信元数据、固定下载域名、摘要校验、发布签名或 Windows Authenticode 校验。
- `tauri.conf.json` 的 CSP 为 `null`，asset protocol scope 为 `["**"]`；这会放大 Manager renderer 被注入或发生 XSS 后的影响。
- Tauri capability 还允许 `http://localhost:1420` 使用完整 Manager 命令集；`CODEX_ELVES_MANAGER_DEV=1` 可让非 debug 构建加载该远程页面。

#### 8.7.2 RCA

- **直接原因**：高权限后端命令接受并执行前端提交的下载地址。
- **根因**：更新检查结果没有由后端持有不可伪造的身份或一次性 token。
- **系统性根因**：缺少“可信元数据 → 内容摘要/签名 → 安装包执行”的完整更新信任链。

#### 8.7.3 修复约束

- 后端保存最近一次可信检查结果，前端只提交不可伪造的 release token。
- 执行更新前由后端重新校验仓库、域名、版本、asset 名和平台。
- 校验发布摘要或签名，并在 Windows 上验证 Authenticode 发布者。
- 下载采用唯一临时路径、大小上限和原子完成标记，失败清理。
- 为 Manager 配置明确 CSP，并收紧 asset protocol scope。
- 生产包禁止通过普通环境变量进入远程 dev URL；开发 capability 与生产 capability 分离。

### 8.8 P1：分层压缩标记可由普通 user 文本伪造

#### 8.8.1 已确认调用链

1. `synthetic_local_compaction_payload` 不只接受 `type == "compaction"`，还接受 `role == "user"` 的普通 message。
2. `structured_local_compaction_payload` 使用 `text.find("codex-elves-compaction-v3:")`，标记可出现在 user 文本任意位置。
3. 标记后的 JSON 被直接反序列化为 `StructuredLocalCompactionPayload`。
4. `expand_synthetic_local_compaction_request` 用 payload 中的 assistant summary 和 `retained_tail` 替换原 user message。
5. Responses、Chat Completions、Anthropic 转换和 native Responses 归一化均调用该展开函数。

#### 8.8.2 RCA

- **现象**：用户输入特定前缀和合法 JSON 后，可被代理解释成内部压缩历史，而不再是普通用户文本。
- **直接原因**：内部协议 sentinel 与不可信自然语言共用同一文本字段，且没有来源认证。
- **根因**：为兼容历史格式，解析器通过内容模式推断“这是 CodexElves 生成的内部对象”。
- **系统性根因**：内部状态没有独立类型、签名或不可伪造 metadata，测试只覆盖合法恢复，没有覆盖 sentinel spoof。

#### 8.8.3 影响与修复

payload 的 `retained_tail` 是任意 `Value` 列表，可能构造 assistant、developer、tool 等历史项，形成协议历史完整性风险。

修复时不能只把 `find` 改成 `starts_with`；必须取消普通 user message 的无认证恢复路径，或增加仅本地生成、不可由用户文本伪造的认证 metadata/HMAC。新增测试必须覆盖 user 文本、引用文本、工具输出和跨协议转换中的伪造标记。

### 8.9 P1：本地代理日志保留明文会话内容

#### 8.9.1 事实

- `ProxyRequestRecord` 保存 `request_body`、`response_body`、Continue Thinking 前后正文和分层压缩前正文。
- 请求正文最多保留 64 KiB；响应正文按默认或 full capture 策略可保留更大内容。
- 未发现对 prompt、代码、文件内容、工具输出、Authorization 或疑似 secret 的统一脱敏。
- 日志明细以文件形式长期保留在 CodexElves 状态目录。
- `clear_local_proxy_logs` 清理当前索引和明细，但后台异步日志 worker 仍可能在清理返回后写入已排队记录；“清理完成”不是严格操作栅栏。

#### 8.9.2 风险与修复

该功能可能把用户对话、源码、工具结果和密钥持久化到明文文件。多窗口会增加并发请求和记录量。

必须增加：

- 默认元数据模式，正文 capture 需要显式 opt-in 和醒目隐私提示。
- JSON/header/text 多层脱敏和 canary secret 测试。
- TTL、总容量和单会话保留策略。
- clear operation fence：先阻止/排空旧 generation 写入，再删除，保证返回后旧记录不会复活。
- Manager 中明确展示捕获状态、目录和剩余记录数。

### 8.10 P1：CLI wrapper 凭据生命周期和参数处理

已确认：

- API key 被写入 `codex-wrapper.cs`，并进入编译产物。
- `should_refresh_cli_wrapper` 在功能关闭但旧 wrapper 仍存在时仍返回 true。
- `wrapper_settings_for_refresh` 会从旧 C# 源码解析并继续使用旧 base URL 和 API key，因此关闭开关不等于撤销凭据。
- wrapper 把完整 CLI 参数写入 `codex-wrapper.log`，无脱敏和轮转。
- 手工 quoting 会把所有反斜杠翻倍，不符合 Windows argv 的完整转义规则，含空格路径可能被改写。
- 先写源码、后覆盖编译产物；编译失败可能留下“新源码 + 旧 exe”的分裂状态。

修复要求：

- 密钥不写入源码和二进制，改用受控环境或凭据存储。
- 禁用时删除或失效 wrapper 和旧凭据，不能自动沿用。
- 使用 `ArgumentList` 或平台正确 argv API，不手工拼接 `Arguments`。
- 日志只记录脱敏元数据并轮转。
- 源码和 exe 采用临时目录编译、验证后原子替换。

### 8.11 P1：环境变量冲突清理不可恢复

已确认：

- `is_codex_env_conflict_name` 把所有 `OPENAI_*` 变量视为冲突。
- 备份文件只保存名称、来源和 `value_present`，不保存原值。
- 删除按变量逐个修改当前进程和 Windows 用户环境；中途失败不会恢复已删除项。

因此当前“备份”只能证明变量曾存在，不能执行恢复，也可能删除与 CodexElves 无关的用户配置。应改成明确 allowlist、展示影响范围、加密保存原值、先生成可恢复事务记录，再执行删除并支持回滚。

### 8.12 P1：Computer Use Guard 修改外部运行时但缺少恢复闭环

已确认：

- Guard 会修改 Codex home 的 `config.toml`、本地 marketplace 配置和外部 `@oai/sky/package.json` exports。
- 运行时包优先根据 notify exe 推导，失败后按最新修改时间选择 `package.json`。
- 首次修改会生成 `package.json.bak-codexpp-runtime-exports`，但未发现对应的禁用/卸载恢复入口。
- launcher 启动后还会多轮重试 Guard，可能与 Codex 更新或其他配置写入并发。

风险包括修改错误版本、更新覆盖、共享文件写竞态和长期残留。修复需要版本身份校验、统一资源锁、操作日志、禁用/卸载恢复、备份版本匹配和更新后重新评估，不能只依赖“有一个 `.bak` 文件”。

### 8.13 P1：设置和派生文件存在多条非事务写链

#### 8.13.1 cc-switch 导入

`save_settings` 和 relay 切换会获取 `settings_write_mutex`，但同步函数 `import_ccs_providers` 未进入该锁，执行“读取 settings → 合并 provider → save”。它可与 Manager 保存、renderer `/settings/set` 或 relay 切换并发，产生丢失更新。

#### 8.13.2 模型目录同步

Manager 先 `store.save(&settings)`，随后才执行 `sync_applied_model_catalog_after_settings_save`。目录同步失败时设置已经成功，返回消息只追加警告，没有事务回滚或 pending/reconcile 状态。聚合 relay 分支还直接跳过该同步。

修复方向：

- 所有设置写入口使用同一跨进程 revision/CAS，不只覆盖 Manager 内部 mutex。
- 把 settings、Codex home config、模型目录和 relay 应用建模为 operation。
- 持久化 desired state，派生文件失败后进入明确 pending/retry，而不是返回“设置已保存”后依赖用户手工调整。
- 用 operation ID 和最终状态向所有窗口广播。

### 8.14 P1：聚合 relay 对话分配表无界增长

`RelayRotationSelector` 的 `conversation_assignments: HashMap<String, String>` 只插入、不删除、不限制容量。只要进程持续运行并出现新的 conversation ID，内存占用会持续增长；多窗口会加快增长速度。

应增加会话完成/过期回收、LRU/TTL 和容量上限，并明确多个窗口打开同一 conversation 时是否必须保持相同 relay。相关测试需覆盖配置 revision 变化、成员删除、同会话并发和十万级不同 conversation ID。

### 8.15 P1/P2：皮肤、卸载和 Radar 的独立功能风险

#### 8.15.1 皮肤

- `upsert_skin`、`delete_skin` 忽略 `write_list` 错误，Manager 仍可能显示成功。
- 导入先解码并写图片，再调用忽略错误的 `upsert_skin`；列表写失败会留下孤儿图片。
- base64 图片解码前没有输入或解码后大小限制。
- 删除皮肤不会删除由导入产生的图片。
- 皮肤列表读改写没有共享锁，多窗口/Manager 并发操作可能覆盖。

该项为 P1 数据一致性和资源治理风险。

#### 8.15.2 安装与卸载

- `uninstall_entrypoints` 在入口卸载成功后调用 `remove_owned_data()`，但忽略删除失败，最终仍返回成功。
- `remove_owned_data()` 只删除 `default_app_state_dir()`。
- `%APPDATA%\CodexElves` 用户脚本/皮肤配置、`~/.codex-elves-cli` wrapper/日志以及可能由 Guard 或 provider sync 创建的托管产物不在同一清单中。
- Windows 入口安装按“第一个快捷方式 → 第二个快捷方式 → 注册表”顺序执行，后续失败没有补偿前序产物。

需要先定义“入口卸载”和“完整卸载”的产品语义，再建立所有权清单、dry-run、逐项结果、失败状态和可重试清理。当前不能向用户承诺“托管数据已全部移除”。

#### 8.15.3 Codex Radar

Radar 请求有较长网络超时；Manager 缓存没有 singleflight，并发强制刷新会重复访问网络。失败时也没有返回最近一次 stale 数据。该项为 P2 稳定性问题，不阻断多窗口基础设施，但应补并发去重、短超时和 stale-while-revalidate。

### 8.16 待验证而非已确认缺陷

以下项目当前只确认“证据不足”，不能写成现存 bug：

1. Continue Thinking 的 HTTP 多轮状态主要是请求内状态，WebSocket coordinator 主要是连接内状态；尚需真实双窗口测试是否出现取消、relay 切换或日志关联串线。
2. 多窗口是否共享同一 storage partition，决定 Fast draft、localStorage 抑制项等冲突的实际概率。
3. `clear_local_proxy_logs` 与日志 worker 的复活窗口需要可控并发测试量化；正文隐私风险不依赖该测试，已经成立。
4. Computer Use Guard 是否会在当前 Codex 更新过程中实际命中错误 `@oai/sky` 版本，需要构造多版本 runtime fixture。
5. Windows Store、便携版和 beta 并存时，更新、安装、重启和进程归属的组合行为仍需实机验证。

其中 localStorage partition 不是普通低优先级待办，而是阶段 0 阻断证据：它直接决定 Fast draft、排序投影、项目投影等状态究竟是 target-local 还是跨窗口共享。未验证前不得冻结状态作用域设计。

### 8.17 追加复核：插件、CDP、SSH 和全局运行时

#### 8.17.1 P1：插件缓存刷新缺少路径 containment

已确认：

- `split_plugin_id` 只校验 `name@marketplace` 两段非空，不拒绝 `/`、`\`、`..`、根路径或前缀。
- `plugin_cache_root` 直接执行 `home/plugins/cache/<marketplace>/<name>`。
- marketplace manifest 中的插件 `source.path` 可为绝对路径或 `../`，`resolve_marketplace_path` 会原样接受绝对路径，或把相对路径直接 join 到 marketplace root。
- `force_refresh_plugin_cache` 会递归复制 `source.root`，替换目标目录，并删除 `cache_root` 下除当前版本之外的所有目录。

因此损坏或恶意的 plugin ID/source 可以让刷新操作越过预期 cache/source 根目录。实际删除范围应在临时 Codex home 中动态验证，但路径 containment 缺失已经闭环。

修复要求：

- plugin name、marketplace、version 使用严格 path segment 校验。
- 对 source、staging、destination、backup、cache root 全部 canonicalize，并断言位于明确根目录内。
- 递归删除前再次验证解析后的绝对路径，拒绝符号链接、junction 和 reparse point 越界。
- marketplace manifest 的本地 source 若确实需要根目录外路径，必须显式授权为只读来源，不能同时决定递归删除根。
- ZIP 下载虽限制压缩包为 128 MiB，但解压逐 entry `read_to_end`，还需增加解压后总量、文件数和单文件上限。

#### 8.17.2 P1：CDP 是高权限控制面，reserved flags 未治理

已确认当前现场 CDP 只监听 `127.0.0.1:51111`，没有发现当前局域网暴露。

但代码仍存在：

- launcher 固定追加 `--remote-debugging-port` 和 `--remote-allow-origins`。
- 没有显式追加 `--remote-debugging-address=127.0.0.1`。
- `codexExtraArgs` 只 trim 后原样追加，可再次传入 debugging port/address/origin 等冲突参数。
- 同机能连接 CDP 的进程可执行 `Runtime.evaluate`、添加脚本和 binding。
- 注入日志记录完整 target title 和 URL，查询参数可能进入诊断日志。

修复要求：

- 固定显式 loopback 地址。
- 对 `codexExtraArgs` 建立 reserved flag 拒绝清单，禁止覆盖调试端口、监听地址、allow origins、user data dir 等安全边界。
- 启动后检查真实监听地址和进程命令行，不符合预期立即降级并停止注入。
- target 日志只记录分类和脱敏 URL，不记录完整查询参数。

#### 8.17.3 P1 待动态确认：SSH destination 的 option-like 参数

远程 Git 命令正文已经使用 `shell_quote`，未发现正文拼接注入；遗漏发生在 SSH destination：

- host 校验拒绝空白、控制字符和部分 URL 字符，但不拒绝前导 `-`。
- user 没有独立字符校验。
- `user@host` 直接作为 `ssh` 参数，前面没有显式参数边界保护。

需要用假的 `ssh.exe` 记录 argv，并在隔离环境验证 Windows OpenSSH 对 `-o...` 等 option-like destination 的解释。在证据闭环前，只能定性为参数边界缺失和高风险待验证项，不能直接宣称已实现命令执行。

#### 8.17.4 P1：Manager、DevTools 和重启命令语义不一致

- `LauncherRuntimeService.open_devtools` 重新枚举并调用 `pick_page_target`，可能打开与来源窗口无关的页面。
- `CoreRuntimeService.open_devtools` 使用预配置 `devtools_target_id`，两条实现语义不同。
- `restart_codex_elves` 会调用全局 `stop_launcher_processes_and_wait` 和 `stop_codex_processes_and_wait`，因此一次重启会影响所有窗口，不是窗口局部操作。
- Manager wake guard 接受 loopback 单字节命令即可显示窗口或弹出更新入口，没有 nonce；当前主要是骚扰/审计可信度风险。

多窗口 UI 必须把“重启全部 Codex 窗口”明确展示为进程级破坏性操作，增加确认和 operation 状态。DevTools 只能使用 RequestOrigin 的 target ID，删除重新 pick 路径。

#### 8.17.5 P1：Codex `config.toml` 和 WebSocket capability 仍有独立写链

- `ensure_marketplace_configs` 对 Codex home `config.toml` 执行独立读改写和 atomic write。
- relay 应用、模型目录、base URL、context 管理也会读写同一 `config.toml`。
- 这些入口没有统一的跨进程资源锁和 revision。
- Responses WebSocket 手动 probe 在 Manager 内部获取 `settings_write_mutex`，但其持久化仍是“重新 load 全量 settings → 修改一个 profile → save”，无法与 renderer 或 launcher 跨进程写形成 CAS。

因此“Manager 内部有 mutex”不能作为全局正确性证明。`settings.json` 与 Codex `config.toml` 必须分别建立统一写协调器，不能混成一个抽象文件锁。

#### 8.17.6 P1/P2：全局 relay 语义和运行时容量

- `GLOBAL_SELECTOR` 在 launcher 进程内共享 failover/request/weighted index 和 conversation assignment。Mutex 保证内存安全，但窗口 A 的失败是否应推进窗口 B 的全局 failover 是产品语义问题，当前没有明确规格。
- Anthropic reasoning 兼容缓存 key 已确认只有模型名，缺少 relay/endpoint 隔离；这是 P1 已确认污染风险。
- `upstream_http_client()` 每次调用创建新 `reqwest::Client`，缺少连接池复用；多窗口会放大连接和握手开销，属于 P2 性能风险。
- 会话删除、撤销、导出、移动、usage 和排序均进入同一 launcher Tokio runtime 的 `spawn_blocking` 池。当前没有资源级并发、公平调度、队列长度或窗口配额。

需分别处理：

- 明确 relay 轮转是进程全局、conversation 全局还是窗口局部，并用同会话多窗口测试锁定。
- Anthropic 缓存改为 `(relay_id, normalized_endpoint, model)`。
- 复用长期 HTTP Client。
- 对昂贵数据操作增加资源级 semaphore、operation ID、排队状态和超时，避免一个窗口批量操作拖住其他窗口。

### 8.18 追加复核：自更新、安装器和资源保留

#### 8.18.1 P1：自更新完成后 launcher 和 Bridge 不会自动恢复

已确认调用链：

1. Manager `perform_update` 下载并非阻塞地启动 NSIS 安装包。
2. NSIS `Install` section 无条件执行：
   - `taskkill /IM codex-elves.exe /F`
   - `taskkill /IM codex-elves-manager.exe /F`
3. 安装完成后只 `Exec` 新的 `codex-elves-manager.exe`。
4. 安装脚本没有重新启动 `codex-elves.exe`。

因此自更新会终止发起更新的 Manager 和当前 launcher/Bridge，并只恢复 Manager。若 Codex 主进程仍然存活，现有窗口会继续存在但 CodexElves 增强已失效；若 Codex 随 launcher 退出，则全部窗口关闭。当前代码没有 Job Object 证据，Codex 是否成为孤儿进程需要实机验证，不能在静态审计中确定。

同时 `latest-status.json` 没有更新专用的 `updating/stopped` 收尾，重新打开的 Manager 可能继续显示旧 `running`。

修复要求：

- 更新前进入 `updating` operation，通知全部 target 和 Manager。
- 安装器只终止本次安装拥有的 PID，不按镜像名盲杀。
- 安装后明确恢复 launcher，再由 coordinator 重新发现/注入仍存活的 Codex 窗口。
- 新 Manager 等待 launcher/helper/target 收敛后再显示“更新完成”。
- 更新失败、取消或安装器启动失败时恢复旧运行态并清理临时包。

#### 8.18.2 P1：更新包和 Guard 备份没有保留策略

已确认：

- 更新安装包写入 `default_app_state_dir()/updates/<asset_name>`。
- 未发现 `updates/` 的 TTL、总容量、保留数量或成功安装后删除逻辑。
- Computer Use Guard 每次替换 active marketplace 时，将旧目录重命名为 `openai-bundled.bak-guard-<timestamp>`。
- 未发现 `.bak-guard-*` 的清理或保留数量限制。
- Guard 启动后会按多个时间点重试；持续判定需修复时可生成多份完整 marketplace 备份。

必须给所有托管 artifact 建立统一清单和策略：用途、owner operation、创建时间、当前是否可回滚、最大数量、最大总容量、清理时机和失败重试。不能让各模块自行创建永久备份。

#### 8.18.3 P1/P2：安装/卸载按镜像名终止，watcher 可能残留

已确认：

- NSIS 安装和卸载都使用 `taskkill /IM codex-elves.exe /F` 与 `taskkill /IM codex-elves-manager.exe /F`。
- 该操作不携带 launcher PID、安装目录或当前用户会话身份。
- 标准 NSIS 卸载和 `uninstall_entrypoints` 均未调用 `watcher::uninstall_watcher()`。
- watcher 安装会创建 HKCU Run 值和 Startup 快捷方式；卸载程序删除二进制后，这些入口可能继续指向不存在的 launcher。

按镜像名是否会跨 Windows 用户会话终止其他用户进程，需要管理员多用户环境实测；静态审计只能确认它没有做当前安装实例的身份限制。

完整卸载必须先停用并删除 watcher，再终止当前实例拥有的 PID，最后删除入口、二进制和用户选择的数据。每一步返回真实状态，失败可重试。

#### 8.18.4 P2/P3：崩溃残留和更新检查节流

- `atomic_write` 使用固定 `<file>.tmp`。进程在 write 后、rename 前被强杀会留下孤儿 `.tmp`；下次写会覆盖，但缺少启动清理和崩溃恢复判断。该项为 P2，核心风险仍是并发写覆盖而不是临时文件本身。
- launcher 每次启动都会异步检查 GitHub Release；除总开关外没有 last-check/cooldown。正常影响较小，但 stale recovery 或反复重启会重复请求，属于 P3。

### 8.19 功能入口反向盘点新增结论

#### 8.19.1 P0-S：Manager 命令权限是单一高权限域

反向检查全部 Tauri command 注册和 capability 配置后确认：

- `allow-manager-commands` 同时授权更新执行、会话删除、市场脚本安装、插件缓存刷新、环境变量删除、watcher 安装/卸载、relay 文件写入、设置重置等全部命令。
- 同一 capability 既允许本地 bundled 页面，也允许 `http://localhost:1420` 和其子路径。
- release 代码可由环境变量 `CODEX_ELVES_MANAGER_DEV=1` 切换到该外部 URL。
- Manager `csp` 为 `null`，asset protocol scope 为 `["**"]`。

因此，第 8.7 节的更新执行链只是最严重结果之一。更完整的系统性风险是：一旦 Manager 前端、允许的 dev server 或其依赖内容失陷，攻击面不是单个更新命令，而是整套管理能力。

该结论是条件型高危：当前没有证据证明默认 bundled 页面已经存在可利用 XSS，也不能把“存在高权限命令面”直接写成公网可利用漏洞；但发布前必须完成以下隔离：

- 生产构建移除 remote dev origin 和环境变量开启外部页面的能力。
- 按只读、配置写、文件系统变更、进程控制和安装执行拆分 capability。
- 更新、环境变量删除、插件/脚本安装、完整卸载等高风险命令使用后端生成的短期确认 token。
- 为 bundled 页面配置 CSP，缩小 asset protocol scope。
- 所有命令继续执行后端输入校验，不能把 Tauri capability 当作唯一授权层。

`open_external_url` 已限制为 `http`/`https`，没有发现独立命令执行问题；但脚本市场 homepage 属于外部不可信内容，UI 应显示目标 host，并在跨 host 跳转时明确确认。该项为 P3 防钓鱼改进。

#### 8.19.2 P1：Provider 同步可能“报告跳过但实际部分提交”

第 7.1.8 节已经闭环其调用链。需要特别强调：

- 每个 SQLite 事务只保护单库，不能保护多个数据库之间的一致性。
- `prune_backups()` 位于 SQLite 和 global-state 写入之后；该步骤失败也会进入错误分支。
- 错误分支只恢复 rollout 文件，不恢复已经提交的 SQLite/global-state。
- 最终状态统一为 `Skipped`，会让 UI 和运维误以为没有发生变更。
- `provider-sync.lock` 不校验 owner 存活，异常退出后可永久阻断功能。

该功能必须按可恢复 operation 重构，不能只增加更多备份文件。

#### 8.19.3 P1：会话撤销备份缺少敏感数据生命周期

`BackupStore` 的 JSON 备份包含：

- 被删除会话的完整 SQLite 表行。
- rollout 文件的 Base64 原文。
- 源数据库绝对路径。
- 多数据库撤销清单。

这些文件位于 `~/.codex-session-delete/backups`，当前没有 TTL、总容量、成功撤销后消费删除或统一清理策略。undo token 也可以重复使用。

因此这不仅是磁盘增长问题，也是本地会话内容长期复制保留的问题。需要：

- token 一次性消费或显式保留策略。
- 按时间、数量和总字节数清理。
- 成功撤销后删除不再需要的单库和聚合清单。
- 诊断、卸载和日志不得枚举或复制备份正文。
- 将会话撤销、relay `auth.json` 实时备份、Provider 同步备份、更新包和 Guard 备份纳入统一敏感 artifact 清单。

#### 8.19.4 P1：upstream worktree 的确认边界早于副作用

renderer 在用户选择 upstream 分支后立即调用 `prepare(fetch: true)`。因此“选择候选分支”和“确认创建 worktree”之间已经发生 fetch。

正确语义应拆分为：

```text
inspect（只读）
→ 用户确认
→ prepare/fetch（有 operation token）
→ create
→ committed / failed-partial
```

fetch/create 还必须是独立 capability，不能因为 renderer 能读取 worktree 默认值，就自动拥有任意仓库变更能力。

### 8.20 修订后的总优先级

#### P0-S：现存安全发布阻断项（含条件型高危）

1. Helper HTTP/WebSocket capability、Origin/Host 和路由权限边界。
2. renderer Bridge 强制 generation、execution context、脱敏 DTO 和最小命令能力。
3. 诊断报告、诊断日志和剪贴板输出的统一脱敏。
4. 脚本市场 hash/签名、来源限制、默认禁用和权限隔离。
5. 更新流程后端持有可信状态，校验域名、摘要/签名和安装包发布者。
6. Manager 生产 capability 分层、移除 remote dev origin、启用 CSP 并限制 asset scope。

#### P0-MW：多窗口功能发布阻断项

1. target registry 与严格 target 分类。
2. RequestOrigin、generation 和 target-local 路由。
3. settings、抑制列表、会话、Codex home 的共享写协调器。
4. 带 revision 和 partial 结果的全窗口广播。
5. localStorage/storage partition 现场证据，冻结 Fast draft、排序和项目投影的真实作用域。

#### P1：产品正确性与隐私

- 分层压缩 sentinel spoof。
- 代理正文日志的脱敏、TTL 和清理栅栏。
- CLI wrapper 凭据撤销、日志和 argv。
- 环境变量清理的 allowlist 和可恢复事务。
- Computer Use Guard 的资源身份、锁和恢复。
- cc-switch 导入、模型目录和派生配置的统一 operation。
- Provider 同步的可回收锁、跨库补偿和真实状态。
- relay 对话分配淘汰。
- upstream worktree 的只读 inspect、确认 token、资源锁和操作幂等。
- 会话撤销及其他敏感备份的 TTL、容量和消费清理。
- 皮肤导入/删除原子性和资源回收。
- 卸载所有权清单和真实失败反馈。
- 自更新 operation、launcher/Bridge 恢复和真实 stopped/updating 状态。
- 更新包、Guard 备份和临时 artifact 的统一保留策略。
- 安装/卸载按 owner PID 执行并清理 watcher。
- 插件缓存 source/cache containment 和 ZIP 解压配额。
- CDP reserved flags、真实监听验证和 URL 日志脱敏。
- DevTools target 绑定、全局重启确认和 SSH destination 参数边界。

#### P2：稳定性与性能

- Codex Radar singleflight 和 stale fallback。
- `atomic_write` 崩溃残留清理和更新检查 cooldown。
- 8 窗口资源容量、observer、timer、heartbeat 和日志吞吐。
- 真实双窗口 Continue Thinking、WebSocket、压缩和 relay 切换烟测。

## 9. 用户脚本专项约束

当前用户脚本设计只要求脚本作者自行幂等，并明确禁用脚本不会撤销已执行副作用。多窗口广播 reload 会扩大以下问题：

- 重复事件监听和定时器。
- 重复 DOM 注入。
- 重复 patch `fetch`、prototype 或 Electron bridge。
- 禁用/删除后旧窗口副作用继续存在。

实现多窗口广播前必须决定：

1. 是否引入可选 `dispose()` 生命周期。
2. 是否以脚本 revision/实例记录每个 target 的加载状态。
3. 无法安全热重载的旧脚本是否改为提示刷新窗口。
4. 新 target 加入和 reload 并发时，如何保证最终执行恰好一次当前 revision。

## 10. 可观测性

结构化日志至少增加：

- `reconcile_id`
- `trigger`: startup / interval / target-event / repair / shutdown
- `target_id`
- `target_kind`
- `target_url_class`，不记录敏感完整 URL
- `runtime_generation`
- `observed_target_count`
- `eligible_count`
- `managed_count`
- `healthy_count`
- `installing_count`
- `failed_count`
- `action`: ensure / replace / stop / noop
- `reason`
- `attempt`
- `install_ms`
- `health_ms`
- `shutdown_ms`
- `outcome`: healthy / partial / degraded
- 广播 `revision` 和每个 target 结果

## 11. 自动化测试与真实烟测门禁

### 11.1 纯函数和模拟 CDP

- target 分类返回全部合格主页面，结果不依赖 `/json` 顺序。
- avatar overlay、DevTools、第三方浏览器页、未知页面始终拒绝。
- 新 target 生成 `Ensure`，消失生成 `Stop`，WebSocket 变化生成 `Replace`。
- A 健康时新增 B，仍会安装 B。
- B 关闭或安装失败不影响 A。
- 同 target 并发 reconcile 最多安装一次。
- 一个 handler 阻塞不影响其他 target。
- 退出后 runtime、连接和 in-flight 计数归零。
- 广播返回每 target 结果并正确表达 `partial`。

### 11.2 功能一致性

- A 修改设置后，B/C 在明确时限内获得同一 revision。
- A 切换 Fast/Standard 后，每个窗口请求携带正确 service tier。
- A 重载用户脚本后，B/C 的脚本 revision 一致。
- A 修改皮肤后，B/C 更新 overlay，不重复安装整套 Bridge。
- A 删除、撤销或移动会话后，B/C 的侧边栏、抑制集、排序和项目投影收敛。
- 两窗口同时修改不同设置不丢字段。
- 两窗口同时操作同一会话有确定、可重复的最终结果。

### 11.3 真实 Codex 烟测

- 新窗口注入 P95 目标不超过 2 秒，硬上限 5 秒。
- 同时保持 2、3、8 个主窗口，功能均可用且总并发受限。
- 连续打开和关闭窗口 20 次，无残留 runtime、连接、菜单、observer 或 timer。
- 刷新、导航、renderer crash/recreate 后自动恢复。
- stable 和 beta 各保留脱敏 target fixture。

### 11.4 安全与独立功能回归

- Helper 缺失 token、错误 Origin、错误 Host、过期 generation 的 HTTP 和 WebSocket 请求全部拒绝。
- 浏览器恶意页分别通过 HTTP、HTTPS、iframe 和 WebSocket 测试 loopback，记录 PNA/mixed-content 的实际行为，但不把浏览器拦截当服务端授权。
- Bridge payload 省略 generation、generation 错误、非主 execution context、非主 frame 时全部拒绝。
- `/settings/get` 的 renderer DTO 不含任何 canary key、auth/config 原文或敏感路径。
- 诊断报告、诊断日志、代理日志和剪贴板结果不含 canary secret。
- 市场脚本 hash 不匹配、重定向到未知 host/private IP、ID 文件名冲突时安装失败且旧版本不被覆盖。
- 更新命令伪造 asset URL、篡改 release token、摘要不匹配、签名/发布者不符时不落盘执行。
- 生产 Manager 不接受 localhost remote origin；不同风险等级命令不能由同一 capability 无差别调用。
- 普通 user 文本、引用内容和工具输出包含 compaction sentinel 时不得展开为内部历史。
- plugin ID 含 `..`、分隔符、绝对路径，source 越界、junction 越界和 ZIP 解压超额时均 fail-closed；测试只在临时目录执行。
- `codexExtraArgs` 试图覆盖调试端口、监听地址、allow origins 或 user data dir 时保存/启动失败。
- 环境变量清理中途失败时原值可恢复；备份不以明文泄露密钥。
- CLI wrapper 禁用后旧凭据失效，含空格/反斜杠/引号参数保持 argv 不变，日志不记录敏感参数。
- 皮肤导入超限、列表写失败、删除图片失败时返回真实错误且不留下未登记文件。
- 完整卸载逐项报告状态，删除失败不得返回整体成功。
- 自更新使用隔离测试安装器验证：进入 updating、只终止 owner PID、重启 launcher、恢复全部存活 target、最终写入 running/stopped。
- 已安装 watcher 后执行标准卸载，Run 值、Startup 快捷方式和失效入口全部移除。
- 多版本更新包和多份 Guard 备份超过策略上限时自动清理，当前可回滚版本不被误删。
- Provider 同步在第二个 SQLite、global-state 和备份清理阶段分别注入失败，不能留下部分提交或错误返回 `Skipped`。
- Provider 同步持锁进程异常退出后，下一次运行可验证 owner 并回收陈旧锁。
- 选择 upstream 分支后取消创建，不产生 fetch；确认后的 fetch/create 具有 operation token 和 per-repo 串行。
- 会话撤销成功后 token 不可重放，过期备份按时间、数量和总字节数清理，清理过程不影响有效撤销点。

## 12. 待验证项与尚未闭环的边界

本节只列静态审计后仍缺少运行证据的项目。第 8 节已经完成证据闭环的风险不应降级成“待验证”。

### 12.1 多窗口与 renderer 运行时

- 新窗口创建到 CDP target 可连接的实际延迟分布。
- 主窗口刷新、renderer crash/recreate 后 `target_id` 和 execution context 的变化方式。
- 当前 Electron 版本中应用内 webview 的 target 类型和 URL 形态。
- avatar overlay 或其他辅助窗口是否可能导航为主页面 URL。
- 多主窗口是否共享同一 storage partition；若共享，现有 localStorage read-modify-write 的实际覆盖行为。
- renderer 内部是否存在可稳定获取的窗口/会话身份；它只能作为诊断字段，不能替代 `target_id`。
- 真实 2、3、8 窗口下 Continue Thinking、Responses WebSocket、分层压缩、relay 切换和取消是否串线。

### 12.2 浏览器、本地端口与高权限边界

- 当前 Chromium/WebView2 对恶意 HTTP、HTTPS、iframe 和 WebSocket 页面访问 loopback Helper 的 PNA、mixed-content 和预检行为；该结果只影响利用链，不改变 Helper 服务端缺少授权的结论。
- bundled Manager 页面是否存在可利用的 XSS；`CODEX_ELVES_MANAGER_DEV=1`、localhost dev server 和 Tauri remote capability 的真实组合行为。
- `Runtime.bindingCalled.executionContextId` 与主 frame/default world 的稳定映射方式。
- `codexExtraArgs` 覆盖调试地址、端口、allow origins 或 user data dir 后，Windows/IPv4/IPv6 的真实监听和命令行结果。
- plugin cache/source 在 junction、symlink、reparse point 和 ZIP 特殊条目下的实际越界行为。
- SSH host/user 以 `-` 开头或包含边界字符时，当前 OpenSSH 客户端的最终 argv 解释。

### 12.3 安装、自更新和资源所有权

- Windows Store、便携版、beta 和多安装并存时，自更新、重启、进程归属和安装目录选择。
- 安装器终止 Manager/launcher 后，Codex 是否成为无 Helper/Bridge 的孤儿进程，以及 launcher 是否能自动恢复。
- 按镜像名 `taskkill` 在管理员多用户会话中是否会终止其他用户的同名进程。
- 标准 NSIS 卸载、入口卸载和 watcher 卸载的真实组合结果。
- launcher/Manager 在固定 `.tmp` 写入后被强杀时的残留和下次恢复行为。

### 12.4 并发、恢复与容量

- `clear_local_proxy_logs` 与异步日志 worker 的精确复活窗口。
- Computer Use Guard 在多个 `@oai/sky` runtime 并存时是否会选错版本。
- 两个 Windows 进程同时操作同一 `settings.json.tmp` 的具体错误和恢复模式。
- Provider 同步在第二个 SQLite、global-state、备份清理阶段失败时的部分提交范围。
- Provider 同步 owner 进程消失后的陈旧锁回收策略。
- 8 窗口下 observer、timer、heartbeat、SQLite/rollout 读取、日志队列和 `spawn_blocking` 的资源上限。

## 13. 实施前停止条件

出现以下任一情况，必须停止实现并重新评审：

- 目标分类无法可靠区分 Codex 主页面和第三方页面。
- target-local 路由仍然依赖全局“当前 WebSocket”。
- 共享状态没有 revision、锁或资源级串行方案。
- 用户脚本 reload 语义未确定。
- 一个窗口失败仍会被进程级 `status: ok` 掩盖。
- 自动化测试不能模拟两个以上独立 target。
- Helper 或 Bridge 仍以“loopback 可达”“页面已注入”代替调用者身份。
- 生产 Manager 仍把 localhost remote origin 与 bundled 页面放在同一高权限 capability，或继续使用空 CSP 和全量 asset scope。
- 更新安装包或市场脚本仍缺少可验证的内容真实性。
- 诊断、代理日志或 renderer 设置 DTO 仍可能返回原始凭据。
- plugin cache/source 的 canonical path containment 无法证明。
- localStorage/storage partition 的真实共享语义尚未验证。
- Provider 同步仍可能在部分提交后返回 `Skipped`，或陈旧 owner 锁无法自动、安全回收。
- upstream worktree 在用户明确确认前仍会 fetch，或 fetch/create 仍可由普通 renderer 能力直接调用。
- 会话撤销和其他含凭据/会话正文的备份没有 TTL、容量、消费和卸载处理策略。
- 自更新后 launcher/helper/Bridge 无法自动恢复并报告最终状态。
- watcher、更新包、Guard 备份和临时 artifact 没有统一所有权与清理策略。
