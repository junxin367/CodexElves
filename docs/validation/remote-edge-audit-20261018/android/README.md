# Android 运行边界复核

2026-10-18。状态：**PARTIAL**。本轮发现的两项源码问题已修复，生产方法定向验证通过；没有连接设备、安装 APK、调用真实账号接口或构建 APK。主代理负责整体 Java/Gradle 验证。

## 发现与修复

| 项目 | 原行为与后果 | 修改与验证 |
| --- | --- | --- |
| renderer 退出后遗留失效页面 | 当前非试用资源的 `onRenderProcessGone()` 走通用启动失败处理，`rollbackTrial()` 返回 false 后只显示 Toast。旧 WebView、ready 状态、文件选择回调及当前页面绑定仍保留，不能恢复可用页面 | 主代理修改 `MainActivity.java`：立即失效当前页面代次、通知绑定、资源任务、导航与返回回调，取消选图；先移除再销毁失效 View，显示手动重试。保留授权和待打开通知；旧 View 的迟到退出回调只清理自身 |
| 密钥被写入 Activity 恢复状态 | `login_key` 有固定 View ID，未关闭状态保存。AOSP Android 15 的 `EditText.getFreezesText()` 返回 true，`TextView.onSaveInstanceState()` 把内容写入 SavedState，`View.dispatchSaveInstanceState()` 在有 ID 且未关闭保存时把状态写入容器；与不持久化明文密钥的约定冲突 | 本子任务仅在 `LoginView.java` 增加 `keyInput.setSaveEnabled(false)`。实际 SDK 35、JDK 17、生成 R 的定向编译通过；未改变域名保存、系统权限、版本或签名 |

这两项结论的范围分别是“系统已回调 renderer 退出”和“系统保存登录页面状态时输入框已有密钥”；不能据此断言此前用户手机的每次断线都由 renderer 退出造成，也没有读取任何真实密钥或设备恢复文件。

## 验证

| 证据 | 结果与边界 |
| --- | --- |
| `renderer-baseline.json` | 修改前生产 `onRenderProcessGone`、`handlePageStartupFailure` 原样方法体，10 项断言复现失效 View 与状态保留 |
| `renderer-fixed.json` | 修改后 30 项断言通过。包含当前/过期 View、finishing/destroyed、选图一次取消、待打开通知保留、先移除再销毁、ready 入队/已执行两种先后、未完成试用回退、已 ready 版本保留、完整 pending 保留、一次手动重试不自动循环 |
| `policy-boundaries.json` | 实际生产 `AppUpdatePolicy.copy`、`AppUpdateCancellation`、`ResourceStatePolicy` 共 16 项通过。完整读取、半包、超长、末次 EOF/进度回调取消、预先取消、连接归属、截止期限、试用恢复、取消/不完整下载保留既有候选 |
| `login-saved-state-baseline.json` | 固定 View ID、修复前未关闭保存及 AOSP 三段实际保存实现的来源、摘要与方法摘录；不包含真实密钥 |
| `verification.json` | 本轮源码摘要、SDK 定向编译、空白检查与清理结果 |

renderer 夹具直接抽取当前生产清理、选图及资源选择/markReady/rollback 方法体，保存方法摘要；View、通知、SharedPreferences、资源文件读取和 UI 调度使用确定性替身。手动重试的 UI/openSession 路由为夹具，资源恢复使用实际选择方法与策略。它不是 Android WebView 或实体设备运行验证。

## 其余边界

- 普通前后台切换没有原生 reload；原生资源成功更新只写完整 pending，不替换正在运行的页面。切源/授权失效会递增连接代次、取消资源任务及旧选图；未发现新的跨身份污染路径。
- 安装更新继续使用独立线程、私有缓存、大小/摘要/包身份/签名检查和当前安装来源授权核验。暂停只关闭弹窗，销毁取消操作；失效完成回调受操作归属保护。此次补测针对传输与策略，未模拟 Android 系统安装 UI。
- 失败试用版本会回退，但没有失败版本排除表；后续资源检查可以再次把同版完整资源放入 pending。现有 15 分钟自动检查节流、同代次进行中合并与显式重试仍在。缺少“失败版本应屏蔽多久”的产品合同，因此本轮不引入永久黑名单或退避新策略，也不把可重试本身当作已确认缺陷。
- `HapticFeedbackController` 的前台、主线程、附着、可见、启用和窗口焦点限制及 50/80ms 限流仍在；登录变更未增加触觉调用。
- 系统照片选择器迟到结果、真实 WebView 桥、设备锁屏/网络切换/进程回收、物理触感、来源授权与安装取消仍需设备验收。本轮没有调整这些权限与平台策略。

## 原因与排除

renderer 问题的机制是把“终止后不可再用的 View 清理”复用为“试用资源启动失败回退”，导致稳定资源分支没有履行清理责任；不是资源下载失败，也不是每次 onResume 主动重载。密钥问题来自有 ID 的输入框沿用框架默认状态保存，没有在该字段关闭；禁用 autofill 和密码遮罩不会关闭此路径。未把历史记忆中的候选直接判为当前缺陷。

修复后 renderer 不自动重建，用户点击重试才复用 `selectForPage()`：未完成试用恢复 previous；markReady 已完成则保留 active；已有完整 pending 按原合同在新页面启用。资源工作在同一单线程执行，旧 ready 工作未执行时会因取消而跳过；已经执行完的 markReady 会先于后续资源选择生效。无需额外异步 rollback。

临时验证源码与字节码仅在本任务专用 `temp/remote-edge-audit-20261018/android/`。自动审批先拒绝包含递归清理的组合命令，随后也拒绝删除本次明确创建的单个 `audit_edges.py`，两次均只返回 `blocked by policy`，没有执行；未改用其他工具绕过。清理状态为 **BLOCKED**，残留明确清单见 `remaining-temp-files.txt`。未改网页、Notification 类、构建入口、版本、签名、权限或历史临时目录，未提交或推送。
