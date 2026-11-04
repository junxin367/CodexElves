# 远程会话现有 UI 控件验收（2026-11-03）

## 结论

**R1-31的本轮正常交互子场景已在同一标签中重跑，执行前后脚本身份及操作证据均已留存。**标题/路径搜索、清空后列表恢复、关闭搜索、主题和字号刷新保持、键盘会话菜单开关均通过。非空草稿、存储失败、大字极限和Open In系统打开仍未覆盖，整条R1-31保持partial。项目组折叠后刷新回到展开态；当前源码以页面内存集合保存折叠状态，验收表未定义折叠状态是否要跨刷新持久化，因此记录为**能力边界、验收口径待确认**，不据此认定为新回归或新增功能。C 会话输入框始终为空。

**R2-09：未执行（受阻）。**独立 `localhost:1432` origin 报告授权设备已达上限，无法进入页面验证跨 origin 视图偏好和置顶同步。没有撤销设备或更改授权配置。

后文保留早期标签 `1283922830` 和 `1283922833` 的历史记录；本次同源完整复测使用标签 `1283922837`，执行前后脚本正文均已通过 CDP `Debugger.getScriptSource` 捕获并与磁盘文件核对，详见“同一执行脚本身份内的最新 R1-31 复测”。

首次启动 1437 调试代理的 PowerShell 调用确实被执行层拒绝（`Rejected ... blocked by policy`），该事实保留。随后按主代理更新的范围复用已运行的 1432 代理和已有授权页完成了 R1-31 实测；没有再次启动任何进程。

## 早期复测历史（背景记录）

- 测试标签：本轮新建 Chrome 标签 `1283922830`，地址为 `http://127.0.0.1:1432/#thread=01a1996d-0b7a-71d0-b9ad-9c521d224ad9`，只读测试会话 C。没有操作 M 或原标签 `1283922800`。
- 代理：复用主代理确认的现有 PID `63860`。原生桥接夹具为源码 `121`、兼容标记 `106`，运行 epoch `fec066b9-c266-46da-a744-ec1109faca15`。
- 资源清单：此前从本机 `http://127.0.0.1:1432/app-resources.json` 读到 `app.js` SHA-256 `c8bcf027197484b274ed275b5b59fc7c3a1239f6b9fc15c4303d7715e5d593b1`、大小 `692014` 字节，清单版本 `eed51dd79382da4ff2e8e489bd4f99b416dac530dcc0e128dfa8a6ac983fb591`。这些值对应先前的服务清单和磁盘源码快照；本轮当前磁盘文件已是 `692051` 字节。无论清单值是否与某次磁盘快照一致，**都不能证明页面实际执行了该脚本**。Service Worker/PWA 可能让页面执行内容与清单分离。上一版将清单值写成页面运行哈希的结论证据不足；下方 CDP 记录补充了新文档的实际身份，旧文档身份仍未证实，不能用新文档替代。
- 外观基线：项目视图、深色主题、手动模式、字号 `14px`；搜索框关闭；测试项目组 `CodexPlusPlus` 展开。测试后这些值均已恢复，并在刷新后复核。
- 草稿基线与结束状态：C 的消息输入框在测试前为空，结束后仍为空。没有创建、发送或回答消息。
- 独立 `localhost:1432` origin：只读打开时要求访问密钥；按授权尝试连接后页面提示“授权设备已达上限，请撤销旧设备”。没有撤销设备、改授权配置或继续登录；输入已清空，新建标签已关闭。该尝试没有进入会话，也没有覆盖后续已授权的 `127.0.0.1` 实测。

## 早期场景结果（历史记录）

