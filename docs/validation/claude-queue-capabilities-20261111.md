# Claude 队列能力与远程待发送队列

## 当前网页行为

用户确认改为远程服务端持久化队列：执行中发送的网页消息尚不进入 Claude，可逐条引导、编辑、删除和排序。只在空闲续发或显式引导某条时，才把该条完整消息交给当前同一个原生会话。CLI 未发送草稿不经过网页队列。

旧版已经投递的原生排队记录仍按“已发送”核对，不迁回未发送队列。服务重启会把派发中状态转成待核对，并先重新读取原生快照；不会自动重发未知结果。自动续发逐次核对原设备授权，需要确认上一条原生轮次完成；网页停止生成会暂停后续队列。

原生整队能力核对保留在下文，属于改造前的接口调查及验收记录；不再代表网页只支持整队立即执行。

## 远程待发送队列验收

独立入口使用 `47137`，网页会话为 `claude-a1fa6891-825c-4f99-af72-9ad6abf2d3fe`，原生 UUID 为 `364c0e62-65b7-4aea-adee-b547e66c74e3`，后台短 ID 为 `364c0e62`。模型为 `gpt-6.1-sol`，推理等级 `max`。

实际操作与结果：

- A 执行期间连续提交 B、C、D，三条均显示独立“引导”及操作菜单，原生历史只有 A。
- 点击 C 的“引导”，只有 C 被投递；B、D 仍保存在远程队列。原生历史记录和回执均按该条消息核对，没有把远程整队发送。
- 在网页编辑 B，保留一份未发送的网页草稿；仅重启独立服务并刷新页面，编辑内容、编辑标识和原生会话身份均恢复。C 在服务离线期间正常完成，编辑中的 B 阻止后面的 D 自动抢先发送。
- 保存 B 后恢复原网页草稿，通过队列手柄的方向键将 D 移到 B 之前，再从菜单删除 D。
- 恢复队列后，编辑后的 B、后续 E 依次自动投递；原生实际用户消息顺序为 A、C、B、E，各出现一次。B 原文、已删除 D 和网页未发送草稿均未进入原生历史，工具调用数为零，待发送队列和未完成编辑最终清空；已结束编辑的回执仍用于幂等核对。
- 已确认空闲时暂停队列只暂停待发内容，不向空闲 CLI 发送按键，不把已完成回复改成“已停止”。此边界另有独立回归。

本轮检查：网页状态测试 236 项、远程队列 Rust 测试 7 项、Linux 适配器测试 13 项、验收写入边界测试 1 项均通过；22 个网页 JS 资源检查及管理器 TypeScript 检查通过。更宽的 Claude 测试曾遇到同一工作区新增未读标记用例失败，最终源文件的该项单独复跑已通过；不把这些局部检查表述成全量 remote-access 测试通过。

资源修订号为 `2026111118`。本轮仅管理自己的 `47137` 进程，保留主线程管理的 `47136`。原安装环境的 6 份配置哈希及原管理器、launcher/代理、正式远程服务的进程和启动时间均未改变。没有暂存、提交、推送或发版。

证据：

- `temp/claude-remote-queue-live.json`：会话身份、提交标识、首次排队及 Sol / Max。
- `temp/claude-remote-queue-restart-before.json`：重启前 B 编辑、D 待发送、原生 C 的绑定。
- `temp/claude-remote-queue-native-before-restart.json`：A、C 已进入原生，B、D 尚未进入。
- `temp/claude-remote-queue-native-final.json`、`temp/claude-remote-queue-live-check.json`：最终顺序、去重、删除及无工具调用。
- `temp/claude-remote-queue-isolation-check.json`：原安装环境隔离核对。
- `temp/claude-remote-queue-before.png`、`temp/claude-remote-queue-editing.png`、`temp/claude-remote-queue-auto.png`、`temp/claude-remote-queue-completed.png`：真实网页操作画面。

## 原生接口核对与原整队方案记录

日期：2026-11-11。仅核对 `codex-elves-claude-test` 中原版 Claude Code 2.1.322，不操作 `claude-us`。

| 能力 | 原版行为与当前接入 |
| --- | --- |
| 执行中补充消息 | 已接入原生队列，由原版决定何时读取及是否合并回复 |
| 立即执行 | 原版支持整队立即执行；网页显示一次“立即执行全部”，中断当前回复后让原版接收全部队列 |
| 逐条引导 | 本版本后台没有按消息 ID 单独释放一条的接口；不套用 Codex 逐条按钮 |
| 排序 | 官方交互文档未提供逐条排序，当前后台协议也没有重排操作；不开放网页拖动排序 |
| 编辑、删除 | CLI 的 `Up` 会把待发送内容整批取回共享输入框，编辑后作为一条重新排队，清空可丢弃；没有网页可用的逐条编辑、删除接口 |

