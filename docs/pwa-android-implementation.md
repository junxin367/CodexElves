# 远程会话 PWA 与 Android 本地资源实现

日期：2026-10-16。本文记录本次实施边界和主代理确定的接口约定。

## 已确认的产品行为

- 网页和 PWA 登录只输入访问密钥，沿用当前访问域名。
- Android 未登录时显示域名和访问密钥；输入后登录。
- Android 保存当前连接的域名及设备会话，不保存明文密钥。授权仍有效时再次打开直接加载会话页面。
- Android 使用本地网页资源渲染，接口和消息连接访问用户填写的同一个域名。
- Android 从该域名后台检查、下载网页资源更新；不依赖 GitHub 下载网页资源。
- APK 由用户从 GitHub 手动下载安装。当前任务完成构建能力，不自动发布。
- 当前只保存一个服务连接，不增加多服务器管理、推送或离线发送功能。

## 实施决定

### 资源版本协议

新增公开的 `GET /app-resources.json`，仅描述已经公开的网页静态资源：

```json
{
  "schemaVersion": 1,
  "minShellVersion": 1,
  "version": "<64 位小写 SHA-256>",
  "files": [
    {
      "path": "/",
      "contentType": "text/html; charset=utf-8",
      "size": 123,
      "sha256": "<原始未压缩内容的 SHA-256>"
    }
  ]
}
```

- `files` 按路径排序，包含页面运行需要的全部静态模块、样式和图标；不包含本清单自身、`native.js`、API、会话内容或附件。
- 整体 `version` 对排序后的各文件路径、内容类型、大小和摘要生成稳定摘要；相同内容保持版本不变。
- 下载仍使用清单给出的同源路径；客户端限制根相对路径、拒绝重复路径、跳转到其他域名及路径穿越。
- 每个文件校验大小和 SHA-256；下载期间电脑端更新造成摘要不符时放弃整次更新，继续当前版本，后续再检查。
- 清单使用重新验证策略；静态资源沿用 gzip/ETag，动态接口继续禁止缓存。
- `schemaVersion` 或 `minShellVersion` 超出客户端支持时不启用新版资源。

### Android

- 新建 `apps/codex-elves-android`，使用 Java、Android WebView 和 AndroidX WebKit；原生层负责域名登录、本地资源及下载更新。
- 首版内置从同一 `assets/remote-access` 生成的网页资源。WebView 的页面地址使用用户填写的 HTTPS Origin；只拦截清单内的静态路径并返回本地文件，API/WebSocket 继续同源访问真实服务。
- 不放开服务端跨域和来源校验。原生登录的 `Origin` 使用目标服务 Origin，将服务器签发的 HttpOnly 设备 Cookie 写入 WebView CookieManager。
- 支持省略 `https://` 的域名输入，规范化为 HTTPS 根地址；不接受用户名、密码、query、fragment 或非根路径。
- 页面不持有原始登录密钥。仅在登录请求期间使用，成功后清空输入和内存引用。
- 网页与原生层使用受 Origin 约束的 `CodexElvesAndroid.postMessage(JSON.stringify({type}))` 通道，只接受主 frame 的固定消息：`ready`、`authenticated`、`auth-required`、`logged-out`。不提供文件、命令、任意网络或任意脚本执行桥接。
- `ready` 表示网页模块已成功加载；`authenticated` 表示设备会话已通过服务校验；`auth-required` 和 `logged-out` 返回原生域名与密钥入口。网络暂不可用不清除设备授权。
- 原生层先显示现有资源；后台下载至独立暂存目录，完整校验后记录为待启用版本。下次新建页面时使用新版本，不刷新用户正在操作的页面。
- 保留上一个完整资源版本；网页启动失败时回退。资源、更新任务和持久化状态都按规范化 Origin 隔离，切换域名或退出后的迟到结果不得覆盖新连接。
- 页面内浏览器 Service Worker 在 Android 容器内不注册，避免与原生缓存形成两套更新流程。
- App 只在自身运行时做后台检查，不新增常驻 Android 后台服务。
- 对 HTTPS 且严格单层的 `*.shares.zrok.io`，原生层在加载页面前写入当前主机的 zrok 公告 Cookie，等待写入完成再加载；回前台时刷新。该标记不代替设备授权，不扩大 Cookie 域范围。
- 新进程首次创建资源存储时清理以前异常退出遗留的 `.staging-*`，保留完整版本；同进程 Activity 重建不再次执行，避免删除正在下载的目录。

### 网页与 PWA

