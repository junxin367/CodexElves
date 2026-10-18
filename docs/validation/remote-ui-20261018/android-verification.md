# Android 图标与登录页验证

验证日期：2026-10-18。

实现与 Debug 构建已完成；未运行真机或模拟器，原生登录布局与键盘交互仍需设备验收。

## 修改结果

- 启动图标沿用原云朵终端图案，将原 PNG 的浅色圆角底和外围阴影移除，改为透明前景与完整的 `#F6F3FA` 背景；系统蒙版负责最终轮廓。单色主题使用独立透明图层，终端符号保留镂空。
- APK 内彩色前景和单色图层均为 `432×432` PNG，逐像素对比与源码一致。前景实心像素离画布中心的最大距离为 `30.63dp`，在本次采用的半径 `33dp` 安全圆内。
- 登录页改为网页版的星形标记、左对齐标题与说明、标签表单、黑色连接按钮、折叠帮助和“密钥勿外传”页脚。服务链接为额外字段，读取已有保存值。
- 登录表单最大宽度为 `420dp`。`ScrollView.fillViewport` 与内层居中布局使内容放得下时垂直居中；内容超过视口时按内容高度滚动，保留 `adjustResize`。
- 增加密钥显示/隐藏按钮，切换时保留光标选区；连接中禁用输入、切换与提交，错误使用可被辅助服务播报的状态文本。
- HTTPS Origin 校验、`/api/connect`、设备 Cookie、密钥成功后清空、授权恢复及资源缓存流程保持原实现。

## 与网页版的实际差异

- Android 原生登录额外显示“服务链接”，没有单独新增服务列表或服务管理功能。
- 原生登录继续使用浅色页面，未放置没有对应能力的“设置”按钮，也没有新增与网页登录外观配置不同步的本地设置。
- 原生输入字体和控件由 Android 渲染；错误继续显示在连接按钮下方，访问密钥提示不声称会自动修剪空白，保留已有原生密钥提交语义。

## 构建与检查

在仓库根目录执行：

```powershell
.\scripts\build-android.ps1 -Configuration Debug `
  -JavaHome "E:\code\junes\github\CodexPlusPlus\temp\android-build-20261017-050\microsoft-jdk\jdk-17.0.20.1+1" `
  -AndroidHome "E:\code\junes\github\CodexPlusPlus\temp\android-build-20261017-050\android-sdk" `
  -GradlePath "C:\Users\junes\.gradle\wrapper\dists\gradle-8.11.1-bin\bpt9gzteqjrbo1mjrsomdt32c\gradle-8.11.1\bin\gradle.bat"
```

结果：`BUILD SUCCESSFUL`，包含 `assembleDebug`、`lintDebug`。Lint 为 `0 errors, 1 warnings`；唯一警告是现有 `mipmap-anydpi-v26` 目录在 `minSdk 26` 下不需要版本限定。

`apksigner verify --verbose` 验证通过，APK v2 签名有效。`aapt2 dump xmltree` 确认前景、背景、单色图层分别引用对应资源；`aapt2 dump resources` 确认背景颜色为 `#FFF6F3FA`，APK 包含新登录布局。

产物：

```text
E:\code\junes\github\CodexPlusPlus\dist\android\CodexElves-0.5.0-android-debug.apk
字节数：4719392
SHA-256：c673ae3d5caf7000e5d8552168229e617b1da1198d9109abde89aed4b52ded95
```

APK 内置 `app.js` 与 `style.css` 和当前候选源文件 SHA-256 一致：

```text
app.js    31c2f4b8844951ba3573db5a946b4bcaca25532e1be4f7c95135191cda291473
style.css c8feb7dc13b9265b5952a4c1a890a0695487ecabc74a0ef48b80780da0f58819
```

## 可视证据与剩余边界

`android-icon-masks.png` 为原资源与调整后资源的蒙版对比。`android-apk-icon-preview.png`、`android-apk-icon-circle.png` 和 `android-apk-icon-round.png` 来自 APK 实际提取图层的合成；均不是设备截图。机器检查详情在 `android-apk-verification.json`。

现成工具链只有 JDK、SDK platform 35 与 build-tools 35.0.0；没有 platform-tools / `adb`、模拟器或 AVD。本次没有下载大型工具链，没有安装 APK，没有重启正式服务，没有修改版本，没有提交或推送。

剩余设备验收：手机竖屏/横屏的垂直居中、小屏与键盘弹出时滚动至连接按钮、显示/隐藏密钥时光标保留、错误播报、真机登录与授权恢复，以及具体手机桌面蒙版的图标显示。
