# 模型菜单再次点击关闭

2026-10-18：模型菜单的按钮原本已有开关判断，但从菜单选项点击回按钮时，`focusout` 的微任务会在新焦点建立前看到 `document.activeElement=BODY`，提前关闭菜单。随后按钮 `click` 读取到关闭状态，又把菜单打开。

本次仅修改 `app.js` 的全局模型菜单 `focusout` 监听器：当 `relatedTarget` 仍位于模型按钮或模型菜单内时，不触发失焦关闭检查。真正移出该区域、窗口失焦和其他关闭入口保持原逻辑，不新增开关状态或定时器。

真实 Chrome 对照使用生产菜单函数、按钮绑定，以及完整的全局指针、焦点、键盘事件；模型目录及周边接口为本机替身，没有操作真实会话。

- 修改前：两次点击后都保持打开。事件记录显示第二次 `focusout.relatedTarget=model-button` 时，菜单已提前关闭，随后的 `click` 又打开。
- 修改后：14 项状态检查通过，覆盖打开／关闭／重新打开、菜单内搜索、搜索框回到按钮、模型与推理选项选择、外部焦点、键盘回车、Escape、外部点击及两次触摸开关。
- 所有状态的 `hidden` 与 `aria-expanded` 一致；选项选择仍保留正确模型和推理等级。
- 触摸通过 Chrome 当前标签的 `Input.dispatchTouchEvent` 验证，不是 Android 真机验收。
- `node --check assets/remote-access/app.js` 和定向 `git diff --check` 通过。

完整状态和事件顺序见 `browser-results.json`。临时脚本、服务、浏览器标签和视口模拟已清理。本轮未重新构建安装包、提交或部署。

最终 `app.js` SHA-256：`0a46f087da2cb6c8d4e6ad1e038cc2784db54d626d04e055eeff162b1befcf7c`。
