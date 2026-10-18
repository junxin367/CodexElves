# CodexElves 项目工作规则

## 适用范围

本文件适用于整个仓库。处理 `BigPizzaV3/CodexPlusPlus` 原仓库提交、冲突解决、重命名维护、协议代理、管理器 UI、安装包构建、GitHub 发版及发布验收时，必须优先遵守本文件。

## 核心原则

保留 CodexElves 的本地产品形态。合并上游只能带入仍然有价值的 bugfix、兼容性修复和安全修复，不能恢复已移除功能，不能把项目改回 CodexPlusPlus / Codex++ / codex-plus 命名，不能破坏本地代理和管理器交互改造。

## 上游合并流程

1. 合并前先运行 `git fetch upstream main`，再用 `git log --oneline HEAD..upstream/main` 查看新增提交。
2. 对每个上游提交先看 `git show --stat` 和补丁内容，只合入有效修复；遇到品牌、路径、版本或已移除功能相关变更时，手工映射到当前代码。
3. 如果上游提交仍使用 `codex-plus-*` 路径，必须映射到当前 `codex-elves-*` 路径，不能直接接收上游目录重命名。
4. 如果上游提交只改版本号、上游品牌、赞助/社区/推荐内容、Zed、mobile relay，默认跳过。
5. 如果用手工 cherry-pick 方式合入修复，需要再用 `git merge -s ours --no-ff upstream/main` 记录上游基线，避免后续重复出现同一提交。
6. 合并过程中不要暂存 `.zeroone/hook-debug/stop-await-submission.ndjson`，这是本地钩子日志。

## 项目命名硬约束

必须保持以下命名：

- 产品名：`CodexElves`
- GitHub 仓库：`https://github.com/junxin367/CodexElves`
- Rust crate / workspace 包名：`codex-elves-*`
- launcher 产物：`codex-elves.exe`
- manager 产物：`codex-elves-manager.exe`
- Windows 安装包：`dist/windows/CodexElves-<version>-windows-x64-setup.exe`
- 管理器包名：`codex-elves-manager`

禁止把任何运行时代码、构建配置、安装脚本、README、用户可见 UI 改回：

- `CodexPlusPlus`
- `Codex++`
- `codex-plus`
- `codex_plus`
- `codex-plus-plus`
- `codex-plus-manager`

允许的品牌关键词例外如下：

- 必须保留 `README.md` 顶部 fork 来源说明：`当前项目 fork 自 [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus)`。该行用于说明项目来源，不属于产品品牌回退；不得删除、改写或移动到不醒目位置。
- 脚本市场（插件市场）数据源地址以用户指定的 fork 源为准，当前固定为 `https://raw.githubusercontent.com/BigPizzaV3/CodexPlusPlusScriptMarket/main/index.json`，不得擅自修改或改回其它仓库。

- 该地址是用户明确要求保留的 fork 源插件市场，属于外部第三方数据源仓库名，**不属于本产品品牌**；因此其中出现的 `CodexPlusPlus` 字样是有意保留的例外，不受上述品牌硬约束限制。
- 涉及文件：`crates/codex-elves-core/src/script_market.rs` 的 `DEFAULT_MARKET_INDEX_URL`。
- 品牌关键词扫描时必须排除 `README.md` 顶部 fork 来源说明和脚本市场数据源地址，不能因这些有意保留的来源信息命中 `CodexPlusPlus` 就判定违规。

## 已移除功能禁止回流

合并或修复时不得恢复以下功能、入口、资源、文案或测试断言：

- 概览页顶部官方中转站推荐
- 推荐内容
- Zed 相关功能、配置、测试和文案
- 请作者喝咖啡、赞助商、赞助二维码、赞助图片
- Discord 社区、Telegram 频道
- README 中的赞助商、交流与支持、友情链接
- mobile relay / 移动端中继相关 app、crate、入口和测试
- 已废弃功能的测试断言

如果上游修复依赖这些模块，先提取可复用的纯修复逻辑，不能把模块整体恢复。

## 本地改造必须保留

合并后必须保持以下本地行为：

