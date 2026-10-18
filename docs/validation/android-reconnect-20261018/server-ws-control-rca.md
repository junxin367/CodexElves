# 服务端事件 WebSocket 控制帧缺陷与最小修复

日期：2026-10-18。追加范围：`crates/codex-elves-core/src/remote_access.rs::stream_websocket_events`。先完成精确依赖审计和真实本机控制帧对照，再按主代理明确授权修复这一个函数。Android Java renderer 缺陷本轮不改。

## RCA 结论

- 状态：**部分确认现场原因；已确认并修复控制帧处理缺陷**。
- 现象：用户反馈 Android 前台和恢复前台时均可能频繁断线。追加检查发现服务端事件流原实现只发送 Text，没有读取客户端帧。
- 直接原因：在已授权、服务运行且对端发送合法 Ping 或 Close 的条件下，原函数没有驱动 WebSocket 读取，因此不会解析控制帧，不能触发依赖库的自动 Pong 或 Close 应答。
- 根因：原事件流循环仅等待业务广播并发送事件，把“业务事件只从服务端发送”实现成“WebSocket 连接无需读取”。`send()` 的 write/flush 链不会替代读方向的协议推进。
- 系统性根因：能确定当前函数缺少控制帧处理分支；其历史设计或测试流程为何遗漏该合同未进一步归因。本轮用真实控制帧构造了可检出的反事实对照，但依照任务要求没有保留永久测试。
- 关键因果链：合法 masked Ping 到达 → 原函数始终不 poll 接收流 → tungstenite 不执行 `read_message_frame()` → 没有自动排入 Pong → 参与该 WebSocket 控制帧协议且要求 Pong 的对端可据此终止连接。最后一项在手机/公网链路是否发生未知，因此不认定为用户现场唯一根因。

对 Close 的独立链路为：对端发送 Close、等待服务端应答并保持 TCP 打开 → 原函数没有读取 Close → 没有 Close 应答，仍可发送 Text，函数局部广播 receiver 继续存活。**这不等于已经完成 TCP 关闭后永久泄漏订阅**。原实现仍可能在后续发送失败、授权撤销或服务停用时退出。

## 精确依赖与官方合同

`Cargo.lock` 中服务端链路为：

```text
axum 0.8.9
  -> tokio-tungstenite 0.29.0
     -> tungstenite 0.29.0
```

工作区直接配置的 `tokio-tungstenite 0.26.2` 用于其他客户端，并非 axum 服务端内部版本。本次实际函数测试恰好使用该客户端与服务端 0.29.0 建立真实本机连接；没有混淆两者。

| 判断 | 类型 | 可复核证据 |
| --- | --- | --- |
| axum 升级后没有替应用独立运行读取循环 | 事实 | `axum-0.8.9/src/extract/ws.rs:359` 创建 socket 后直接 `callback(socket).await` |
| axum 的 `send` 和 `recv` 是两条调用链 | 事实 | 同文件 `:555` 的 `recv()` 调用 `next()`；`:560` 的 `send()` 调用 inner 的 `SinkExt::send()` |
| 异步发送不会读取控制帧 | 事实 | `tokio-tungstenite-0.29.0/src/lib.rs:333`、`:345`、`:365` 的 Sink 只调用 flush/write；`:291` 的 Stream `poll_next()` 才调用 `read()` |
| 自动 Pong 和 Close 依赖读取推进 | 事实 | `tungstenite-0.29.0/src/protocol/mod.rs:664` 处理 Close；`:668` 处理 Ping 并排入 Pong；`:676` 返回 Pong；`:717` 的 `do_close()` 更新状态并排入 Close 应答 |
| 收到 Ping 后需要对应 Pong | 合同 | RFC 6455 §5.5.2、§5.5.3；回应 Pong 的 payload 必须与 Ping 相同，已收到 Close 时存在规范例外 |
| 收到 Close 后需要 Close 应答 | 合同 | RFC 6455 §5.5.1；双方完成关闭消息交换后终止底层 TCP |
| JSON 心跳不是协议 Pong | 事实与合同判断 | 原函数使用 `Message::Text` 发送 `remote/heartbeat`；协议 Pong 的 opcode 为 `0xA`，不是 Text |

本地读取的精确依赖源码位于：

```text
C:\Users\junes\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\
```

