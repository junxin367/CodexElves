# 远程会话网页架构

远程会话网页位于 `assets/remote-access/`，是不经构建步骤直接运行的原生 ES module。同一份文件同时用于：本机远程服务（Rust 构建期内嵌）、浏览器 PWA（Service Worker 按版本清单缓存）、Android APK 内置网页包和热更新、Windows 独立远程客户端内置网页包。

原入口 `app.js` 约 1.8 万行，现拆分为 62 行的入口和 `ui/`、`core/`、`features/` 下的 64 个模块（最大模块约 1100 行）。拆分只移动代码和改写模块间的状态写入方式，不改变页面行为；原有注释（含用户需求注释）全部随代码迁移。

## 目录与分层

| 层 | 位置 | 职责 | 允许依赖 |
| --- | --- | --- | --- |
| 入口 | `app.js` | 按固定顺序调用各模块的启动函数，最后发起连接 | 全部 |
| 功能 | `features/` | 会话、侧栏、输入区、正文、队列、目标、外观、设置等业务功能 | `features/`、`core/`、`ui/`、根目录独立模块 |
| 基础 | `core/` | 跨功能共享状态 `state.js`、存储实例 `stores.js`、工具 `util.js` | `core/`、`ui/`、根目录独立模块 |
| 通用组件 | `ui/` | 不含会话业务的界面组件 | `ui/`、根目录独立模块 |
| 独立模块 | 根目录其余文件 | `channels.js`、`platform.js`、`pwa.js`、`markdown.js`、`session-menu.js`、`session-tools.js` 等既有模块 | 只能依赖根目录独立模块和 `ui/` |

`features/` 按领域分目录：

| 目录 | 内容 |
| --- | --- |
| `session/` | 实例目录 `instances.js`、会话能力 `capabilities.js`、连接与线路延迟 `connection.js`、连接生命周期 `lifecycle.js`、登录 `login.js`、关联/侧边会话 `related.js`、工作区侧边面板 `side-panel.js`、会话展示缓存 `thread-cache.js` |
| `sidebar/` | 列表渲染 `list.js`、会话操作与偏好 `actions.js`、类型筛选与项目显示上限 `filters.js` |
| `composer/` | 输入区 `composer.js`、草稿 `drafts.js`、编辑器 `prompt-editor.js`、模型与 Fast `models.js`、新建会话 `new-chat.js`、权限 `permissions.js`、Skill 与斜杠命令 `skills.js`、输入区菜单 `composer-menus.js`、用量 `metrics.js`、Claude 额度 `claude-usage.js`、提示词优化 `prompt-optimization.js` |
| `conversation/` | 会话选择 `selection.js`、历史读取 `history.js` 与渲染 `history-view.js`、实时事件 `events.js`、待发送 `pending.js`、上下文压缩 `compaction.js`、过程事件 `transcript-events.js` |
| `messages/` | 正文条目 `items.js`、用户消息 `user-content.js`、消息编辑 `message-edit.js`、媒体 `media.js`、工具输出 `tool-output.js`、子代理 `subagents.js`、活动分组 `activity.js`、变更文件 `edited-files.js` |
| `queue/` | 排队消息 `queue.js`、排队编辑 `queued-edit.js`、发送显示 `sent-display.js` |
| `goal/` | 目标输入 `composer.js`、目标状态 `status.js` |
| `appearance/` | 外观模型与主题 `theme.js`、设置对话框 `dialog.js` |
| `settings/` | 环境、指令与回顾设置 `remote-settings.js` |
| 根 | 接口调用 `api.js`、页面骨架 `shell.js`、审批 `approvals.js`、通知 `notifications.js`、Claude 控制提示 `claude-notices.js`、全局输入 `global-input.js`、本机文件 `local-files.js` |

`node scripts/check-remote-access-assets.mjs` 检查分层，出现反向依赖（如 `ui/` 引用 `features/`）即失败；同时用 V8 只链接、不执行全部模块，具名导入在目标模块中不存在时报告出错的模块，避免页面上线后整页无法加载。`features/` 之间允许互相引用，目前存在一个较大的循环依赖分量，因此必须遵守下一节的加载期规则。