- 桌面端及管理器版本号以根 `Cargo.toml` 的 `workspace.package.version` 为准，不得因合并上游而被上游版本覆盖。Android 版本独立维护于 `apps/codex-elves-android/version.properties`，不随管理器版本同步；除非用户明确要求，否则不主动改显示版本。Android 每次正式发布必须递增 `versionCode`，并保持现有正式签名证书。
- 默认本地代理端口保持当前项目端口，不恢复旧端口 `57321`。
- 管理器关闭按钮默认最小化到托盘；再次打开 manager 时如果在托盘中，必须显示主窗口。
- manager 打开时保持上一次窗口位置，不能跨显示器跳回旧位置。
- 设置页“界面主题”只删除 UI，不删除底层功能。
- 工具与插件页背景高度必须随内容撑开，不能固定高度导致内容溢出。
- 会话管理页同步目标和删除提示文案保留当前间距优化。
- 快捷入口中文不能乱码；NSIS 编译必须使用 `/INPUTCHARSET UTF8`。
- manager 左上角 logo 和 ico 使用当前项目 icon 资源。

## 协议代理和模型配置约束

不得破坏当前协议代理设计：

- 用户通过“启用本地代理”显式开关控制代理，不依赖上游协议字段暗示是否走代理。
- 模型列表按协议分别维护：Responses API、Chat Completions、Anthropic。
- 展示模型列表时聚合排序，但配置仍保留协议归属。
- Responses API 模型请求不得被翻译成 Chat Completions；必须直接调用 Responses 上游 Base URL。
- Chat Completions 和 Anthropic 转换逻辑必须保留，并用协议测试覆盖。
- 显式模型协议配置优先，不得批量改写已有模型的显式协议；缺少归属时按模型名称推断：GPT/OpenAI 系列使用 Responses API，Claude 和其他非 GPT 模型默认使用 Anthropic。空模型名不能用于协议推断。
- 思考深度按模型能力处理：GPT 类最高 `xhigh`，Claude 类最高 `max`，其他模型按能力表处理，不能用单一固定等级覆盖全部模型。
- 保留常见模型上下文大小表；模型列表新增或加载模型时优先使用已填写值和已知表值，仍缺少上下文大小时默认填入 `1000000`，不得覆盖已有手动值。
- 模型配置 UI 保持表格交互：请求模型、协议、上下文大小、删除 icon；协议为下拉选择。

## UI 注入功能约束

不得破坏当前 Codex App 注入增强：

- 使用 `CodexElves` 插件入口和菜单命名。
- 纯 API 模式下仍要恢复可用的 fast/service tier 控制位置。
- 请求模型下拉层级必须高于周边面板，点击空白处不能误触模型列表。
- 插件入口、工具与插件、会话管理、模型配置等现有交互不能因上游 UI patch 回退。

## 构建约定

用户说“编译”或“编译安装包”时，默认执行：

```powershell
.\build.ps1
```

该脚本必须完整生成：

- `target/release/codex-elves.exe`
- `target/release/codex-elves-manager.exe`
- `dist/windows/app/codex-elves.exe`
- `dist/windows/app/codex-elves-manager.exe`
- `dist/windows/CodexElves-<version>-windows-x64-setup.exe`（`<version>` 取自根 `Cargo.toml` 的 `workspace.package.version`）

不要改回手工 NSIS 命令作为默认流程。需要修改构建流程时，优先更新 `build.ps1` 和 `scripts/build-windows-installer.ps1`。

### Android 同步编译

- 用户要求“编译”或“编译安装包”时，必须判断 Android 是否较上次成功正式构建发生变化；不能只看未提交的 Git 差异。
- 检查范围包括 `apps/codex-elves-android/` 下的源码、资源、独立版本和构建配置，`scripts/` 中的 Android 构建脚本，以及 APK 内置的 `assets/remote-access/` 网页资源；排除构建产物、工具链、缓存和本机私有配置。
- 相关内容有变化、没有成功构建基线，或上次 APK 缺失、摘要不符时，同步执行 Android **Release** 构建及 lint，生成 `dist/android/CodexElves-<androidVersion>-android.apk`。`<androidVersion>` 取自 Android 独立版本配置。
- 相关内容没有变化且 APK 校验有效时跳过 Android 构建，并说明跳过原因。用户明确只编译 Android 时直接执行 Android 构建。
- 复用既有正式签名材料；缺少签名配置时明确报告 Android 构建失败，不得创建替代证书或以 Debug APK 交付。Debug 仅用于明确的开发验证或不持有发布密钥的 PR 检查。
- 只有正式 APK 构建及校验成功后才能更新本机 Android 构建基线；失败不得记录为已构建。
- Android 发布使用独立 `android-v<androidVersion>` 标签和更新清单，不覆盖桌面版 `latest.json` 或 GitHub Latest 标记。

### GitHub 发版与 Android 发布验收

