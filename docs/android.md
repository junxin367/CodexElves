# CodexElves Android 构建与运行

Android 客户端位于 `apps/codex-elves-android`，使用 Java 17、Android Gradle Plugin 8.9.2、Gradle 8.11.1、`compileSdk 35`、`minSdk 26` 和 AndroidX WebKit 1.17.1。

应用版本独立维护于 `apps/codex-elves-android/version.properties`，当前 `versionName=0.0.5`、`versionCode=23`，不随桌面管理器版本变化。旧正式包为 `versionName=0.5.0`、`versionCode=1`；Android 按递增的 `versionCode` 判断覆盖升级，因此新的显示版本仍能升级旧包。每次正式发布必须递增 `versionCode` 并沿用现有签名。原生 Shell 协议版本仍为 `1`，不等同于应用版本。

版本文件必须包含唯一的 `versionName` 和 `versionCode`：前者为 `主版本.次版本.修订版本` 三段数字，后者为 `2` 到 `2100000000` 的整数；缺失或无效时构建失败。

## Windows 与 Android 同步构建

日常“编译”或“编译安装包”执行根入口：

```powershell
.\build.ps1
```

Windows 安装包完成后，入口自动比较 Android 内容与上一次成功正式构建的摘要。范围包括 Android 源码、资源、版本与 Gradle 配置、`scripts/*android*.ps1`、根 `build.ps1` 和内置 `assets/remote-access` 网页文件；排除构建目录、缓存、IDE 状态、本机 `local.properties`、签名文件和工具链。已提交、未提交、新增和删除的文件均参与判断。

相关内容发生变化、首次没有成功基线、APK 或更新清单缺失及摘要不匹配时，自动执行 Release 和 lint；否则输出跳过原因。成功基线保存在已忽略的 `dist/android/build-state.json`，只在签名、lint、APK 元信息及清单摘要校验成功后更新；构建过程中输入发生变化也不会记录成功。

只读检查或单独同步 Android，无需再次构建 Windows：

```powershell
.\scripts\sync-android-build.ps1 -CheckOnly |
  Select-Object NeedsBuild, Reason
.\scripts\sync-android-build.ps1
```

也可向根入口传入隔离工具链：

```powershell
.\build.ps1 `
  -AndroidJavaHome "C:\path\to\jdk-17" `
  -AndroidSdkHome "C:\path\to\android-sdk" `
  -AndroidGradlePath "C:\path\to\gradle-8.11.1\bin\gradle.bat"
```

## 本地 debug 构建

不要修改系统级 Java 或 Android SDK 配置。通过参数向构建脚本提供隔离工具链：

```powershell
.\scripts\build-android.ps1 `
  -Configuration Debug `
  -JavaHome "C:\path\to\jdk-17" `
  -AndroidHome "C:\path\to\android-sdk" `
  -GradlePath "C:\path\to\gradle-8.11.1\bin\gradle.bat"
```

上述 Debug 命令执行 `assembleDebug` 和 `lintDebug`，产物为：

```text
dist/android/CodexElves-<version>-android-debug.apk
```

如果没有传入 `-GradlePath`，脚本依次尝试完整 Gradle Wrapper、系统中的 `gradle.bat`，最后将 Gradle 8.11.1 下载到仓库 `temp/android-gradle`。临时工具链和构建产物不应提交。

## 正式签名构建

直接运行 `.\scripts\build-android.ps1` 默认执行 Release；测试构建必须显式指定 `-Configuration Debug`。

直接使用以下四个环境变量提供正式签名：

```text
CODEX_ELVES_ANDROID_KEYSTORE
CODEX_ELVES_ANDROID_STORE_PASSWORD
CODEX_ELVES_ANDROID_KEY_ALIAS
CODEX_ELVES_ANDROID_KEY_PASSWORD
```

之后执行：

```powershell
.\scripts\build-android.ps1 `
  -Configuration Release `
  -JavaHome "C:\path\to\jdk-17" `
  -AndroidHome "C:\path\to\android-sdk" `
  -GradlePath "C:\path\to\gradle-8.11.1\bin\gradle.bat"
