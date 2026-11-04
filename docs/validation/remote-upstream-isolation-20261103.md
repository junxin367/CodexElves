# 受控上游失败验收：会话级隔离条件核对

日期：2026-11-03，Asia/Shanghai。

**状态：协议字段已从本机实际CLI确认；隔离 app-server 启动尝试在业务 RPC 前被系统代理连接守卫中止，R3-06真实队列失败仍未执行。** 未更改用户供应商、Codex 全局配置或系统代理。

## 已确认的协议边界

本机安装包为 `OpenAI.Codex 26.1027.3384.0`，其 `app/resources/codex.exe --version` 为 `codex-cli 0.166.0-alpha.5.1`。执行：

```text
codex.exe app-server generate-json-schema --experimental --out temp/remote-upstream-schema-20261103
```

退出码0。归档目录 `dist/validation/remote-acceptance-20261103/upstream-isolation/` 保存准确可执行文件路径、SHA-256、命令、退出码及以下4份原始schema；不是根据其他CLI版本推断。

| 协议对象 | 本机实际定义 | 可证范围 |
| --- | --- | --- |
| `ThreadStartParams` | 存在可空 `model`、`modelProvider` 和对象 `config`；`config.additionalProperties=true` | 新建测试线程可以提出线程级配置覆盖请求，尚不证明任意供应商配置会被接受 |
| `TurnStartParams` | 存在 `model`，描述为覆盖本轮和后续轮次；不存在 `modelProvider` 或 `config` | 单轮不能借不存在的provider字段进行隔离；model覆盖会影响后续设置，需记录并恢复 |
| `ThreadQueueAddParams` | 必需字段只有 `threadId`、`clientUserMessageId`、`input`，没有model | 不能假设Q自带模型覆盖；要独立核下一轮设置与实际消费 |
| `ThreadQueueStartParams` | `threadId`必需、`queuedSubmissionId`可选 | 只证明现有队列启动请求结构，不证明上游失败后的自动策略 |

产品 `native.js` 的入队路径会在准备和接收前调用 `updateThreadSettingsForNextTurn` 应用模型设置。后续测试须核实际Q消费时的模型与完整输入，不能只检查网页下拉值。

## 下一步的真实验证条件

1. 匹配版本源码已确认 `thread/start.config` 会作为 request override 进入新 thread 的 `Config`；下一步只需在满足 `cwd` 信任停步条件后，验证 managed policy 接受唯一 provider ID、该 thread 的实际模型请求命中 loopback，且全局配置与其他会话未变。若路由或全局隔离不能证明，不对共享上游制造失败。
2. 需要让活动轮已被接受且准确Q已入队，再产生真实模型上游失败。任意不存在模型可能在本地预检就拒绝，或在Q入队前快返失败；这两者都不能代替目标场景。
3. 故障请求、上游结果与活动turn必须有准确关联，保存实际尝试次数和最终失败。Codex内部模型重试与远程重复派发不是同一概念，不能为验收擅自禁用内部重试。
4. 对原Q只观察原生自动承接或完整保留；结果未知时只读核对。不得补发同一用户消息或把原生持久历史不存在当作已失败。
5. 独立网页端口和远程HTTP服务不等于独立模型执行。只有证明故障入口仅影响该测试thread，或使用真正独立的执行实例后，才进行失败注入。

当前没有实际故障请求、原生失败轮、队列转场或隔离恢复的运行证据，R3-06保持blocked/not_run。下方记录的启动尝试只证明隔离守卫捕获到系统代理连接，不构成模型上游失败或队列验收。此文不改变既有停止和审批取消的通过范围。

## 收尾

4份schema及生成身份已归档。临时生成目录 `temp/remote-upstream-schema-20261103` 的删除被自动审批审查拒绝，仅返回 `blocked by policy`；未换方式重试，目录保留且被Git忽略，见归档目录的 `cleanup.json`。本次未构建安装包、安装、提交或推送。

## 匹配安装版本的源码映射补充（2026-11-03，只读）

