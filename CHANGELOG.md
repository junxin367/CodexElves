# 更新日志

## 0.2.4 - 2026-07-10

- 模型目录资源同步 OpenAI 官方 `codex-rs/models-manager/models.json`，生成目录按模型读取各自的默认提示词和 `use_responses_lite`，并兼容供应商前缀及快照模型。
- Fast 模式新增 GPT-5.6 系列支持，覆盖 `gpt-5.6`、Sol、Terra、Luna 及对应快照模型；生成的模型目录同步写入 `priority` service tier。
- 兼容 Codex 桌面应用并入 ChatGPT 后的新版外壳：Windows 支持 `ChatGPT.exe` 进程和入口，同时保留 `Codex.exe`；macOS 支持 `ChatGPT.app` 和旧版 Codex bundle 名称。
- Watcher、窗口激活、退出等待和重启流程可识别新版 ChatGPT 进程，并避免把 ChatGPT Classic 或 `resources\codex.exe` CLI 当作桌面应用结束。
- CDP 注入目标兼容 `ChatGPT` 标题和 `app://-/` 桌面应用页面；管理器和文档同步更新应用名称提示。
- 移除已不兼容新版 ChatGPT 的“模型选择优化”功能及其菜单预加载、扁平化注入和配置项。
- 修复新版 ChatGPT 在启用 Fast 控件后，composer 原生隐藏测量节点撑高输入面板并显示滚动条的问题。

## 0.1.12 - 2026-06-30

- Chat Completions 路径补全 web_search MCP 兜底:无 MCP 搜索 fallback 时剥离 web_search 工具,避免 CPA 等第三方 Chat Completions 上游无法执行服务端搜索导致模型调用后空结果死循环。
- 有 MCP 搜索 fallback(tavily/exa)时保留 web_search function,响应方向已能改写成 MCP 工具由客户端执行真实搜索。
- 扩展协议代理回归测试,覆盖 Chat 路径 web_search 在有无 MCP fallback 下的剥离/保留行为。
- 版本号更新到 `0.1.12`。

## 0.1.10 - 2026-06-27

- 压缩/中断续写的会话历史以 function_call 开头时，改用 drop-leading 策略替代补占位 user：丢弃开头悬空的 tool_use/tool_result，按 id 精准剔除合并进同一 user 的悬空 tool_result 并保留真实文本，仅在全部丢空时才补占位 user。
- 扩展协议代理回归测试，覆盖并行 tool_use、合并 user、assistant 纯文本开头、全悬空兑底等多种压缩历史形态。
- 版本号更新到 `0.1.10`，同步 Rust workspace、Tauri 和前端 package 配置。

## 0.1.9 - 2026-06-27

- 修复 Responses 历史转换到 Anthropic Messages 时，`tool_result` 与普通用户文本被合并到同一个 user message 导致上游 400 的问题。
- Anthropic 转换路径新增工具调用 ID 跟踪，孤儿工具输出会降级为普通用户文本，避免生成无前置 `tool_use` 的裸 `tool_result`。
- 修复被中断/压缩续写的会话历史以 function_call 开头时，转换后首条为 assistant[tool_use] 违反 Anthropic “首条必须为 user”规则导致 400 的问题，首条为 assistant 时补一个占位 user。
- 修复使用 Claude 模型时网页搜索死循环：web_search 在无 MCP fallback 时声明为 Anthropic 原生 server-side 工具（`web_search_20250305`），由 Claude 服务端执行搜索，避免被当客户端工具导致空结果反复重试。
- 补充协议代理回归测试，覆盖工具结果隔离、首条补 user、孤儿工具输出降级、web_search server-side 声明与响应转换。
- 安装/更新完成后自动启动 CodexElves 管理工具。
- 新增文档记录协议代理 web_search 行为与 GPT 原生场景的差异（`docs/protocol-proxy-web-search.md`）。
- 版本号更新到 `0.1.9`，同步 Rust workspace、Tauri 和前端 package 配置。

## 1.2.4 - 2026-06-08

- 修复供应商同步在存在多条 `session_meta` 记录时只处理部分会话元数据的问题。
- 修复 Windows 单实例启动保护，在默认端口被异常占用时改用更稳健的锁与端口回退逻辑，降低无法启动的概率。
- 限制 Codex 快速服务档位只对支持的模型生效，避免不兼容模型收到无效配置。
- 修复 macOS DMG 打包和 bundle 结构，恢复 launcher / manager 二进制重命名逻辑。
- 补充混合登录中继模式文档说明。
- 版本号更新到 `1.2.4`，同步 Rust workspace、Tauri、前端 package 和后端展示版本。

## 1.1.8 - 2026-05-26

- 新增上游分支 worktree 支持，可从上游仓库/分支创建和选择独立工作区。
- 新增上游分支列表获取、默认值处理、远端解析和 worktree 创建相关接口与测试。
- 优化供应商同步逻辑，保留 rollout 文件 mtime，减少同步后不必要的会话状态变化。
- 新增独立的「工具与插件」页面，用于统一管理 CodexElves / Codex 的 MCP、skills、plugins，不再绑定到单个供应商。
- 切换供应商时会合并当前启用的工具与插件配置，同时避免把供应商专属配置误写入通用配置。
- 工具与插件列表改为从当前 Codex 配置实时读取启用状态，支持直接开关和删除条目。
- 调整通用配置提取逻辑，改为手动提取，减少自动覆盖和配置污染。
- 修复供应商切换隔离问题，避免 `model_catalog_json`、旧 `model_provider`、历史 provider 表和旧 `auth.json` 被带到新供应商。
- 修复纯 API 模式下 `auth.json` 没有写入 API Key 的问题，并固定供应商 provider 名称为 `CodexElves`。
- 优化模型目录写入方式，支持与原始模型目录合并，并在预览中显示真实路径。
- 供应商配置页新增模型插入方式、模型列表、上下文大小、压缩上下文大小、目标功能等配置项。
- 官方模式下隐藏仅混入 API Key 场景使用的模型列表和模型插入方式。
- 将 Base URL、API Key、上游协议移动到模型列表之前，测试模型和上下文选项收进「更多选项」。
- 修复 `model_reasoning_effort`、`plan_mode_reasoning_effort` 重复写入导致 TOML 解析失败的问题。
- 修复重复插件表、空配置体、布尔值解析等导致配置文件解析失败的问题。
- 优化供应商详情页布局，保持顶部返回和提示区域固定，增大默认窗口尺寸并减少顶部缝隙。
- 移除脚本安装时的 checksum 阻断，避免市场脚本校验不一致导致安装失败。
- 清理关于页和状态页中不需要展示的登录、当前供应商、配置文件路径等信息。
- 调整提示信息居中显示，避免遮挡重启按钮。
- 更新讨论群二维码、README 说明和 macOS DMG 打包脚本。
