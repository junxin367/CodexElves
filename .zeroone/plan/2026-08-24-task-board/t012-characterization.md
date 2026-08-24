# T-012 当前 Codex 宿主特征化

取证日期：2026-08-24

本记录只包含版本、DOM/API 形态、布尔能力结论和脱敏计数。未记录项目路径、项目名称、会话 ID、会话标题、composer 正文、凭据或配置值。

## 宿主版本

- Windows 包：`OpenAI.Codex`
- Codex App 包版本：`26.818.5229.0`
- Chromium / 可执行文件版本：`151.0.7922.170`
- CDP 协议版本：`1.3`
- 主页面：`app://-/index.html`
- 主页面标题：`🐴 Codex`
- 当前主 bundle：`index-DEcY3ZNM.js`
- 当前宿主核心 bundle：`app-initial-BhpTek7p.js`
- 当前 app main bundle：`app-main-TUSoJdL_.js`

## 脱敏运行时事实

- 当前主页面可见且 `document.readyState === "complete"`。
- 当前页面有 11 个带 `data-app-action-sidebar-project-row` 的项目行。
- 当前页面有 27 个带 `data-app-action-sidebar-thread-id` 的会话行。
- 会话行 ID 使用 `local:` 前缀；脱去前缀后为永久 UUID 形态。
- 当前会话同时存在：
  - 活跃 sidebar 行；
  - `[data-above-composer-conversation-id]` 永久 ID 信号；
  - 两者规范化后相等。
- `app://-/index.html` 路由本身不携带会话 ID，不能依赖 URL 恢复或导航。
- 当前语言为 `zh-CN`。项目新会话按钮的 aria 文案是本地化的“在 `<project>` 中开始新聊天”，不是英文 `Start new chat in ...`。

## 原生项目新会话

结论：**supported，按项目逐项探测**。

证据：

- 每个本地项目行都包含三个结构稳定的按钮角色：
  - 带 `aria-haspopup` 的项目操作菜单；
  - 不带 `aria-haspopup`、不带 `data-app-action-sidebar-select-project` 的原生新会话按钮；
  - 带 `data-app-action-sidebar-select-project` 的隐藏项目选择按钮。
- 11 个项目行中有 10 个原生新会话按钮可用，1 个被宿主禁用，因此 `probe(project)` 必须按目标项目返回能力，不能使用全局开关。
- 宿主源码显示，本地项目新会话动作调用 `Ixl -> Pxl -> p2`，设置原生 `activeProject` 后通过宿主 router 导航到 `/`。该流程不是 Electron dispatcher 消息。

实现约束：

- 禁止使用英文 aria 前缀作为唯一 selector。
- 应先用规范化 cwd 匹配 `data-app-action-sidebar-project-id`，再按按钮结构定位原生新会话按钮。
- 必须检查目标按钮实际存在且未禁用；不存在或禁用时返回 `native_create_unavailable`。
- 不构造未知 URL，不伪造 dispatcher payload。

## Composer 与首条指令提交

结论：**supported，但本任务只读，未实际发送消息；W4 必须做一次真实验收**。

证据：

- 原生 composer 可由 `[data-codex-composer][contenteditable="true"][role="textbox"]` 定位。
- 当前 editor 为 ProseMirror，根节点带 `pmViewDesc`。
- 从 editor 最近的 React fiber 可稳定找到 `memoizedProps.composerController`。
- 当前 controller 构造器为 `MGa`，提供：
  - `focus()`
  - `setText()`
  - `setPromptText()`
  - `insertTextAtSelection()`
  - `getText()`
  - `getPersistedText()`
  - `hasText()`
  - `view`
- `controller.view` 是 ProseMirror view，提供 `dispatchEvent()`、`dispatch()`、`focus()` 和 `state`。
- 宿主注册了 `composer.submit` 命令；composer editor 有原生 `keydown`/`beforeinput` 处理链。

实现约束：

- 首条指令只保存在调用栈内；不得写入 `sessionStorage`、任务文件或诊断日志。
- 优先通过发现到的原生 `composerController.setText()` 写入，并验证 controller 已持有非空文本。
- 通过原生 composer 的 Enter/submit 事件链提交；提交失败返回 `composer_submit_failed`。
- controller、fiber 或 composer 在流程中被替换时返回 `runtime_replaced`，不继续操作旧对象。
- 当前取证时宿主正在执行已有 turn，因此没有为验证而发送额外内容。

## 永久会话 ID

结论：**supported**。

证据：

- 新会话临时 ID 形态包含 `(client-)?new-thread:`。
- 永久 ID 可从 `[data-above-composer-conversation-id]` 观察。
- 当前永久信号为 UUID 形态，并与 active sidebar 行规范化后相等。
- 宿主内部存在 `clientThreadId -> conversationId` 的替换状态；sidebar 的临时 key 会被永久 ID 替换。

实现约束：

- `startConversation` 必须忽略任何临时 ID。
- 最长等待 15 秒；只在观察到永久 ID 后返回成功。
- 成功后可从匹配 sidebar 行读取最新会话标题；标题缺失时允许空字符串，由 Bridge 按永久 ID 重新解析真实会话。

## 按永久 ID 打开会话

### 已挂载 sidebar 行

结论：**supported**。

证据：

- 会话行具有 `data-app-action-sidebar-thread-id`、active/selected/kind 等稳定标记。
- 行的 React `onClick` 会调用宿主原生选择函数。
- 支持同时匹配原始永久 ID 与 `local:<永久 ID>`。

### 项目折叠

结论：**supported**。

证据：

- 项目行同时具有 `aria-expanded` 和 `data-app-action-sidebar-project-collapsed`。
- 项目行有原生 `onClick`/键盘处理，可展开后等待会话行挂载。

### 会话行因虚拟列表未挂载

结论：**unsupported（当前版本没有可稳定调用的外部 fallback）**。

取证到的宿主内部事实：

- 宿主内部存在 `navigateToLocalConversation`，并由内部 AppScope/router bridge 调用。
- 当前页面没有稳定、公开、可从注入层直接获得的 AppScope 导航接口。
- 没有验证出可安全复用的 Electron dispatcher 消息与 payload。

实现约束：

- `openSession` 依次尝试直接行点击、项目展开后行点击。
- 两层 DOM 路径均失败时返回 `native_navigation_unavailable` 或 `session_unavailable`。
- 不得为了覆盖虚拟列表而猜测 dispatcher payload、拼接内部 URL 或修改 SQLite。

## CT-004 能力结论

| 能力 | 当前版本结论 | 说明 |
| --- | --- | --- |
| `probe(project).canStart` | 条件支持 | 目标项目行、原生新会话按钮、composer controller 和永久 ID 信号均存在且按钮可用时为 true。 |
| `probe(project).canOpen` | 条件支持 | 目标会话行已挂载，或可通过项目展开使其挂载时为 true。 |
| `startConversation` | 支持 | 走原生项目按钮、原生 composer controller/submit、永久 ID 观察；W4 需真实发送验收。 |
| `openSession` DOM 层 | 支持 | 直接行点击和项目展开后点击。 |
| `openSession` 私有导航 fallback | 不支持 | 没有稳定外部接口或已验证 dispatcher payload。 |

OPEN-001 状态：**已关闭**。结论为“DOM/原生 controller 路径 supported，私有 dispatcher fallback unsupported”，不改变 CT-004 的签名、错误码、超时或隐私边界。
