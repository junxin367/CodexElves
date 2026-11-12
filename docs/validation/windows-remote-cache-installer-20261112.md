# Windows 远程客户端缓存与安装包验收

日期：2026-11-12，Asia/Shanghai。版本：0.7.0。网页资源修订：2026111242。

## 交付行为

- Windows 安装包包含 launcher、管理器、任务看板和独立远程客户端。桌面与开始菜单的 CodexElves 分组均创建“CodexElves 远程会话”快捷方式，直接启动 `codex-elves-remote.exe`。卸载清理程序和快捷方式，保留独立用户数据。
- 客户端内置公开网页资源，按服务 Origin 保存校验后的当前包及上一包。完整更新在下次导航启用，当前页面不自动刷新；前台请求期间暂缓后台资源下载。API、上传、登录和 `/native.js` 不进入缓存。
- 无保存地址时，只检查当前用户管理器配置的回环端口及默认端口，验证公开资源清单后预填；已保存地址启动时自动打开，已有有效 Cookie 继续复用。迟到的探测结果不能覆盖手动输入。
- 连接页已删除用户指定的两段窗口和托盘说明，原有窗口与托盘功能保留。

## 自动检查

`cargo test --locked -p codex-elves-remote --lib`：16 项通过。覆盖地址与权限边界、配置持久化、回环地址候选、清单和资源摘要校验、大小限制、API 与 Origin 隔离、gzip 更新、完整包切换、磁盘损坏校验和离线重开；同时核对公网隧道跳过提示页请求头的域名边界。

`node --test scripts/remote-access-ui-state.test.mjs scripts/workspace-tabs.test.mjs scripts/windows-remote-titlebar.test.mjs scripts/windows-remote-cache.test.mjs`：315 项通过。新增验证预填地址与保存地址的优先级、输入竞态、失败恢复、HTTP 下原生资源缓存、前台繁忙与空闲通知及手动更新。

管理器 `npm run check` 通过，包括 23 个远程 JS 文件语法、import、服务路由及 TypeScript 检查。远程客户端 Rust 格式检查、两份 PowerShell 构建脚本语法检查和 `git diff --check` 均通过。

## Windows WebView2 实际运行

使用独立应用标识和合成测试数据目录，隐藏启动测试客户端两次。测试 HTTP 服务只提供公开清单与 API，不提供 HTML、JS、CSS；测试未访问用户的实际会话或授权数据。

两次启动均确认原生缓存启用，分别从本地提供 17 个网页资源，请求 API 仍到达测试服务器。第一次响应设置合成持久 Cookie，第二次启动服务器成功收到该 Cookie，验证缓存初始化没有清除登录状态。

此记录验证实际 WebView2 资源拦截与登录数据保留；未在用户机器执行安装或卸载。

## 最终构建

通过 `scripts/build-windows-installer.ps1` 构建成功，退出码为 0，完成时间为 2026-11-12 17:17:02 +08:00。本次进程临时跳过 Cargo 缓存清理，未修改持久环境设置。

构建输入检查确认管理器与客户端内嵌的远程网页、原生桥和注入资源与工作区一致。四个程序在 `target/release-fast`、`target/release` 和 `dist/windows/app` 中的 SHA-256 一致；便携客户端与安装输入中的客户端也一致。

使用 NSIS 的 `/PPO` 展开脚本，并以 UTF-8 核对客户端 File 指令、两个中文快捷方式及对应卸载指令。安装包由 NSIS 正常压缩生成；此检查没有执行安装程序。

| 产物 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `dist/windows/CodexElves-0.7.0-windows-x64-setup.exe` | 53,280,578 | `911f4c4df7dbcc449add397e2744e04e54e4519c90564c57ff01f4c7a1df2a39` |
| `dist/windows/CodexElves-Remote-0.7.0-windows-x64.exe` | 22,978,048 | `ffb99472320b54f0b45719ba5fe0b4b5c5f0e6fb03b9a5aab1bf5d24653a1193` |

本次仅生成本地 Windows 产物，没有应用正式服务更新、构建 Android、推送新提交或发布版本。
