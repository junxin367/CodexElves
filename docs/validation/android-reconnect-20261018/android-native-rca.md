# Android 原生容器断线审计

日期：2026-10-18。范围：`apps/codex-elves-android/` 的生命周期、Cookie、授权状态、静态资源更新、WebView 销毁与渲染进程退出处理，以及当前本地 APK 的内置资源身份。

本轮只读审计源码，新增此报告和 `android-native-evidence.json`。未修改原生代码，未运行 Gradle 构建，未连接设备、安装 APK、修改授权、发送消息或重启服务。此前未提交修改保持原样。网页连接控制与事件 worker 由其他审计任务负责。

## RCA 结论

- 状态：**部分确认**。
- 现象：用户反馈 Android App 频繁断开重连；持续前台使用，以及切后台、锁屏、选图后回来，两类场景均发生。这是用户反馈，本轮没有真机复现。
- 直接原因：现场连接关闭事件、关闭码、连接代次、网络切换和原生生命周期记录缺失，尚不能判定频繁断线的直接原因。
- 根因：原生审计没有发现前后台回调、Cookie 刷新或资源更新主动反复重载页面的路径。确认存在一项独立的渲染进程恢复合同缺陷：当前稳定页面的 renderer 退出后，代码不销毁或重建已经不可用的 WebView。它不能直接被认定为现场频繁重连的根因。
- 系统性根因：现场问题未确认。渲染进程缺陷的代码机制为：终止回调复用“试用资源启动失败回退”处理器，稳定资源的非试用分支只有 Toast，没有专门的 WebView 终止清理与恢复。
- 关键因果链：`onRenderProcessGone()` → `handlePageStartupFailure()` → `rollbackTrial()` 在稳定资源上返回 `false` → 只弹提示、保留 `webView` 和视图 → 原生回调返回 `true`，但失效 WebView 未被清理。官方合同规定该 WebView 已不能使用，必须移除、销毁并清理引用；继续显示网页需要新实例。用户设备是否发生 renderer 退出未知。

## 证据

以下行号来自本轮读取的当前源码；`MainActivity.java` SHA-256 为 `a909b22736487b4434f83db8b287e85a63d045163047e80c21799553386f2f83`。

