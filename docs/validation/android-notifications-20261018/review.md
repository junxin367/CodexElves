# Android 会话事件通知独立审查

审查日期：2026-10-18。终态：PASS（源码静态审查）。本轮发现均已修正复核，无遗留必修项。

本报告以 `docs/android-notifications.md` 为冻结合同，只审查本次通知增量。已有的 v43 模型修复、`/E:/` 图片路径兼容和历史正文错误修复不计入本次发现。没有修改生产文件、运行 Gradle/网络测试、操作系统权限或发送真实通知。

## 已发现并修正

### N1：未投递事件被误记为已经提醒

- 初始触发：关闭“需要授权”或对应频道时收到仍未处理的授权，然后恢复该设置。
- 初始影响：`NotificationEventState` 无条件记录 `seen`，`NotificationPublisher` 恢复时把未曾展示的通知当作用户已划掉，后续完整 pending 快照也无法恢复提醒。
- 修正复核：`NotificationEventState.java:238` 现在仅在 `Sink.notify()` 接受事件后新增投递记录；禁止投递时保留已有记录。`NotificationPublisher.java:86` 对禁止投递返回 `false`，配置主动清除的待处理提醒另存恢复记录。首次被设置阻止的 pending 可以在后续快照投递。
- 状态：已关闭。配置切换与恢复的进一步复核见 N3、N4。

### N2：原生失败回复被网页解析器忽略

- 初始触发：原生返回 `settings-save-failed`、`activity-not-visible`、`permission-request-in-progress` 等明确失败。
- 初始影响：原生在完整状态对象上增加 `error`，网页只接受 `type/requestId/error` 三字段失败包，请求最终拖到 15 或 60 秒超时。
- 修正复核：`NotificationController.java:251` 已改用 `NotificationSettings.java:90` 的 `failure()`，与 `assets/remote-access/platform.js:119` 的三字段校验一致。
- 集成证据：`native/cross-bridge-failure.json` 报告以生产 `NotificationSettings.failure()` 生成的包驱动生产 `platform.js`，`settings-save-failed` 在约 1.16 毫秒内拒绝，重试可用，完成后监听器和超时均为 0。
- 状态：已关闭。

## 修正后的去重与恢复复核

### N3：已划掉的 pending 在配置切换或大量后续事件后重新提醒

- 原级别：P2。状态：已关闭。
- 复核位置：`NotificationEventState.java:176`、`:238`、`:244`；`NotificationPublisher.java:86`、`:269`、`:362`；`NotificationDismissReceiver.java:8`。
- 较短触发：pending A 已展示，用户划掉 A；关闭“需要授权”并触发 resync；再打开“需要授权”。
- 初始代码结果：关闭期间 `Sink.notify()` 返回 `false` 后删除已有 `seen`；再开启时 A 变成未展示记录，重新出现并可能响铃。
- 另一个初始触发：A 被划掉后处理 1024 条其他完成事件，再读取仍包含 A 的完整 pending 快照，`MAX_SEEN` 淘汰 A，尽管 A 仍在 `pending`。
- 修正复核：禁止投递不再删除已有 `seen`；有界淘汰跳过仍 pending 的身份，快照先完整恢复 pending 再处理增量。显式不可变的 delete PendingIntent 交给不可导出的 `NotificationDismissReceiver`，按当前身份及已知 pending 身份记录用户划掉；投递时优先遵守该记录，配置恢复不会复活它。请求结束和完整快照会清理相关记录。
- 安全边界：接收器不接收网页桥消息，只更改本功能的私有通知记录；校验当前 Cookie 身份、固定 action、固定 URI 结构与 ledger 中的既有通知 ID，不执行会话操作。

### N4：系统先撤下通知后的权限恢复分支

