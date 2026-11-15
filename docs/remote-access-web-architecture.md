# 远程会话网页架构

远程会话网页位于 `assets/remote-access/`，是不经构建步骤直接运行的原生 ES module。同一份文件同时用于：本机远程服务（Rust 构建期内嵌）、浏览器 PWA（Service Worker 按版本清单缓存）、Android APK 内置网页包和热更新、Windows 独立远程客户端内置网页包。

原入口 `app.js` 约 1.8 万行，现拆分为 90 行的入口和 `ui/`、`core/`、`features/` 下的 102 个模块（最大模块约 1100 行）。拆分分两步：先按领域拆出模块，再按用户要求完成完整分层重构、消除模块间的循环依赖（2026-11-15）。两步都只移动代码、改写模块间的状态写入与上层调用方式，不改变页面行为；原有注释（含用户需求注释）全部随所属声明迁移。

## 目录与分层

| 层 | 位置 | 职责 | 允许依赖 |
| --- | --- | --- | --- |
| 入口 | `app.js` | 注册上层动作，按固定顺序调用各模块的启动函数，最后发起连接 | 全部 |
| 功能 | `features/` | 会话、侧栏、输入区、正文、队列、目标、外观、设置等业务功能 | `features/`、`core/`、`ui/`、根目录独立模块 |
| 基础 | `core/` | 跨功能共享状态 `state.js`、存储实例 `stores.js`、工具 `util.js`、上层动作入口 `actions.js` | `core/`、`ui/`、根目录独立模块 |
| 通用组件 | `ui/` | 不含会话业务的界面组件 | `ui/`、根目录独立模块 |
| 独立模块 | 根目录其余文件 | `channels.js`、`platform.js`、`pwa.js`、`markdown.js`、`session-menu.js`、`session-tools.js` 等既有模块 | 只能依赖根目录独立模块和 `ui/` |

`features/` 按领域分目录：

| 目录 | 内容 |
| --- | --- |
| `session/` | 实例 `instances.js` 与实例目录加载 `instance-catalog.js`、会话能力 `capabilities.js` 与动作能力 `action-capabilities.js`、连接状态 `connection.js`、连接共享状态与线路延迟 `connection-state.js`、连接生命周期 `lifecycle.js`、页面挂起与恢复 `page-lifecycle.js`、登录 `login.js`、关联/侧边会话 `related.js` 及其状态 `related-state.js`、工作区侧边面板 `side-panel.js`、会话展示缓存 `thread-cache.js` 与显示快照 `thread-display.js` |
| `sidebar/` | 列表渲染 `list.js` 及其状态 `list-state.js`、会话操作与偏好 `actions.js`、删除会话 `thread-deletion.js` 与删除记录 `deletion-state.js`、类型筛选与项目显示上限 `filters.js` |
| `composer/` | 输入区 `composer.js` 与发送状态 `send-state.js`、草稿存储 `drafts.js` 与恢复 `draft-restore.js`、编辑器 `prompt-editor.js` 与撤销历史 `prompt-edit-history.js`、模型目录加载 `models.js`、选项状态 `options-state.js`、当前模型 `model-choice.js`、模型目录查询 `model-catalog.js`、新建会话 `new-chat.js` 及其选择状态 `creation-state.js` 与记忆设置 `new-chat-settings.js`、权限 `permissions.js`、斜杠命令 `skills.js`、Skill 目录 `skill-catalog.js`、已选 Skill `selected-skills.js`、输入区菜单 `composer-menus.js`、用量 `metrics.js`、Claude 额度 `claude-usage.js`、提示词优化 `prompt-optimization.js` |
| `conversation/` | 会话选择 `selection.js` 及其状态 `selection-state.js`、历史读取 `history.js`、正文视图 `history-view.js` 及共享状态 `history-state.js`、正文暂存 `transcript.js`、实时事件 `events.js` 与同步状态 `event-sync.js`、写操作与回执 `pending.js`、待发送状态 `pending-state.js` 与本地保存 `pending-store.js`、上下文压缩 `compaction.js`、过程事件 `transcript-events.js` |
| `messages/` | 正文条目 `items.js` 与条目展示 `item-presentation.js`、用户消息 `user-content.js`、消息编辑 `message-edit.js`、媒体 `media.js` 与 `media-state.js`、工具输出 `tool-output.js` 与工具识别 `tool-info.js`、子代理 `subagents.js`、活动分组 `activity.js` 与 `activity-state.js`、变更文件 `edited-files.js` |
| `queue/` | 排队消息 `queue.js`、队列共享状态 `state.js`、排队编辑 `queued-edit.js` 与恢复 `queued-edit-recovery.js`、发送显示 `sent-display.js` |
| `goal/` | 目标输入 `composer.js` 与提交确认 `composer-submission.js`、目标状态 `status.js`、共享状态 `state.js` |
| `appearance/` | 外观模型与主题 `theme.js`、外观应用 `apply.js`、设置对话框 `dialog.js` |
| `settings/` | 环境、指令与回顾设置 `remote-settings.js` |
| 根 | 接口调用 `api.js`、页面骨架 `shell.js` 与共享引用 `shell-state.js`、审批面板 `approvals.js`、审批请求 `approval-requests.js` 与状态 `approval-state.js`、通知 `notifications.js`、Claude 控制提示 `claude-notices.js`、全局输入 `global-input.js`、本机文件 `local-files.js` |

