# Android 会话事件通知

本功能仅面向 Android App。手机和桌面浏览器不显示通知入口，不申请浏览器通知权限，不保存通知偏好。现有远程网页、授权、问答、队列和消息执行行为不因通知改变。

## 页面与行为

- 设置页在“字体大小”下方增加“通知事件”入口。
- 点击入口在同一设置弹窗显示二级页面：左上标题为“通知事件”，右上原关闭按钮改为返回图标。返回时保留原设置内容和滚动位置。
- 二级页包含“接收通知”总开关，以及“需要授权”“需要回答”“执行完成”“执行失败”四类开关。
- 总开关首次默认关闭，四类事件默认开启。在通知页开启总开关时申请必要的系统通知权限。尚未获准或系统关闭通知时明确提示当前不能接收；不能把取消权限弹窗写成用户明确拒绝。已支持的通知页面始终保留系统通知设置入口。
- 偏好保存在 Android 原生私有设置中。磁盘提交成功前，页面和通知线程只读取上一次确认值；失败恢复原偏好、保留错误提示，不以在途推送覆盖为保存成功。更改事件选项不修改模型、主题、字体等其他设置。
- 设置与权限请求标志写盘不持有页面生命周期锁；提交前和发布确认值时校验页面及身份。页面替换或销毁不会等待该锁中的磁盘提交，失效写入沿原回滚路径恢复，不向旧页面推送成功。
- 应用级系统权限与各通知频道分别检查。已启用的事件分类若被系统关闭，通过原有状态提示列出，不把单频道关闭误报为全局权限被拒绝。
- Android 后退和弹窗 Escape 在二级页返回设置首页；设置首页仍关闭弹窗。二级页没有“恢复外观默认值”动作。
- 通知只提示事件类别和会话标题，不包含工具参数、命令、问题正文、模型回复、访问密钥或授权令牌。锁屏采用私有可见性和通用公开摘要。
- 点击事件通知打开相应会话，不直接批准、回答或发送消息。有效点击会记录具体 pending 提醒已被用户查看，避免系统 autoCancel 后重新提醒；过期授权或不同服务/设备身份的通知不能切换到旧服务或写入该记录。
- 前后台都可接收事件通知；后台连接由 Android 前台服务维护，并显示低打扰的连接状态通知。关闭总开关、退出登录或授权撤销时停止接收并清除本功能通知。系统强制停止 App 或网络不可用时不承诺持续送达。

## WebView 与原生设置协议

仅在受信任的当前服务 Origin、主 frame、当前 WebView 和连接代次内接受消息。网页不能通过桥接指定任意通知正文、网络地址或文件路径。

请求 `requestId` 为 1–80 个 ASCII 字母、数字、下划线或短横线。消息大小最多 4096 字符。类型与字段如下：

| type | 允许的字段 |
| --- | --- |
| `notification-settings-get` | `type`、`requestId` |
| `notification-settings-set` | `type`、`requestId`、`settings` |
| `notification-permission-request` | `type`、`requestId` |
| `notification-open-system-settings` | `type`、`requestId` |

`settings` 是只包含五个布尔值的完整对象：`enabled`、`approval`、`question`、`completed`、`failed`。未知字段、类型或非布尔值拒绝。

对应回复为：

```json
{
  "type": "notification-settings",
  "requestId": "request-id",
  "supported": true,
  "settings": {
    "enabled": false,
    "approval": true,
    "question": true,
    "completed": true,
    "failed": true
  },
  "permission": "prompt",
  "delivery": "disabled"
}
```

`permission` 为 `prompt`、`granted` 或 `denied`。`delivery` 为 `disabled`、`permission-required`、`connecting`、`connected`、`disconnected`、`unsupported` 或 `auth-required`。可附加最多 200 字符的固定用户提示 `message`。失败回复只包含 `type`、`requestId`、`error` 三个字段，`error` 是 1–128 个 ASCII 字母、数字、下划线、点或短横线组成的固定错误码，由网页显示中文提示；不暴露异常堆栈或凭据。

系统权限或服务状态变化可发送 `notification-settings-changed`，字段与上述状态一致但没有 `requestId`。异步回复不能修改已关闭、已返回或属于旧 WebView 的页面。

网页导出 `getAndroidNotificationSettings()`、`setAndroidNotificationSettings(settings)`、`requestAndroidNotificationPermission()`、`openAndroidNotificationSystemSettings()` 和 `subscribeAndroidNotificationSettings(listener)`。普通浏览器不发桥接请求；旧外壳超时或不支持时保留可读提示，不伪造已保存状态。请求按 ID 关联并清理监听器与超时。

原生后退通过可取消的 `codex-elves-native-back` DOM 事件交给当前页面；仅弹窗实际处理时 `preventDefault()`。原生检查返回值及 WebView/连接代次，未处理时沿用原后退行为。

通知点击在网页同时完成 `ready` 和 `authenticated` 后，通过 `window` 派发可取消的 `codex-elves-notification-open` 自定义事件，`detail` 仅含合法 `threadId`。Android 网页接管后 `preventDefault()`，经既有 `selectThread` 路径保存当前草稿并打开目标；初始化未就绪时暂存该目标。原生仅在返回 `defaultPrevented=true` 且身份/代次仍有效时清除待导航目标。普通浏览器不注册此入口。

## 事件协议

新增受现有设备鉴权保护的只读操作 `/api/read/notifications`。正常网页无需调用此操作。

```json
{
  "ok": true,
  "schemaVersion": 1,
  "notificationEpoch": "native-adapter-epoch",
  "notificationCursor": 12,
  "reset": false,
  "truncated": false,
  "pending": [],
  "events": []
}
```

