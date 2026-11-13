# Claude 原生引导验收

日期：2026-11-13。

## 行为

- 网页尚未投递的队列保留排序、编辑与删除；点击引导只释放选中的一条。
- 原生入口使用 `reply(nextTurn: true)`，在本次验证的 Claude Code 2.1.323 中映射为 `priority: "next"`。
- 工具调用完成后由原生吸收引导；本轮先结束时由原生安排下一轮。引导不发送 Esc、不等待停止，也不标记旧轮次中断。
- 停止、工具审批保持独立；权限等待中的引导只进入原生队列，不替用户批准、拒绝或取消工具。
- 接收回执与实际吸收分开处理，正文以原生消息记录为准，继续使用客户端消息与原生 UUID 的关联去重。未知结果不自动重发。
- 保留此前的真实执行状态读取、完成记录纠正和无回复终态收敛。

## 验证

仅使用独立测试 WSL `codex-elves-claude-test`，模型 `gpt-6.1-sol`、推理等级 `max`。临时配置只允许测试用的 `sleep 12` 命令，复用既有 API 配置；未修改原实例配置。

| 验证项 | 结果 |
| --- | --- |
| 工具执行中投递引导 | 同一原生会话受理，工具正常完成，`interrupted=false` |
| 原生吸收方式 | 引导记录为 `attachment / queued_command` |
| 同轮证据 | 引导前没有轮次结束记录，全程仅一条 `turn_duration` |
| 新指令生效 | 未输出原要求的 `FIRST_DONE`，按引导输出 `STEER_DONE` |
| 单次投递 | 原生引导消息仅匹配一次，待接收列表无残留 |
| 中断副作用 | 中断按键调用次数为 0 |
| 接收阶段耗时 | 两次隔离验证分别约 2.01ms、1.89ms；仅包含本地原生受理，不代表网页端到端耗时 |
| 连接端回归 | 既有 Python 测试文件中的 88 项通过 |
| 服务端队列回归 | 选中 C 时只投递 C，B、D 保留；原轮次仍为执行中；定向 Rust 测试通过 |
| 清理 | 测试后台结束，临时文件清理，原配置摘要前后一致 |

本次验证覆盖真实原生链路与隔离队列回归，不替代正式网页、Android 或 Windows 客户端的端到端验收。开发验证阶段未执行正式服务热更新或安装包构建。

## 实现参考

- [Claude Agent SDK 的 SDKUserMessage 优先级](https://code.claude.com/docs/en/agent-sdk/typescript#sdkusermessage)
- [Paseo 的 Claude steerActiveTurn 实现](https://github.com/getpaseo/paseo/blob/main/packages/server/src/server/agent/providers/claude/agent.ts)
- [Claude Code UI 的 steerIntoTurn 实现](https://github.com/siteboon/claudecodeui/blob/main/server/modules/providers/list/claude/claude-runtime.provider.ts)
