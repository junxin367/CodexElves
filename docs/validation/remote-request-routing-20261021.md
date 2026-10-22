# 远程会话统一请求选路验收

## 完成范围

- 所有会话业务 API 统一按当前设备、服务实例、连接代次对应的线路延迟选择一个可用入口。覆盖提示词优化及取消、会话读写、审批问答、队列、侧边会话、模型与 Skill、通知、回执及变更列表和详情。
- 图片、缩略图、视频封面和视频内容使用相同选路结果。第二入口媒体链接使用独立只读短期凭证；该凭证不能调用会话 API、写操作或订阅事件，也不能替换为具有写权限的业务凭证。
- 前台 WebSocket 和共享事件 worker 选择一路，延迟变化时重新选择；保留序号去重、缺号补读和失败退避。Android 独立后台通知保留原有双路 SSE 冗余订阅。
- 写操作只提交一次，网络失败、超时或回包丢失后不自动换路重发。普通业务读取也不再双路竞速；只有用于更新延迟的会话测速请求保留双路探测。
- 页面及静态资源继续使用当前入口和已有缓存机制。初次登录尚无通道凭证，继续使用登录入口。
- 旧服务按原有能力回退；旧 Android APK 不把新写操作或媒体发往其原生策略尚未允许的第二入口。

## 验证

- JavaScript 路由验证：477 个隔离场景通过，覆盖 26 种写操作、42 类业务路径、双向入口、延迟变化、未知或不可用线路、单次发送、错误服务身份、取消、非法地址、媒体、事件切换和新旧 Android 能力。
- Android 规则验证：使用真实 `ServiceOrigin.java` 和最小 `Uri` 测试替身进行 Java 编译和执行，340 项检查通过。没有执行 APK 构建。
- 远程服务模块：76 项通过、3 项按既有标记忽略。增加媒体凭证职责区分后，再次运行通道 CORS、媒体授权、WebSocket 与撤销回归，通过。
- 提示词优化集成测试：7 项通过，覆盖 Responses、Chat Completions、Anthropic、上下文、取消和超时。
- Chrome：10 项关键交互通过，使用隔离接口及模拟延迟，不调用真实模型或修改真实会话。提示词优化、历史、变更、图片正文、视频封面和 Range 播放均请求较快入口；业务与媒体凭证分别核对；播放前没有视频内容请求；只读媒体凭证续期后播放位置从 1 秒继续。
- JavaScript 语法、Rust 格式和 `git diff --check` 通过。

浏览器记录位于：

- `C:\Users\junes\.codex\visualizations\2026\10\21\01a15d8c-fc74-7eb1-8f16-3f64af747649\remote-routing-browser.json`
- `C:\Users\junes\.codex\visualizations\2026\10\21\01a15d8c-fc74-7eb1-8f16-3f64af747649\remote-routing-browser.png`

## 编译与热更新

- 正式程序：`cargo build -p codex-elves-manager --bin codex-elves-manager --release` 成功。
- 程序大小：59116544 字节；SHA-256：`69123da064de2207c63da73e68fe1ebd870ce9ac68a854628f2104c75a9568fe`。
- 新程序完整嵌入当前 `app.js`、`channels.js`、`attachments.js`、`session-tools.js`、`markdown-renderer.js`、`platform.js` 和 `style.css`。
- 后台从 PID `45040` 切换至 `44052`，原生连接正常，原有 3 台设备授权和设置摘要保持一致。
- 运行目录：`C:\Users\junes\.codex-session-delete\remote-access-runtime\remote-routing-20261021-69123da064de`。上一轮视频控件运行目录保留用于回退。
- 本机与公网资源版本均为 `72aeb89a167b86958c3471e4186ab25df4937149ff1e8c609a42ec9e3937be42`；23 项资源清单均与源码一致，7 项实际下载的修改资源与构建一致。
- 两处匿名会话及优化提交均返回 401；以媒体授权方案访问会话 API 返回 403。没有使用真实设备的写操作验证。

运行目录保存构建、启动、浏览器和服务检查结果，不保存管理令牌、设备 Cookie 或通道凭证。

## 交付边界

- 网页及后台已热更新，重新打开远程页面后加载新网页资源。
- Android 原生通道与媒体策略、`app-info.requestRoutingVersion` 已修改，但尚未构建或安装新版 APK，未进行真机验收。旧 APK 保持兼容范围，完整写请求及媒体择优需要新版 APK。
- 未修改桌面或 Android 版本号，未提交、推送或发布 Release。
- 本轮临时验证源文件已清理；保留正常编译产物、运行程序及验收证据。
