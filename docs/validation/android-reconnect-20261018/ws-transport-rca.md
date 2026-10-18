# 远程事件 WebSocket 连接调查与验证

日期：2026-10-18。范围：`assets/remote-access/events-client.js` 与 `events-worker.js`。本记录使用 Node v22.22.2 的隔离 VM 直接执行源文件，以可控 WebSocket、MessagePort、可见性、在线状态与时钟验证实际分支。未连接用户会话、发送消息或审批，未安装、重启正式服务或构建 APK。

## RCA 结论

- 状态：**部分确认**。已复现并修复 fallback 隐藏页面时主动拆除正常连接的条件缺陷；尚不能据此判定用户持续前台和所有 Android 现场重连的原因。
- 现象：用户反馈持续前台也频繁断线，切后台、锁屏和选图返回同样会出现。现场是否每次均发生 WebSocket 关闭、或只是界面连接状态改变，缺少手机侧连接日志。
- 直接原因：旧 `fallbackStream.visibility()` 在 `document.hidden` 变成 `true` 时调用 `pause()`；`pause()` 无条件清定时器、执行 `closeSocket()`，并把公开连接状态改为 `CONNECTING`。恢复可见后重新握手。
- 根因：fallback 将页面可见性变化直接当作连接不可用，未先区分仍处于 `OPEN` / `CONNECTING` 的连接与真实的离线、异常关闭、活动超时。隔离环境中无需网络故障，仅隐藏 2 秒即可主动关闭原连接。
- 系统性根因：本次范围可定位到两处连接生命周期决策：`app.js` 与 fallback 都独立处理隐藏时关闭。即使上层保留连接，下层也会自行关闭。原始设计依据、实际发布资源版本与手机运行日志未取得，系统层归因仍限于源码中的这两处决策，不能延伸为真机整体原因。
- 关键因果链：页面隐藏 → fallback 的 `pause()` → 关闭健康或握手中的 WebSocket → 恢复页面时创建新连接 → 再次触发连接打开与状态核对。

## 证据

| 判断 | 类型 | 证据 | 来源 |
| --- | --- | --- | --- |
| 隐藏触发的连接关闭不需要网络异常 | 事实 | 修改前 `fallback-hidden-keeps-open-socket`、`fallback-hidden-keeps-handshake` 均失败，原 socket `closeCalls` 为 1 | `ws-baseline.json`；旧 `events-client.js` 的 `visibility → pause → closeSocket` |
| 两条路径的健康前台心跳未触发自发重连 | 事实 | 分别模拟 10 分钟、每 15 秒一条心跳，均为 1 socket、1 次 open、0 error | `ws-baseline.json`、`ws-fixed.json` 的两个 `foreground-10m` 场景 |
| 当前超时机制已有暂停调度宽限 | 事实 | watch 间隔 5 秒；握手 45 秒、活动 75 秒；超过 30 秒调度间隔后给 20 秒恢复宽限 | 两份事件源码常量与 `watch()` |
| 服务端具有事件心跳 | 事实 | 正常无事件时，`receiver.recv()` 等待 15 秒后发 `remote/heartbeat` | `crates/codex-elves-core/src/remote_access.rs:2962` |
| 移除主动关闭可阻断该条件链 | 结论，限定隔离场景 | 相同 19 场景修改前 16 通过、3 个预期失败；修改后 19 全通过 | 两份 JSON 对照 |
| Android 用户现场也执行了相同条件链 | 待验证 | 未取得 WebView 版本、SharedWorker 可用性、实际资源版本与 close code/time | 真机日志缺失 |

## 已排除解释

- “所有前台周期性断线都来自过短的 WS 超时”：受控健康前台心跳在两条路径都稳定 10 分钟，未触发任何超时。该结果只排除本次受控序列，不能排除真实公网心跳停止、系统调度或代理中断。
- “旧 socket 的 error/close 连续回调会重复重连”：两条路径的 socket 身份检查均通过受控旧回调对照，1 次故障只新建 1 条连接，旧消息未传给消费者。
- “重复 online 会无条件创建多个 socket”：fallback 与共享多订阅场景均只有一条新的连接。
- “worker 的 pong 会掩盖真正 WS 断线”：保持 port 响应但停止 WS 心跳，worker 仍在 75 秒后报告活动超时并重试。
- “只是调大超时才让验证通过”：所有超时、退避常量和真正的 error/close 处理均未修改。

## 修复与防复发

