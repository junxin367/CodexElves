# Android 资源与图片网络链路核对

日期：2026-10-18，取证时间约为北京时间 08:08–08:16。此记录仅覆盖 Android 原生资源选择、资源更新、请求头和图片路由；消息历史分页与事件刷新由同目录其他验证记录覆盖。

## RCA 结论

- 状态：**部分确认**。已核实当前源码的线程、版本比较、文件复用、gzip 和图片路由，没有取得用户手机上已安装包、当前资源快照或请求时序，不能判定截图故障的完整根因。
- 现象：按任务提供的截图描述，Android 已有会话标题、队列与编辑框，主消息区只见一条活动且队列缩略图空白；同期网页已有正文和缩略图。截图描述不是本轮真机复现。
- 直接原因：本轮尚未取得缺失正文与图片请求的真实响应、耗时及错误，无法确定。
- 根因：没有证据支持把 `ResourceStore` 下载或本地文件校验认定为这次故障根因。
- 系统性根因：未确认。缺少真机版本与网络时序，只能作为调查缺口。
- 关键因果链：`selectForPage()` 在资源线程校验并选择完整快照 → UI 线程创建 WebView → 本地静态资源被拦截返回 → `app.js` 发出 `ready` → 原生更新检查与 `connect()` 可能重叠 → 会话、历史及 `/api/images/` 由 WebView 发起同源网络请求。最后一步是否失败及其原因尚无真机证据。

## 证据

| 判断 | 类型 | 可复核证据 | 来源 |
| --- | --- | --- | --- |
| 页面资源校验完成后才创建 WebView | 事实 | `startSessionPage()` 将 `selectForPage()` 提交给 `resources`；成功后才调用 `displaySessionPage()` | `MainActivity.java` 的上述方法 |
| 截图中已经出现队列后，显示页面前的资源校验已经结束 | 推断 | 队列依赖已建立的 WebView 与网页脚本；其创建顺序晚于资源选择 | 上一条控制流；不代表后台更新已经结束 |
| 会话/图片网络没有通过 `ResourceStore` 下载器 | 事实 | 非 GET 返回 `null`；`/api/` 被 `ResourceManifest.canonicalPath()` 排除后返回 `null`；没有清单条目时也返回 `null` | `MainActivity.interceptResource()`；`ResourceManifest.canonicalPath()` |
| 资源更新会在首轮会话读取之前启动 | 事实 | `app.js` 末尾依次执行 `postPlatformMessage("ready")` 与 `connect()`；原生 `ready` 分支立即调用 `checkForResourceUpdate()` | `app.js`；`MainActivity.handleBridgeMessage()` |
| 当前实现不会把内置资源标识 `builtin` 当作内容摘要比较 | 事实 | 比较 `manifest.version` 与 `current.manifest.version`，相同时直接返回 `UNCHANGED` | `ResourceStore.checkForUpdate()` |
| 未改变的文件会从当前/前一版/内置快照复用 | 事实 | `sameContent()` 对比路径、Content-Type、大小和 SHA-256；复制时再校验内容，只有无法复用的条目才下载 | `ResourceStore.reusableSnapshots()`、`reuseAndVerify()` |
| 显式 gzip 请求与手动解码匹配 | 事实 | 发送 `Accept-Encoding: gzip`，依据 `Content-Encoding` 构造 `GZIPInputStream`，分别限制编码与解码大小并核对摘要 | `ResourceStore.open()`、`encodedInput()`、`decode()`；Android 官方 `HttpURLConnection` 文档 |
| 两个现有 APK 的全部内置资源符合各自清单 | 事实 | 两包各 20 个文件，大小与 SHA-256 失败数均为 0 | `android-transport-artifacts.json` |
| 当前服务资源与现有 release 包资源同版 | 事实 | 完整版本均为 `9878532f04978dfaaecd9f63a90783a9f52f1e2f969221512869df926bb6e43d` | 公共服务清单及 release APK 内置清单 |
| debug 包与服务仅有两个文件不同 | 事实 | `/app.js`、`/style.css` 不同，其余 18 个条目满足 `sameContent()` | `android-transport-artifacts.json` |
| 当前工作区尚有未部署的静态资源修改 | 事实 | 取证时工作区 `/app.js`、`/style.css` 与服务清单摘要不同 | `android-transport-artifacts.json` 的 `sourceDifferences`；这是时间点快照 |

WebView 官方契约规定，`shouldInterceptRequest()` 返回 `null` 后由 WebView 正常加载资源。因此资源更新的 15 秒连接超时、30 秒读超时并不控制网页历史或图片请求。本文核对的 `skip_zrok_interstitial: 1` 和手动 Cookie 头作用于原生连接，不会自动附加给 WebView API；WebView API 路径依靠自身 CookieManager 中的同源 Cookie。当前代码会在加载前准备严格匹配的 zrok 主机 Cookie，尚未取得截图设备的 Cookie 运行状态。

本地静态响应的 CSP 为 `img-src 'self' data:`。队列附件当前只生成同源 `/api/images/<id>` 或经校验的 `data:image/...`，本轮没有看到它使用 `blob:` 的证据，故不能仅因 CSP 不含 `blob:` 就判定图片失败。

## 对照测量

只读请求目标为本机现有服务 `http://127.0.0.1:45222`，未调用登录接口、未读取设备凭据、未发送消息，也未重启或替换正式服务。