## 加载期规则

1. 模块顶层只做声明：函数、常量和状态初始值。页面事件监听、会话存储恢复、定时器、启动时应用外观等副作用都放在模块导出的 `installXxx()`、`restoreXxx()`、`startXxx()`、`applyXxx()` 函数中，由 `app.js` 按原有执行顺序显式调用。调整调用顺序前须核对监听注册次序与恢复依赖。
2. 模块顶层的初始化表达式不能直接或经函数调用读取其他 `features/` 模块的 `let`/`const` 绑定。循环依赖中对方模块可能尚未求值，读取会抛出 `ReferenceError`（TDZ）。例如 `readAppearance()` 在加载时校验已保存的项目显示上限，所以 `projectThreadLimits` 必须与它同在 `features/appearance/theme.js`，侧栏筛选从外观模块导入。函数声明会提升，加载期引用其他模块的函数本身不受此限。
3. 通用组件通过注册解耦：`ui/popup.js` 打开弹层前调用的 `closeAppMenus` 由入口 `registerAppMenuCloser(closeComposerMenus)` 注册；Select 选项提交由 `ui/select.js` 加载时 `registerSelectCommitter(commitSelectValue)` 注册，弹层不反向依赖 Select。

## 状态归属

- 每个可变状态只属于一个模块。其他模块通过 `import` 读取（ES module 实时绑定，始终是最新值），写入必须调用归属模块导出的 setter：`setXxx(value)`，返回写入的值。个别 setter 为避免与既有函数重名改为 `assignCreationBackend`、`assignGoalEditPending`。
- 不能对导入的绑定直接赋值（ES module 导入为只读，运行时抛 `TypeError`）。
- 多个功能共用的会话级状态放在 `core/state.js`：`session`、`sidebar`、`selectedId`、`thread`、`turns`、`connected`、`selectionVersion`、`busy`、`creating`、`stopping`、`pageLeaving`、`nativeConnectionVersion`、`selectionToken`。

## 通用 UI 组件

| 模块 | 导出 | 说明 |
| --- | --- | --- |
| `ui/dom.js` | `$`、`element`、`button`、`escape` | DOM 查询与创建 |
| `ui/icons.js` | `icon` | 内置图标 |
| `ui/popup.js` | `openPopup`、`togglePopup`、`closePopup`、`renderPopup`、`fitPopup`、`activePopup`、`registerAppMenuCloser`、`registerSelectCommitter` | Select 与按钮菜单共用的弹层宿主：定位、搜索、键盘导航、焦点恢复与 dialog 顶层规则 |
| `ui/select.js` | `selectControl`、`setSelectValue`、`setSelectOptions`、`commitSelectValue`、`createSelectComponent` | 主题化 Select；页面使用默认实例，`createSelectComponent` 保留依赖注入供测试替换 DOM |
| `ui/dialog.js` | `confirmAction` | 确认对话框，返回 `Promise<boolean>` 并恢复触发前焦点 |
| `ui/notice.js` | `notice` | 页面提示条 |
| `ui/clipboard.js` | `copyText` | 复制文本 |
| `ui/tooltip.js` | `installComposerTooltips`、`composerTooltips` | 输入区工具提示 |
| `ui/position.js` | `menuHorizontalLeft` | 浮层水平对齐规则，Select、按钮菜单、模型菜单与会话菜单共用 |

新增通用组件放在 `ui/`，只依赖 DOM 与其他 `ui/` 模块；需要业务行为时由调用方传入回调或通过注册函数接入。

## 网页资源登记

资源清单由同一条目录规则生成，新增、拆分或删除网页文件不需要逐处登记：

- `assets/remote-access/` 下（含子目录）扩展名为 `.html`、`.js`、`.mjs`、`.css`、`.png`、`.webmanifest` 的文件都是版本壳资源，按扩展名确定 MIME，`index.html` 提供为 `/`。
- `cache-recovery.html`、`cache-recovery.js` 不进入版本清单，由服务端单独以 `no-store` 提供。
- 其他扩展名（资源版本号、许可证文本等）不作为网页资源。