`node scripts/check-remote-access-assets.mjs` 检查分层，出现反向依赖（如 `ui/` 引用 `features/`）即失败；同时用 V8 只链接、不执行全部模块，具名导入在目标模块中不存在时报告出错的模块，避免页面上线后整页无法加载。`features/` 之间允许互相引用，但全部静态 `import` 不得成环：检查脚本发现环即失败，并给出一条最短的具体环（如 `a.js → b.js → a.js`）。动态 `import()` 的可选模块不参与求值顺序，不计入。

## 消除循环依赖

出现环时按以下两种方式之一处理，不能靠调整入口导入次序规避：

1. **下沉共享声明**：多个模块共用的状态、setter 及只读写这些状态的辅助函数，放到被它们共同依赖的状态模块（`*-state.js` 或领域目录下的 `state.js`，如 `composer/options-state.js`、`queue/state.js`），视图与流程模块从状态模块导入。状态模块不导入使用它的模块。
2. **经上层动作反转调用**：下层模块需要触发上层流程（刷新输入区、切换会话、重新读取、渲染侧栏与正文等）时，调用 `core/actions.js` 的同名转发函数，不直接 `import` 实现模块。

`core/actions.js` 为每个上层动作导出同名转发函数，并标注实现所在模块；实现由 `app.js` 在任何安装函数运行前用 `registerActions({...})` 一次注册。调用尚未注册的动作会抛出“上层动作尚未注册：动作名”，暴露装配遗漏。新增动作时，在 `core/actions.js` 添加同名转发函数，并在 `app.js` 的 `registerActions` 中加入实现；本来就依赖实现模块的模块可以继续直接导入实现，不必经转发。目前共 25 个动作，例如 `updateComposer`、`selectThread`、`reloadThread`、`renderSidebar`、`renderMessages`、`mutate`。

## 加载期规则

