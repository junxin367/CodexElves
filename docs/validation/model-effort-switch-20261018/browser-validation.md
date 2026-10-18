# 模型、推理等级与登录说明验证

前端检查结果：**PASS**。Chrome 扩展加载完整生产网页，通过真实菜单和输入框操作检查选择行为，并向本机隔离 HTTP 服务发送 6 次请求。模型目录、WebSocket 和回执由模拟服务提供，没有发送真实聊天消息或调用模型。模拟服务记录的 `appliedInFixture` 仅用于页面回显，不作为原生实际应用配置的证据；原生验证另见本目录原生报告。

## 模型与推理等级

| 操作 | 生产前端实际发送的配置 |
| --- | --- |
| 当前模型 A，仅选择极高 | `send`：`model=preview-a`、`reasoningEffort=xhigh` |
| 切到模型 B，再选择轻度 | `send`：`model=preview-b`、`reasoningEffort=low` |
| 切到无推理等级的模型 | `send`：`model=preview-c`，不包含 `reasoningEffort` |
| 新建会话，仅选择中等推理 | `create`：`model=preview-a`、`reasoningEffort=medium` |
| 切到模型 B，直接采用默认等级 | `send`：`model=preview-b`、`reasoningEffort=max` |
| 切换会话，不选择覆盖值 | `send`：不包含 `model` 和 `reasoningEffort` |

6 次请求的幂等 ID 均不同，没有重复提交。当前模型/等级正确显示，重复选中同一模型不会重置已选推理等级；选择模型或推理后菜单关闭；再次点击模型按钮可关闭菜单。目录中的 `minimal` 不展示，未开放功能开关时 `ultra` 不展示。无推理模型没有旧等级显示。切换会话后清除当前本地覆盖，恢复目标会话的配置。

`browser-results.json` 保存 15 次状态观察。关闭的菜单保留上次绘制的 DOM，只有重新打开时才重新绘制，因此该文件中隐藏菜单的 `checked` 字段可能属于上次菜单，不代表当前按钮或发送参数错误。

## 登录页说明删除

正常状态下 `#pair-key-hint` 内容为空，`hidden=true`、`display:none`；输入框与连接按钮之间仅保留既有 18px 间距，没有旧说明的文字或留白。输入不符合格式的测试内容后，实际提交处理仍显示校验错误并设置 `aria-invalid=true`；继续编辑或清空输入后错误节点隐藏。输入框的 `aria-describedby` 保留，错误提示仍可关联。

Android 同步移除了默认说明 TextView 及唯一引用的字符串资源，未改变输入框 hint、格式校验和登录逻辑。`node --check`、Android XML 解析与定向引用检查通过。本次没有重新执行 APK 构建或 Android 真机验收。

## 证据与清理

- `browser-results.json`：界面状态与登录错误提示检查。
- `browser-submitted-requests.json`：6 次真实前端 HTTP 请求载荷。
- `browser-payload-checks.json`：请求数量、唯一 ID、精确参数检查结果。
- `browser-server-records.json`：本机模拟服务的读取/写入记录；登录提交请求为 0。
- `login-without-hint.png`：删除默认说明后的实际浏览器画面。

浏览器测试站点存储已清除，标签已关闭，视口已恢复，本次临时服务和脚本已清理。上述结果不等于正式服务已更新，也不等于真实模型推理请求验收。
