# 压缩期间普通消息进入原生队列：原生链路验收

日期：2026-10-18（Asia/Shanghai）。

功能验证状态：**PASS**。生产 `native.js` 的隔离回归 **61/61** 通过，最终 `sendState` 释放增量补测 **2/2** 通过；Rust 定向路由与参数验证 **2/2** 通过；生产桥接对接本机同安装包的独立临时 app-server，手动压缩、自动压缩、顺序释放、并发去重和回包恢复均通过。清理状态为 **PARTIAL**，原因见末节。

本轮没有操作真实聊天、调用真实模型提供方、重启桌面或正式服务、安装、正式打包、提交或推送。所有模型请求均发往临时监听的 `127.0.0.1` 无认证 fixture。

## 契约与产品改动

普通 `action=send` 支持可选布尔字段 `queueOnly`；`true` 强制经过原生排队入口。缺省和 `false` 保持普通发送兼容。该字段不能用于目标、新建或其他写操作。`session.capabilities.compactionQueueSend=true` 声明后端能力。

产品源码改动限于：

| 文件 | 本轮改动 |
| --- | --- |
| `assets/remote-access/native.js` | 重复注入 guard 与导出版本从 44 升至 45；增加字段校验；显式排队或实际压缩时使用原生队列；普通发送完成异步准备后再次检查压缩状态 |
| `assets/remote-access/native.js` | 删除普通新增排队的两处压缩拒绝；保留原生 coordinator 的活动轮次、pending turn start、权限、状态及发送调度 |
| `assets/remote-access/native.js` | 入队前读取原生队列；如已有 local 消息或准备过程中出现 local 消息，交给原生 coordinator，避免直接追加 server queue 插到 local 消息前面 |
| `assets/remote-access/native.js` | 同 thread/client ID 的在途请求相同内容共用 promise；正文、图片、Skill、模型或推理等级不同返回 409 |
| `assets/remote-access/native.js` | `pending`、`sending`、`outcome-unknown` 本地占位不再证明已排队；未知回执保留为只查询记录，未确认前不重放 add；原生确认 queued/accepted 后释放记录 |
| `assets/remote-access/native.js` | 正常 `sendState` 查询确认 queued/accepted 后释放已终态 unknown 的图片和 Skill 身份；仍在执行的 promise 继续保留，避免打开并发加入窗口 |
| `crates/codex-elves-core/src/remote_access.rs` | 增加能力声明与字段校验；鉴权、只读、请求回执和既有目标校验保持原路径 |

没有修改模型协议、图片物化实现、Skill 目录、前端页面、Android 或其他写操作的压缩门禁。既有 v43 模型/等级修复和 v44 通知逻辑保留。

## 本机版本与实际源码

现场重新查询的 Appx 为 `OpenAI.Codex 26.1007.9446.0`，包内版本为 `26.1007.71326`。从当前安装的 `app.asar` 重新提取后，以下摘要与既有模型核查报告完全一致：

| 文件 | SHA-256 |
| --- | --- |
| `app-initial-102e45562729.js` | `c59b2a0be6c13496b30ebda167925cdccab26799cae350c62d2b37afe3589b57` |
| `app-primary-cd52df2361eb.js` | `9602eea8c6dd07ddfbbc2772eac271e8ec7f765e1f275fe47ef681b996ac056a` |
| `app-shared-30671c57ec82.js` | `392b023063a3569a2efc47a019accb2d1e72295b1bfec2cbb81fc37e1515580a` |

实际执行的 app-server 为上述同一安装包内的 `app/resources/codex.exe`，版本 `codex-cli 0.162.0-alpha.17.2`，SHA-256 为 `e98c516f1c8935789aabe9122ee7e05ec4c3efd1ba6acf179242e031c495b99e`。

原生函数的字节位置与摘要保存在 `native-regression.json`。实际 `TurnCoordinator`、`ifn`、`ufn`、`Dln`、`fnn`、`ManualContextCompactions`、`T9t` 和 `_V` 参与隔离执行；没有用另写的排队算法代替它们。

## 原生调度结论

- `T9t` 发起手动压缩前，实际 `ManualContextCompactions.register` 先增加 pending 计数，再通过 `fnn` 同步插入 `status=inProgress` 的压缩占位轮次。没有 turn ID 时命中原生 `hasPendingTurnStart`；已有 turn ID 时命中 `getActiveTurnId`。
- 实际 `ifn` 在 `queueModeOverride:"queue"` 且存在活动轮次或 pending start 时调用 enqueue，不调用 steer。实际 `ufn` 在最新轮次仍为 `inProgress` 时拒绝自动执行 queued 消息。
- 压缩在异步准备期间结束时，消息继续经过原生队列，可以由原生及时启动下一轮。桥接没有增加浏览器队列或另一套持久消息队列。
- 原生 server queue 的展示顺序为 server 在 local 前；因此存在 local 消息时，不能直接调用 server add。修复前隔离执行稳定复现插队，修复后由原生 coordinator 保留先后顺序。
- 当前安装版真实 app-server **不会按 `clientUserMessageId` 自动去重**。隔离协议探测重复加入产生两个不同的 queued ID；探测用的重复项在释放压缩前删除。桥接负责合并同 ID 在途请求、拒绝在途冲突，以及避免未知结果重放。

