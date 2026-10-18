# 按钮比例调整后的 Android Debug 验证

验证日期：2026-10-18。本记录对应本轮网页按钮 CSS 与原生密钥显隐按钮调整，不覆盖上一轮 Android 历史记录。

在仓库根目录执行：

```powershell
.\scripts\build-android.ps1 -Configuration Debug `
  -JavaHome "E:\code\junes\github\CodexPlusPlus\temp\android-build-20261017-050\microsoft-jdk\jdk-17.0.20.1+1" `
  -AndroidHome "E:\code\junes\github\CodexPlusPlus\temp\android-build-20261017-050\android-sdk" `
  -GradlePath "C:\Users\junes\.gradle\wrapper\dists\gradle-8.11.1-bin\bpt9gzteqjrbo1mjrsomdt32c\gradle-8.11.1\bin\gradle.bat"
```

结果：`BUILD SUCCESSFUL`，`assembleDebug` 和 `lintDebug` 均通过。Lint 为 `0 errors, 1 warnings`，仅有现存 `mipmap-anydpi-v26` 目录的 `ObsoleteSdkInt` 警告。

最终产物：

```text
E:\code\junes\github\CodexPlusPlus\dist\android\CodexElves-0.5.0-android-debug.apk
字节数：4719483
SHA-256：9c011608dac507c8ad2462caf9228cf54b767a372b3a64dac0b780ebb5a03f83
```

APK 内置资源与当前源文件逐字节一致，两侧 SHA-256 分别为：

```text
app.js    31c2f4b8844951ba3573db5a946b4bcaca25532e1be4f7c95135191cda291473
style.css 51e5d2995fc55b64afc7cd8b63ca440dbdd9cc668707ac6d9c4c8699dd4450a2
```

`aapt2 dump xmltree` 确认 APK 中密钥显隐按钮外框为 `36×36dp`，内边距 `9dp`，字段内间距 `7dp`，图标绘制区域保持 `18×18dp`；新的波纹资源包含半径 `6dp` 的显式 mask。

证据文件：

- `android-build-debug.log`：本轮完整构建日志。
- `android-lint-debug.txt`：本轮 lint 结果。
- `android-apk-verification.json`：APK 哈希、网页资源一致性和原生按钮检查结果。
- `android-apk-login-layout.txt`：APK 内登录布局资源。
- `android-apk-button-ripple.txt`：APK 内按钮波纹资源。

本次未运行真机或模拟器，未安装或发布 APK，未修改版本，未提交或推送。设备键盘、触摸波纹及实际桌面显示的运行验收仍未执行。