| 判断 | 类型 | 证据 | 来源位置 |
| --- | --- | --- | --- |
| 普通回到前台不会重载页面 | 事实 | `onResume()` 只调用 `refreshZrokCookie()`，以及按条件调用 `checkForResourceUpdate()`；没有 `openSession()`、`loadUrl()` 或 `reload()` | `MainActivity.java:753` |
| 当前容器没有主动暂停或恢复 JavaScript 定时器 | 事实 | 原生目录没有 `WebView.onPause()`、`WebView.onResume()`、`pauseTimers()` 或 `resumeTimers()` 调用；Activity 没有覆写 `onPause()` 或 `onStop()` | 本轮定向 `rg`；官方 WebView 文档区分 `onPause()` 与全局 `pauseTimers()` |
| 资源检查间隔不等于周期性重连间隔 | 事实 | 15 分钟常量只用于 `onResume()` 触发检查的节流；没有周期性检查 Runnable。下载成功只 Toast“将在下次启动页面时启用” | `MainActivity.java:72`、`:656`、`:687`、`:753` |
| 更新下载不替换当前运行页面 | 事实 | `checkForUpdate()` 只写入完整已验证候选的 `pending` 状态；`selectForPage()` 才在下次建立页面时选择候选。当前 WebView 闭包持有原 `pageSnapshot` | `ResourceStore.java:51`、`:141`、`:220`；`MainActivity.java:320`、`:613` |
| zrok 刷新不会删除设备授权 Cookie | 事实与合同推断 | 刷新只写 `zrok_interstitial`；设备 Cookie 名为 `codex_elves_device`。Android `setCookie` 仅替换同 host/path/name 的既有 Cookie | `ZrokCookiePolicy.java:11`、`:53`；`MainActivity.java:68`、`:350`、`:372`；官方 CookieManager 文档 |
| zrok Cookie 刷新失败不会关闭当前页面 | 事实 | 失败分支只有 Toast“当前页面保持不变” | `MainActivity.java:372` |
| 容器不按时间主动清除授权 | 事实 | `SessionStore` 保存 origin 与 authorized 布尔值，没有 TTL 或定时器。正常运行中的清理入口来自受同源、主 frame、当前 generation 约束的桥消息 `auth-required` / `logged-out` | `SessionStore.java:7`、`:21`、`:29`、`:33`；`MainActivity.java:542`、`:575`、`:601`、`:801` |
| 静态资源网络失败不清除授权 | 事实 | `ResourceStore` 返回 `FAILED`；Activity 只显示重试提示，没有调用 `clearAuthorization()` | `ResourceStore.java:230`；`MainActivity.java:694` |
| renderer 退出处理没有履行清理合同 | 事实与合同判断 | `onRenderProcessGone()` 只调用通用失败处理并返回 `true`；稳定资源的 `rollbackTrial()` 返回 `false` 后只 Toast | `MainActivity.java:512`、`:770`；`ResourceStore.java:127`；官方 WebView 终止文档 |
| 通常的 Activity 销毁会销毁 WebView，重新创建会重新建立页面 | 事实 | `onDestroy()` 调用 `destroyWebView()`；`onCreate()` 授权有效时调用 `openSession()`。没有读取 `savedInstanceState` 的页面恢复逻辑 | `MainActivity.java:97`、`:956`、`:978` |
| 未注册原生网络切换监听 | 事实 | Android 原生目录没有 `ConnectivityManager`、`NetworkCallback`、`registerDefaultNetworkCallback` 或 `registerNetworkCallback`；Manifest 中声明 `ACCESS_NETWORK_STATE` 不等于已注册监听 | 本轮定向 `rg`；`AndroidManifest.xml:4` |
| 常见界面配置变化已声明自行处理 | 事实 | Manifest 包含 keyboard、orientation、screenLayout、screenSize、smallestScreenSize、uiMode；launch mode 为 singleTask | `AndroidManifest.xml:19` |
| 当前本地 APK 与上一轮最终产物相同 | 事实 | 当前 Debug APK SHA-256 为 `cdeab7c56c3f4ce971a09fc7c2704b8bd3feb641ccba8916a8aea1158720faca`，与上一轮最终记录相同 | 本轮 `Get-FileHash`；上一轮 `artifact-verification.json` |
| 当前 APK 的 20 项内置网页资源仍与源码一致 | 事实 | APK 内置清单版本 `1a236d8e4a164a33cba2e88f1b820cbac3051bcaaaa12e40a5315a33a1a161c2`；逐项大小、APK SHA-256、清单 SHA-256 和源码 SHA-256 全部一致 | `android-native-evidence.json` |

上述 `onPause`/定时器结论仅限“应用代码没有主动调用这些 API”，不推断用户设备 WebView、操作系统或厂商后台策略的实际调度行为。Android 官方只规定 `WebView.onPause()` 本身不暂停 JavaScript，而 `pauseTimers()` 会暂停所有 WebView 的解析、布局和 JavaScript 定时器；未取得现场证据时不能把平台策略写成确定原因。

Android 官方说明，默认网络变化后，新连接使用新网络，旧默认网络上的已有连接稍后可能被强制终止。因此 Wi-Fi/蜂窝或 VPN 切换是仍需设备证据验证的外部候选。没有原生监听只能证明当前原生层不会发出自定义恢复信号，不能证明 WebView 不会感知网络，也不能单独证明断线根因。

## 已排除解释

- **“原生 onResume 每次都 reload，所以前后台反复断线”**：当前 `onResume()` 没有重载调用。
- **“每隔 15 分钟强制刷新网页”**：15 分钟只控制前台恢复时的资源检查节流；成功结果只保存候选并提示下次启用。
- **“刷新 zrok Cookie 会覆盖设备授权 Cookie”**：两个 Cookie 名不同，官方替换合同不支持该归因；刷新失败也没有清除授权或销毁页面的分支。
- **“选图返回会原生重建页面”**：当前 `onActivityResult()` 只校验 request code 并完成 URI 回调，不调用 `openSession()`。Activity 在选图期间若实际被系统重建是另一条件，尚未取得设备记录。
- **“缺少网络权限”**：当前源码和上一轮最终 APK 权限核验均包含 `INTERNET` 与 `ACCESS_NETWORK_STATE`。未重复归因照片/全库权限。
- **“当前源码或本地 APK 缺少上一轮图片修复”**：当前 APK 与最终交付摘要相同，当前 MainActivity 与最终记录摘要相同；本轮没有把旧包审计结果套用到新包。此结论不覆盖用户设备。
- **“资源下载超时直接就是 WebSocket 超时”**：资源下载器只负责清单内静态文件，API 和事件连接由 WebView 网络处理。未发现原生资源更新调用关闭 WebSocket。

