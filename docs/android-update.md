# Android 版本与应用更新

Android 使用独立版本配置 `apps/codex-elves-android/version.properties`，当前显示版本为 `0.0.1`。管理器版本继续由根 `Cargo.toml` 管理。Android 的 `versionCode` 从此前正式包的 `1` 增加到 `2`，保证改用独立显示版本后仍可覆盖升级；以后每次正式发布都必须增加 `versionCode`。

## 设置页与原生桥

Android 设置弹窗底部左侧显示当前已安装 APK 的 `v版本号`，使用比按钮文字略小的字号和弱化颜色；“恢复默认”“完成”继续靠右。点击版本号可手动检查更新。普通网页与 PWA 不显示 Android 版本。

网页通过当前受信任来源的 `CodexElvesAndroid` 消息桥发送：

```json
{"type":"app-info"}
```

原生使用同一消息通道回复：

```json
{"type":"app-info","versionName":"0.0.1","versionCode":2,"updateSupported":true}
```

版本取自 APK 的 `BuildConfig`，不能从电脑端版本、GitHub 最新版本或网页资源版本推断。旧版外壳没有回复时不显示猜测版本。手动检查使用 `{"type":"check-app-update"}`，网页不能指定下载地址、文件路径、包名或签名。

## GitHub 发布协议

仓库固定为 `junxin367/CodexElves`。Android 正式版本使用 `android-v<versionName>` 标签，例如 `android-v0.0.1`；不读取桌面版的 `latest.json`，Android Release 不取代桌面版的 GitHub Latest 标记。

每个 Android Release 包含：

- `CodexElves-<versionName>-android.apk`
- `android-update.json`

更新清单结构如下；其中大小和摘要必须从实际签名后的 APK 生成：

```json
{
  "schemaVersion": 1,
  "applicationId": "com.junxin367.codexelves",
  "versionName": "0.0.1",
  "versionCode": 2,
  "minSdk": 26,
  "apk": {
    "name": "CodexElves-0.0.1-android.apk",
    "size": 0,
    "sha256": "实际 APK 的 64 位 SHA-256"
  }
}
```

上述大小与摘要是格式示例，不是可发布清单。APK 和清单地址取自对应 GitHub Release 的附件，并限定到本仓库、本标签及准确文件名。忽略草稿、预发布和其他平台标签；发布时校验标签与 Android 独立版本相符。

## 更新行为

启动或返回前台后自动检查，检查在独立后台任务执行，不阻塞会话登录、网页资源更新和图片读取；自动检查间隔为 6 小时。手动检查可以立即触发，同一时间只进行一次检查或下载。

发现新版后提示用户下载并更新，下载完成后校验文件大小、SHA-256、包名、显示版本、递增的 `versionCode` 及当前安装包的签名证书，再交给 Android 系统安装。安装来源权限及最终安装确认由系统处理；不申请整个相册或存储权限，不静默安装。GitHub 请求不携带远程会话 Cookie 或访问密钥。

下载使用应用私有缓存目录，只允许通过专用 FileProvider 将经过校验的 APK 临时提供给系统安装器。不接受任意网页提供的安装地址。网络失败、取消下载、校验失败及安装权限被拒绝时保留当前可用版本，允许用户重试；切后台和 Activity 销毁后不弹出失效界面。

## 构建规则

后续“编译”或“编译安装包”继续使用根 `build.ps1`。Android 源码、资源、独立版本、构建脚本或 APK 内置的 `assets/remote-access` 文件发生变化时，同步构建正式 Android APK；没有变化且已有产物校验有效时跳过。首次没有成功构建基线或产物缺失时重新构建。

本地判定依据上一次成功正式构建的内容摘要，包含已提交、未提交和新增的相关文件；不能只检查 `git diff`。签名配置缺失时明确失败，不能用 Debug APK 冒充正式包。版本、产物摘要和成功基线只在构建及校验成功后保存。

GitHub 对 Android 相关变更执行 Android 构建；PR 使用无需发布密钥的 Debug 检查，受信任分支生成正式签名构建产物，Android 标签生成独立 Release 及更新清单。签名沿用现有 Actions secrets，密钥仅在运行器临时目录使用并于结束后清理。
