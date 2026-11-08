# Android 诊断日志

Android App 会记录最近的连接、WebSocket、图片及页面故障，便于区分手机网络、WebView、隧道、授权和网页资源版本问题。此能力需要包含 `app-info.diagnosticsVersion=1` 的新 APK，网页热更新不能替代原生采集。

## 手机导出与主动发送

- 已打开会话：进入“设置 → 诊断日志”，选择“导出”或“发送到电脑”。
- 无法连接：登录页可直接点击“导出诊断日志”。
- 页面启动失败或 WebView 渲染进程退出：原生错误页同样提供导出按钮。

“导出”生成 `CodexElves-diagnostics-<时间>-<随机标识>.json`，打开系统分享。可以保存文件，也可以把文件上传到当前排查对话。“已打开系统分享”只表示文件已准备好，不代表已经发给第三方。

“发送到电脑”使用当前认证及主入口/已授权 ngrok 选路，收到电脑端确认后才显示已收到。主入口可以是 zrok 或 FRP。断网时保留本地日志，可以先导出；不会把日志当成消息或审批发送。

## 在电脑上按需获取

在仓库目录使用 PowerShell：

```powershell
# 列出当前授权设备、最近日志元信息及等待中的采集请求
.\scripts\remote-diagnostics.ps1 -Action List

# 对指定手机请求日志，最多等待 30 秒
.\scripts\remote-diagnostics.ps1 -Action Collect -DeviceId "<设备 ID>"

# 只下发请求，或获取已经收到的日志
.\scripts\remote-diagnostics.ps1 -Action Request -DeviceId "<设备 ID>"
.\scripts\remote-diagnostics.ps1 -Action Read -DeviceId "<设备 ID>"
```

脚本只通过本机已有的后台管理接口工作，不新增监听端口。管理凭证从当前用户的后台状态文件读取，不作为命令行参数或输出。

`Collect` 收到与本次 `requestId` 一致的报告后返回 `Status=received` 和日志文件路径；等待结束仍未返回时显示 `Status=pending`。离线、旧 App、尚未启用新网页资源都不能报告采集成功。请求有效期为 120 秒；App 在有效期内重连后，原有会话状态轮询会补达当前设备的请求。超过有效期需要重新请求。日志上传失败可在后续状态轮询中重试，成功及进行中的同一请求会去重。

读取后的默认导出目录：

```text
%USERPROFILE%\.codex-session-delete\remote-client-diagnostics-export\
```

可以用 `-OutputPath <新文件路径>` 指定位置；脚本不会覆盖已有文件。电脑端原始存储位于 `remote-client-diagnostics/`，文件名为设备 ID 的 SHA-256，每设备保留最近一份。`Read` 返回的可能是此前收到的报告，排查时应核对 `requestId`、`receivedAtMs` 和 `capturedAtMs`。

## 采集范围与容量

| 范围 | 记录内容 |
| --- | --- |
| HTTP 请求 | 路径类别、主机、方法、状态码、响应类型、阶段、耗时及安全格式的 `requestId` |
| WebSocket | `socket.connect/open/ready/closed/timeout/error/retry/resume` 等状态转换，含关闭码 `closeCode`、是否正常关闭 `wasClean`、重试等待 `delayMs` 和本轮连续重试的尝试计数 `attempt` |
| SharedWorker | `worker.create/open/timeout/error/fallback/resume/close`，用于区分 Worker 启动、事件流超时和回退 |
| 双线路 | 测速结果、取消与超时区分、网络和响应读取错误 |
| 图片 | 缩略图或大图、成功或失败、自然尺寸、请求地址的安全部分 |
| JavaScript | 错误类型、受控来源文件及行号，不记录错误正文 |
| WebView | 原生错误码、SSL 错误、HTTP 错误、渲染进程退出；控制台只输出固定错误分类 |
| App 与资源 | App、Android、WebView 版本，精确网页资源 SHA-256、连接代次及资源选择和更新结果 |
| 生命周期 | 前后台及页面状态、在线状态、Wi-Fi/蜂窝/VPN 等网络类别；`isTrusted` 标记浏览器事件可信状态，`backgroundMs` 记录返回前台时本次隐藏时长 |
| 自动主题 | 当前、切换前及目标主题，自动/手动模式、触发来源、本地小时和时区、光照原值及前值、采样年龄、候选开始/取消/应用与等待时长 |
| 光照原生状态 | 上报或导出时的传感器支持状态、实际注册结果、前台状态、订阅数、最新原始光照及采样年龄 |