- 直接修复：fallback 隐藏时仅取消 watch 与待重试定时器，保留当前连接及其状态。
- 根因修复：可见或在线恢复时，有 socket 则恢复 watch 并使用原有 20 秒恢复宽限；无 socket 才连接。真实 `offline`、`error`、`close`、45 秒握手超时、75 秒活动超时与 `pagehide` 继续执行原有断开或重试逻辑。
- 系统性防线：上层 `app.js` 的隐藏时主动关闭须同步处理；该项由主代理负责。本次没有新增生产连接日志或永久测试文件。
- 生产修改仅 `events-client.js` 的 `restore()` 与 `visibility()`；未发现需要修改 `events-worker.js` 的已复现缺陷。
- 验证范围：以下 19 个隔离场景，加两份 JS 的 `node --check` 与指定文件的 `git diff --check`，退出码均为 0。

| 场景 | 修改前 | 修改后 |
| --- | --- | --- |
| fallback 前台 10 分钟/15 秒心跳 | 通过 | 通过 |
| fallback 隐藏保留已打开连接 | 失败：主动 close | 通过 |
| fallback 隐藏保留握手 | 失败：主动 close | 通过 |
| fallback 后台真正关闭，前台恢复重连 | 通过 | 通过 |
| fallback 后台连接陈旧，恢复宽限后超时重连 | 失败：隐藏已主动 close | 通过 |
| fallback 暂停调度 120 秒后，排队心跳恢复 | 通过 | 通过 |
| fallback 暂停后仍无心跳，宽限后真正超时 | 通过 | 通过 |
| fallback 真正握手超时 | 通过 | 通过 |
| fallback error/close 合并与旧代际隔离 | 通过 | 通过 |
| fallback 重复 offline/online 不重复连接 | 通过 | 通过 |
| fallback pagehide 终止后不自动重启 | 通过 | 通过 |
| SharedWorker 前台 10 分钟/15 秒心跳 | 通过 | 通过 |
| SharedWorker 隐藏 120 秒保持心跳与原连接 | 通过 | 通过 |
| SharedWorker 暂停 120 秒后排队心跳恢复 | 通过 | 通过 |
| SharedWorker 真正握手超时 | 通过 | 通过 |
| SharedWorker port 有 pong、WS 无心跳时真正超时 | 通过 | 通过 |
| SharedWorker error/close 合并与旧代际隔离 | 通过 | 通过 |
| SharedWorker 启动无响应时切 fallback | 通过 | 通过 |
| SharedWorker 单个订阅离线不关闭其他订阅；全离线后正确恢复 | 通过 | 通过 |

实际执行：

```powershell
node temp/android-reconnect-ws-20261018/ws-lifecycle.cjs --baseline --output temp/android-reconnect-ws-20261018/ws-baseline.json
node temp/android-reconnect-ws-20261018/ws-lifecycle.cjs --output temp/android-reconnect-ws-20261018/ws-fixed.json
node --check assets/remote-access/events-client.js
node --check assets/remote-access/events-worker.js
git diff --check -- assets/remote-access/events-client.js assets/remote-access/events-worker.js
```

基线运行在生产修改前，`--baseline` 要求恰好只有上述三个已定位场景失败，命令退出码为 0；它不表示基线全部通过。验证脚本为本次临时文件，按任务要求由主代理统一清理。本记录与 JSON 保留实际运行结果。

验证后文件 SHA-256：

- `events-client.js`：`F8831E1D29AB3F7D833299B9B7663886E90A6F6E934BDC9D772D94C3224456E8`
- `events-worker.js`：`8BAF0CF284EF7A3E3EAD8A0C62ACDD6F3A3889E2ED37ED467E1E32ECDFEBD5D3`
- 临时验证脚本：`A0A7C8ECDDE9D76EB9BC72C4D324653438D3F6B9CA8F138881F7F1C923833E85`

## 未闭环项与下一步

- 缺少 Android 手机本轮真实的 socket 创建/打开/关闭时间、close code、页面可见性、网络变化与实际加载资源哈希；因此不能宣称已修复全部频繁断线。
- 主代理另行复现并修复 `app.js` 将普通 HTTP 读取失败传播成连接断开的链路；这里的 WS 稳定性结果不可替代其验证。
- 公网地址的可达性、隧道重置和手机网络暂断可能形成真实 WS 故障，当前隔离验证没有覆盖公网链路。
- 端到端下一项是将更新后的两层生命周期策略用于 Android 真机，同时核对 WS 的真实关闭与界面状态变化；无需修改超时值或跳过真正重连核对。
