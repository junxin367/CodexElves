# Android 版本与应用更新

Android 使用独立的 `apps/codex-elves-android/version.properties`。当前为 `versionName=0.1.1`、`versionCode=25`；桌面管理器版本来自根 `Cargo.toml`。普通编译不递增 Android 版本。准备正式 Android 发布时，按发布规则将 `versionName` 修订号加 1，并使 `versionCode` 大于当前值和已公开 Android 版本中的最大值。

## 设置页与原生桥

Android 设置弹窗显示 APK 内的版本号。版本值取自当前已安装 APK 的 `BuildConfig`，网页资源版本和桌面版本都不能替代它。普通浏览器和 PWA 不显示 Android 版本。

网页通过当前受信任来源的 `CodexElvesAndroid` 桥发送：

```json
{"type":"app-info"}
```

Release APK 的原生回复包含以下字段：

```json
{
  "type": "app-info",
  "versionName": "0.1.1",
  "versionCode": 25,
  "updateSupported": true,
  "requestRoutingVersion": 2,
  "diagnosticsVersion": 1,
  "browserVersion": 1
}
```

Debug APK 的 `updateSupported` 为 `false`。旧 APK 没有回复时，网页不猜测版本。手动检查通过 `{"type":"check-app-update"}` 触发；网页不能指定下载地址、文件路径、包名或签名。

## Android Release 与更新清单

应用更新只读取 `junxin367/CodexElves` 中已公开、非草稿且非预发布的 `android-v<versionName>` Release。它不使用桌面 Release 的 `latest.json`，也不会更改桌面 GitHub Latest 标记。

每个 Android Release 必须包含：

- `CodexElves-<versionName>-android.apk`
- `android-update.json`

清单字段如下。APK 大小和 SHA-256 必须从最终签名 APK 计算；以下当前版本值只用于说明字段，示例摘要不可用于发布。

```json
{
  "schemaVersion": 1,
  "applicationId": "com.junxin367.codexelves",
  "versionName": "0.1.1",
  "versionCode": 25,
  "minSdk": 26,
  "apk": {
    "name": "CodexElves-0.1.1-android.apk",
    "size": 1024,
    "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
  }
}
```

安装包下载地址由清单所在的 Android Release 附件确定。客户端核对仓库、标签和文件名，并拒绝草稿、预发布、重复附件、不受信任的主机和跨仓库跳转。

## 检查、下载与安装

Activity 每次返回前台后会排入一次自动检查；自动检查至少间隔 6 小时。检查在独立后台线程执行，不阻塞登录、网页资源更新或图片读取。同一时间只运行一个检查或下载。用户点击设置页版本号可立即手动检查。

发现更高版本后，App 提示用户选择下载。下载后检查清单与附件大小、SHA-256、APK 包名、版本名、递增的 `versionCode`、`minSdk` 和当前 APK 的签名证书，再交给 Android 系统安装器。更新请求使用 HTTPS，不读取或发送远程会话 Cookie、访问密钥。

APK 下载到应用私有缓存，只通过专用 FileProvider 向系统安装器提供已校验文件。需要时 App 引导用户打开“允许安装未知应用”设置，返回后重新检查授权，再显示系统安装确认。App 不申请整个相册或公共存储权限，也不静默安装。

网络失败、用户取消、清单或 APK 校验失败、安装来源授权未获准时，当前版本仍可使用，用户可重试。Activity 退出后不会显示属于旧页面的更新弹窗。

## 网页资源更新

APK 更新和远程网页资源更新是两个独立流程。APK 从 Android GitHub Release 获取；网页资源从用户配置的服务 Origin 获取，完整校验后在后续新建页面时启用。Android 不从 GitHub 下载网页资源。资源清单、静态路径拦截和失败回滚见 [远程会话 PWA 与 Android 本地资源](pwa-android-implementation.md)。

## 构建入口

只编译 Windows 管理端安装包时执行 `.\scripts\build-windows-installer.ps1`，该入口不会编译 Android。根目录 `.\build.ps1` 会在 Windows 构建后运行 Android 摘要检查；仅在 Android 输入变化、缺少成功基线或 APK/清单校验失败时才同步执行 Android Release 和 lint。

明确要求 Android Release 构建时执行：

```powershell
.\scripts\build-android.ps1 -Configuration Release
```

普通构建不会递增 `versionName` 或 `versionCode`。正式 Android 发布还需要正确的独立版本、既有正式签名材料及 Android Release 标签；Actions 的构建产物本身不等于已发布。工作流触发和发版验收见 [Android APK 构建与发布](android-release.md)。
