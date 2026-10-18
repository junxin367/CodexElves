# Android 原生更新验证

日期：2026-10-18。原生实现与下述本机验证通过；正式签名构建另见集成阶段记录，设备安装尚未验收。

## 改动

- `MainActivity` 保留现有 Origin、主 frame、WebView 实例和连接代次校验，增加仅含 `type` 的 `app-info`、`check-app-update` 消息。版本直接使用 APK 的 `BuildConfig`。
- `AppUpdateController` 使用独立线程检查、下载和校验。返回前台后延迟 2 秒自动检查，检查间隔 6 小时，同一版本的自动提示间隔 24 小时；手动请求与进行中的操作合并。
- GitHub 来源固定为 `junxin367/CodexElves`，最多扫描 5 页、每页 100 项，按 Android 数字版本降序保留 32 个清单候选。历史 Android Release 超过 32 个不会直接报错；最终候选仍要求递增的 `versionCode`。
- GitHub 请求不读取远程服务 Cookie 或密钥。初始附件地址必须与仓库、标签、文件名完全一致；每次重定向重新检查 HTTPS 和主机，仅允许 GitHub 及明确列出的 GitHub Release CDN。
- 清单上限 64 KiB，单页列表上限 4 MiB，APK 上限 256 MiB。连接和读超时分别为最多 15 秒、20 秒，并检查整个检查任务的 90 秒期限、下载任务的 10 分钟期限和取消标记。
- 下载先写入私有 `cache/updates/`，校验实际大小、SHA-256、包名、版本、最低 Android 版本和与当前安装包完全相同的当前签名证书集合，拒绝 Debug 包和非递增版本。进入系统安装器前再次读取文件校验。
- 安装来源权限使用独立请求码 `4001`，避开从 `4107` 开始的文件选择请求码。授权返回后重新校验再交给系统安装；拒绝授权后只有再次点击原生按钮才会打开设置。
- 专用 FileProvider 只映射 `cache/updates/`，不导出，不申请相册或整个存储权限。取消、失败和失效回调清理未交给安装器的文件；交给安装器的文件保留至缓存过期清理。
- Activity 暂停时关闭本轮弹窗，结果保留至前台；销毁时取消任务、关闭连接并阻止原实例回调弹窗。

## 可执行证据

临时 Java 验证器直接调用本次生产函数，未以源码字符串匹配代替行为验证：

```text
PASS native update policy: 276 assertions
PASS native production transport: 26 assertions
PASS live production transport: HTTP 200, 544406 bytes, Android candidates=0
PASS aapt2 compile/link: actual manifest and all Android resources
PASS javac 17: all production Android Java sources with SDK 35,
actual AndroidX classes and aapt2 generated R
```

覆盖稳定 Android Release 选择、桌面/草稿/预发布忽略、缺失和重复附件、错误仓库/标签/文件名、40 个历史版本的候选裁剪、字段类型和缺失、相同与旧版本、最低系统版本、大小和 SHA-256、包名/版本/当前签名集合拒绝、重定向白名单与循环上限、HTTP 错误、压缩拒绝、连接清理、流中断，以及 200 次连接注册与取消竞态。取消发生在末次读取时，也不能生成成功结果。

HTTP 传输验证使用生产 `AppUpdateClient.open`、`validateContentLength` 函数和可控 HTTPS 连接，另用同一生产函数实际请求公开 GitHub API。实际 API 尚未提供符合本合同的 Android Release，未下载或安装线上 APK。

本机编译使用仓库既有隔离 JDK 17.0.20.1、Android SDK 35、真实 AndroidX Core 1.1.0、WebKit 1.17.1。AAPT2 验证真实资源和清单；仅在临时清单中补齐 Gradle 平时提供的包名和 authority 占位符。Java 编译使用构建目录中既有的 release `BuildConfig`，仅证明符号和 API 兼容；独立版本生成及 R8、正式签名由后续集成构建验证。

`verification.json` 保存本轮结果和生产输入摘要，`javac.log` 保存编译器输出。临时测试源码、class、AAR 提取件和临时资源包曾位于 `temp/android-app-update-20261018/`，不作为长期测试提交。子代理的批量清理命令被自动审批以 `blocked by policy` 拒绝；主代理核实全部文件、目录和无链接情况后，采用明确文件列表及逐层空目录删除，已完成清理。

## 未覆盖的设备行为

没有使用真机或模拟器执行来源授权、系统安装确认、安装取消、进程被系统回收后的界面恢复；也未在 Android `PackageManager` 中实际解析本轮正式签名 APK。Activity 生命周期和过期回调边界已检查生产代码，但不代替设备验收。

Activity 销毁会取消本实例操作；若系统在授权设置期间回收应用进程，用户需重新检查并下载更新，不自动恢复旧进程的安装会话。当前可用版本保持不变。
