# Android 媒体访问与覆盖安装资源修复

日期：2026-10-22，北京时间。

## 本轮修复

- ngrok 媒体请求：在 Android 原生层接管受限的图片、视频及封面 GET/HEAD 请求，补充 `ngrok-skip-browser-warning`。跨源请求仍先通过既有 `ServiceOrigin` 校验，使用原有只读媒体凭证；不发送主线路 Cookie，不跟随重定向。
- 媒体响应：流式返回，保留 Range/If-Range、206、Content-Range 和服务端 CORS/CORP 响应头。关闭页面时断开媒体连接，不经过业务 API 或静态资源的单线程队列。新增脱敏的 `native.media` 响应与读取结果记录。
- 视频封面：正文封面、绑定封面、弹窗 video 及用于测量尺寸的封面图片，均在设置地址前启用匿名 CORS。保留点击播放后才设置视频 src 的行为。
- 覆盖安装资源：按服务 Origin 保存已识别的 APK 内置资源摘要。首次识别或内置摘要变化时优先启用本包资源，清除旧 pending，保留健康缓存用于启动失败回退；同一内置摘要再次启动时继续正常热更新流程。版本号不变的本地覆盖安装也能识别。

未修改服务端 CORP、Origin 白名单、媒体凭证或问答/审批路由。未针对原因不明的 zrok 首次停滞和图片 404 叠加修复。

## 验证

| 检查 | 结果 |
| --- | --- |
| 真实资源实现及临时 Android 桩的 Java 检查 | 48 项通过，编译、运行退出码均为 0 |
| 媒体生产实现的 JVM 检查 | 47 项通过 |
| 生产媒体客户端访问实际 ngrok 的无业务数据对照 | 3 项通过：跳过提示 HTML、到达服务鉴权、保留当前来源 CORS |
| 视频生产函数检查 | CORS 属性早于 src/poster、正文与绑定覆盖、未点击不加载视频等检查通过 |
| `:app:compileDebugJavaWithJavac :app:lintDebug` | `BUILD SUCCESSFUL`，0 个 error、4 个 warning |
| 独立增量代码审查 | 未发现阻塞问题 |
| `git diff --check` | 通过 |

资源检查覆盖首次迁移、同包重开、正常热更新 pending、回滚及进程中断、损坏缓存、Origin 隔离、取消，以及服务端同摘要返回 UNCHANGED。

媒体检查覆盖 URL/方法限制、禁止重定向、请求头范围、Range/206、响应头透传、流式读取、关闭、取消和错误响应。线上对照使用不存在的图片 ID 和无效的测试媒体令牌，预期返回服务端 `401 application/json`，未请求真实图片、问答或审批。

线上 JVM 对照使用 JDK 的 `-Dsun.net.http.allowRestrictedHeaders=true`，避免桌面 JDK 默认过滤 Origin 干扰该项验证；此参数仅用于测试，不进入 Android 运行配置。

本轮临时验证源码及资源、视频检查 fixture 已清理，不保留新测试源码。原生媒体检查生成的 8 个 class 文件仍位于 `temp/android-media-hotfix-20261022/native-check/classes/`：递归清理和单文件清理均被自动审批以 `blocked by policy` 拒绝，未继续重试；这些文件不参与正式构建。

## 生效边界

本轮只修改源码并完成开发验证，未重新生成正式 APK 或 Windows 安装包，未安装、热切换远程后台、提交或发布。原生媒体适配和覆盖安装资源迁移需要重新构建并安装 Android 包才能生效；网页封面修复也需要相应资源交付。

尚未完成真实 Android WebView 的覆盖安装、跨网络封面显示、播放拖动及关闭取消验收。zrok 初次停滞所在网络阶段与图片 404 的具体服务端分支仍待进一步日志定位。

## 后续正式本地构建

同日按用户要求完成两个交付包：

| 平台 | 产物 | 大小 | SHA-256 |
| --- | --- | ---: | --- |
| Windows 0.5.3 | `dist/windows/CodexElves-0.5.3-windows-x64-setup.exe` | 45,419,495 字节 | `53bb060e25f0986f0fa16e9e64abb3275195d8167d53acd892350609eca28716` |
| Android 0.0.3 / 5 | `dist/android/CodexElves-0.0.3-android.apk` | 2,375,743 字节 | `b47327106930c2aecb38b0b0d5abf099cbe6fe1963faa6f479d44de476278200` |

构建使用上次已验证的隔离源码基线，加上本轮修复，未带入工作区中另行进行的通知、样式和未完成的会话工具修改。Windows 执行 `scripts/build-windows-installer.ps1`，Android 执行 `scripts/build-android.ps1 -Configuration Release`，最终退出码均为 0；版本号未递增。

首次 Windows 产物核对发现旧附件脚本仍被构建缓存复用，未予交付；强制重编资源模块并重新运行标准构建入口后，管理器内嵌的 24 项资源逐字节核对通过，与正式 APK 的资源版本同为 `cf19e0f8cb71eb81efeb5a0a7e28c06afe8a6f8e47f7b99118aad22ed9741731`。

Android 正式证书与既有证书一致，Release lint 为 0 个错误、4 个警告，包内确认包含原生媒体适配与资源迁移标识。完整校验清单位于 `dist/build-media-hotfix-20261022.json`。两个包均仅本地生成，未安装或发布；建议先更新 Windows，再覆盖安装 Android，以保持服务端与 APK 网页资源一致。