立即执行使用原版 `Esc` 行为，保留原版 CLI 未发送草稿。它不是逐条引导，也不会由网页另起执行器或重发队列。原生正在等待审批或问答时，不把这个动作当作回答面板的按键。

本机后台协议的操作集合包含 `reply`、`attach`、`subscribe` 等，没有按队列消息 ID 编辑、删除、排序或释放单条的操作。`reply` 只提交整条内容，且不返回原生队列项 ID。上述结论限定为本机当前版本，不代表 Claude 的其他客户端或未来版本没有相关能力。

网页复用现有队列展示和操作回执，以一条当前队列消息的 ID 核对状态，实际返回 `scope: all`。已创建的旧 print 会话不声明该能力。CLI 原版输入和队列行为保持。

官方核对来源：

- `https://code.claude.com/docs/en/interactive-mode.md`：Queue messages while Claude works、When Claude Code sends what you queued、Take back what you queued。
- `https://code.claude.com/docs/en/agent-view.md`：Peek and reply。

自动检查：网页测试 233 项、Claude 后端测试 9 项、验收写入边界测试 1 项、管理端资源与 TypeScript 检查通过。

真实网页会话为 `claude-1be9fdf4-2aab-408c-8e54-cb31a3f24531`，原生 UUID 为 `406b1a2a-06d5-4444-bdda-2ea0da5d17c1`。在原版 CLI 中先输入但不发送 `CLI-DRAFT-KEEP-QUEUE-TEST`；网页随后在纯文本生成期间排入两条消息，并点击“立即执行全部”。实际回执为 `completed`、`scope: all`，原生成轮次变为 `interrupted`，两条排队消息均由原版接收并回复，队列最终为零。

操作后在原版 CLI 中重绘界面，输入框仍显示原草稿；原版 `claude logs` 也保留操作后的草稿画面。规范消息中没有这条草稿。7 条实际提交的用户消息之外，原版另生成一条中断标记；核对规范记录时不把它误算为重复发送。

初次按钮验收被独立入口的旧白名单拒绝（409），没有触发原生控制；该次不算通过。白名单补齐并增加回归断言后才完成上述验收。原版拒绝了初始样例中的单独 `sleep 90`，未绕过其限制，后续改用纯文本生成进行中断测试。网页也已去掉 Codex 的单条引导预览，避免在整队操作期间重复显示首条消息。

本地证据：`temp/claude-queue-controls-check.json`、`temp/claude-queue-cli-draft-check.json`、`temp/claude-queue-cli-proof.ansi`。界面截图为 `temp/claude-queue-controls-before.jpg`。核对后已清空仅为测试输入的草稿并断开自己的 CLI 客户端，保留后台会话供查看。

最终资源版本为 `2026111113`，已更新 47136 独立验收入口。刷新后核对同一测试提示只显示其两次实际提交，没有额外引导预览；完成画面保存在 `temp/claude-queue-controls-after.jpg`。临时浏览器网络覆盖已恢复。`cargo fmt --check`、`git diff --check` 通过；原安装环境的 6 份配置、原管理器/代理/正式远程服务进程及启动时间保持不变，详见 `temp/claude-queue-isolation-check.json`。

## 已接收消息的队列残留恢复

用户会话 `claude-93c54b6c-5936-4234-b00a-dc254fbcd365` 中，“当前还有哪些未完成的”已在 2026-11-11 15:03 被原版以 `absorbed_mid_turn` 接收。原生历史使用 `queued_command` 人类消息附件记录此次输入，旧连接端漏读该类型，留下客户端消息 `e37b1d3a-6dbd-4323-b3a5-4bf22032077c` 的排队残留。

连接端现将已接收的人类排队附件映射回用户消息，保留客户端身份，并自动移除对应排队记录。已结束会话的恢复也传入保存的后台 ID，不依赖仍在线的 supervisor 才能补齐历史。

旧引导回执不存在时，网页仅在只读核对“目标已不在队列”和“原生历史已包含对应客户端消息”后结束等待；其余不确定情况保留页面内提示，不反复弹红色错误。验收入口的派发前拒绝现明确返回 `notSent`，客户端据此清理未提交操作的等待。整队操作回执也返回原消息标识，供恢复时核对身份。

实际核对：该排队残留已清除，历史中对应消息只有一条；用户原页面的旧等待已解除。原会话队列区和待确认区为空，输入临时检查文字时发送按钮可用，检查后恢复输入，未发送测试消息。临时浏览器覆盖已恢复，用户后来切换的页面保持。

资源版本 `2026111114`。前端 235 项、Python 历史与离线恢复 4 项、Claude 后端 9 项及验收边界 1 项检查通过；管理端资源与 TypeScript 检查通过。证据为 `temp/claude-stale-queue-recovery-check.json`、`temp/claude-stale-queue-browser-check.json`，截图为 `temp/claude-stale-queue-fixed.jpg`。
