# 远程发送延迟与 Checkpoint 锁等待

日期：2026-11-12，以下现场时间均为北京时间。检查与修复未发送测试消息、未修改问题会话的项目文件和现有快照、未切换运行中的服务。

## 现场证据

手机截图中的会话通过 Codex 原生 CDP 路径发送。服务执行日志中，最近三条已确认发送分别耗时 11,878、10,625、19,334 ms；截图对应请求耗时 45,021 ms 后结束 CDP 等待，回执被保留为 unknown。同期 sendState 查询只需 262～416 ms。

手机诊断记录中，同一次 `/api/action/send` 在 40,700 ms 后成功收到 HTTP 200。这里的成功只是收到服务的 pending 回执，不是原生已接受消息；随后页面继续查询同一消息身份，未自动重发。

本机桥接日志、原生会话只读历史及 app-server 日志确认了更深的阻塞链：

| 时间 | 事件 |
| --- | --- |
| 20:57:28.292 | 上一轮 `/workspace-checkpoint/complete-turn` 开始 |
| 20:57:59.749 | 下一条消息的 `/workspace-checkpoint/create` 开始等待 |
| 20:58:37.235 | 远程 CDP 调用达到原有 45 秒上限 |
| 20:59:00.679 | 原生收到该条用户消息；此前快照等待已达到 60 秒上限 |
| 21:00:38.242 | 上一轮 complete-turn 完成，总耗时 189,949 ms |
| 21:00:38.683 | 下一条 create 完成，总耗时 158,933 ms；锁释放后只用了约 441 ms |

同一工作区还出现过 187,181 ms 的快照创建和 206,851 ms 的轮次封存。普通创建快照通常只需约 0.4～1 秒。

现场来源：

- CodexElves 的 `codex-elves.log`、`remote-access-receipts.json` 及手机诊断报告。
- 当前桌面会话的只读 RPC 历史，以及 `.codex/logs_2.sqlite` 的只读查询。
- 相关工作区的 Checkpoint Git 文件树与索引。

原始日志留在本机，未复制消息正文、访问凭据或设备授权到仓库。

## 最终处理：移除工作区 Checkpoint

用户在进一步收敛要求后明确要求删除该功能。前一阶段的索引复用和 90 秒发送等待方案已撤回，不作为最终实现保留。

- 删除工作区快照引擎、创建/绑定/封存/恢复路由、专用设置、Tauri 管理命令和权限、管理页、桌面入口、定时和通知订阅、发送前后拦截。
- 删除对应的旧功能测试；新增旧配置及旧路由无法重新启用捕获的回归。
- 原生提交只保留 Fast 参数兼容，原生成功及失败回执直接返回。远程桥接继续使用原有 45 秒边界。
- “打开目录”改用独立只读接口 `/session/workspace-context`，保留准确会话身份校验。
- 原有磁盘快照未删除、迁移或压缩。协议续传与上下文压缩中同名的内部记录不属于工作区快照，未改动。

运行中的正式实例已通过既有设置桥接停用自动快照，并同步桌面设置。只读核对确认持久设置及桌面实际功能开关均为 false。未重启管理器、launcher、代理或会话，未向用户会话发送测试消息。

## 同时完成的会话优化

1. 删除普通发送入口的一次重复会话读取，保留前置权限校验、提交内部读取和原生锁内执行状态复核。
2. 网页及原生桥已有同一消息的接受证据时直接确认；正在等待的慢查询不会阻止新到达的真实接受事件收敛回执。
3. 送达核对排除没有真实轮次、等待准入、发送占位和未确认队列记录；历史查询释放 RPC 资源，连接变化时拒绝迟到结果。

未知结果仍保留原请求，不自动重发；没有新增模型调用、自动恢复会话或乐观成功展示。

## 验证与交付范围

- 前端状态回归 331 项通过，新增接受事件优先、未准入记录、旧授权、迟到连接及资源释放场景。
- 管理器 TypeScript 与 23 个网页 JS 资源检查通过。
- `cargo check --workspace --profile release-fast` 通过。
- Rust 回归 372 项通过：原生桥接 258、桥接路由 26、启动器及本机服务 88。沿用既有环境限制，单独排除 1 项 IPv6 回环测试。
- `cargo fmt --all --check`、`git diff --check` 通过；最终工作区编译检查无警告。

验证命令：

```powershell
node --test scripts/remote-access-ui-state.test.mjs
cargo test -p codex-elves-core --profile release-fast --test cdp_bridge --test bridge_routes --test launcher -- --skip list_targets_can_query_ipv6_loopback_cdp_endpoint
cargo check --workspace --profile release-fast
cargo fmt --all --check
git diff --check
# apps/codex-elves-manager 目录
npm run check
```

网页资源号为 `2026111259`，原生桥版本为 `147`。源码删除和其他会话优化尚未部署；当前正式实例通过停用开关立即避开自动快照流程。未提交或推送。

后续按用户要求运行 `scripts/build-windows-installer.ps1`，于 2026-11-12 21:55 完成 Windows 0.7.0 安装包（52,682,938 字节）及独立远程客户端（23,019,008 字节）。四个 Windows 程序的 release 镜像与安装输入副本摘要一致，内嵌资源及已移除快照入口均已核验。未自动安装，未构建 Android。