- 新增 `/manifest.webmanifest`、图标、`/pwa.js`、`/platform.js`、`/sw.js`。
- manifest 使用同源 `id`、`start_url`、`scope`，以独立窗口显示。
- Service Worker 只缓存清单内的公开静态资源，绝不缓存 `/api/`、密钥、消息、图片附件或 POST 请求。
- 按完整资源版本缓存，安装失败不替换旧 worker；采用正常等待激活，不强制刷新已打开页面。
- 网页和 PWA 保持现有密钥登录。Android 的会话恢复失败区分授权失效与暂时断网；断网提供重新连接，不误退回登录。
- PWA 在初始化、恢复页面、回前台和恢复网络时同步刷新当前 zrok 主机的公告 Cookie，再执行会话恢复请求。Android 容器跳过这段逻辑，由原生层处理。

## 并行编码所有权

1. 服务端代理：`crates/codex-elves-core/src/remote_access.rs` 的资源与路由部分，以及必要的既有测试；提供资源协议、静态路由和 HTTP 回归证据。
2. 网页代理：`assets/remote-access` 网页/PWA 相关资源及 `scripts/dev-remote-access.mjs`；提供浏览器缓存、登录和容器协作行为。
3. Android 代理：`apps/codex-elves-android`、专用 Android 构建脚本及必要的 Android 忽略规则；提供客户端、构建和本地资源更新验证。

主代理负责接口决策、跨部分集成检查及最终验收。所有代理禁止提交、推送、修改无关文件、再次派发代理或使用 PAL。本次新建的临时测试文件验证完成后删除。

## 验收范围

- 公网静态资源清单完整、摘要准确、版本随资源内容变化，未登录 API 仍为 401。
- 网页/PWA 只有密钥入口；manifest 和图标可加载；离线缓存只包含公开资源。
- Android 域名与密钥登录、重开恢复、授权撤销、网络失败恢复、同源 Cookie 和消息连接。
- 更新失败、摘要错误、版本不支持和下载中断不破坏当前资源；新版不在当前会话中途激活。
- Android 构建和静态检查、Rust 定向测试、JavaScript 检查；区分构建验证与实体 Android 运行验收。

## 本轮验证结果

- Rust 远程访问相关 5 项测试通过，覆盖资源清单、内容摘要、资源版本、未授权访问及原有设备授权恢复；最终资源清单定向测试再次通过。
- 真实 Chromium 中 manifest 解析无错误，Service Worker 激活，缓存 18 个公开资源且不含 API；缓存首页保留 CSP，断网重载仍能显示登录页。
- 隔离网页验证 Android 桥的成功恢复、401、退出和断网重试，普通网页仍只有密钥入口。
- Android 资源清单、版本回退和提交条件专项通过；zrok Cookie 域名边界、异步回调顺序、孤立暂存目录回收及活动目录保留专项通过。临时测试源码和临时测试依赖均已移除。
- 对当前配置的 zrok 入口进行了无密钥 HTTP / WebSocket 握手对照：缺少公告标记时返回提示 HTML；带公告标记时到达后端并返回未授权 JSON，设备鉴权仍生效。
- Debug 构建、未签名 Release 构建、R8 及两种 Lint 通过。Lint 剩余一项图标目录版本限定冗余提示，无错误。
- 最终调试 APK 通过 APK v2 签名验证，内置 18 个资源与当前源码的大小和 SHA-256 全部一致。
- 独立复核发现的 zrok 入口兼容和孤立暂存目录问题均已修复，定向复核通过。
- GitHub 手动构建工作流已通过 YAML 和配置静态校验，本轮未触发远程工作流、未上传 APK。

调试产物：`dist/android/CodexElves-0.5.0-android-debug.apk`，大小 `4,718,659` 字节。

```text
APK SHA-256:
bfeced8644b24757a05bebd07acd18e6929dd5d846f8b1be8830297ddab75891

内置生产网页资源版本:
2a5ffcad625c9e0459c2a6a57cea810390d4a5991ecc4180da3936bfc0a5be99
```

开发预览脚本会替换首页标题，因此其整体资源版本与生产版本不同；比较交付资源时应逐项核对内容，而非直接比较预览清单版本。

本轮没有实体 Android 或模拟器端到端验收，真实设备上的 WebView、Cookie 持久化、键盘及文件选择仍需验证。电脑端运行中的程序没有替换；需要更新电脑端程序后，公网入口才会提供新增 PWA 和资源清单。正式 APK 需配置项目签名证书，操作步骤见 `docs/android-release.md`。