首次请求正文为空对象；后续携带 `epoch` 和非负安全整数 `cursor`。首次或 epoch 变化时，`reset=true`，返回当前待处理授权/问答，建立游标，不回放历史完成通知。相同 epoch 下返回游标之后的事件。仅保留最近 256 条事件；游标落后于保留窗口时 `truncated=true`，返回可用新事件与完整待处理快照，不全量读取聊天历史。

事件对象：

```json
{
  "epoch": "native-adapter-epoch",
  "sequence": 12,
  "id": "native-adapter-epoch:12",
  "action": "notify",
  "notificationId": "native-adapter-epoch:12",
  "kind": "approval",
  "threadId": "thread-id",
  "title": "会话标题",
  "createdAtMs": 1792313000000
}
```

- `action` 为 `notify` 或 `dismiss`。`dismiss` 的 `notificationId` 指向已发提醒的身份，自身仍有新的顺序号和事件 ID。
- `kind` 为上述四类事件之一。请求完成或撤销时撤下相应待处理提醒。
- `threadId` 使用现有 1–120 字符会话 ID 约束；标题最多 160 字符，并去除控制字符。
- 授权/问答按请求身份分别跟踪，数字 ID `0` 有效，不因同一会话有多个请求而只保留一个。相同待处理请求在重复快照中保持同一个通知身份，结束后再出现是新事件。
- `turn/completed` 的 `completed` 状态产生执行完成通知，`failed` 产生执行失败通知；用户主动中断、取消和未知状态不伪装为成功。只有实时完成事件产生通知，读取历史不会触发。
- 不向主会话用户发送子代理会话的通知，避免子代理内部回合造成通知洪泛。
- 异步问答必须覆盖已加载主会话中的请求，不依赖手机此前是否打开过该会话。

规范事件经现有广播发送为 `remote/notification`，`params` 为上述对象。普通网页收到此方法直接忽略，避免将其当作消息条目合并。

Android 使用 `/api/events?notifications=true`。该 SSE 订阅只传递 `remote/notification`、心跳、重同步、连接/授权状态及会话删除事件；不重复下载正文流和附件。非通知 SSE 和原 WebSocket 行为保持。

SSE 先后配合快照：

1. 首次建立快照基线并处理当前待处理请求。
2. 打开过滤后的 SSE，再用基线游标读取一次增量，填补建连间隙。
3. 持续处理流事件，重连或重同步时用已有 epoch/游标补读。按通知身份与顺序号去重，重复流事件不重复响铃。

## Android 生命周期与安全边界

- 使用 `POST_NOTIFICATIONS`、`FOREGROUND_SERVICE` 和 `FOREGROUND_SERVICE_REMOTE_MESSAGING`；前台服务不可导出。由可见 Activity 在用户开启通知或恢复已启用设置时启动。
- 服务只访问已保存且获授权的 HTTPS Origin，沿用设备 Cookie；不保存或传递登录密钥，不向网页返回 Cookie。禁止跨 Origin 重定向。
- 原生发送、网络连接和偏好写入分别受当前授权身份与连接代次约束。退出登录、切服务、Cookie 身份变化或 401 后，旧连接及其迟到事件失效。
- 单独网络线程接收事件，不占登录、图片或网页资源更新线程。连接超时、读取心跳超时和断线退避有上限；停止时关闭在途连接，不创建多条重复连接。
- 使用四类事件频道和一个安静的连接状态频道。尊重系统通知权限和频道开关，禁止绕过系统设置。
- 通知使用显式且不可变的 `PendingIntent`。身份包含服务与设备会话的摘要，打开时再次校验；仅允许当前服务下的合法会话 ID。
- 待处理通知的用户划掉行为由显式、不可变的 DeleteIntent 交给不可导出的原生接收器记录；用户点击则在 Activity 校验当前身份、会话和已知 pending 账本后记录。两者沿用已有 `dismissed:<identity>` 私有持久键，与系统/配置撤下分别处理；仍 pending 的身份不会被已结束事件的去重窗口淘汰。恢复权限或事件开关时，未投递或系统撤下的 pending 可以恢复，包括 Android 8 未捕获到频道关闭广播的情形；用户已划掉或已查看的同一提醒保持静默。新的 pending 身份仍正常提醒，历史完成/失败事件不回放。
- 原生只接收事件元数据，不自动执行任何授权、问答、消息、目标、队列或文件操作。

## 验证和交付

需要验证：普通浏览器无入口/桥接请求；Android 一级/二级页与返回；偏好往返、拒权与迟到回复；同一聊天多个授权/问答、异步问答、完成/失败/取消、快照和流去重、重连补读、会话删除；原生权限和生命周期、身份隔离、点击导航；Android 资源/Java 编译及 lint；后端鉴权和原有事件流不回退。

本功能源码实现不等于已构建或安装正式包。系统通知展示、后台存活及手机厂商省电行为须区分原生编译/隔离验证与设备实测。

2026-10-18 实现与验证已完成：原生适配器版本 `44`，后端 native 19 项、Rust 6 项检查通过；Android 原生策略与模拟传输 100 条断言通过；完整生产网页的二级页面和桥接交互通过。Android Gradle 的 `compileReleaseJavaWithJavac`、`lintRelease` 均退出 0，覆盖资源合并和编译；lint 为 0 错误、3 条已有登录布局/资源提示。尚未运行正式 APK/R8 打包、签名发布、安装或设备验收。证据见 `docs/validation/android-notifications-20261018/`。
