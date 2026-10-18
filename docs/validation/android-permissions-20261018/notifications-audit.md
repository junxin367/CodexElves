# Android 通知授权只读审计

审计日期：2026-10-18。原始审计状态：**ISSUES**。本轮随后按主代理授权完成 N2/N3 的原生最小修复和频道提示，**47 条定向断言通过**；修复细节与验证边界见 `native-fixes/README.md`。N1 及前端由主代理负责，完整 Android 集成/设备验收不属于本子任务结论。

下文保留针对修复前工作区源码的原始审计和当时行号。已确认两条权限/保存错误路径，以及一条 API 26–27 条件可达的通知恢复问题；结论来自实际调用链和 Android 官方文档、AOSP 源码。原审计为只读；随后仅按明确授权修改四个原生文件并执行隔离定向验证。没有运行系统权限弹窗、ADB、模拟器、设备操作、Gradle、安装或服务重启。本文不能作为设备验收通过记录。

## 范围与实际调用链

- `apps/codex-elves-android/app/build.gradle:46–47`：`minSdk=26`、`targetSdk=35`。
- `apps/codex-elves-android/app/src/main/AndroidManifest.xml:6–8,21–24`：声明 `POST_NOTIFICATIONS`、`FOREGROUND_SERVICE`、`FOREGROUND_SERVICE_REMOTE_MESSAGING`，通知服务不可导出且类型为 `remoteMessaging`。
- `assets/remote-access/app.js:558–586`：开启总开关时，先保存完整偏好，再读取确认，然后请求系统通知权限。四类事件开关不会单独弹权限框。
- `assets/remote-access/platform.js:184–212,328–342`：通过当前 Android 桥发送请求，按 ID 关联结果；设置读取、保存和系统授权分别有超时。
- `MainActivity.java:567–633`：检查当前 Origin、主 frame、WebView、连接代次，再进入 `NotificationController.handle()`。
- `NotificationController.java:177–220`：检查可见 Activity 和当前授权身份，在后台写入“曾请求权限”，之后由主线程调用 `requestPermissions()`。
- `MainActivity.java:1181–1185` 只转发请求码；Controller 回调重新查询当前实际授权，不直接相信旧的授权结果。
- `NotificationPublisher.java:78–96,485–494`：实际投递同时检查系统运行时授权、应用通知总开关、原生事件偏好和该通知频道。
- `MainActivity.java:836–861` / `NotificationController.java:137–145,223–238`：回前台重新读取实际系统权限、恢复或停止通知服务，并向当前通知页推送状态。

`remoteMessaging` 的两个前台服务权限是 Manifest 声明项，官方列出的该类型运行时前置权限为无；不应另外要求用户授权一个不存在的 FGS 运行时弹窗。系统本身允许未获通知权限的 App 启动 FGS，但本项目主动在通知未获准时停止通知连接，这是当前产品策略，不是缺失权限声明。[A1][A3]

## 权限情景表

下表均为代码行为核对；“可继续”不代表已经在设备上执行。