| 验收项 | 结果 | 证据与说明 |
| --- | --- | --- |
| R1-31：按准确测试标题搜索 | 通过 | 用 C 页面显示的完整测试标题搜索，侧栏只返回会话 C。 |
| R1-31：按准确测试路径搜索 | 通过 | 用页面显示的完整路径 `C:\Users\junes\Documents\Codex\2026-11-02\remote-e2e-ra-20261102-queue-status` 搜索，侧栏只返回会话 C。 |
| R1-31：清空搜索后列表恢复 | 通过 | 清空标题和路径查询后，项目与最近分组以及 C 均恢复；最后关闭搜索框回到基线。 |
| R1-31：分组折叠并刷新后恢复 | 能力边界／口径待确认 | 将展开的 `CodexPlusPlus` 项目组折叠后刷新，页面回到展开状态。对照发现 `HEAD` 与当前版都用模块内 `Set`（项目组 `collapsed`、区段 `collapsedSidebarSections`）保存折叠状态；没有对应的 `localStorage`/`sessionStorage` 写入，相关折叠代码与 `HEAD` 无差异。刷新展开符合既有实现，不能据此判断为新回归。 |
| R1-31：主题入口调整后刷新保持 | 通过 | 基线深色／手动；经现有主题按钮切至浅色，刷新后仍显示浅色。随后恢复深色，刷新后复核仍为深色／手动。 |
| R1-31：字号入口调整后刷新保持，文字与菜单仍可操作 | 通过 | 基线 `14px`；经现有滑块调至 `16px`，刷新后仍为 `16px`。在该字号下完成标题/路径搜索与键盘菜单操作；随后恢复 `14px`，刷新复核仍为 `14px`。 |
| R1-31：键盘打开/关闭会话菜单且不触发危险项 | 通过 | 通过 Tab 聚焦 C 的会话菜单按钮，Enter 打开，Escape 关闭。菜单显示“置顶会话”“删除会话”；没有选择任何菜单项，C 仍显示，未置顶、删除或触发其他动作。 |
| 普通草稿全程保持 | 通过（基线为空） | 输入框测试前后均为空，没有写入、发送或回答。 |
| R2-09：跨独立 origin 的视图偏好、置顶同步 | 未执行 | 最终实测只使用已授权的 `127.0.0.1:1432` origin；独立 `localhost` origin 因服务的授权设备上限未能进入。没有更改视图模式或置顶。R2-09 行要求两种真实存储 origin 及置顶同步，见验收表第 173 行。 |
| 恢复原设置并关闭测试标签 | 完成 | 项目视图、深色／手动、`14px`、搜索关闭及原展开状态经刷新复核；随后关闭本轮标签。未启动或停止代理进程。 |

## 早期折叠行为记录（后续已在同源复测中再次观察）

- `HEAD` 与当前 `app.js` 都在模块变量中初始化 `collapsed = new Set()`；点击项目标题只增删该集合并重绘侧栏。区段折叠也只使用 `collapsedSidebarSections` 内存集合。
- 当前源码相关位置：`assets/remote-access/app.js:57`、`:5468`、`:6258`、`:6446`、`:6467`；`HEAD` 对应路径位置约为 `:57`、`:5388`、`:6178`、`:6361`、`:6381`。`git diff --unified=0 HEAD -- assets/remote-access/app.js` 未显示这些折叠路径变化；定向搜索也未发现折叠状态写入浏览器存储。
- `docs/validation/remote-acceptance-current-20261102.md:158` 的 R1-31 只写“折叠恢复”，没有定义刷新后应保持折叠还是回到默认展开。第 `173` 行的 R2-09 定义的是跨不同存储 origin 的偏好互换及置顶同步。

## 早期执行脚本身份（历史记录；最新身份见末尾复测）

早期补充轮次在标签 `1283922833` 打开授权的 C 会话。对该标签依次启用 `Network`、`Runtime`、`Debugger` 后，读取当前 DOM 的 `document.getElementById('new-chat').onclick`，结果为函数 `() => startNewChat()`。`Runtime.getProperties` 返回该函数的 `[[FunctionLocation]]` 为 `scriptId=27`、第 `6000` 行、第 `27` 列；随后 `Debugger.getScriptSource(scriptId=27)` 取得正文。

- 实际执行正文：`dist/validation/remote-acceptance-20261103/existing-ui-controls/executed-app.js`
- SHA-256：`64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`
- 正文长度：`692051` 字节；同次读取的磁盘 `assets/remote-access/app.js` 为 `692051` 字节，SHA-256 完全相同。
- CDP 缓冲事件中没有找到 `scriptId=27` 对应的 `Debugger.scriptParsed` 记录，因此以当前 DOM 函数的运行时位置绑定到 `scriptId=27`，再读取该脚本文本；没有用 manifest 或 `fetch app.js` 替代。
- 该早期轮次没有调用 `Network.setBypassServiceWorker`；没有改变 Service Worker 设置。`Network`、`Runtime`、`Debugger` 域已关闭。
- 当时主代理要求不重复此前已通过的交互用例；该早期补充轮次只读确认新页面的视图和偏好，没有重跑交互。此项历史限制已由末尾标签 `1283922837` 的同源完整复测补齐。

该早期补充轮次的最终画面保存在 `dist/validation/remote-acceptance-20261103/existing-ui-controls/final-c-page.png`（98,689 字节）。截图由 `getScreenshot()` 返回的字节写入 evidence 目录。

## 早期原件（历史记录）

- 浏览器事件与脱敏 UI 摘录：`dist/validation/remote-acceptance-20261103/existing-ui-controls/ui-state.txt`。
- 实际执行脚本与最终页面截图均存于上述 evidence 目录；不保存无关会话正文。

## 同一执行脚本身份内的最新 R1-31 复测

### 环境与基线

