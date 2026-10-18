# Android 网页轻触反馈接入验证

**状态：PASS（定向 VM/DOM 与隔离 Chrome 验证）**

本次只改了 `assets/remote-access/platform.js` 和 `assets/remote-access/app.js`。网页在 Android 容器内对受信任的可点击控件发送单字段 `type` 消息；普通浏览器不安装点击监听。队列拖动反馈要求真实输入，开始、顺序变化和结束分别受激活、实际 DOM 变更及有效提交状态控制。

验证通过：两个源码文件的 `node --check`、临时 VM/DOM 夹具、`git diff --check` 及主代理完成的隔离 Chrome 验证。VM/DOM 夹具覆盖四种桥类型、50ms/80ms 限流、后台页面、普通浏览器无桥、合成/非主键点击、禁用与 inert 控件、文本/range、队列手柄、label 转发 checkbox、键盘点击、拖动和键盘队列排序。

Chrome 验证确认模型按钮打开和选择各触发一次；禁用控件、正文、输入、队列边界及相同位置拖动不触发；有效键盘换位产生一次 tick；触摸拖动的开始、实际换位和提交各触发一次，取消不提交且没有结束反馈；checkbox label 只触发一次；Space 激活正常。普通手机网页同类操作产生 0 次触觉请求。记录中共有 3 次隔离队列重排请求（Android 2 次、普通网页 1 次），没有真实会话写入。

临时 VM/DOM 夹具脚本已删除，既有 Android 临时验证材料未改动。Chrome 样本与桥/请求记录保存在同级 `browser` 目录。
