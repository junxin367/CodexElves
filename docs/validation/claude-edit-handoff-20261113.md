# Claude 编辑交接验收

日期：2026-11-13。

## 结论

网页编辑 Claude 历史消息改为交接方式（用户确认的 B 方案），流程为：停止同一后台会话 → print 协议 `rewind_conversation` 结构化回退 → `claude respawn` 恢复同一会话 → 发送修改后的消息。

在测试 WSL 中用真实模型验收通过，具体结果如下：
- 原版终端在线时可以完成编辑；
- 会话 UUID 与后台短 ID 不变，原生记录只追加回退标记；
- 后台任务运行期间拒绝编辑，且不停止会话；任务结束后可以编辑；
- 终端重新 attach 后显示编辑后的内容。

## 环境

- WSL：`codex-elves-claude-test`，用户 `elves-test`；Claude Code 2.1.323，模型 `gpt-6.1-sol`。
- 使用临时的隔离 `CLAUDE_CONFIG_DIR`，带独立 daemon 与控制密钥；配置目录与工作目录均在验收后删除。
- 连接端为仓库当前的 `claude_shared.py`，按远程服务相同的 stdin/stdout JSON 行协议驱动。

## 结果

| 场景 | 结果 |
| --- | --- |
| 依次完成 READY、W1、带 Bash 工具调用的 T1 三轮；原版终端 `claude attach` 在线时编辑 W1 | 4.5 秒完成，回执 `reverted: true`、`accepted: true`；当前分支变为 READY → W1-EDITED |
| 原生记录 | 追加 `last-prompt`（`explicit: true`、`rewound: true`），旧分支保留；只有一个记录文件，没有分叉会话 |
| 身份 | 后台短 ID 与会话 UUID 不变 |
| 原版终端 | 显示 `Session … has exited — run claude attach … to resume it` |
| 已见校验：网页当前分支最后一条 user 记录为工具结果 | 通过 |
| 后台命令 `sleep 45` 运行期间编辑（底栏 `1 shell · ↓ to manage`） | 拒绝，返回 `EBUSY`“Claude 仍有后台任务在运行…”；会话保持运行 |
| 后台任务结束后编辑：最后一条 user 记录为任务通知 | 成功，当前分支变为 READY → W1-SECOND |
| 原版终端重新 `claude attach` | 画面显示 W1-SECOND，不含旧回复，输入框为空 |

## 验收中发现并修正的问题

第一次做后台任务验收时，后台任务已经结束，连接端仍判定为“无法确认后台任务状态”。

原因是连接端单独订阅画面快照时，每帧读取上限为 1 MiB，而该会话的快照超过了这个上限；另外订阅流只重绘变化区域，分隔线不一定在画面里。

修正后，连接端优先使用自己持续维护的会话画面，只检查画面底部三行里带箭头的 `↓ to manage`，复测通过。

## 边界

- `rewind_conversation` 是 print 协议中未公开的控制请求，CLI 升级后需重新执行本验收。
- 后台任务依据原生底栏提示判断；画面无法读取时拒绝编辑，不冒险停止会话。
- 原版终端自己的 `/rewind` 不写记录标记，网页在终端发出下一条消息后才显示其结果。
- 交接期间如果有人重新打开该会话，连接端会报告“回退可能未生效”，不发送修改后的消息。
- 同一原生会话的网页编辑由连接端文件锁串行化；正式服务与验收服务连接同一实例时同样互斥。