对应文件摘要及锁文件摘要记录在 `server-ws-control-evidence.json`。Context7 返回月度配额不足；本轮使用固定版本官方 docs.rs、RFC 和本地精确依赖源码，不以经验补全库行为。

## 修改前的真实本机对照

临时程序直接复用现有编译产物中的 tungstenite 0.29.0 和 tokio，使用真实环回 TCP、WebSocket 握手、合法 masked 客户端控制帧和真实 broadcast receiver。没有完整 Cargo 构建。发送间隔为测试用的 100 ms，不能当作生产的 15 秒间隔。

| 条件 | 只发送分支 | 读取并发送的对照 |
| --- | --- | --- |
| 发送 Ping | 约 525 ms 内收到 6 个 Text、0 个 Pong | 收到 1 个 payload 完全匹配的 Pong |
| 发送 Close，但客户端继续保持 TCP 打开 | 约 527 ms 内仍收到 6 个 Text、0 个 Close；receiver count 为 1 | 收到 Close；约 20 ms 的观测时 receiver count 为 0、循环已经结束 |

4 个条件断言通过，直接 `rustc` 编译和程序运行退出码均为 0。此阶段没有直接运行原生产函数；精确依赖调用链把该对照与原函数只 send 的机制连接起来。后续另行运行了实际修改后的生产函数，见下节。

证据：`server-ws-control-evidence.json`。原始临时文件位于 `temp/android-reconnect-native-20261018/`，按主代理要求未删除。

## 最小修复

最终永久 Rust 差异只在 `stream_websocket_events`，31 行增加、6 行删除：

- 创建持续存在的 15 秒 sleep，通过 `tokio::select!` 同时等待 socket 接收、业务广播和该心跳。只有心跳自身到期时重设下一次到期；Ping、Pong、Text、Binary 均不重新创建或推迟心跳等待。
- 收到 Ping 后只 `flush()` 库已自动排入的 Pong，不构造第二个 Pong。Pong 被读取并忽略。
- 收到 Close 后调用现有 `close()` 驱动库已排入的 Close 应答，再退出；接收错误或流结束直接退出，释放局部广播订阅。
- 客户端 Text/Binary 被读取后忽略，不进入动作分发。
- 保留等待前和发送前的 `device_active`、`accepting_requests` 核验，以及 `remote/unauthorized`、`remote/disconnected`、广播 lagged 的 `remote/resync` 语义。
- 没有提高超时，也没有修改 Cookie、网络权限、Java、服务配置或 renderer 策略。

最终函数 UTF-8 文本 SHA-256（去除尾部空白后）：`10d38e549048757c4e0f83adf772751b39a68569ecb6a7e3e58a7418b3b1d225`。

## 实际生产函数验证

临时测试通过既有 `remote_access::tests` 模块中的临时 include 直接调用实际修改后的 `stream_websocket_events`，使用真实 axum 升级、tokio-tungstenite 客户端和 `RemoteAccessController`；授权撤销调用实际 `admin("revoke", ...)`。所有状态目录位于指定临时目录，没有真实设备凭据、原生会话动作或公网服务。

执行：

```text
cargo test -p codex-elves-core --lib ws_control_function_probe -- --test-threads=1 --nocapture
```

最终结果：**5 通过、0 失败，638 项未运行，退出码 0**；该轮测试报告耗时为 33.37 秒。不是 Android 或公网耗时。

| 实际函数场景 | 观测 |
| --- | --- |
| 合法 Ping、客户端 Text/Binary、随后 Close | Pong payload 匹配；Text/Binary 没有调用 native action；收到 Close 应答后 receiver count 为 0 |
| 持续控制帧与业务广播并存 | 每 40 ms 发送 Ping 和 unsolicited Pong；376 个 Ping 期间读取到 375 个对应 Pong；业务 Text 正常到达，JSON heartbeat 在 15.014 秒到达，随后 Close 释放订阅 |
| 对端直接丢弃 TCP | receiver count 归零的等待观测为 3 ms，没有等待下一次 15 秒心跳 |
| 实际 admin revoke | 收到 `remote/unauthorized` 和 Close，receiver count 为 0 |
| 服务 running 变为 false，并唤醒广播 | 收到 `remote/disconnected` 和 Close，receiver count 为 0 |