该规则有以下实现，修改规则时须同步全部位置：

| 实现 | 用途 |
| --- | --- |
| `scripts/remote-access-resources.mjs` | 调试入口、资源检查、Windows 构建输入核对 |
| `crates/codex-elves-core/build.rs` | 服务端内嵌资源、静态路由和资源清单 |
| `apps/codex-elves-android/app/build.gradle` 的 `bundledWebResources` | APK 内置网页包 |
| `apps/codex-elves-remote/src-tauri/build.rs` | Windows 独立客户端内置网页包 |
| `scripts/update-remote-service.ps1` 的 `Get-RemoteUpdateSource` | 正式服务更新的目标清单与内嵌资源核对 |

服务端对清单内全部资源统一返回 `Cache-Control: no-cache` 并以 ETag 复验。此前 `session-tools.js`、`session-files.js`、`workspace-preview.js`、`session-menu.js`、`file-preview.html` 未列入静态资源判定，返回 `no-store`；现与其他版本壳资源一致。`/side-thread` 与缓存恢复页仍为 `no-store`。

新增模块的步骤：

1. 把文件放入对应层目录，从使用方 `import`。
2. 如果入口会静态依赖它，在 `index.html` 增加 `<link rel="modulepreload">`（检查脚本会提示缺漏或多余）。
3. 递增 `assets/remote-access/resource-revision.txt`。
4. 执行下方检查；调试入口需重启以重新扫描资源。

Service Worker 安装时下载清单中全部非延后资源；`ui/`、`core/`、`features/` 下的模块按目录规则排在入口 `app.js` 之后下载，无需写入 `criticalResourceOrder`。

## 测试约定

- `scripts/remote-access-source.mjs` 的 `loadRemoteAppSource()` 读取入口与全部模块，提供：
  - `section(start, end)`：在单个模块内截取源码，`start` 必须且只能出现在一个模块中，`end` 须在同一模块的 `start` 之后，省略 `end` 表示截取到模块末尾；结果去掉 `import`/`export`，可直接放进 `vm` 运行。跨模块的片段要分段截取后拼接。
  - `wireVm(vm)`：运行片段前补齐入口负责的装配，即跨模块 setter（写入同名全局）和 `closeAppMenus`。
  - `moduleText(path)`、`corpus`：单个模块或全部模块的脚本文本，用于源码断言。
- `scripts/remote-access-ui-state.test.mjs` 的 `sourceSection()` 即上述 `section()`；Select 组件测试直接导入 `ui/select.js` 的 `createSelectComponent`。
- `crates/codex-elves-core/tests/cdp_bridge.rs` 中读取网页源码的测试同样通过该辅助脚本截取，源码包含断言使用全部模块合并后的文本。
- 远程回归的源码指纹覆盖整个 `assets/remote-access/`；工作区已删除但尚未提交删除的跟踪文件不计入指纹。

## 检查清单

修改远程网页后至少执行：

```powershell
node scripts/check-remote-access-assets.mjs        # 语法、import 目标与具名导出、分层、modulepreload、各端规则
node --test scripts/remote-access-ui-state.test.mjs
node --test scripts/workspace-tabs.test.mjs scripts/windows-remote-cache.test.mjs scripts/windows-remote-titlebar.test.mjs
node --test scripts/remote-access-e2e-evidence.test.mjs scripts/remote-access-e2e-environment.test.mjs scripts/remote-access-e2e-receipt-driver.test.mjs
```

改动服务端资源内嵌或路由时，另执行 `cargo test -p codex-elves-core --lib remote_access::tests::app_resource_` 与 `cargo test -p codex-elves-core --test cdp_bridge remote_`。`remote_access::` 全量用例耗时较长，部分用例对运行负载敏感；出现失败时，先在改动前的代码上以相同参数复跑对比，再判断是否由改动引起。