| 资源 | identity 响应实体 | gzip 响应实体 | gzip 本机单次耗时 |
| --- | ---: | ---: | ---: |
| `/app-resources.json` | 3,430 B | 1,308 B | 0.001870 秒 |
| `/app.js` | 494,465 B | 122,806 B | 0.002094 秒 |
| `/style.css` | 83,378 B | 16,022 B | 0.002572 秒 |

三组请求均返回 HTTP 200。六份响应都经过解码后摘要对照，全部与服务清单一致。原始响应头见 `android-transport-*-headers.txt`，详细字节数、耗时与摘要见 `android-transport-http.json`。

按当前 `ResourceStore` 算法和这两份 APK 的内置快照计算：

- **现有 release 包**：内置资源与服务同版，获取 gzip 清单约 1,308 B 后直接结束，不下载资源文件。
- **现有 debug 包**：下载两个差异文件，解码前实体共 138,828 B，连同清单共 140,136 B；解码后差异共 577,843 B，其余 18 个文件本地复用。即使发生一次差异更新，也不是全包 6,391,862 B 都经网络传输。
- 5,254,649 B 的 Mermaid 在两包与服务中摘要相同，可本地复用，不属于上述网络差异。

上述字节数是 HTTP 响应实体大小，不含请求头、响应头和 TLS；毫秒级耗时是本机环回测量，不能当作公网或手机速度，也没有测得后台更新与正文加载同时发生时的真实竞争程度。若手机当前快照不是这些内置资源，实际差异会变化。

本轮取证的包哈希：

```text
Debug APK SHA-256:
9c011608dac507c8ad2462caf9228cf54b767a372b3a64dac0b780ebb5a03f83
Debug 内置资源版本:
64767e9760199c8d8ca58119a30d2db74939e14de82c421c3d496e59e088a81f

Release APK SHA-256:
498ff0e866329e9e1d4e773047801ecd3f755185924e905a89d4734512b2a1f9
Release 内置资源版本:
9878532f04978dfaaecd9f63a90783a9f52f1e2f969221512869df926bb6e43d
```

包哈希仅用于识别这次检查的本地文件，不证明用户手机安装了该包，也不证明其当前启用的网页快照仍为内置版本。

## 已排除解释

- **“当前实现每次启动必下载 6 MB，且关闭 gzip”**：当前源码有内容版本短路、文件摘要复用和显式 gzip；现有 release 包与服务同版，debug 包只有约 139 KB gzip 文件差异。旧审计中的该问题不能直接套用到当前源码。
- **“把 ResourceStore 的网络超时调大就能解决历史/缩略图”**：该类只下载清单内静态资源，历史和图片走 WebView；修改这些超时不会改变对应请求。
- **“显示队列之后仍在等待显示页面前的主线程全量校验”**：当前选择已在 `resources` 线程，且 WebView 在选择成功后才建立。该先后关系不能解释已经显示队列后的正文缺失。

以上只排除相应解释对**当前源码与本轮本地包**的成立，不能替代已安装原生代码和用户设备运行证据。

## 未闭环项与下一步

- **缺少已安装包与快照身份**：需要真机包版本、APK 摘要，以及当前 origin 的 `active` / `pending` / `trial` 内容摘要；仅版本名 `0.5.0` 不能区分上述不同包。该项可能改变“手机上是否具备当前优化”的判断。
- **缺少消息/图片请求证据**：需要 WebView 中失败请求的 URL 路径、起止时间、HTTP 状态、编码响应大小和控制台错误，才能区分队列图片 API、图片解码、CSP、会话历史串行或重连。
- **缺少原生更新的竞争时序**：需要同一会话启动中的资源清单/文件下载和历史/图片请求时间轴；只有确认实际存在资源文件传输且推迟前台请求后，才可把让出网络的调度列为必要修复。目前只证明存在启动重叠窗口。
- **缺少公网与真机对照**：本轮没有取得 ADB/设备运行证据，现有项目隔离 SDK 只有 build-tools 与 platforms，PATH 未发现 adb；未安装工具、App 或模拟器。环回测量只用于内容与压缩验证。
- **部署版本差异**：本轮工作区网页源码与服务有差异。完成修复和构建后仍需分别确认 APK 内置资源与服务公开清单；不能以源文件修改代替部署证据。

## 修复与防复发

- 直接修复：本子任务不修改 `ResourceStore.java`、`ResourceManifest.java` 或 `ResourceStatePolicy.java`。没有取得足以决定必要原生资源修复的故障证据。
- 根因修复：历史读取与事件刷新优化由网页/原生桥子任务完成；本报告不抢占其根因判定。
- 系统性防线：后续真机验收同时保留包摘要、当前网页资源摘要及请求时间轴，确保版本差异可以判定。此为验收建议，本轮未新增日志、遥测或产品设置。
- 验证范围：只读公共 HTTP、APK 全资源摘要、现有控制流核对及官方 API 契约；没有运行 Gradle 全构建、没有真机运行验收，没有修改授权、安装、提交或推送。

官方参考：

```text
https://developer.android.com/reference/java/net/HttpURLConnection
https://developer.android.com/reference/android/webkit/WebViewClient#shouldInterceptRequest(android.webkit.WebView,%20android.webkit.WebResourceRequest)
```