- 状态：代码分支已关闭，真实设备行为仍未实测；不把此前的设备行为推断作为独立已确认缺陷。
- 复核位置：`NotificationService.java:44`、`:135`；`NotificationPublisher.java:269`、`:322`。
- 原分支：A 已展示并进入 `seen`；系统关闭权限或频道时已经移除 A；随后只枚举 active 通知无法登记恢复原因，恢复时同一 pending 可能一直被跳过。
- 修正复核：全局通知权限被撤销时清理事件游标，恢复后以当前 pending 建立新基线，不回放旧完成事件；用户划掉记录保留。频道和频道组变化由系统广播触发，使用独立 pending ledger 记录恢复原因，恢复不依赖系统仍保留 active 通知；记录用户划掉的身份除外。完整 pending 快照与 dismiss 会清理无效记录。

## 已完成的静态核对

| 范围 | 结论与依据 |
| --- | --- |
| 普通浏览器 | 通知入口只在 `isAndroidContainer()` 为真时显示；设置请求、权限请求和订阅均先检查外壳，普通浏览器不会发送通知桥请求；通知点击 DOM 入口也只在 Android 注册。 |
| 原生桥 | `MainActivity.registerBridge()` 检查 Origin、主 frame、当前 WebView、连接代次和当前页面 URL；4096 字符限制、请求 ID 和设置五个布尔字段均受校验。网页不能指定通知正文、网络目标或操作。 |
| 权限与前台服务 | Manifest 声明三项所需权限、不可导出的 `remoteMessaging` 服务；可见 Activity 控制服务启动，服务再次校验权限与当前身份；后台没有启动权限弹窗的路径。 |
| 网络身份 | 仅使用已授权 HTTPS Origin 和设备 Cookie；两个端点均为原生常量，拒绝重定向；停止时关闭所有登记连接，使用独立网络线程和有上限的连接、读取与重试时间。 |
| 旧请求隔离 | 网络回调在共享身份锁内复核服务实例、连接代次、Cookie 摘要和授权状态；旧 401 不会清除已替换身份。Activity 收到授权失效后还会检查当前 Origin 下的摘要。 |
| 划掉接收器 | Manifest 中不可导出，delete PendingIntent 显式且不可变；接收时校验当前身份和已知通知 ID，只保存用户划掉状态，不启动后台连接或会话操作。 |
| 点击导航 | 显式、不可变的 PendingIntent 包含服务和设备身份摘要；打开时复核身份及合法会话 ID；网页 ready 和 authenticated 后仅派发会话 ID，走现有 `selectThread()` 保存草稿并读取目标，没有批准、回答或发送动作。 |
| 页面返回与迟到回复 | 二级页面恢复原标题、原设置内容和图标；Escape、原生后退与关闭均清理订阅并增加页面修订号；异步完成先检查当前页面。平台请求按 ID 清理超时和监听器。 |
| 后端元数据 | 原生请求 ID 保留类型，数字 0 有效；异步问答扫描已加载会话，状态回调独立同步；子代理过滤、实时完成/失败分类、pending 与 256 条事件窗口分开维护。Rust 读取端点经过既有设备鉴权，过滤 SSE 重建允许字段。 |

## 已有验证证据及限制

已读取 `backend/README.md`、`backend/native-checks.json`、`backend/rust-checks.txt`、`backend/final-checks.txt`：后端代理报告 19 项 native 隔离检查、6 项 Rust 路由/事件流检查通过，持续正文下通知心跳为 15.01 秒，普通 SSE / WebSocket 正文和鉴权回归通过。本审查未重新运行这些测试。

已读取 `native/validation.json` 和 `native/policy-validation.txt`：独立审查取证时原生代理报告 Java 17 / Android API 35 编译及 99 项隔离断言通过；主代理最终核对的报告已补充为 100 条断言，使用进程内伪造 HTTPS 连接，无真实网络。已逐一比对该报告中的 9 个 Java 源文件 SHA-256，与本轮最终审查的工作区文件一致，包括新增划掉接收器；主代理最终摘要复核也一致。该报告的资源符号为临时生成 `R.java`，不能替代最终 Gradle 资源构建与 lint。本审查没有另行运行或替代这些测试。

主代理随后完成 Gradle 资源及 Release Java 编译、lint 和 Chrome 原生桥模拟，均通过，结果另见根验证报告。真实系统通知展示、权限弹窗、后台存活和厂商省电行为没有在本审查中实测。