| 情景 | 当前代码真实行为 | 能否继续启用 / 结论 |
| --- | --- | --- |
| API 26–32，应用通知允许，首次开启 | 不申请 `POST_NOTIFICATIONS`；保存总开关后由可见 Activity 启动前台服务 | 可以；符合该版本没有通知运行时弹窗的机制 |
| API 26–32，在系统设置关闭应用通知 | `areNotificationsEnabled()` 返回不可投递；显示 `denied`，服务停止；权限请求方法不会弹框 | 可以从页面的系统通知设置入口恢复；不能靠旧版本运行时弹窗恢复 |
| API 33+，全新安装且尚未开启总开关 | 默认偏好关闭，实际权限未授予，返回 `prompt` / `disabled` | 不主动弹权限框；开启总开关后申请 |
| API 33+，首次开启后选择允许 | 实际权限查询变为允许；回调或 `onResume()` 在 Activity 可见时启动服务 | 可以；没有发现“允许了仍被本地历史标志拦截”的分支 |
| API 33+，首次选择不允许 | 用户开启意图保留，实际投递停止；页面显示未获权限并提供系统设置入口 | 可以关闭再开启总开关触发再次请求；也可从系统设置允许 |
| API 33+，划走弹窗，没有选择按钮 | 系统权限保持原状，但应用预先写入的 `permission_requested=true` 仍在；页面显示“已拒绝” | **N1：提示不准确**；不会越权投递，也没有把重新请求路径封死 |
| API 33+，多次明确拒绝 | 应用仍可调用请求 API，但系统可能不再显示弹窗；实际查询仍返回未获准 | 系统设置入口仍可达；不能承诺无限次弹框。[A4] |
| 授权弹窗停留超过网页的 60 秒请求超时 | 网页请求可能先超时；原生保留请求并等待系统结束，后续状态推送仍可更新当前页面 | 不能把此超时当作已拒绝或 App 永久不支持；没有执行设备时序验证 |
| 从系统通知设置允许后返回 App，总开关已开 | `onResume()` 重新查权限、恢复服务并推送当前状态 | 可以；不依赖网页自己收到 `visibilitychange` |
| 从系统设置允许后返回，总开关仍关闭 | 仍保持关闭，服务停止 | 需要再开启总开关；不会未经用户意图自行接收 |
| 系统撤回应用通知权限 | 回前台立即检查；运行服务还会在系统广播或网络活动检查时发现，关闭连接、清理通知及旧游标 | 权限恢复且 App 可见后可重建当前 pending 基线；设备实际回调和厂商行为未测 |
| API 28+，单个事件频道关闭 | 对应事件被 `channelEnabled()` 拦截；频道变更广播登记撤下原因并请求重同步，其他频道保持运行 | 系统设置重新打开后可恢复；未做广播实际送达验证 |
| API 26–27，单频道关闭且至少一次心跳观察到关闭 | 心跳 `clearDisabled()` 可根据投递账本记录撤下原因；回前台请求快照 | 当前代码具备恢复依据；后台单纯重新开启不会主动请求快照，最迟需回前台或重连/重同步 |
| API 26–27，频道在两次观察之间关闭再开启 | 系统已移除通知，App 没有观察到关闭；回前台时频道已开，旧提醒可能落入“已见且不再 active”的静默分支 | **N3：条件可达恢复问题**；重复快照仍不重发，关闭再开启应用总开关建立新基线才可恢复 |
| 只关闭单个频道后回通知页 | 全局权限仍可为 `granted`，连接仍可为 `connected`；分类偏好显示用户保存值 | 页面不展示该频道受系统阻止，也会隐藏系统设置按钮；见 U1，不据此判定越权 |
| 保存总开关失败，`commit()` 返回 false | Android 内存可能已变成新值；当前代码仍发送 changed 状态，通知线程也直接读取同一内存值 | **N2：页面和服务可偏离最后确认偏好** |

## N1：未作选择却显示“系统通知权限已拒绝”

级别：P2。状态：**已确认源码条件可达，未设备实测**。

触发条件：API 33+，首次授权未授予；用户开启总开关后划走权限弹窗，或“曾请求”写入成功后 Activity 不再可见导致实际请求没有执行。

因果链：

1. `NotificationController.java:198` 在系统弹窗前调用 `NotificationSettings.markPermissionRequested()`，该方法在 `NotificationSettings.java:115–116` 写入 `true`。
2. 系统调用位于 `NotificationController.java:211–213`；保存完成到主线程回调之间还可能命中 `204–207` 的 Activity 不可见分支，根本不显示弹窗。
3. 官方通知权限文档明确规定，划走弹窗不会改变权限状态。[A1]
4. 后续 `NotificationController.permission():297–307` 只在“未获准且从未请求”时返回 `prompt`；任何已记为请求的未获准状态都返回 `denied`。
5. `app.js:505,517–518` 把 `denied` 渲染为“系统权限已拒绝”“系统通知权限已拒绝”，把“尚未授予”和“用户明确拒绝”混成同一种事实。

最小修复建议：保持当前桥接枚举与系统查询，把 `denied` 的面向用户文案改为“尚未允许”或“未开启”。本轮无需引入权限状态机。只有产品确实需要区分取消与明确拒绝时，才有必要转发 `permissions/grantResults`；官方指出取消请求的结果数组为空。[A4]