- 用户要求“发布”“发版”或“推送发布标签”时，必须分别检查桌面和 Android 的交付状态；用户明确限定发布平台时遵守指定范围。
- 判断 Android 是否需要发布时，以上一次**已公开且 APK、更新清单校验有效的 Android Release**为基线，比较上述 Android 同步编译范围；不能只看未提交差异、上次本机构建或 Actions 构建。相关内容有变化、尚无有效 Android Release，或远端附件缺失、校验失败时，必须将 Android 列入本次发布；内容未变化且远端附件有效时可跳过，并说明依据。
- 按下表区分构建和发布，不得因 Actions 总体成功或桌面 Release 已发布就认定 Android 已发布：

| 触发方式 | Android 实际交付 | 可报告状态 |
| --- | --- | --- |
| `main` 推送 Android 相关变更 | 正式签名构建、lint，上传 Actions Artifacts | 已构建未发布 |
| 推送桌面版本标签，如 `v<desktopVersion>` | 触发桌面发布，不触发 Android 独立发布 | 单独核对 Android 状态 |
| 手动运行 Android Release，`tag` 留空 | 仅上传 Actions Artifacts | 已构建未发布 |
| 推送 `android-v<versionName>`，或手动运行 Release 并填写该标签 | 正式构建后执行独立 Android 发布任务 | 完成下述验收后才可报告已发布 |

- 本次发布已获用户授权且 Android 需要发布时，按 Android 独立版本规则核对 `versionName`、递增的 `versionCode` 和既有正式签名，创建并推送对应的 `android-v<versionName>` 标签，或对已存在的正确标签触发 Release 工作流；不得遗漏 Android 标签，也不得移动已发布标签或覆盖不同内容的已发布 APK。具体步骤遵循 `docs/android-release.md`。
- 等待 Android 工作流结束，单独确认 `Publish independent Android Release` 任务成功；该任务为 `skipped`、失败或仍在运行时，不得报告发布完成。
- 发布后确认对应 Android Release 已公开，包含 `CodexElves-<versionName>-android.apk` 和 `android-update.json`；下载两项附件，核对版本、`versionCode`、APK 大小和 SHA-256 与更新清单及本次正式构建一致，并确认桌面 Latest 标记和 `latest.json` 未被 Android 发布覆盖。
- 最终分别报告桌面和 Android 状态：`已发布`、`已构建未发布`、`进行中`、`失败/受阻` 或 `跳过（附依据）`，并提供对应运行记录、Release 或产物位置。只要求检查、诊断或本地编译时，不得擅自创建、推送发布标签或触发远程发布；发现未发布应明确列为待完成项。

## 合并后的强制检查

合并上游或改名相关变更后，至少执行：

```powershell
cargo fmt --check
cargo check --workspace
npm run check
cargo test -p codex-elves-core --test launcher
cargo test -p codex-elves-core --test protocol_proxy
git diff --check
```

合并后必须做关键词扫描。运行时代码、README、安装脚本、资源路径中不得出现旧品牌或已移除功能关键词：

```powershell
# 注：脚本市场数据源 script_market.rs 和 README.md 顶部 fork 来源说明含有意保留的 fork 来源名，不算违规。
git grep -n -E "codex-plus|codex_plus|CodexPlusPlus|Codex\\+\\+" -- apps crates assets scripts README.md README_EN.md Cargo.toml ":(exclude)crates/codex-elves-core/src/script_market.rs" | Select-String -NotMatch "^README\\.md:1:当前项目 fork 自 \\[BigPizzaV3/CodexPlusPlus\\]\\(https://github\\.com/BigPizzaV3/CodexPlusPlus\\)$"
git grep -n -i -E "discord|telegram|官方中转站|推荐内容|请作者|咖啡|mobile.?relay|(^|[^a-z])zed([^a-z]|$)" -- apps crates assets scripts README.md README_EN.md Cargo.toml
git grep -n -i -E "sponsor|赞助" -- README.md README_EN.md apps/codex-elves-manager/src apps/codex-elves-manager/src-tauri crates assets scripts
```

允许依赖锁文件里出现第三方包的 sponsor 元数据；不允许用户可见文案、资源或运行时代码恢复赞助功能。

## 停止条件

出现以下情况时停止合并并说明原因：

- 上游修复无法和 CodexElves 重命名共存。
- 上游修复必须恢复已移除功能才能编译。
- 协议代理改造测试无法通过。
- 构建脚本无法生成 `dist/windows` 安装包。
- 需要恢复 `.zeroone`、历史工作树或用户未授权文件变更。
