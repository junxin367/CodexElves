# CodexElves Android 构建与运行

Android 客户端位于 `apps/codex-elves-android`，使用 Java 17、Android Gradle Plugin 8.9.2、Gradle 8.11.1、`compileSdk 35`、`minSdk 26` 和 AndroidX WebKit 1.17.1。

应用版本名从根 `Cargo.toml` 的 `workspace.package.version` 读取。首版 Android `versionCode` 和原生 Shell 协议版本均为 `1`，两者不随桌面产品版本自动递增。

## 本地 debug 构建

不要修改系统级 Java 或 Android SDK 配置。通过参数向构建脚本提供隔离工具链：

```powershell
.\scripts\build-android.ps1 `
  -Configuration Debug `
  -JavaHome "C:\path\to\jdk-17" `
  -AndroidHome "C:\path\to\android-sdk" `
  -GradlePath "C:\path\to\gradle-8.11.1\bin\gradle.bat"
```

脚本默认执行 `assembleDebug` 和 `lintDebug`，产物为：

```text
dist/android/CodexElves-<version>-android-debug.apk
```

如果没有传入 `-GradlePath`，脚本依次尝试完整 Gradle Wrapper、系统中的 `gradle.bat`，最后将 Gradle 8.11.1 下载到仓库 `temp/android-gradle`。临时工具链和构建产物不应提交。

## 正式签名构建

只有以下环境变量全部存在时，脚本才允许生成正式 APK：

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

正式产物为 `dist/android/CodexElves-<version>-android.apk`。debug APK 仅用于本地验收，不能作为正式发布包。

## 连接和安全边界

- 域名可省略 `https://`，但必须是 HTTPS 根 Origin，不能包含 userinfo、路径、query 或 fragment。
- 原生登录只向当前 Origin 的 `/api/connect` 提交访问密钥；成功后立即清空输入，不持久化明文密钥。
- 设备会话使用服务返回的 `codex_elves_device` HttpOnly Cookie，并同步到 WebView。
- 对严格匹配 `*.shares.zrok.io` 的 HTTPS Origin，原生层会在首次加载页面前写入仅限当前主机、有效期 7 天的 `zrok_interstitial=1` Cookie，并在应用回到前台时刷新；其他域名不会写入该 Cookie。
- WebView 地址保持服务 HTTPS Origin。只有已校验清单内的静态路径从本地资源返回，`/api/` 和 WebSocket 始终访问真实同源服务。
- `CodexElvesAndroid` 消息桥只允许当前 Origin 的主 frame 发送 `ready`、`authenticated`、`auth-required`、`logged-out`。
- 跨源重定向、证书错误、HTTP 明文地址和资源目录穿越均被拒绝。
- 完整下载并校验的新资源只记录为待启用版本，在下次新建页面时启用；启动失败会回退到上一完整版本。
- 新进程首次创建资源仓库时会删除上次异常退出遗留的 `.staging-*` 目录；同一进程内 Activity 重建不会再次清理，因此不会删除正在下载的暂存目录。