已排除“拒绝后再也不能申请”的判断：`app.js:573–575` 在每次把总开关从关闭改为开启时都申请权限，`NotificationController.requestPermission()` 不以 `wasPermissionRequested` 阻止重试。系统永久拒绝后，弹框是否出现由系统决定；系统设置恢复路径仍在。

建议复核情景：全新安装 → 开启总开关 → 划走权限框 → 回通知页；应显示未获准且不接收通知，不能声称用户已经明确拒绝。该序列本审计没有在设备上执行。

## N2：保存失败仍发布新内存值，且最终回滚不足以隔离通知线程

级别：P2。状态：**已确认源码与 Android 实现条件可达，未注入真实写盘故障**。

适用约束：`docs/android-notifications.md:11` 要求保存成功后更新页面；`app.js:531` 自身承诺保存失败时保留上次读取的设置。用户保存失败的新值不应通过 changed 回调伪装成确认值。

可复核链：

1. `NotificationSettings.save():103–105` 直接把新 JSON 交给 SharedPreferences `commit()`，读方法 `95–100` 又直接读取同一 SharedPreferences。
2. Android API 文档只保证 `commit()` 返回 true 表示已写入持久存储。AOSP Android 13 的 `SharedPreferencesImpl.commit()` 先执行 `commitToMemory()`，后执行磁盘写入；写盘失败返回 false 时，没有把 `mMap` 自动恢复。`getString()` 读取的正是此 `mMap`。[A5][A6]
3. `NotificationController.java:102–106` 虽然仅在成功时调用 `refreshDelivery()`，但失败后仍无条件 `emitChanged()`；`status():277–286` 再次读取已变成新值的内存。
4. 前端 `app.js:600–604` 接纳 changed；它可能在操作期间排队并于 `536–542` 最终重绘，也可能在失败回调之后直接重绘。两种交付时序都可能把失败的新设置显示为当前值。
5. 服务还有独立读取者：`NotificationService.java:251`、`NotificationPublisher.java:93`。例如原通知已开启，用户尝试关闭，`commitToMemory()` 暂时暴露 `enabled=false`；网络线程恰在此时处理一帧，`NotificationStreamClient.java:282–283` 因 `callback.active()==false` 退出，`117–119` 关闭连接并排队 `stopped()`。即使稍后把内存恢复为 true，该退出已经发生。

“保存失败只发失败回复、不发 changed；失败后回写旧值”的方案可以消除主要页面错误，但仍有以下遗漏：

- 回滚前，网络线程可能已经读到失败的新值并停止。
- 回滚之后补一次 `refreshDelivery()` 也不必然消除此时序：`NotificationService.startFromVisibleActivity():117–122` 可能仍看到旧 `running.client` 非空，只请求重同步；随后已经排队的 `stopped()` 再次停止服务。
- 单个 Controller 的 `settingsWriter` 只能串行写请求，不能阻止服务/投递线程读取 SharedPreferences。已有 `pageLock` 也没有被这些读者使用。

最小语义修复建议：

- 保留现有 SharedPreferences 和单写线程；不换存储框架，不改桥接协议。
- 保存进行中，让 `NotificationSettings.read()` 返回上一次确认的设置；仅在提交成功后向读者发布新设置。
- 提交失败后恢复对应旧键的进程内值，保证直接读取 SharedPreferences 的后续路径也不带入失败值。恢复缺失键时应使用 `remove`，而不是把首次未配置状态伪装成一个已保存值。
- `markPermissionRequested()` 失败也应恢复对应旧标志；不能因记录失败留下一次没有发生的请求历史。
- 保存失败只发失败回复；前端清理本次操作累积的过时状态，保留当前确认值和明确错误。
- 若采用同步，应仅对进程内快照做短同步，避免把新增磁盘等待放到主线程；现有异步写盘仍在 writer 中执行。

