# 远程会话 PWA 与 Android 本地资源

本文说明当前远程会话网页、PWA 和 Android WebView 的资源交付方式。Android APK 与网页资源使用不同更新渠道：APK 从 Android GitHub Release 获取，网页资源从用户配置的服务 Origin 获取。

## 服务端资源清单

远程服务提供公开的 `GET /app-resources.json`。当前清单包含以下字段：

| 字段 | 含义 |
| --- | --- |
| `schemaVersion` | 清单结构版本，目前为 `1` |
| `minShellVersion` | 加载这些资源所需的最低原生 Shell 版本 |
| `revision` | 服务端网页资源修订号，来自 `assets/remote-access/resource-revision.txt` |
| `version` | 根据有序资源条目的路径、Content-Type、大小和 SHA-256 计算的整体摘要 |
| `files` | 按路径排序的静态网页文件清单，含路径、Content-Type、字节数和 SHA-256 |

清单不把 API、会话内容或附件列为静态文件。`/app-resources.json` 是清单接口本身，不是清单文件条目；`/api/` 属于保留路径。网页模块、样式、图标和 PWA 文件按服务器与 Android Gradle 的资源表维护。

Android 和 PWA 都校验清单版本、路径、文件类型、大小和摘要。资源地址必须保持服务 Origin；重定向、重复路径、路径穿越、摘要不匹配或客户端不支持的 `minShellVersion` 都会阻止该次更新。

## Android WebView 加载与资源更新

Android 使用用户输入的 HTTP/HTTPS 服务 Origin 作为页面地址。登录只把访问密钥发送到该 Origin 的 `/api/connect`；服务返回的 `codex_elves_device` HttpOnly Cookie 用于后续会话。

APK 内置网页资源作为首次可用快照。安装新 APK 且其内置资源摘要变化时，先试用新内置快照并保留可用的回退版本；不能一直用旧缓存盖住新 APK。Android 的 `MainActivity` 仅对同 Origin 的 GET 请求执行本地静态资源查找：请求路径必须是规范化路径，且存在于当前已校验快照的 manifest 中。其他请求继续使用远程服务或已授权的通道规则；API 和 WebSocket 不会从本地静态快照返回。WebSocket 使用 HTTP 主地址对应的 `ws://` 或 `wss://`。

资源更新按以下顺序处理：

1. 从当前服务 Origin 读取 `/app-resources.json`，检查 `schemaVersion`、`minShellVersion`、`revision` 和资源版本。相同摘要不重复更新；已有有效快照时，候选 `revision` 必须更大，不能仅因同修订号下文件摘要不同就启用新资源。
2. 下载缺少或内容变化的文件到按 Origin 隔离的暂存目录，逐项核对 Content-Type、大小和 SHA-256；全部通过后再校验完整目录并将其移入版本缓存。
3. 完整版本先登记为待启用资源。当前页面继续使用原快照；下次新建页面时才试用待启用版本。
4. 页面报告就绪后，试用状态转为正式当前版本。试用页面未完成启动确认或进程在确认前退出时，恢复上一份完整版本。

下载失败、取消、清单无效或摘要不匹配不会替换当前完整快照。缓存、活动版本、回退版本和待启用版本按服务 Origin 隔离；新进程首次使用对应资源仓库时会清除上次异常退出遗留的 `.staging-*` 目录。

Android 容器不注册网页 Service Worker，以免原生快照和网页缓存并行管理同一批静态文件。Android 资源更新状态和 APK 更新状态也彼此独立。

## 普通网页与 PWA

普通浏览器和已安装 PWA 继续使用当前服务 Origin。登录仍输入访问密钥；Service Worker 的离线资源缓存不保存访问密钥、API 响应、会话消息或附件。经授权的正文展示与草稿由独立客户端存储处理，见 [会话缓存](remote-cache.md)。

`pwa.js` 只在安全上下文且不处于 Android 容器时注册 Service Worker。Service Worker 使用资源清单中的整体版本创建缓存，校验同源静态文件后再提供离线读取；资源清单不支持、完整性校验失败或缓存未就绪时，不把未验证缓存当成可用版本。

对符合严格单层 `*.shares.zrok.io` 格式的 HTTPS 服务地址，普通网页/PWA 负责准备 zrok 公告 Cookie；Android 容器跳过该网页逻辑，由原生层在页面加载前为当前主机设置公告 Cookie。该公告标记不代替设备授权。

## 代码位置

- 服务端资源清单、静态资源路由和资源修订号：`crates/codex-elves-core/src/remote_access.rs`、`assets/remote-access/resource-revision.txt`
- Android 内置资源清单和资源路径表：`apps/codex-elves-android/app/build.gradle`
- Android 快照验证、下载、缓存和回退：`apps/codex-elves-android/app/src/main/java/com/junxin367/codexelves/ResourceManifest.java`、`ResourceStore.java`、`ResourceStatePolicy.java`
- Android WebView 静态路径拦截：`apps/codex-elves-android/app/src/main/java/com/junxin367/codexelves/MainActivity.java`
- 普通网页 Service Worker 与 PWA 行为：`assets/remote-access/pwa.js`、`assets/remote-access/sw.js`

## 历史验证

[2026-10-18 Android 更新历史验证报告](validation/android-update-20261018/README.md)记录了当时的构建、资源完整性及隔离验证，以及没有执行的发布和真机验收。报告中的 APK 版本、摘要、构建结果和验收边界仅代表当时的源代码与环境，不是当前提交的验证结论。
