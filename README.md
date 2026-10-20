当前项目 fork 自 [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus)

# CodexElves

<p align="center">
  <img src="docs/images/icon.png" alt="CodexElves 图标" width="128">
</p>

<p align="center">
  中文 | <a href="README_EN.md">English</a>
</p>

<p align="center">
  <img alt="Release" src="https://img.shields.io/github/v/release/junxin367/CodexElves">
  <img alt="Stars" src="https://img.shields.io/github/stars/junxin367/CodexElves">
  <img alt="License" src="https://img.shields.io/github/license/junxin367/CodexElves">
</p>

CodexElves 是 Codex 桌面工作区的增强工具，提供多供应商模型配置、本地协议代理、请求日志、会话管理和手机远程会话。支持带 Codex 工作区的 ChatGPT 桌面应用及 Codex App，通过独立启动器和管理工具使用。

**[下载安装](https://github.com/junxin367/CodexElves/releases) · [效果展示](#效果展示) · [手机远程会话](docs/remote-access-mobile.md)**

## 主要功能

| 功能 | 可以做什么 |
| --- | --- |
| 供应商与模型配置 | 管理官方登录、纯 API、官方混入 API 等接入模式；为模型配置协议、别名和上下文大小 |
| 本地协议代理 | 按模型配置连接 Responses API、Chat Completions 或 Anthropic 上游；Responses 请求直接转发，其他协议按需转换 |
| 请求日志与用量 | 查看请求结果、首字时间、耗时、模型速率，以及完整请求、返回和上游报告的 Token 用量 |
| 会话管理 | 浏览本地会话，批量删除、修复供应商归属；在桌面启用 Markdown 导出和项目移动等增强 |
| 桌面增强 | 按需启用任务看板、插件入口兼容、原生 Fast 入口、工作区快捷打开、会话 Token 统计和用户脚本 |
| Chrome 与图片生成兼容 | 修复部分桌面版本在 API Key 模式下的 Chrome 插件调用问题，以及兼容 API 已返回图片但 Codex 未显示生成结果的问题 |
| 工作区 Checkpoint | 在 AI 轮次开始前保存外部 Git 快照，恢复对应轮次的文件；遵循 `.gitignore`，不写入项目 Git 仓库 |
| 手机远程会话 | 通过网页、PWA 或 Android 连接电脑上的 Codex，查看项目、继续会话、查看执行过程并处理支持的审批和询问 |

模型可用性、上下文容量及工具支持取决于上游服务与桌面版本；添加模型名称不会自动获得该模型的访问权限。

## 安装与开始使用

先安装可正常打开 Codex 工作区的桌面应用，再从 [GitHub Releases](https://github.com/junxin367/CodexElves/releases) 下载适合的平台安装包：

| 平台 | 安装包 |
| --- | --- |
| Windows x64 | `CodexElves-<版本>-windows-x64-setup.exe` |
| macOS Intel | `CodexElves-<版本>-macos-x64.dmg` |
| macOS Apple Silicon | `CodexElves-<版本>-macos-arm64.dmg` |

手机远程会话的内置 zrok 自动下载目前仅支持 **Windows x64 电脑端**；macOS 安装包可用不代表支持相同的公网自动配置流程。

安装后有两个入口：

- **CodexElves 管理工具**：配置供应商、模型、增强功能和远程访问，查看请求日志与运行状态。
- **CodexElves**：静默启动 Codex 桌面应用并加载增强功能，不显示管理面板。

第一次使用建议按以下顺序操作：

1. 打开 **CodexElves 管理工具**。在「供应商配置」选择接入方式；使用自定义 API 时填写自己的 Base URL、Key 和模型列表，并核对每个模型的协议。
2. 需要协议转换时，在对应供应商的编辑页启用「本地代理」，保存配置并选择该供应商的「使用」。使用官方登录时，先在 Codex 桌面应用完成登录。
3. 在「功能增强」按需选择功能并保存。修改启动相关设置后，点击右上角「重启 Codex」，等待主界面就绪。
4. 开始使用桌面会话；通过本地代理发送的模型请求可在「本地代理」页查看日志。仅需要手机访问时，继续阅读 [远程会话移动端指南](docs/remote-access-mobile.md)。

供应商配置会更新 Codex 的配置及认证文件。切换前请保留自己的配置备份；「官方混入 API」仍需有效的官方登录状态，API Key 不能替代官方账号权限。

## 效果展示

以下桌面图片统一为 **1200 × 810**，于 **2026-10-19** 制作。管理器图片来自当前源码预览；Codex 图片使用当前运行时控件与样式的本地演示副本，优化设置弹窗取自当前注入脚本模板。会话、供应商、路径及用量均为演示数据，API Key 保持遮挡；图片不作为真实工具执行、性能测试或端到端兼容验收的证据。可点击图片打开原图查看细节。

### Codex 内的快捷增强

增强入口直接放在 Codex 工作界面中，减少切回管理工具的次数：

| 入口 | 用法与作用 |
| --- | --- |
| Open in | 在会话顶部选择可用的编辑器、文件管理器或终端，打开当前会话的工作目录 |
| Token 信息 | 在右上角置顶摘要附近查看当前会话的累计和最近一轮用量，包括输入、输出、缓存；统计包含关联的子代理 |
| 原生 Fast | 仅为支持 Fast 的 GPT 模型恢复原生入口，非 GPT 模型不开放；实际支持与计费由供应商决定 |
| 提示词优化 ✨ | 在发送前优化输入框中的文字；右键打开模型、思考深度、风格和上下文设置 |

[![Codex 增强入口演示：顶部 Open in、右侧 Token 用量及输入框旁的服务模式和提示词优化按钮](docs/images/showcase/codex-workspace-controls.png)](docs/images/showcase/codex-workspace-controls.png)

「快速打开工作区」「会话 Token 统计」「原生 Fast 入口」在管理工具的「功能增强」中按需启用并保存；原生 Fast 入口保存后需重新启动 Codex 生效；Token 统计需展开桌面的置顶摘要面板。界面入口会随桌面版本适配，图中位置与样式来自当前运行时。

### Open in 打开当前工作区

点击顶部图标可用首选应用打开当前工作目录，旁边下拉菜单可选择其他目标。图中展示 VS Code、Visual Studio、File Explorer 和 Git Bash；实际列表由本机安装情况及 Codex 提供的目标决定。

[![Open in 下拉菜单演示：选择编辑器、文件管理器或终端打开工作目录](docs/images/showcase/codex-open-in.png)](docs/images/showcase/codex-open-in.png)

### 发送前优化提示词

启用功能增强后，在输入框填写草稿，点击 **✨** 优化文字；优化后再次点击同一按钮可恢复原文，再由自己确认发送，不自动提交任务。右键点击 **✨** 可选择模型、思考深度，以及「简洁」「结构化」「编程」风格，并设置是否附带最近一轮上下文。

[![提示词优化设置演示：模型、思考深度、优化风格、最近一轮上下文及系统提示词](docs/images/showcase/codex-prompt-optimize.png)](docs/images/showcase/codex-prompt-optimize.png)

优化会调用所选上游模型，需要可用的供应商配置。图中只展示设置，不包含真实优化请求。

### Chrome 与 imagegen 兼容

| 场景 | CodexElves 的处理与使用条件 |
| --- | --- |
| API Key 模式下无法使用 Chrome 插件 | Windows 下的「API Key 浏览器插件兼容」处理部分版本的 `unsupported Codex auth method: apikey` 及浏览器请求头身份兼容问题。保持该开关启用、保存增强设置并重启 Codex；Chrome 扩展仍需正常安装、连接并具备目标站点权限 |
| imagegen 已返回图片，Codex 却没有显示 | 请求经过本地代理时，兼容上游的图片生成结果，将合法的 PNG/JPEG/WebP 保存到本机并转换为 Codex 可显示的消息。仍需使用支持图片生成的上游、模型和工具；不会给原本不支持的服务增加生图能力 |

Chrome 插件调用兼容与图片结果显示兼容是不同路径。这里介绍的是本项目已实现的适配，本轮文档制作未重新验收 API Key 模式下的插件调用或代理生图。

### 供应商与模型配置

在同一张模型表中分别设置协议、别名和上下文大小，按供应商选择是否启用本地代理。

[![供应商编辑页：接入模式、Base URL、已遮挡的 Key 和模型协议表](docs/images/showcase/manager-models.png)](docs/images/showcase/manager-models.png)

### 本地代理与请求日志

查看代理状态、按模型汇总的速率、协议转换结果及单次请求的耗时；点击「查看」打开详情。

[![本地代理页：运行状态、模型速率和请求日志](docs/images/showcase/manager-proxy-logs.png)](docs/images/showcase/manager-proxy-logs.png)

请求详情展示上游报告的输入、输出、缓存命中、缓存写入和推理 Token，并可展开请求与返回的 JSON。

[![请求详情：Token 用量和可展开的请求、响应 JSON](docs/images/showcase/manager-request-detail.png)](docs/images/showcase/manager-request-detail.png)

### 按需启用桌面增强

在「功能增强」集中管理任务看板、会话操作、插件兼容和桌面界面开关。

[![功能增强页：基础控制、任务看板、会话删除和其他可选功能](docs/images/showcase/manager-enhancements.png)](docs/images/showcase/manager-enhancements.png)

### 手机继续电脑上的会话

手机端的界面演示、首次连接步骤、PWA 与 Android 的区别，以及连接问题处理，单独整理在 **[远程会话移动端指南](docs/remote-access-mobile.md)**。其中提供约 15 秒 GIF 和静态图片，便于查看操作过程。

## 数据与运行方式

CodexElves 通过独立 launcher 启动桌面应用并注入增强脚本。桌面界面升级可能影响注入兼容性；「API Key 浏览器插件兼容」还会备份并适配相关运行时文件，因此部分功能也会涉及运行时文件变更。

| 数据 | 默认位置 |
| --- | --- |
| Codex 配置 | `~/.codex/config.toml` |
| Codex 认证信息 | `~/.codex/auth.json` |
| Codex 会话数据库 | 优先使用 `~/.codex/sqlite/*.db`，兼容旧的 `~/.codex/state_5.sqlite` |
| CodexElves 设置与日志 | `~/.codex-session-delete/` |
| 供应商归属同步备份 | `~/.codex/backups_state/provider-sync` |

管理工具的「设置」页可调整适用的目录配置。API Key、远程访问密钥、设备授权及请求日志可能包含敏感信息，分享日志或截图前请检查内容。删除会话会删除相应数据库记录和 rollout 文件，无法通过归档功能恢复。

## 常见问题

### CodexElves 菜单或增强没有出现

从 **CodexElves** 入口启动桌面应用，或在管理工具右上角点击「重启 Codex」；在「功能增强」确认相关开关已启用并保存。仍有问题时，在「安装维护」检查安装与后端状态。

### 配置了模型，为什么请求仍然失败

先在「供应商配置」核对已使用的供应商、Base URL、Key、请求模型名和协议，再到「本地代理」查看失败请求详情。官方登录、模型 API 权限和第三方工具能力是不同条件；界面显示某个模型不代表上游允许调用。

### 手机上能打开网页，却看不到会话

电脑需保持开机，负责远程访问的 CodexElves 进程和 Codex 桌面应用需保持运行。网页可打开只说明入口可达；还需保存访问密钥、完成手机授权，并等待桌面连接就绪。完整步骤见 [移动端连接与排查](docs/remote-access-mobile.md#连接问题)。

### 更新、退出与平台说明

管理工具的「关于」页提供更新检查。Windows 关闭管理器窗口默认最小化到托盘，退出需使用托盘菜单。macOS 安装包按 Intel 和 Apple Silicon 区分；未签名或未公证的安装包可能被系统拦截，运行前请确认来源与安装包可信。

## 更多文档

- [远程会话移动端指南](docs/remote-access-mobile.md)：面向使用者的连接步骤、GIF 与手机界面说明。
- [远程会话技术说明](docs/remote-access.md)：权限、协议、状态恢复和开发验证记录。
- [Android 构建与运行](docs/android.md)、[Android 与 PWA 发布说明](docs/android-release.md)：移动应用构建、资源更新和发布规则。

## 开发

前端开发需要 Node.js 和 npm，Rust 构建需要相应工具链。Windows 的完整安装包通过仓库根目录的 `build.ps1` 构建。

```powershell
# 管理工具前端检查
cd apps/codex-elves-manager
npm install
npm run check
npm run vite:build

# Rust 检查
cd ../..
cargo fmt --check
cargo check --workspace

# Windows 安装包
.\build.ps1
```

主要目录：

```text
apps/codex-elves-launcher/    静默启动入口
apps/codex-elves-manager/     Tauri 管理工具
apps/codex-elves-android/     Android 应用
assets/inject/               桌面注入脚本
assets/remote-access/        手机网页与 PWA
crates/codex-elves-core/     启动、代理、注入和远程访问
crates/codex-elves-data/     会话数据、导出和供应商归属同步
```