## 验证结果

| 验证 | 结果与证据 |
| --- | --- |
| 真实 JS 函数与生产桥隔离回归 | **61 通过、0 失败**；见 `native-regression.json` |
| `sendState` 释放补充验证 | **2 通过、0 失败**；未确认保留、已确认终态释放、在途查询不提前释放；见 `send-state-release.json` |
| 真实 app-server 手动压缩 | 压缩流保持未完成时，两条普通消息只留在真实 server queue；释放后按 first、second 执行；见 `app-server-regression.json` |
| 真实 app-server 自动压缩 | 用合成 usage 触发实际自动压缩，保持压缩流时无后续模型请求；释放后按原触发轮、first、second 执行 |
| 真实 app-server 同 ID 并发 | 生产桥接的相同内容请求仅一次 add；不同正文在途请求返回 409 |
| 真实 app-server 回包恢复 | 模拟丢弃已接受的 add 回包后，生产桥接查询真实队列确认；队列释放后同 ID 回放被真实历史确认 accepted，未再 add |
| 本地未知结果与 server 暂不可见 | `pending`/`sending`/`outcome-unknown` 不被误报为 queued；同 ID 重放不再次发送或 add；原生后续可见时恢复确认 |
| 内容与设置 | 文字、图片、Skill、换模型清空旧等级、设置 false/异常、不就绪、已有队列顺序和普通发送均通过 |
| Rust 定向验证 | **2 通过、0 失败**；鉴权、只读、能力声明、非法字段、非普通操作、相同回执去重、冲突、普通发送、unknown 与 queueSyncPending；见 `rust-tests.log` |
| Rust 集成检查 | `cargo check -p codex-elves-core --lib` 通过；见 `cargo-check.log` |
| 静态检查 | Node 语法、目标 Rust rustfmt 与目标文件 `git diff --check` 通过；见 `checks.json` |

保留的修复前证据分别为：

- `before-queue-boundaries.json`：未知占位被误报 queued、server/local 插队。
- `before-concurrent-dedup.json`：同 ID 并发双 add、本地请求悬挂、不同内容和设置被接受。
- `before-unknown-replay.json`：server 回包未知且结果暂不可见时重放双 add。
- `app-server-before-dedup.json`：实际 app-server 不按 client ID 去重的首次协议观察。

最终增量仅改变 `sendState` 确认后的内存身份释放，原状态读取实现保留原文并改名为 `readQueuedSendState`。原 61 项与实际 app-server 的调度、入队、顺序和回包证据按覆盖范围复用；没有把旧摘要伪写为最终代码摘要。最终代码摘要、两组证据摘要与复用依据均列入 `checks.json`。

## 隔离与验收边界

app-server 子进程使用独立临时 `CODEX_HOME`、配置、工作目录、`USERPROFILE`、`HOME`、`APPDATA`、`LOCALAPPDATA`、`TEMP` 和 `TMP`。父进程与用户配置未修改。环境使用显式白名单，没有继承 API key、token、secret 或认证变量；provider 固定为无认证 loopback mock，fixture 对意外 Authorization 请求头作断言且不返回工具调用。实际模型请求记录共 9 条，全部是本轮合成流。

本机实际 JS 函数证明的是当前安装版调度语义；VM 中的存储、权限环境和附件准备仍为内存夹具。app-server 联调的 manager 历史映射、附件 prepare 也是合成适配，仅真实 app-server 的队列、压缩和轮次调度实际运行。没有进行真实图片物化、真实账号模型推理、桌面 UI 或正式部署验收；浏览器部分由主代理另行验证。

同 ID 在途冲突拒绝与未知结果保护属于本次注入实例内的请求状态。升级/重建整个桌面注入实例后，仍依赖原生队列、原生历史和 HTTP 持久回执恢复身份；本轮没有把该状态扩展为另一套持久消息队列。

## 临时文件清理

临时测试、提取脚本副本、schema、app-server 配置和合成会话均只在 `temp/compaction-queue-20261018/native` 下生成。所有本轮测试源、临时 Rust 模块、提取 bundle 与函数副本、schema 均已删除。

自动审批拒绝了临时 `app-server-fixture` 的递归删除、逐文件管道删除，以及该临时目录内已知配置和合成会话文件的删除；工具仅报告 `rejected: blocked by policy`，没有提供更具体原因。没有请求提权或改用其他语言绕过。当前残留 **163** 个临时 app-server 配置、合成会话、内置 skill 和缓存文件；完整清单与阻断原因见 `cleanup.json`。本轮隔离子进程已退出，按任务命令行检查未发现遗留运行进程。