```

未完整提供这些变量时，Windows 本地入口会自动调用现有的 `%LOCALAPPDATA%/CodexElves/signing/android/Build-Release.ps1`，向它传入仓库位置，并复用其正式签名及默认 JDK、SDK、Gradle；显式传入的工具链参数优先。签名辅助脚本运行于子 PowerShell 进程，解密后的签名变量不会保留在父进程中。没有环境变量和现有辅助脚本时明确失败，不能创建替代证书或生成 Debug 包冒充正式包。

正式构建必须执行 `lintRelease`，不允许 `-SkipLint`。随后使用 SDK `build-tools/35.0.0` 中的 `apksigner` 验证签名并拒绝 Debug 证书，使用 `aapt` 核对包名、显示版本、`versionCode` 和 `minSdk`。

正式输出：

- `dist/android/CodexElves-<versionName>-android.apk`
- `dist/android/android-update.json`：大小及 SHA-256 取自实际签名 APK，无下载 URL。
- `dist/android/build-state.json`：本机成功基线，不作为 GitHub Release 附件。

Debug APK 仅用于本地验收和 PR 检查，不更新正式成功基线。应用内更新和发布合同见 [Android 版本与应用更新](android-update.md)。

## 交互触觉反馈

- 仅 Android App 对按钮点击、选择控件和队列排序提供轻量触觉反馈；普通手机网页不调用震动或原生桥。
- 队列长按进入拖动、实际换位和提交新顺序时分别反馈；取消拖动、顺序未变化、禁用操作、文字输入、滚动和自动刷新不触发。
- 原生登录的连接、密码显隐和帮助展开按钮也有点击反馈；输入法按键继续由系统处理。
- 使用 `View.performHapticFeedback`，遵循系统触觉设置，无需 `VIBRATE` 权限。后台、无焦点或已失效页面不播放；连续反馈有频率限制，不延迟补播。
- 网页桥只接受 `haptic-click`、`haptic-drag-start`、`haptic-drag-tick`、`haptic-drag-end` 四种单字段消息，不能指定震动时长、强度或波形。旧 App 不识别这些消息时不影响原有操作。

## 系统权限与申请时机

| 功能 | 授权处理 |
| --- | --- |
| 会话事件通知 | Android 13+ 尚未获准时，在用户开启“接收通知”后申请 `POST_NOTIFICATIONS`。较低版本读取系统通知开关。未授权或关闭通知时保留明确提示和系统设置入口；各事件频道也单独检查。 |
| 应用更新安装 | 声明 `REQUEST_INSTALL_PACKAGES`，安装前查询当前应用是否允许安装未知来源应用；需要时引导进入该应用的来源设置，返回后重新核验，再交系统安装器确认。 |
| 选择图片 | 通过系统照片/文件选择器获得所选图片的临时 URI 访问授权，不申请全相册或公共存储权限。 |
| 交互触觉 | `View.performHapticFeedback` 遵循系统触觉设置，不申请 `VIBRATE` 或运行时权限。 |
| 网络与通知后台连接 | 网络和前台服务使用 Manifest 普通权限声明。`remoteMessaging` 类型本身无需额外运行时授权；本应用只在通知偏好及系统权限允许时从可见 Activity 启动连接服务。 |

系统授权是否已获得以当前 Android API 的实际返回为准，不以应用开关或“曾经申请”标志代替。通知偏好保存的确认快照不会缓存系统权限。

## 连接和安全边界

- 服务链接支持显式 `http://` 或 `https://` 根 Origin，可保留自定义端口；省略协议时默认 HTTPS。使用 frp 时，填写管理器中已应用为主链路的固定公网地址及其映射端口。地址不能包含 userinfo、路径、query 或 fragment。
- 原生登录只向当前 Origin 的 `/api/connect` 提交访问密钥；成功后立即清空输入，密钥输入框禁用 View 状态保存，不随 Activity 状态持久化明文密钥。
- 设备会话使用服务返回的 `codex_elves_device` HttpOnly Cookie；原生登录后写入主 WebView，打开独立浏览器入口时再同步到浏览器 WebView 的 CookieManager。
- 对严格匹配 `*.shares.zrok.io` 的 HTTPS Origin，原生层会在首次加载页面前写入仅限当前主机、有效期 7 天的 `zrok_interstitial=1` Cookie，并在应用回到前台时刷新；其他域名不会写入该 Cookie。
- WebView 地址保持用户填写的 HTTP/HTTPS 服务 Origin。只有已校验清单内的静态路径从本地资源返回，`/api/` 和 WebSocket 始终访问真实同源服务；HTTP 主地址使用 `ws://`，HTTPS 主地址使用 `wss://`。
- `CodexElvesAndroid` 消息桥只允许当前 Origin 的主 frame 发送受支持的消息；应用版本及更新消息为 `app-info`、`check-app-update`，网页不能指定 APK 下载地址或签名。
- 跨源重定向、证书错误和资源目录穿越均被拒绝。HTTP 主地址允许明文连接，HTTPS 第二通道仍按既有域名及授权规则校验。
- 无法连接远程服务时，网页提供「重新连接」与「退出」。确认退出后取消重连、清理本地草稿和会话缓存，并通知 Android 清除本地授权、返回原生连接页面；远程退出请求最多等待 2 秒，失败不阻止本地退出。普通浏览器在远端撤销未确认时提示可在电脑端撤销设备，本页刷新仍要求重新输入访问密钥。
- 完整下载并校验的新资源只记录为待启用版本，在下次新建页面时启用；启动失败会回退到上一完整版本。
- WebView 渲染进程退出时立即移除并销毁失效页面，取消其选图、资源和桥接回调，显示“重试”入口；保留已授权连接及 Cookie。手动重试复用资源选择流程，未完成启动确认的试用版本回退，已确认版本继续使用，不自动循环重载。
- 新进程首次创建资源仓库时会删除上次异常退出遗留的 `.staging-*` 目录；同一进程内 Activity 重建不会再次清理，因此不会删除正在下载的暂存目录。
