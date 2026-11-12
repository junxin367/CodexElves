# Windows 远程客户端

`CodexElves-Remote-<版本>-windows-x64.exe` 是独立的 Windows x64 远程会话客户端。把 EXE 复制到另一台 Windows 电脑即可启动；需要已安装 Microsoft Edge WebView2 Runtime。客户端不需要安装 CodexElves 管理器。

## 连接

1. 在服务端电脑的 CodexElves 中启用远程会话，取得可从客户端电脑访问的服务首页地址。
2. 启动 EXE，在“服务地址”填写 HTTP 或 HTTPS 地址；访问密钥在连接后的原有登录页输入。
3. 按远程网页流程完成配对。客户端独立保留登录状态，之后启动自动恢复上次成功连接的地址。

服务端电脑和远程后台需要保持运行。客户端使用服务端提供的会话网页，能力和权限沿用该网页；仅支持 Android 原生桥的功能仍由 Android 提供。

地址无效时输入框保持可编辑。DNS、网络或证书等加载失败会返回连接页，保留本次输入并显示错误；失败地址不会替换已保存的成功地址。需要更换地址时，从托盘菜单打开“连接设置”；刷新当前页面也使用托盘菜单。

## 窗口与托盘

最顶部标题栏固定为 36 个逻辑像素，按 Windows 显示缩放比例显示。背景、文字及底部分隔线跟随会话页面的明暗模式、主题与自定义颜色，修改外观后立即同步。标题栏支持拖动、最小化、最大化和还原，窗口可缩放并记住位置与大小。关闭按钮将窗口收进托盘；点击托盘图标或再次启动同一 EXE 可唤回原窗口。窗口不显示“连接”“窗口”菜单栏；托盘菜单提供打开、收起、最大化／还原、连接设置、刷新与退出。

PC 布局中“文件”“变更”在右侧标签面板打开，可以同时保留多个文件及差异，聊天继续可用。拖动面板左侧边界调宽，标签可切换和关闭；网页与 EXE 共用此界面，更新服务端网页后使用相同行为。

配置默认位于 `%LOCALAPPDATA%\com.codexelves.remote\`，其中 `settings.json` 保存成功连接地址和窗口状态，`webview\` 保存独立登录缓存。它与管理器及普通浏览器的数据分开。开发验收可用 `CODEX_ELVES_REMOTE_DATA_DIR` 指向独立的绝对路径目录。

## 构建

在仓库根目录执行：

```powershell
.\scripts\build-windows-remote.ps1
```

仅构建客户端，无需前端 npm 构建，也不会构建 Android 或启动任何服务。产物为：

- `dist/windows/CodexElves-Remote-<版本>-windows-x64.exe`
- `dist/windows/CodexElves-Remote-<版本>-windows-x64-build-info.json`，记录版本、大小及 SHA-256。

版本取根 `Cargo.toml`；默认使用本地增量 `release-fast`，正式优化构建可加 `-Profile release`。常规 `scripts/build-windows-installer.ps1` 也会额外生成该便携 EXE，管理器安装包仍单独交付。GitHub Windows 构建和发布流程提供相同的独立客户端产物。

客户端源码位于 `apps/codex-elves-remote/`。本地标题栏和远程内容使用不同的 WebView；窗口控制只授权本地标题栏，连接设置命令只授权内置连接页。远程网页仅可同步严格校验的 RGB 配色，本机还会核对其为当前配置的服务来源；其他来源的链接交给默认浏览器打开。
