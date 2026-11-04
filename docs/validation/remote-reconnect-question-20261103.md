# 服务重启后问答自动补读修复

日期：2026-11-03，Asia/Shanghai。

**限定场景通过：专用服务停止期间，原生生成同步问题；服务恢复后，网页自动补读并显示同一问题，单次回答成功，普通未发送草稿保持。** 文件审批仍只登记能力缺失，本轮没有开发或恢复文件审批功能。完整 R3-08／R3-15、安装包及 Android 真机验收仍未完成。

原件目录：`dist/validation/remote-acceptance-20261103/reconnect-question/`。以下文件名均相对此目录。

## 原因与最小修改

旧网页把连接探测与会话补读并行执行。SharedWorker 中的事件连接先恢复，网页主线程的 HTTP 线路仍有断线退避；`route-recovery-events.json` 实际记录 thread/history 的 `candidateCount=0`、`failures=2`。因此自动读取在发出 HTTP 前就被拒绝。后续连接轮询恢复在线，只刷新侧栏，不读取当前会话；原问题没有新变化时不会再通知，页面一直缺题。

本次先等待原有 `syncConnectionState` 应用成功，再补读会话；现有 15 秒连接轮询只有在离线转在线时补读一次，覆盖首次探测仍被退避拒绝的情况。没有新增轮询、重发、权限限制或请求版本重置。HTTP 探测成功但 WS 尚未打开时仍允许手动读取，原生暂未就绪时仍可查询原回执。

原有回归只覆盖离线事件触发新读，没有把“事件连接已恢复、HTTP 线路仍在退避、问题已在停服期间生成”串起来。新增回归以原问题自动到达、无写操作和草稿保持为断言；另由子代理独立复核恢复顺序与真实原件。

## 实际版本

| 对象 | 身份 |
| --- | --- |
| 修复前实际网页 | `app-before.js`，SHA-256 `a4d9c02b2b222608cd73df59ccea6d99b6e2c99d7130fb035df9f074f3a37c55` |
| 最终实际网页 | `final-app-runtime.js` 与当前 `assets/remote-access/app.js` 全文一致，SHA-256 `c8bcf027197484b274ed275b5b59fc7c3a1239f6b9fc15c4303d7715e5d593b1` |
| 原生 | 沿用 119 业务代码／106 兼容标记夹具；epoch `6d25b8d3-c92a-41ca-8fac-df1fcff314a7`，本轮未替换注入脚本 |
| HTTP 服务 | 原有独立产品服务，`127.0.0.1:45430`；网页代理为 `127.0.0.1:1435`，tab `1283922826` |
| 测试会话 M | `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`；只在该既有测试会话产生3个测试轮次 |

第一次声称加载修复后的尝试实际命中了旧 PWA 资源：`app-after-runtime.js` 哈希仍为 `a4d9…`。`first-fixed-*` 不计修复验收。最终仅对专用测试页临时绕过 Service Worker／HTTP 缓存，重新加载并核对实际执行脚本；这不是 PWA 自动更新验收。

题卡判据是 `#approval-area` 及实际按钮。早期读取 `#pending-area` 的记录只表示其他状态栏，不用于判断有无问题；最终使用 `final-automatic-question-ui.json`、截图及原生/HTTP对象深比较。

## 真实验收

| 时间（北京时间） | 操作与实际结果 | 原件 |
| --- | --- | --- |
| 07:02:35.534 | 只停止专用服务；用户原 45222 服务与 Codex 保持运行 | `final-service-stopped.json` |
| 07:03:25.014 | 停服期间原生已产生同步请求11，问题“重连验收：现在继续吗？”；队列为空 | `final-waiting-before.json` |
| 07:03:28.137 | 专用服务启动，PID 91956 | `final-service-restarted.json` |
| 07:03:56.256–.293 | 既有轮询从离线转在线，自动调用刷新；thread 路线候选为1，发出唯一 thread HTTP，返回200 | `final-reconnect-events-2.json` |
| 自动补读后 | 响应内请求11与原生快照逐字段一致，turn 为 `01a19e80-135f-7802-8a74-22262ad8e442`；题卡可见，普通稿 `E52_UNSENT_DRAFT_保留`仍在 | `final-read-responses.json`、`final-automatic-question-ui.json`、`final-automatic-question.png` |
| 稳定在线窗口 | 后续5次轮询没有新增 thread/history 请求；从断开到题卡显示期间无自动 action | `final-stable-online-events.json`、`verification.json` |
| 手动回答一次 | 只回答请求11；原回执 `d134bb3c-7ff3-4208-83e9-b53934ec89a6` 为 completed，原生题消失、轮次完成，草稿保持 | `final-answer-events.json`、`final-answer-responses.json`、`final-answered-before.json`、`final-answered-ui.json` |
| 收尾 | M 恢复 idle／default，问答和队列为空；模型、思考深度、权限及目标设置与基线一致；原41轮持久历史逐字段保持，仅新增3个测试轮次 | `baseline-before.json`、`restored-before.json`、`verification.json` |

服务启动至首次自动 thread 请求为 **28.156秒**，包含既有线路退避及轮询等待；未连续记录题卡首次呈现时间，不能将该数值当作题卡首显耗时，也不宣称重连性能已全面优化。

## 验证与边界

- 当前前端状态回归 **90/90通过**，包含16条恢复顺序、过期结果隔离及既有入口行为回归；`node --check assets/remote-access/app.js`、`git diff --check` 退出码0。
- `verification.json` 从原生快照、HTTP原响应、网络事件、实际网页脚本与UI记录验证：同一请求、一次自动补读、零自动写、一次回答、草稿与既有历史保持。它不代替完整 E2E runner 或所有功能验收。
- 真实测试覆盖 localhost 单线路、普通文本草稿及轮询恢复分支。open/resync/resume的其他时序、双线路、公网、复杂草稿、正式121原生版本及 Android 不据此标通过。安卓与 Windows 闪窗仍按用户约定交由用户验收。
- 本轮未构建、安装、提交或推送。
- 专用服务45430、网页代理1435及本轮新建标签已停止／关闭；1432原测试页的 `STOP_LIVE_2_UNSENT_DRAFT_保留`保持。浏览器缓存绕过与调试设置已撤销，原1435空草稿已恢复。
- 临时脚本删除被自动审批审查拒绝，仅返回 `blocked by policy`，未提供更具体原因；未换命令绕过。`temp/remote-reconnect-20261103/native.mjs`、`verify.mjs`仍保留且被Git忽略，收尾状态见 `environment-restored.json`，不宣称临时文件已清理。