连续控制帧测试在心跳到达后就发起关闭，最后一个 Ping 可能仍在途，不能把 376/375 报为修复后丢失 Pong 的结论。

首轮为 4 通过、1 失败：连续控制帧场景的业务和心跳断言已经通过，但临时测试误要求关闭后的下一帧必须立刻为 Close，读到了此前在途的 Pong。只修正测试为继续读取到 Close 后核对，再完整执行同一 focused filter，得到最终 5/5；没有为测试放宽生产超时。

最终 `cargo fmt -p codex-elves-core -- --check`、`git diff --check` 均退出 0。验证完成后已移除源码中的临时 include，永久源码差异仅剩上述函数。临时测试文件、状态目录和程序由主代理统一清理，本子任务没有删除文件。

证据：`server-ws-function-evidence.json`、`server-ws-function-tests.log`。主代理已收到“源码稳定，可统一 build”的通知。本子任务没有进行安装包构建、安装、服务重启、提交或推送。

## 已排除解释

- **“axum 会在独立后台自动读取，所以只 send 也会 Pong”**：当前固定版本升级后只等待应用回调；收发调用链与真实对照均否定该解释。
- **“应用层 JSON heartbeat 足以响应协议 Ping”**：两者使用不同帧类型，不能替代。
- **“一接收到任意客户端数据就需要执行动作”**：控制帧读取是连接协议职责；客户端 Text/Binary 没有动作入口，实际测试 native action 调用数为 0。
- **“只要修复 recv，就可以每次 Ping 重建 15 秒 timeout”**：持续控制帧会让这种结构不断延后 JSON heartbeat。本次使用独立持久计时器，并由超过 15 秒的真实持续控制帧测试验证没有该问题。

## 异常状态溯源

- 适用约束：连接已建立、尚未接收 Close 时，合法 Ping 必须使服务端回应 payload 相同的 Pong；合法 Close 必须推进服务端的 Close 应答与终止。
- 约束依据及状态：RFC 6455 §5.5.1—§5.5.3；已确认。
- 本次运行前提命中：真实本机受控输入命中；用户手机/公网现场无法判定。
- 值判断：修改前 send-only 对照中的 Pong/Close 应答缺失违反该条件下的协议合同。
- 实际来源与传播路径：测试客户端发送合法 masked 控制帧 → 底层 TCP 接收缓冲 → 应用未 poll Stream → 依赖未解析控制帧 → 自动响应未排入发送队列。最早失去合同保障的位置为生产函数只等待业务 receiver 的分支。
- 最后满足约束的状态 → 首次违反点：连接握手成功、帧合法 → 服务端没有驱动读方向，超过受控观察窗口仍只发送 Text。修复后实际函数收到对应 Pong/Close，关键链路被阻断。
- 调查边界：精确依赖、真实本机连接和实际修复函数；手机、隧道端点及公网关闭策略未知。

## 未闭环项与下一步

- 尚无手机/公网链路发送 Ping 的原始证据，也没有对端因缺少 Pong 而关闭的日志；因此不能把已修复的协议缺陷写成现场频繁断线的唯一根因，不能量化断线概率。
- 本机测试未模拟公网代理背压、Android 后台调度或用户设备实际 WebView。它验证的是当前服务端协议处理与授权终止路径。
- 修复需要主代理统一构建并更新电脑侧运行服务后才生效；本轮没有部署。网页心跳 UI 与 worker 修复由主代理合并验收。
- 若更新后仍频繁断线，需要同一连接的关闭码、控制帧/网络变化与前后台时间线继续判定，不能继续用缺权限或 renderer 退出猜测。

## 官方参考

```text
https://docs.rs/axum/0.8.9/axum/extract/ws/struct.WebSocket.html
https://docs.rs/axum/0.8.9/axum/extract/ws/enum.Message.html
https://docs.rs/tokio-tungstenite/0.29.0/tokio_tungstenite/struct.WebSocketStream.html
https://docs.rs/tungstenite/0.29.0/tungstenite/protocol/struct.WebSocket.html
https://www.rfc-editor.org/rfc/rfc6455.html#section-5.5.1
https://www.rfc-editor.org/rfc/rfc6455.html#section-5.5.2
https://www.rfc-editor.org/rfc/rfc6455.html#section-5.5.3
```