## 异常状态溯源

- 适用约束：在某个 WebView 的 `onRenderProcessGone()` 已被调用后，该实例不能再用于展示网页；应用必须将其从视图树移除、销毁并清理引用，需要继续展示网页时必须创建新实例。
- 约束依据及状态：Android 官方 `WebViewClient.onRenderProcessGone()` 与 `Handle WebView termination`，已确认；本轮无合同冲突。
- 本次运行前提命中判定：**用户现场无法判定**。源码条件分析明确覆盖回调已发生且当前资源不是 trial 的路径；未执行真机 renderer 崩溃试验。
- 场景命中证据：当前 `onRenderProcessGone()` 实现与 `rollbackTrial()` 的非 trial 分支。
- 值判断：在上述条件路径中，`webView` 继续保留已失效实例，违反官方生命周期合同；不能因此断言现场发生过此条件。
- 实际来源与传播路径：当前 WebView → 系统终止回调 → 通用启动失败处理 → 非 trial 回退失败 → 仅 Toast → Activity 仍持有该实例。
- 最后满足约束的状态 → 首次违反点：正常 WebView → renderer 终止后返回 `true` 却没有执行必须的清理。
- 调查边界与未知上游：未知用户设备是否发生渲染进程退出、退出原因及 WebView 版本。该缺陷更可能表现为页面失活或无法恢复；不能单独解释同一页面持续成功断开重连的频率。

## 未闭环项与下一步

- **连接关闭证据**：需要同一设备、同一连接中的 WebSocket close code/reason、error、重连时刻、HTTP 鉴权结果和网页连接状态。用于区分真正关闭、错误状态展示、心跳超时与重复连接。
- **设备版本身份**：需要已安装 APK 身份、Android/WebView 版本，以及当前 origin 的 active/pending/trial 快照；本地 Debug 包一致不证明手机安装相同包，热更新后运行资源也可能不是内置版本。
- **生命周期与 renderer 证据**：需要现有 Android 系统日志中与现象同时间的 Activity 重建、进程死亡及 renderer 退出信息；没有这些信息，不把系统后台回收认定为原因。此项是设备验收需要的证据，不是新增产品日志/遥测功能。
- **网络切换证据**：需要现象前后的默认网络变化或系统网络日志；前台现象也可能伴随网络切换，但目前只是候选，不能量化其贡献。
- **鉴权桥消息来源**：本轮确认原生信任边界和清理行为；网页何时发出 `auth-required` 由网页审计任务追踪。不能由原生清理函数反推服务端授权确实过期。

## 建议的最小修复与验证边界

- renderer 退出缺陷可以独立修复：为 `onRenderProcessGone()` 使用专门处理路径，立即清理传入的失效 WebView、当前引用、ready timeout 和待完成选择器，再使用既有会话打开/错误重试流程重新创建实例。稳定资源也必须走恢复路径，不能把 `rollbackTrial() == false` 作为保留已失效实例的理由。保留授权；不要因 renderer 退出清除 Cookie。
- 不依据本次只读原生审计修改权限、Cookie 期限、全局网络配置、renderer 优先级、Java 设置或添加周期性 reload。没有证据证明这些是现场原因。
- 真机验证应分别覆盖持续前台、切后台恢复、锁屏恢复、选图返回；同时确认页面没有意外重建。renderer 专项可在可控调试设备上按照官方方法验证销毁与新建流程，但本轮没有执行，不能当作已通过。
- 本轮完成的是有界静态审计、官方合同核对与本地 APK 内容校验，不是修复或现场频繁重连验收。

## 官方合同

均在本轮通过官方 Android 文档查询。以下 URL 用于复核合同，源码事实另见上表。

```text
https://developer.android.com/develop/ui/views/layout/webapps/handle-termination
https://developer.android.com/reference/android/webkit/WebViewClient
https://developer.android.com/reference/android/webkit/WebView
https://developer.android.com/reference/android/webkit/CookieManager
https://developer.android.com/develop/connectivity/network-ops/reading-network-state
https://developer.android.com/guide/components/activities/activity-lifecycle
```