**状态：thread/start 的 request-scoped 配置覆盖路径已由匹配安装版本的官方源码确认；真实 provider 路由、上游失败和失败后队列分支仍未实测。** 本机证据见 [`schema-identity.json`](../../dist/validation/remote-acceptance-20261103/upstream-isolation/schema-identity.json) 与同目录四份 schema：bundle 是 `OpenAI.Codex 26.1027.3384.0` / `codex-cli 0.166.0-alpha.5.1`，schema 导出未执行业务 RPC、未改供应商配置。

在匹配 tag `rust-v0.166.0-alpha.5.1` 中，`thread_start_inner` 将 `ThreadStartParams.config` 和 typed `modelProvider` 传入 `thread_start_task`；后者通过 `ConfigManager.load_with_overrides` 建立该 thread 的 `Config` 再启动会话。`load_with_cli_overrides` 将 request map 与既有 CLI overrides 合并到一次 `ConfigBuilder` 加载，不写全局 `config.toml`。因此可在新测试 thread 使用唯一 provider ID 和 dotted-key 覆盖，例如 `modelProvider: "r3_06_loopback"`、`config: {"model_providers.r3_06_loopback.base_url":"http://127.0.0.1:<本轮端口>/v1","model_providers.r3_06_loopback.wire_api":"responses","model_providers.r3_06_loopback.requires_openai_auth":false}`。这证明配置传递范围与语法路径，不证明 managed policy 会接受该 ID、有效配置会改变实际请求地址，或本机网络策略允许 loopback。相关官方源码：[thread_processor.rs `thread/start` 配置传递与加载](https://github.com/openai/codex/blob/rust-v0.166.0-alpha.5.1/codex-rs/app-server/src/request_processors/thread_processor.rs#L1136-L1348)、[ConfigManager request overrides 合并](https://github.com/openai/codex/blob/rust-v0.166.0-alpha.5.1/codex-rs/app-server/src/config_manager.rs#L530-L567)、[provider 字段与默认值](https://github.com/openai/codex/blob/rust-v0.166.0-alpha.5.1/codex-rs/model-provider-info/src/lib.rs#L133-L214)。

**全局状态停步条件：** 若 `thread/start` 指定 `cwd`，而该项目尚未信任、不是 projectless，且有效权限允许写入，`thread_start_task` 会调用 `set_project_trust_level` 保存项目受信任状态；该分支不检查 thread 是否 ephemeral。后续验证必须省略 `cwd` 或先只读证明该路径已信任／projectless／有效权限不可写，并核对项目受信任状态与 Codex Home `config.toml` 前后未变。若此条件不能保证，或 provider selection 被 managed policy 拒绝、loopback 网络被策略拦截，立即停止；不得改全局 provider、权限或网络策略。信任写入分支见 [thread_processor.rs `set_project_trust_level`](https://github.com/openai/codex/blob/rust-v0.166.0-alpha.5.1/codex-rs/app-server/src/request_processors/thread_processor.rs#L1354-L1416)。

**最小后续验收顺序（本轮未完成）：** 先以已知可用的 model 创建该测试 thread，使用本轮专属 loopback provider，并保持 Codex 默认重试设置；让本机 mock provider 收到并暂缓 T 的真实模型 HTTP 请求。只有确认返回了活动 `turnId`、原生状态为 `inProgress` 且 loopback 已收到带该 thread/turn 身份的请求后，才用 `thread/queue/add` 加入一次准确 `clientUserMessageId` 和完整 `input` 的 Q，并只读确认 Q 已入队；再让 provider 对 T 返回实际 HTTP 错误，等待该 turn 原生终态并观察原生队列自然行为。`thread/queue/add` 受 experimental handshake 门控，须沿用已有授权的 app-server 客户端能力，不为测试打开全局开关。

同 tag 的 app-server 队列测试证明：活动轮存在时调用 `thread/queue/start` 会返回 busy 并保留队列；活动审批被拒绝终结后，等待中的 Q 会自动开始；被 interrupt 的场景则保留 Q，之后才可显式启动。这些测试不覆盖 provider HTTP failure，不能替代 R3-06 真实结果。见 [thread_queue.rs 自动消费样例](https://github.com/openai/codex/blob/rust-v0.166.0-alpha.5.1/codex-rs/app-server/tests/suite/v2/thread_queue.rs#L667-L733)、[interrupt 后保留与显式恢复样例](https://github.com/openai/codex/blob/rust-v0.166.0-alpha.5.1/codex-rs/app-server/tests/suite/v2/thread_queue.rs#L537-L665)。

若 `thread/start`／`turn/start` 在 loopback 收到模型请求前返回 RPC 错误，属于 schema、配置、model preflight 或策略拒绝，不算上游失败。只有 loopback 记录到同一活动 turn 的模型 HTTP 请求与真实 provider 错误，且原生该 turn 随后为 `failed`，才进入 R3-06 队列验收。线程回复中的 `modelProvider`、全局 `config/read` 与配置文件摘要可辅助确认全局未变；实际 base URL 仍须由 loopback 请求原件证明。自动承接或保留必须以失败后的原生读取判定，未知时停止，不提前调用 `thread/queue/start`。

## 实际受控启动尝试（2026-11-03 11:28，Asia/Shanghai）

运行标识：`r3-06-20261103032819-435d7ea8`。证据归档于 `dist/validation/remote-acceptance-20261103/upstream-appserver/`，包括 `run.json`、`cleanup.json`、app-server 标准输入/输出/错误日志、provider 请求账本及本次驱动副本。使用归档身份中 SHA-256 匹配的 `codex-cli 0.166.0-alpha.5.1`；驱动为 app-server 配置了独立 `CODEX_HOME` 和 loopback guard provider，子进程环境只含该路径、必要 Windows 运行变量及 PATH 项。

启动/initialize 阶段，网络守卫捕获 app-server PID `44796` 与 `127.0.0.1:7897` 建立的 TCP 连接；`initialize` RPC 也成功并收到响应。守卫记录连接后立即终止子进程；7897 的监听进程为 `verge-mihomo`。随后驱动尝试写入 `thread/start` 时发现 stdin 已关闭。RPC 账本仅记录 `initialize` 与 `initialized`；stdin 审计文件在检查可写状态前先记下了待发 `thread/start` 行，因此该行不是已发送的 RPC。没有观察到 `thread/start` 响应、模型请求、turn 或队列项；loopback provider 请求数为0。子进程以守卫的 `SIGTERM` 结束，隔离 `config.toml` 前后 SHA-256 一致。

只读系统核验显示：父进程及驱动 allowlist 均未给子进程传入 `HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY` 或其他 proxy 环境变量；HKCU Internet Settings 的代理启用并指向 7897，HKLM 未启用，WinHTTP 为直连，HKCU bypass 规则含本地/loopback 项。请求目标没有被记录，不能据此判定这是默认 provider 的模型刷新，也不能确认本机代理是否继续转发了请求。没有修改代理、全局 Codex 配置或网络策略。

本机缓存中只有 CodexElves 工作区依赖的 reqwest 源，无法证明它就是安装版 Codex CLI 的匹配依赖；因此 `NO_PROXY=127.0.0.1,localhost` 是否能绕过该安装版所使用的 HKCU 代理路径，仍无明确源码依据。安装版 `app-server --help` 确认支持仅对该进程使用 `-c/--config`，但启动配置探针没有回显生效的 provider 值。这些证据不足以安全重试；不把 7897 加入 allowlist，也不再启动 app-server。R3-06 仍为 `blocked/not_run`，未形成上游失败或队列闭环结论。

## 临时目录清理留存

归档中的 `cleanup.json` 只记录 app-server 进程退出和 loopback provider 关闭，不代表临时目录已删除。一次 PowerShell 清理命令请求对以下两个本轮目录执行 `Remove-Item -Recurse -Force`，但在 PowerShell 启动前被自动审批拒绝，原文为 `blocked by policy`；拒绝前未执行删除，未换命令重试：

- `E:\code\junes\github\CodexPlusPlus\temp\remote-upstream-appserver-20261103\codex-home`
- `C:\Users\junes\AppData\Local\Temp\codex-r3-06-cwd-EcuWuZ`

清理命令尝试次数为1，重试次数为0。两目录按拒绝结果保留；目标、动作与拒绝原文另见 [`cleanup-retained.json`](../../dist/validation/remote-acceptance-20261103/upstream-appserver/cleanup-retained.json)。
