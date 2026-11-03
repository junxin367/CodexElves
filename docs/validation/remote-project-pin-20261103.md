# 远程会话 R1-27 项目与会话置顶同步验收

**日期：** 2026-11-03

**结果：** 项目 P 与会话 C 的置顶、刷新保持、跨端观察及取消置顶恢复均通过。

## 验收对象与环境

- 项目 P：`workspace`，ID `22195cc3-55de-49f6-8c22-2e18fbfb811c`，路径 `temp/checkpoint-live-run-2026-09-02-01/workspace`。
- 会话 C：ID `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。
- 操作端：Chrome 插件自建测试标签 `1283922920`，地址 `127.0.0.1:1432`。
- 观察端：Chrome 插件自建只读标签 `1283922921`，地址 `127.0.0.1:45222`。
- 两端 C 的输入框在验收前后均为空；没有发送消息。没有操作主会话 M 或标签 `1283922838`。

## 结果

### 项目 P

1. 初始时 P 位于 `sidebar-section-project:projects`。
2. 在 1432 的项目菜单选择“置顶项目”后，P 移入 `sidebar-section-project:pinned`；刷新 1432 后仍在置顶区。
3. 刷新 45222 后，P 也出现在置顶区。
4. 在 1432 选择“取消置顶项目”后，P 回到 `sidebar-section-project:projects`。
5. 刷新两端后，P 均保持未置顶；项目列表顺序与本轮初始基线相同。

### 会话 C

1. 初始时 C 位于 `sidebar-section-project:recent`，在最近列表中为第 4 项。
2. 在 1432 的会话菜单选择“置顶会话”后，C 移入 `sidebar-section-project:pinned`；刷新 1432 后仍在置顶区。
3. 刷新 45222 后，C 也出现在置顶区。
4. 在 1432 选择“取消置顶会话”后，C 回到最近列表。
5. 刷新两端后，C 均保持未置顶，并回到置顶前的位置；C 前后相邻会话 ID 与置顶前快照相同。两端 C 输入框仍为空。

在本轮初始基线与 C 置顶前的快照之间，最近列表最前两项互换了顺序：`01a19f1c-e19a-7141-a126-97c1e076f624` 与 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`。本次操作没有选择或修改这两条会话，变化原因未确认。C 仍处于第 4 项，且前后邻项未变；取消置顶后的完整最近列表顺序与 C 置顶前快照一致。

## 服务身份边界

验收开始及结束时读取到的 `/app.js` 响应身份一致：

| 端口 | 大小 | SHA-256 | 本轮用途 |
| --- | ---: | --- | --- |
| `1432` | `692051` 字节 | `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387` | 置顶与取消置顶操作端 |
| `45222` | `683201` 字节 | `e68492372ce4b4465d3261d1380bad1d89ef7de6028ac497f275c1dfcf424fdb` | 旧脚本同步观察端 |

本结论只覆盖 `1432 → 45222` 这条链路；45222 运行的脚本早于 1432。本轮没有验证正在构建中的 `45430` 服务，也不据此推断其行为。

## 证据

完整 DOM 快照、截图、最终状态与脚本身份记录位于：

`dist/validation/remote-acceptance-20261103/project-pin/`

关键文件包括 `00-baseline.json`、`02-project-pinned-1432.*`、`04-project-pinned-45222-menu.*`、`07-project-restored-after-refresh-*.*`、`08-thread-pin-menu-1432.*`、`09-thread-pinned-1432.*`、`10-thread-pinned-after-reload-1432.*`、`11-thread-pinned-after-reload-45222.*`、`14-thread-restored-after-refresh-*.*`、`14-final-state.json` 与 `15-final-identities.json`。
