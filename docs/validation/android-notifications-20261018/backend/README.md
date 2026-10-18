# Android 通知后端验证

状态：PASS。验证对象为当前工作区源码；没有构建安装包或验证真实 Android 系统通知、后台存活。

## 修改

- `assets/remote-access/native.js` 适配器版本为 44。保留原有 v43 模型和思考深度设置逻辑。
- 已加载主会话通过真实 `getCachedConversations()` 枚举。所有状态回调独立同步通知，覆盖手机未打开的聊天和全部已加载活动回合中的异步问答，不调用历史读取、恢复、发送或回答操作。
- 按类型化原生请求 ID、方法、回合和条目跟踪身份；数字 ID `0` 有效。同聊天多个请求独立提醒，重复 pending 快照保持身份，移除后复用 ID 产生新身份。
- MCP 工具审批和支持的浏览器持久授权归为“需要授权”；登录、OTP、工具建议和普通有字段表单保持“需要回答”。
- 只从实时 `turn/completed` 的 `completed` / `failed` 生成结束通知，重复回合去重；取消、未知状态、历史读取和子代理会话不产生结束提醒。
- 当前 pending 集合和最近 256 条元数据事件独立维护；首次或 epoch 变化只返回 pending 基线，同 epoch 按游标补读，超出窗口标记 `truncated`。manager 替换时统一建立新通知 epoch。
- Rust 新增已鉴权只读 `notifications` 操作，校验请求与响应，元数据严格限定九个字段。过滤通知 SSE 使用固定心跳 deadline，并保留原 SSE 与 WebSocket 的正文和鉴权行为。既有 `/E:/` 图片路径兼容五行改动保留。

## 验证证据

- `native-checks.json`：通过完整当前 native 脚本的真实 `call`、回调和生产函数驱动，19 项隔离检查通过。
- `rust-checks.txt`：6 项生产路由和事件流定向测试通过；持续正文下 heartbeat 为 15.01 秒。
- `installed-manager-api.json`：只读解析本机已安装 Codex 26.1007.9446.0 的 `app.asar`，确认 manager 委托 `threadStore.getCachedConversations()`，实现为 `Array.from(this.conversations.values())`。

## 交付边界与清理

- 没有重启真实服务、修改用户设置、创建真实目标或发送消息/授权/回答。
- 不构建安装包、不提交、不推送。源码通过不等于已安装运行版本通过。
- 临时测试位于 `temp/android-notifications-backend-20261018/`，验证后删除具体测试文件、临时 Rust 引用及空目录。保留本目录中的验证结果。
- 最终静态检查与清理结果见 `final-checks.txt`。