不记录问答正文、提示词、附件内容、密钥、Cookie、Authorization、URL 查询参数或片段、完整本地路径和异常堆栈。`requestId` 只用于跨事件和服务端连接日志关联，不包含会话凭据。网页桥、原生存储及电脑端分别执行字段白名单检查；未知字段被拒绝或丢弃。图片注册 ID 在电脑端进一步归类为 `:id`。

原生文件读写位于独立线程，队列最多 128 项。两份普通轮转日志各不超过 224 KiB−64 字节，另为主题日志预留 64 KiB，日志与连接标识合计仍不超过 512 KiB。主题中的切换前后记录、状态快照与日常光照各占最多 32 KiB、64 条，避免 HTTP 或连续光照回调挤掉切换证据。每次快照仍限 400 条、256 KiB，先保留主题记录，再用剩余容量保留最新普通记录，按事件时间合并。各组容量不足时淘汰组内较早记录；日志仍是有界采集，不能保证无限回溯。

退出授权或更换服务会一并清理普通、主题日志和导出文件，避免混入其他连接的数据。导出文件放在私有缓存目录，通过独立 FileProvider 临时授予所选分享应用读取权限。

电脑端仅接收当前认证设备的报告，单请求上限 512 KiB、报告上限 256 KiB、事件上限 400 条，最多保留 32 台授权设备的最近报告。普通主动上传至少间隔 5 秒；本机明确请求的报告可立即返回。设备撤销后不能继续上传，也不能通过媒体凭证上传或读取诊断日志。

## 关联电脑端服务日志

Android 网页的 GET `/api/session` 和 `/api/channels` 请求会附加单独生成的 `diag` 请求 ID；网页或 SharedWorker 的 `/api/events/ws` WebSocket 握手也携带独立 ID。服务端只对这三条 GET 路由记录有效的 `diag`，并把 ID、路由、阶段、状态码和耗时写入电脑端诊断日志：

```text
%USERPROFILE%\.codex-session-delete\codex-elves.log
```

用手机报告中的 `requestId` 对照该文件里的 `remote_access.connection_request` 记录。HTTP 阶段包括 `received`、`response_headers_ready` 和必要时的 `request_cancelled`；WebSocket 记录包括 `websocket_opened`、`websocket_handler_ended` 及 `closeCode`。HTTP 请求到达服务端但握手被拒绝时，仍有入站和响应状态记录，只是没有 `websocket_opened`。响应头就绪不证明手机已收全；找不到服务端记录时也要先排除日志范围、轮转或丢弃，不能直接断定网络故障。服务端日志不保存其他查询参数值。

## 排查时的使用方式

发生问题后尽快导出或请求日志，保留故障前后的记录。结合 `capturedAtMs` 与电脑端 `receivedAtMs` 判断手机时钟偏差；用 `requestId` 关联手机 HTTP/WebSocket 事件和电脑端 `diag` 记录，再按时间核对线路、图片、`closeCode`、`wasClean`、`delayMs`、`backgroundMs` 和 `attempt`。

日志能够提供后续定位证据，本身不能证明某次图片失败或断线的根因。原生分享、手机网络切换和特定厂商 WebView 行为仍须在手机实际复现中核对。

## 自动主题切换采证

自动主题的夜间范围按设备本地时间设为 20:00（含）至次日 08:00（不含），保留 10/45 lux 阈值、滞回、5 秒稳定候选和一分钟触发规则；诊断采集不推动主题切换。

网页用 `theme.ambient` 记录收到的光照，用 `theme.decision` 记录候选等待、取消或应用。实际切换前，`theme.sample` 回看最近 30 秒内最多 8 个观测点；没有近期点时仅附最后已知点。`theme.transition` 以 `before/after` 标记显示切换，并用本页面的 `themeSequence` 关联。切换后 2、5、10 秒再记录最新已知读数，`delayMs` 是计划间隔，`durationMs` 是实际间隔。这些延迟回调只记录，不推动主题切换；发生下一次切换或离开页面时取消旧回调。

`lux` 和 `previousLux` 保留小数。`sampleAgeMs` 表示距原生真实采样的时间；缓存回传不会把年龄清零。旧 APK 没有提供真实采样时间时省略该字段，不能按“刚收到回调”推定“刚采样”。延迟快照未收到新读数时会继续显示旧值及增长的年龄，不伪造新采样。`theme.sensor` 则在导出或上报前补充原生最新状态，可与网页收到的读数交叉核对。

完整采证需要同时更新 Android APK、网页资源和电脑接收端。现有旧报告缺少主题事件，不能据此认定误切原因；更新后按原路径“设置 → 诊断日志 → 发送到电脑”上报即可。
