# Android 原生通知验证记录

日期：2026-10-18。结果：源码编译与隔离策略验证 **PASS**；Android 资源编译、Gradle/lint 和设备实测由主代理统一记录，本记录不代表 APK 已生成或安装。

## 实现范围

仅新增/修改 `apps/codex-elves-android/app/src/main/` 下的 Java、Manifest 和通知资源，以及本验证目录。保留已有更新器、登录布局、图片选择、资源更新和下载逻辑；没有改网页、Rust、构建脚本、签名或版本，没有提交或推送。

| 原生类 | 行为 |
| --- | --- |
| `NotificationSettings` | 五个布尔值私有设置；总开关默认关闭，四类默认开启；严格请求 ID、字段与布尔类型；失败只返回 `type/requestId/error` |
| `NotificationIdentity` | 当前规范 HTTPS Origin 和设备 Cookie 的 SHA-256 身份；不保存登录密钥，不向网页返回 Cookie |
| `NotificationEventState` | 首次快照、SSE 增量、重同步、截断、去重、待处理请求、撤下、会话删除及游标日志 |
| `NotificationStreamClient` | 单独网络线程；只访问固定通知快照和过滤 SSE；禁止重定向；连接 15 秒、读取 45 秒超时；重连延迟 1–30 秒 |
| `NotificationPublisher` | 四事件频道和低打扰连接频道；私有锁屏内容、通用公开摘要；通知正文只含类别和清洗后的会话标题 |
| `NotificationService` | 不导出的 `remoteMessaging` 前台服务；只由可见 Activity 启动；不使用粘性/开机重启；关闭、401、身份失效后关闭在途连接并清理通知 |
| `NotificationController` | 原生权限请求、系统设置返回回读、异步偏好写入；回复受页面代次和原授权身份约束 |
| `NotificationDismissReceiver` | 不导出；仅处理显式不可变的通知 DeleteIntent，记录用户主动划掉，不能导航或执行会话动作 |

`MainActivity` 保留既有 Origin、主 frame、WebView 和连接代次检查，再分流四种通知消息。通知点击也使用显式不可变 PendingIntent；只接受当前原生身份和合法 threadId，等待 `ready`、`authenticated` 后派发 `window` 上可取消的 `codex-elves-notification-open`，只有 `e.defaultPrevented === true` 才清除待导航目标。前台不会重新加载 URL。

原生后退先派发可取消的 `codex-elves-native-back`；页面实际处理后才抑制原后退。API 33 及以上通过 `OnBackInvokedCallback` 进入同一逻辑。

## 去重与恢复修复

- 未展示的新事件不会写成已展示；暂停投递不会抹掉原本已展示的身份。
- 仍待处理的请求身份不会被完成事件的 1024 条保留窗口挤出；恢复日志也保留该身份。
- 原生另存待处理投递账本及用户主动划掉身份。因配置、权限或系统频道变化撤下的提醒可在恢复后再次投递；用户主动划掉的提醒保持静默，直到后台请求结束/撤销或会话删除。
- 判断频道失效依据已知投递账本，不依赖系统当前仍能枚举到通知。API 28 及以上监听应用、频道和频道组变更广播；心跳及 Activity 回前台也重新读取设置。低版本使用回前台和心跳读取。
- 权限失效清除通知快照游标，恢复时重新请求空正文基线，只恢复当前待处理请求，不回放历史完成/失败。用户主动划掉的身份独立保留。

## 已执行验证

1. JDK 17.0.20.1 `javac` 编译全部 Android Java 源码，使用 API 35、真实 AndroidX WebKit 1.17.1 和 Core 1.1.0，退出码 0。为避免与主代理 Gradle 构建争锁，资源 `R.java` 使用临时符号表；这不是 Android 资源编译证明。
2. **100 条隔离断言通过**，执行实际 `NotificationIdentity`、`NotificationSettings`、`NotificationEventState` 和 `NotificationStreamClient`；系统通知展示由 Publisher 替身表达，不调用设备 API。
3. 网络用进程内假 HTTPS URLConnection，无套接字或真实服务连接。覆盖快照后 SSE 建连间隙补读、流重复、重同步、会话删除、302 不跟随、HTTP 401 停止、404 旧服务不支持、关闭连接和退避上限。
4. 策略覆盖同会话多个待处理身份、快照重复、notify/dismiss、完成/失败、序列缺口、截断、epoch 切换、日志恢复、160 个 emoji、控制字符清洗、无效 ID/非布尔值/小数/字符串数字/非安全整数。
5. 精确覆盖“用户划掉 A → 关闭 approval 并 resync → 重新开启”保持静默；“用户划掉 A → 1149 条 completed → 快照和持久化恢复”不重响；“系统先移除 active → 频道恢复”可重新投递；权限重新基线恢复 pending 且不回放完成/失败；用户划掉也能跨权限重新基线保持静默。
6. 原生实际 `NotificationSettings.failure` 生成 `failure-response.json`，交给当前生产 `assets/remote-access/platform.js` 的隔离 VM。立即拒绝原请求、清理全部监听器和超时器，并成功执行下一次请求。结果见 `cross-bridge-failure.json`。
7. 三个新增/修改 XML 均能解析；静态检查服务 `exported=false`、`foregroundServiceType=remoteMessaging` 和删除回调接收器 `exported=false`。原生目录 `git diff --check` 通过。

编译输出见 `javac.txt`，策略输出见 `policy-validation.txt`，源码/资源摘要见 `validation.json`。首次隔离编译曾选到测试用 Core Jar、以及与 Android 不同的 JSON 异常定义，调整验证 classpath 后通过；未修改产品依赖或构建脚本。

## 未执行的边界

没有 ADB、模拟器或真机实测。系统权限弹窗与拒绝、系统设置返回、通知实际展示/声音、DeleteIntent 实际回调、频道广播实际送达、后台存活、厂商省电行为、冷启动点击导航和实体后退手势，仅有源码检查和上述隔离策略证据。

没有下载大型工具链，没有运行 Gradle、生成/安装 APK 或启动真实通知连接。本次由主代理统一执行 Android 资源、Release Java 和 lint 整体验证；R8、正式 APK 打包/签名、安装及真机验证均保留为未执行，不新增打包任务。

## 临时文件

本次临时 Java、PowerShell、JavaScript、资源符号和 class 文件位于独立的 `temp/android-notifications-native-20261018/`，完成验证后逐文件清理；结果记录在 `cleanup.json`。保留本目录的文本/JSON 验证证据。