定向验证建议：以符合 Android 语义的 SharedPreferences 替身，在 `commitToMemory` 和磁盘失败之间设置屏障。让实际 `NotificationSettings.read()` 与服务活动判断在屏障中执行，验证它们仍看到旧值；失败回调后应保持旧页面值、旧服务连接。另测成功提交时仅在确认后发布新值。测试必须明确是受控存储故障验证，不能命名为系统授权实测。

## N3：API 26–27 快速关闭再开启频道可能丢失当前 pending 提醒

级别：P2。状态：**官方系统源码与当前源码构成条件可达证明，未真机复现**。

触发条件同时成立：

1. API 26 或 27；App 通知总开关和对应原生事件偏好均已开启。
2. 某个授权/问答提醒 A 已成功投递，后台请求仍 pending；用户没有划掉或点击 A。
3. 用户在系统设置关闭对应频道，等系统移除 A 后，在 App 下一次检查之前重新开启频道。
4. App 在整个关闭区间都没有执行到 `clearDisabled()`，之后再回到 App 触发重同步。

系统依据：[A7][A8] 中 `updateNotificationChannelInt()` 在频道 importance 为 NONE 时取消该频道通知；`cancelAllNotificationsInt()` 使用 `sendDelete=false`，不会把系统撤下伪装成用户 DeleteIntent。

当前代码链：

1. 成功投递 A 后，`NotificationEventState.java:238–240` 将 A 记入 `seen`；Publisher 也在 `98–103` 保存 pending 投递账本。
2. `NotificationService.onCreate():159–170` 只在 API 28+ 注册频道变更广播。API 26–27 只有心跳/回前台检查当前值。
3. 两次检查之间已完成关闭再开启时，`NotificationPublisher.clearDisabledLocked():290–293` 看到频道已经允许，不登记 `suppressed(A)`。
4. 回前台 `NotificationController.java:235–237` 进入服务恢复，已有连接的 `NotificationService.java:117–122` 请求重同步。
5. 快照仍包含 A，`NotificationEventState.java:195–196,238–239` 将 `previouslySeen=true` 交给 Publisher。
6. `NotificationPublisher.java:108–110` 同时命中“已见、系统没有 active 通知、不在 suppressed”，直接返回 true；不会走到 `147` 的 `manager.notify()`。

结果：系统频道已经恢复、请求仍 pending，但该提醒没有恢复；相同身份与游标下的重复快照仍命中相同分支。关闭再开启应用总开关会清理旧游标并重建基线，属于现有可用的恢复办法。

不能直接删除上述静默分支：

- Publisher 在 `141` 设置 `setAutoCancel(true)`。
- [A8] 中 `onNotificationClick()` 用 `sendDelete=false` 处理 `FLAG_AUTO_CANCEL`，而用户划掉路径使用 true。`cancelNotificationLocked()` 仅在 true 时发送 DeleteIntent。
- 当前 `MainActivity.handleNotificationIntent():1116–1124` 没有记录“用户已点击 A”；原来的 missing-active 分支还承担点击后不重新弹同一 pending 提醒的作用。
- 因此仅对 pending 放开 missing-active，会让用户点击进入会话后仍未处理的提醒在重同步时重现。

最小范围方案：如果修 N3，先在有效、当前身份的通知点击入口记录具体 `notificationId` 已被用户处理；复用既有用户静默记录，校验身份和投递账本。然后才可让 pending 的恢复仅依据明确的划掉/点击记录，而非仅凭“不在 active”推断用户行为。历史完成/失败提醒的去重逻辑继续保留。无需新增系统权限、桥接消息或频道状态协议。

此方案至少需成对验证：用户划掉后不重响、用户点击后不重响、API 26/27 系统关闭再开启且从未被轮询到关闭时能恢复。真实频道移除和点击回调仍需要设备验收。原审计阶段没有修改这部分实现；随后授权修复与上述成对定向验证见 `native-fixes/README.md`。

## U1：单频道关闭后的页面可解释性不足

状态：**已确认页面行为；易用性建议，未列为本轮必须修复的授权缺陷**。

`permissionGranted()` 检查应用级授权，不聚合四个频道；`status()` 也不返回频道状态。因而单个频道关闭时，“系统权限已允许 / 连接已建立”仍可是真实的应用级权限与连接状态，但不能由此推导每一类事件都可投递。

