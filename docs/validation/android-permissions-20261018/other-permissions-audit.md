# Android 非通知功能权限审计

日期：2026-10-18

结论：**PASS（源码与现有合并清单核对）**。未发现需补充的运行时权限或系统特殊访问；未修改生产代码。未安装或构建，也未在 API 26、35 设备上实际操作系统界面。

## SDK 范围

`apps/codex-elves-android/app/build.gradle` 配置 `minSdk 26`、`compileSdk 35`、`targetSdk 35`。图片选择、安装未知来源和前台服务的行为按此范围核对。

## 功能与权限映射

| 功能 | 所需授权 | 当前实现与拒绝/回读处理 | 结论 |
|---|---|---|---|
| 选择并上传图片 | 不需要相册/存储运行时权限。系统只把用户选中的 URI 授予应用；短时使用无需持久化 URI 授权。 | `MainActivity` 将 WebView 文件选择接到系统 Picker / `ACTION_OPEN_DOCUMENT`，通过 `onActivityResult` 读取单 URI 或 `ClipData`，取消时回调 `null`；替换选择请求时会取消旧请求。Android 35 或 Android 11 R Extensions 12+ 才启用有序多选 Picker，这是因为 `EXTRA_PICK_IMAGES_IN_ORDER` 从 API 35 / R Extensions 12 才提供；其余设备走 WebView intent 或 DocumentsUI 回退。远程页面使用 `input type=file accept=image/* multiple`，未设置相机 capture。 | 不应添加 `READ_MEDIA_IMAGES`、`READ_EXTERNAL_STORAGE` 或全盘存储权限。Android 官方说明 Photo Picker 只授予所选媒体访问权；该授权是临时的，当前上传场景没有持久 URI 使用。[Photo Picker](https://developer.android.com/training/data-storage/shared/photopicker) · [`EXTRA_PICK_IMAGES_IN_ORDER`](https://developer.android.com/reference/android/provider/MediaStore#EXTRA_PICK_IMAGES_IN_ORDER) |
| APK 下载与安装 | `REQUEST_INSTALL_PACKAGES` 声明 + 用户针对本应用开启“安装未知应用”特殊访问；不是危险权限运行时弹窗。 | Manifest 已声明。`AppUpdateController` 先调用 `canRequestPackageInstalls()`；未授权时打开带本应用包名的 `ACTION_MANAGE_UNKNOWN_APP_SOURCES` 设置页。返回时在 `onActivityResult` / `onResume` 路径重新读取授权；拒绝后显示仍需授权状态，不直接启动安装器。 | 流程完整；Android 官方 API 要求声明安装请求权限并提供按包名进入来源设置的入口，源码在返回后复核授权。[`canRequestPackageInstalls()`](https://developer.android.com/reference/android/content/pm/PackageManager#canRequestPackageInstalls()) · [`ACTION_MANAGE_UNKNOWN_APP_SOURCES`](https://developer.android.com/reference/android/provider/Settings#ACTION_MANAGE_UNKNOWN_APP_SOURCES) · [`REQUEST_INSTALL_PACKAGES`](https://developer.android.com/reference/android/Manifest.permission#REQUEST_INSTALL_PACKAGES) |
| 安装包 URI 交付 | 不需要公共存储权限；只向安装器授予该 APK 的临时只读 URI。 | APK 写入应用私有 `cache/updates`；FileProvider 仅暴露 `updates/` 子目录且 `exported=false`。安装 intent 使用 `FLAG_GRANT_READ_URI_PERMISSION` 和 `ClipData`，没有写授权。 | 与私有缓存路径匹配，授权范围为只读单 URI。[Android FileProvider 指南](https://developer.android.com/training/secure-file-sharing/setup-sharing) · [`FLAG_GRANT_READ_URI_PERMISSION`](https://developer.android.com/reference/android/content/Intent#FLAG_GRANT_READ_URI_PERMISSION) |
| 触觉反馈 | 无需 `VIBRATE` 权限，也没有运行时授权。 | `HapticFeedbackController` 调用 `View.performHapticFeedback(..., 0)`；不同 API 常量有版本回退。 | 不应添加 `VIBRATE`。[`View.performHapticFeedback`](https://developer.android.com/reference/android/view/View#performHapticFeedback(int,int)) |
| 网络访问 | `INTERNET` 与 `ACCESS_NETWORK_STATE` 均为普通安装时权限，不产生运行时弹窗。 | Manifest 已声明；Android 源码使用 `HttpsURLConnection` 访问更新、资源与服务端；远程页面还读取 `navigator.onLine` 并监听 `online` / `offline` 事件。没有证据证明 WebView 内部实现或依赖不需要 `ACCESS_NETWORK_STATE`，因此本审计保持现有声明，不判定其冗余。 | 网络权限声明齐全。Android 官方将两项权限的保护级别列为 `normal`。[`INTERNET`](https://developer.android.com/reference/android/Manifest.permission#INTERNET) · [`ACCESS_NETWORK_STATE`](https://developer.android.com/reference/android/Manifest.permission#ACCESS_NETWORK_STATE) |
| 前台服务声明 | 目标 API 34+ 的 `remoteMessaging` 类型需要通用 `FOREGROUND_SERVICE`、类型权限、Manifest 类型声明及匹配的 `startForeground` 类型；该类型本身没有运行时前置权限。 | Manifest 声明 `FOREGROUND_SERVICE`、`FOREGROUND_SERVICE_REMOTE_MESSAGING`，服务为 `exported=false` 且类型为 `remoteMessaging`。API 34+ 调用带匹配类型的 `startForeground`；较低 API 使用旧签名。源码从可见 Activity 的通知控制路径启动。目标 API 31+ 的后台启动限制仍适用，但检视到的启动路径受 `visible` 条件约束。 | 声明和调用一致。Android 官方将 `remoteMessaging` 的运行时前置权限列为无；目标 31+ 时从后台启动仍受限。该服务是否属于官方所定义的跨设备文本消息连续性用途，需要业务方结合产品功能确认。通知运行时授权本身不属于本次审计范围。[Foreground service types](https://developer.android.com/develop/background-work/services/foreground-services/types#remote-messaging) · [Background start restrictions](https://developer.android.com/develop/background-work/services/foreground-services/restrictions-bg-start) |

## 不需要新增的权限

当前 Manifest 未声明 `CAMERA`、`RECORD_AUDIO`、`READ_EXTERNAL_STORAGE`、`WRITE_EXTERNAL_STORAGE`、`READ_MEDIA_*`、`MANAGE_EXTERNAL_STORAGE` 或 `VIBRATE`。对 Android Java、资源和远程页面的定向检索未发现相机/麦克风采集、`getUserMedia`、Camera API、MediaRecorder 或震动器 API；图片入口仅为用户主动选择文件。无需因图片选择、触觉反馈或现有网络调用增加这些权限。

## 生命周期与未验证边界

图片 URI 选择结果由当前 Activity 的 `ValueCallback` 接收；Picker 外部 Activity 返回时通过 `onActivityResult` 交付。设备回收主进程后，WebView 与该内存回调无法恢复，因此进行中的选择不会继续上传；本次未在低内存/进程重建条件下实机验证。APK 来源设置返回后的重新读取已在源码中存在，但未在设备上实际切换设置并验证安装器交互。

仓库已有 `docs/validation/android-permissions-20261018/merged-release-manifest.xml`，记录的合并清单权限与当前源码声明一致；`existing-apk-permissions.txt` 对应较早 APK，只有 3 项权限，不能代表当前源码生成的 APK。最终仍未知的是用户手机上实际安装的 APK 权限集合，以及不同厂商系统设置页行为。