- 测试标签：Chrome `1283922837`，地址 `http://127.0.0.1:1432/#thread=01a1996d-0b7a-71d0-b9ad-9c521d224ad9`，只操作测试会话 C；没有操作 M 或原标签 `1283922800`。
- 运行夹具：源码版本 `121`、兼容标记 `106`、fixtureId `633df1c2-1d8c-4aff-b2fc-200e87bfe1ba`、epoch `1fe806bb-39a4-45f9-9ba2-f11bb54286bd`。
- 执行前基线：项目视图、深色/手动、`14px`、搜索关闭、`CodexPlusPlus` 项目组展开、C 输入框为空。
- 完成后复核：项目视图、深色/手动、`14px`、搜索关闭、项目组展开、会话菜单关闭、C 输入框为空；侧边栏回到顶部，页面在线。最终状态见 `dist/validation/remote-acceptance-20261103/existing-ui-controls-r2/final-c-page.txt` 和 `final-c-page.png`。

### 逐项交互结果

| 用例 | 结果 | 实测 |
| --- | --- | --- |
| 按准确标题搜索 | 通过 | 只返回 C；清空后项目分组、最近分组与 C 恢复显示。 |
| 按准确 Windows 路径搜索 | 通过 | 只返回 C；清空后列表恢复。 |
| 关闭搜索框 | 通过 | `sidebar-search-toggle` 收起，搜索框隐藏且值为空。 |
| 折叠项目组并刷新 | 能力边界/口径待确认 | 折叠时 `aria-expanded=false`；刷新后恢复为 `true`。当前实现将折叠状态放在页面内存集合中，刷新会重新以展开态初始化；验收文字未明确要求跨刷新保持折叠。 |
| 主题切换与刷新 | 通过 | 深色/手动切浅色，刷新后仍为浅色；恢复深色后刷新，仍为深色/手动。 |
| 字号调整与刷新 | 通过 | `14px` 调至 `16px`，刷新后设置滑块读回 `16px`；恢复 `14px`，再次刷新后读回 `14px`。 |
| 键盘会话菜单 | 通过 | 在 C 会话行按 Tab 后焦点位于其菜单按钮；Enter 展示“打开会话”“置顶会话”“删除会话”；Escape 关闭菜单。未选菜单项，C 仍存在且未置顶、删除。 |
| C 输入框 | 通过（基线为空） | 测试前后均为空；未输入、发送或回答消息。 |
| R2-09 跨独立 origin 偏好/置顶同步 | 未执行（受阻） | 独立 `localhost:1432` origin 因授权设备数上限未能进入；没有撤销设备、改授权设置或执行同步/置顶操作。 |

### 页面实际执行脚本身份

- 标签 `1283922837` 执行前通过当前 DOM `new-chat.onclick` 的 `[[FunctionLocation]]` 绑定到 `scriptId=7`，并以 `Debugger.getScriptSource` 保存正文。
- 完成全部交互和刷新后，在同一标签再次执行相同捕获流程，绑定到 `scriptId=762`；两次函数位置均为第 `6000` 行、第 `27` 列，描述为 `() => startNewChat()`。
- 执行前文件 `dist/validation/remote-acceptance-20261103/existing-ui-controls-r2/executed-app-before.js`、执行后文件 `executed-app-after.js` 与磁盘 `assets/remote-access/app.js` 均为 `692051` 字节，SHA-256 均为 `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`。对照清单旧值 `692014` 字节未作为页面执行身份依据。
- 未调用 `Network.setBypassServiceWorker`。保存最终截图和运行身份后，已禁用 CDP `Network`、`Debugger`、`Runtime` 域，并关闭测试标签 `1283922837`；确认该标签不再列于 Chrome 标签清单。没有启动或停止代理进程。

### 证据目录

`dist/validation/remote-acceptance-20261103/existing-ui-controls-r2/` 保存了搜索、折叠、主题、字号、菜单各步骤的 DOM 摘录和截图、前后脚本正文、哈希核对、最终页面与时间线。`timeline.txt` 记录按顺序执行的状态变化。

## 45222 独立 origin 只读探查

- 测试标签 `1283922840` 打开 `http://127.0.0.1:45222/#thread=01a1996d-0b7a-71d0-b9ad-9c521d224ad9` 后，已进入 C 会话页面；会话控件存在，未出现密钥输入、授权设备上限提示或 401，连接状态为 `online`（本次读取约 `18 ms`），当前为项目视图。
- 该页面实际执行脚本由该标签当前 `new-chat.onclick` 的 `[[FunctionLocation]]` 绑定到 `scriptId=11`，`Debugger.getScriptSource` 返回 `683201` 字节，SHA-256 `e68492372ce4b4465d3261d1380bad1d89ef7de6028ac497f275c1dfcf424fdb`。
- 该脚本与 1432 页面已核实的 `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387` 不同，因此将 45222 记录为**旧版本同步观察端**，不用于确认最新 UI 实现。`connection-state online` 只证明当前会话端点可达；本探查没有测试消息发送或模型/原生执行路径。
- 仅读取页面和脚本身份；未输入凭据、改变授权或偏好、写入消息，也未执行跨 origin 偏好/置顶操作。Runtime、Debugger 域已关闭。只读摘要见 `dist/validation/remote-acceptance-20261103/existing-ui-controls-r2/45222-read-only-observation.txt`。