1. 模块顶层只做声明：函数、常量和状态初始值。页面事件监听、会话存储恢复、定时器、启动时应用外观等副作用都放在模块导出的 `installXxx()`、`restoreXxx()`、`startXxx()`、`applyXxx()` 函数中，由 `app.js` 按原有执行顺序显式调用。调整调用顺序前须核对监听注册次序与恢复依赖。
2. 静态 `import` 无环，被导入模块总是先于导入方求值完毕，所以模块顶层读取已导入的绑定不会遇到 `ReferenceError`（TDZ）。加载期用到的数据只能来自本模块或它导入的模块：例如 `theme.js` 加载时执行 `readAppearance()` 校验已保存的项目显示上限，`projectThreadLimits` 因此与它同在 `features/appearance/theme.js`，外观设置对话框 `dialog.js` 从 `theme.js` 导入。
3. 模块顶层不能调用 `core/actions.js` 的动作：实现由 `app.js` 在全部模块求值后才注册，加载期调用会抛错。`app.js` 中 `registerActions({...})` 必须位于全部 `installXxx()` 等启动函数之前，因为启动函数注册的监听器和定时器可能立即触发动作。
4. 通用组件通过注册解耦：`ui/popup.js` 打开弹层前调用的 `closeAppMenus` 由入口 `registerAppMenuCloser(closeComposerMenus)` 注册；Select 选项提交由 `ui/select.js` 加载时 `registerSelectCommitter(commitSelectValue)` 注册，弹层不反向依赖 Select。

## 状态归属

- 每个可变状态只属于一个模块。其他模块通过 `import` 读取（ES module 实时绑定，始终是最新值），写入必须调用归属模块导出的 setter：`setXxx(value)`，返回写入的值。个别 setter 为避免与既有函数重名改为 `assignCreationBackend`、`assignGoalEditPending`。
- 不能对导入的绑定直接赋值（ES module 导入为只读，运行时抛 `TypeError`）。
- 多个功能共用的会话级状态放在 `core/state.js`：`session`、`sidebar`、`selectedId`、`thread`、`turns`、`connected`、`selectionVersion`、`busy`、`creating`、`stopping`、`pageLeaving`、`nativeConnectionVersion`、`selectionToken`。
- 只在一个领域内共享的状态放在该领域的状态模块（见上一节），由视图与流程模块导入；页面骨架创建的会话菜单、会话工具等共享引用由 `shell()` 写入 `features/shell-state.js`，其他模块只读取。

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
  - `section(start, end)`：在单个模块内截取源码，`start` 必须且只能出现在一个模块中，`end` 须在同一模块的 `start` 之后，省略 `end` 表示截取到模块末尾；结果去掉 `import`/`export`，可直接放进 `vm` 运行。跨模块的片段要分段截取后拼接。上层动作在 `core/actions.js` 有同名转发函数，标记同时命中实现模块时以实现模块为准；声明迁移到其他模块后，对应截取须改到新模块。
  - `wireVm(vm)`：运行片段前补齐入口负责的装配，即跨模块 setter（写入同名全局）和 `closeAppMenus`。
  - `moduleText(path)`、`corpus`：单个模块或全部模块的脚本文本，用于源码断言。
- `scripts/remote-access-ui-state.test.mjs` 的 `sourceSection()` 即上述 `section()`；Select 组件测试直接导入 `ui/select.js` 的 `createSelectComponent`。
- 远程回归的源码指纹覆盖整个 `assets/remote-access/`；工作区已删除但尚未提交删除的跟踪文件不计入指纹。

## 检查清单

修改远程网页后至少执行：

```powershell
node scripts/check-remote-access-assets.mjs        # 语法、import 目标与具名导出、分层、无循环依赖、modulepreload、各端规则
node --test scripts/remote-access-ui-state.test.mjs
node --test scripts/workspace-tabs.test.mjs scripts/windows-remote-cache.test.mjs scripts/windows-remote-titlebar.test.mjs
node --test scripts/remote-access-e2e-evidence.test.mjs scripts/remote-access-e2e-environment.test.mjs scripts/remote-access-e2e-receipt-driver.test.mjs
```

改动服务端资源内嵌或路由时，另执行 `cargo test -p codex-elves-core --lib remote_access::tests::app_resource_` 与 `cargo test -p codex-elves-core --test cdp_bridge remote_`。`remote_access::` 全量用例耗时较长，部分用例对运行负载敏感；出现失败时，先在改动前的代码上以相同参数复跑对比，再判断是否由改动引起。