`app.js:521` 只在全局拒权或 `permission-required` 时显示系统设置按钮；所以仅关闭一个频道后，本页没有可达设置按钮，用户仍可从 Android 系统设置恢复。原生 `notify()` 正确尊重该频道，未发现越权发送。

现有需求没有明确规定必须展示每个系统频道的状态，因此不把该缺口扩大为“通知实现失效”。如果产品决定在本轮改善，可使用既有可选 `message` 提示受系统频道限制，并保留现有系统设置入口；无需增加协议字段或新权限。

## 已核对通过的边界与未验证项

- Manifest 及 `remoteMessaging` 类型的权限声明完整，API 分支与 `minSdk=26` 一致。
- 实际投递再次读取系统权限与频道；没有发现“偏好已开启就绕过 Android 权限”的路径。
- Activity 回前台、系统授权回调会查询当前实际权限；只传请求码本身不会造成“未授权却当成已授权”，错误在未授权状态的解释文案。
- 请求结果、推送、旧 WebView、退出登录与身份切换有页面代次/身份约束；通知子页还有自己的 revision 与 connected/open 检查。没有因为已关闭子页而重新打开它的回调路径。
- 本次没有发现必须另申请电池优化豁免、通知监听访问权、精确闹钟权限才能执行当前通知功能的依据；不将这些列为必修项。
- 未验证：手机安装包与当前源码是否一致、权限弹窗的实际呈现/取消回调、Android 厂商设置页面、广播实际送达、真实通知声音与横幅、服务后台存活、系统杀进程与设备上点击时序。
- 没有重跑既有 100 条通知事件/模拟传输断言，也没有把那些结果作为系统授权证据。
- 原只读审计阶段仅新增本报告。后续授权修复创建的两份临时验证源码已逐文件删除，没有写入磁盘 `.class` 或测试二进制；清理记录见 `native-fixes/README.md`。

## 官方依据

网页读取使用 fetch；本会话 `web.run` 返回空结果，没有可引用的搜索结果 ID。下列为实际读取的官方 URL，AOSP 文件直接在内存中解码并检索，没有下载到工作区。

- [A1] Android Developers，Notification runtime permission；读取于 2026-10-18，页面标注更新于 2026-10-16。`https://developer.android.com/develop/ui/views/notifications/notification-permission`
- [A2] Android Developers，Create and manage notification channels；读取于 2026-10-18，页面标注更新于 2026-10-16。`https://developer.android.com/develop/ui/views/notifications/channels`
- [A3] Android Developers，Foreground service types / Remote messaging；读取于 2026-10-18。`https://developer.android.com/develop/background-work/services/fgs/service-types#remote-messaging`
- [A4] Android Developers，Request runtime permissions / Manage the permission request code yourself / Handle permission denial；读取于 2026-10-18。`https://developer.android.com/training/permissions/requesting`
- [A5] Android Developers，SharedPreferences.Editor.commit；读取于 2026-10-18。`https://developer.android.com/reference/android/content/SharedPreferences.Editor#commit()`
- [A6] AOSP `android-13.0.0_r1`，`SharedPreferencesImpl.java` 的 `commitToMemory()`、`commit()`、`getString()` 和磁盘失败分支。`https://android.googlesource.com/platform/frameworks/base/+/android-13.0.0_r1/core/java/android/app/SharedPreferencesImpl.java`
- [A7] AOSP `android-8.0.0_r1`，`NotificationManagerService.java` 的 `updateNotificationChannelInt()`、`cancelAllNotificationsInt()`。`https://android.googlesource.com/platform/frameworks/base/+/android-8.0.0_r1/services/core/java/com/android/server/notification/NotificationManagerService.java`
- [A8] AOSP `android-8.1.0_r1`，`NotificationManagerService.java` 的 `updateNotificationChannelInt()`、`cancelAllNotificationsInt()`、`onNotificationClick()`、`onNotificationClear()`、`cancelNotificationLocked()`。`https://android.googlesource.com/platform/frameworks/base/+/android-8.1.0_r1/services/core/java/com/android/server/notification/NotificationManagerService.java`
