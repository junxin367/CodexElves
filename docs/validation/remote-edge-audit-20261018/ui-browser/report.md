# 通知设置二级页浏览器验收

日期：2026-10-18

结果：**PASS（本轮 CSS / 浏览器样本）**

## 样本结果

| 样本 | 结果 |
| --- | --- |
| 浅色、13px、320×360 | 五行均为透明背景且 `border: 0px none`；五个开关均为 32×20px 并在视口内；分类分隔线为 1px 实线。页面无横向溢出，文案未裁切；键盘 Tab 聚焦“接收通知”时 `:focus-visible` 生效，显示 1px 实线轮廓。 |
| 深色、20px、320×360 | 五行无外框；五个开关和标签均未横向溢出。文档 `scrollWidth` 与 320px 视口相等，通知页宽 234px 且 `scrollWidth` 也是 234px。纵向内容可滚动：`scrollHeight` 308px、可视高度 213px；滚到底部后“执行失败”开关完整显示，键盘焦点轮廓仍可见。 |
| 普通网页设置 | `#appearance-notifications-entry` 存在但带 `hidden`，浏览器报告 `isVisible=false`。 |

截图：

- `light-13px-320x360.jpg`：浅色 13px 初始页，五行和键盘焦点可见。
- `dark-20px-320x360.jpg`：深色 20px 顶部视图。
- `dark-20px-bottom-320x360.jpg`：深色 20px 滚动到底部，最后一行、焦点和系统设置按钮可见。

## 证据边界

页面直接加载生产 `assets/remote-access/style.css`、`assets/remote-access/platform.js`，并从生产 `assets/remote-access/app.js` 原样抽取 `openAppearance()`（函数起于 331 行，截止其后的下一个顶层函数）。外围应用 helper 与 Android 通知桥曾由 `temp/remote-edge-audit-20261018/ui-browser/` 中的本地夹具替身提供；该夹具源码已删除。未连接 Android App、真实会话或系统通知权限。

迟到 set / permission、总开关保存失败和返回流程按主代理确认复用 `docs/validation/android-notifications-20261018/` 与 `docs/validation/android-permissions-20261018/` 中既有验收，本轮没有重跑这些状态流程。另做的迟到 GET 演练让旧请求在关闭旧对话框、打开新对话框后返回；新页面在旧回执后仍显示读取中，随后由自己的回执更新。夹具测得从读取旧 request ID 到回执约 4.1 秒，但没有记录生产桥 `postMessage` 的精确发出时刻，因此不把该演练作为独立的 5 秒超时窗口证明。

本轮未发现 CSS 边界问题；未修改生产源码。

## 清理

验收 Chrome 标签已关闭，浏览器视口已恢复；本地夹具站点的存储、缓存、服务工作线程及可用的 IndexedDB / cookie 已清理。localhost 服务已停止，并确认端口 43179 无监听。本轮夹具源码与临时目录已删除；仅保留本目录的三张截图与本报告。
