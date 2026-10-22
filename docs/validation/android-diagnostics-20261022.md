# Android 诊断日志与本地安装包验收

日期：2026-10-22，北京时间。

## 功能交付

- 原生 App 持久记录有容量上限的诊断事件，包含生命周期、资源版本、网络类别、WebView/SSL/HTTP 错误、图片加载以及经过分类的控制台错误。
- 网页设置支持导出和发送到电脑；登录页与原生启动失败页支持离线导出。
- 电脑端通过既有本机管理接口请求、查询和读取指定授权设备的日志。事件和原有会话状态轮询共同补达未过期请求；上传失败可以在后续轮询重试。
- 三端限制字段及容量，不记录问答正文、密钥、Cookie、带参数网址、异常原文或完整本地路径。
- 操作说明见 `docs/android-diagnostics.md`，本机入口为 `scripts/remote-diagnostics.ps1`。

## 验证

- 网页生产模块隔离验证 17 项通过，覆盖脱敏、状态和响应类型、主动取消/超时、大图错误、导出/上传回执、其他设备/过期请求拒绝、成功去重、失败重试、采集期间切换设备，以及旧 APK/浏览器兼容。
- 原生代理执行 180 项 JVM 检查通过，Java 编译及最终 Debug lint 通过。
- 实际交付快照的诊断服务回归 3 项通过；静态资源清单与 HTTP 边界回归 1 项通过。
- 正式 Android Release、`lintRelease`、签名、版本、包大小和 SHA-256 校验通过；lint 为 0 个错误、4 个警告。
- APK 内 24 项网页资源逐项大小、SHA-256 校验通过；DEX 包含原生诊断协议、上传路径以及 CORS/CORP 固定错误分类。
- PowerShell 采集脚本及临时部署脚本语法检查通过；新增 Rust 文件格式检查通过；JavaScript 语法及差异检查通过。
- 独立复核及后续定向复核未发现未解决的确认问题。

这些检查没有操作手机安装，没有实际从手机导出或上传日志，也没有发送真实问答或审批。真机分享、设备网络切换和厂商 WebView 行为尚待安装后验收。

## Android 本地产物

- 标准交付路径：`dist/android/CodexElves-0.0.3-android.apk`。另保留本轮独立副本 `dist/android/local-diagnostics-20261022-2dea5707/CodexElves-0.0.3-android.apk`。
- 版本：`0.0.3 / versionCode=5`，按仅本地编译规则未递增版本，需要手动覆盖安装。
- 大小：`2373119` 字节。
- SHA-256：`2dea570791e6724f3ebdd0f2f99fcc456ec943e90a38e2141d049bbdbff0ede5`。
- 正式证书 SHA-256：`7b15895625b40e03792fc250a1fc489df11fe03a25f7e8f1e177d2442fe9dc2d`，与此前正式包一致。
- APK 网页资源版本：`49671170a1e2c9e526e16d0b152f491abf78b0737a6f996a8b6a3b2a3b1f1953`。
- 同目录保存更新清单、签名结果、lint XML 和包内容验证结果。

用户随后要求同时编译 Windows 安装包与安卓包。Android 输入及正式签名产物未变化，构建入口的基线检查确认无需重复构建；复用上述已验证产物。

## Windows 安装包

状态：**已构建未发布**。通过本轮隔离源码快照的 `scripts/build-windows-installer.ps1` 完整构建，退出码 0，NSIS 使用 UTF-8。桌面版本保持 `0.5.3`。

- 标准交付路径：`dist/windows/CodexElves-0.5.3-windows-x64-setup.exe`。
- 大小：`45421056` 字节。
- SHA-256：`fe73c865bdfc16c03a4352bc1b94aab6253b1142f09b59cb065d2988e8e69cb4`。
- `target/release`、快照打包目录及标准 `dist/windows/app` 的 launcher、manager、task-board 摘要核对一致。
- 最终 manager SHA-256：`d9e459d96a9b31ad88d1e4d3fb5927cde29853b295bd85e3f27fb1debeb5c042`。
- 最终 manager 内嵌的 `app.js`、`channels.js`、`platform.js`、`diagnostics.js`、`sw.js` 与本轮源码逐字节一致。

两平台的产物位置、大小及程序摘要保存在 `dist/build-diagnostics-20261022.json`。未执行安装或发布；当前安装目录的运行程序不因本地编译而更新。

## 源码范围

沿用前一轮 `d06dade83d1a51590b81aac278827614d61f8d62` 的隔离快照，叠加本轮及前面已经确认的连接、问答和图片修复。原工作区的 `workspace.rs`、两份既有文档以及 `session-tools.js` 的 `responseVersion=2` 保留；本轮产品包仍采用快照的工作区协议及 `responseVersion=1`，不合入其他未完成变更。

## 运行状态与边界

2026-10-22 21:35:22，独立诊断后台曾成功启动，PID `52864`，程序 SHA-256 为 `0368fcf956028768a1e4135c772593220acb0bab5903139c4b405bd638863e27`。当时本机管理接口可列出 3 台授权设备，尚无报告或待处理采集请求；切换前后授权和设置摘要一致。

本机 24 项资源核对完成后，zrok 公网验证遇到连接超时；同时新后台记录 zrok 上游 `api-v2.zrok.io` 的 TLS 握手超时，ngrok 保持可用。没有将该轮公网验证记为成功。

中断期间，运行中的程序再次变化。21:43 后检查为安装目录 `D:\foWork\CodexElves\codex-elves-manager.exe`，PID `29652`，摘要 `c5c929275ad915be021be03838ce212b8d1580f1d9d37a6965661ca3af81e2a1`，父进程为安装目录 launcher；该版本没有诊断接口。本轮保留该运行程序，不覆盖来源不同的安装内容。诊断功能实际启用以用户安装本次 Windows 和 Android 包后为准，不把此前临时后台已就绪等同于当前安装版已更新。

运行与验收证据保留在：

```text
C:\Users\junes\.codex-session-delete\remote-access-runtime\diagnostics-20261022-0368fcf95602\
```

建议先安装 Windows 包，再覆盖安装 Android 包；彻底退出 App 后重开。若提示网页资源已更新，再退出重开一次启用。无需清除 App 数据或草稿。

## 清理与发布

未执行 Git commit、push、创建标签、GitHub Release 或自动安装。原生临时测试源码已删除，但自动审批以 `blocked by policy` 拒绝清理 `apps/codex-elves-android/temp/native-diagnostics-check/` 中的 `.class` 文件与空目录，因此这些被忽略的临时编译残留暂保留。
