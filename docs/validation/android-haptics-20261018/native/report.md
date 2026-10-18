# Android 原生交互触觉验证

日期：2026-10-18。结果：**生产实现及控制器验证 PASS；临时字节码清理 BLOCKED，交主代理处理**。

## 改动

- `HapticFeedbackController.java`：仅允许 `haptic-click`、`haptic-drag-start`、`haptic-drag-tick`、`haptic-drag-end`；使用 `View.performHapticFeedback(constant, 0)`。统一使用单调时钟 `SystemClock.uptimeMillis()`，所有触觉共享 50ms 最小间隔，连续换位反馈额外要求距离上次换位反馈至少 80ms。只有实际调用系统反馈方法后才记录时间，不延迟补播。
- `MainActivity.java`：复用现有桥接的 HTTPS Origin、主 frame、当前 WebView、当前页面 URL、连接代次及 Activity 有效性检查。触觉分支在 UI 线程执行前再次检查页面 URL 没有变化且仍属于当前 Origin。仅包含一个字符串 `type` 字段的触觉消息可到达分支，不接收强度、时长、波形等额外字段；无回包或新 listener。`onResume` 允许触觉，`onPause` / `onDestroy` 禁用触觉。
- `LoginView.java`：连接、密码显隐、帮助展开按钮在实际 `onClick` 中各请求一次点击反馈，复用同一个控制器。IME 提交、文本输入、滚动、文本选择未新增触觉调用。

## SDK 选择

| 类型 | SDK 选择 |
| --- | --- |
| 点击 | `CONTEXT_CLICK` |
| 开始拖动 | API 34+ `DRAG_START`，其余 `LONG_PRESS` |
| 拖动换位 | API 34+ `SEGMENT_TICK`，其余 `CLOCK_TICK` |
| 结束拖动 | API 30+ `GESTURE_END`；API 27–29 `VIRTUAL_KEY_RELEASE`；API 26 `VIRTUAL_KEY` |

## 已完成验证

使用现有 JDK 17.0.20.1 与 SDK 35 的 `android.jar` 编译实际生产控制器，退出码 0。随后保留该生产字节码，用临时 Android 运行桩提供时钟、SDK 版本、主线程和 View 状态，执行 **65 项通过**：

- API 26、27、29、30、33、34、35 的四类反馈映射，共 28 项。
- 空、未知、大小写不匹配类型静默拒绝，不调用系统反馈、不消耗限流。
- 未 attach、隐藏、禁用、窗口不可见、窗口失焦、后台、非主线程、空 View 的拒绝与恢复。
- 时间为 0 时首个有效请求正常发送；不同 View / 类型共享 50ms 限流；49ms 拒绝、50ms 接受；拒绝不延长间隔。
- 换位反馈 79ms 拒绝、80ms 接受；换位反馈同时服从其他类型产生的 50ms 限流；被限流的首个换位请求不消耗换位间隔。
- 系统方法返回 `false` 时仍按实际尝试时间限流，不延迟补播。运行桩对每次系统调用检查 flags 严格为 0。

源码检查确认：触觉消息沿用 `payload.length() == 1` 与字符串类型检查；通知类型的原有消息例外不包含触觉类型；没有增加 `Vibrator`、`VIBRATE` 权限、通用触摸监听器或网页可控的振动参数；保留已有通知、更新、图片选择和登录逻辑。`git diff --check` 退出码 0。

命令结果见同目录 `commands.log`，65 项输出见 `controller-check.log`。临时测试源代码和字节码只位于 `temp/android-haptics-20261018/native/`。测试源文件已通过 `apply_patch` 删除。主代理核对了 7 个字节码文件、目录范围和无链接状态，单独删除 `Build.class` 成功；剩余明确清单删除仍被自动审查以 `blocked by policy` 拦截，未提供更具体原因。没有更换执行工具绕过拒绝。当前保留以下 6 个本次生成的字节码文件与目录：

```text
out/runtime/android/os/Build$VERSION.class
out/runtime/android/os/Looper.class
out/runtime/android/os/SystemClock.class
out/runtime/android/view/View.class
out/runtime/com/junxin367/codexelves/HapticFeedbackControllerCheck.class
out/sdk/com/junxin367/codexelves/HapticFeedbackController.class
```

## 未验证边界

- 主代理另已通过 Gradle Release Java 编译和 lint，见上一级 `gradle-validation.json`；未运行整仓检查、正式打包、安装或部署。
- 没有连接设备或模拟器。未验证真实 WebView 消息投递、页面切换竞态、系统触觉开关、无振动硬件设备以及真机实际振感；桥接与生命周期连接仅完成源码检查。
- SDK 降级是在生产控制器字节码上切换运行桩的 SDK 值验证，不等同于在对应 Android 系统版本上运行。
- 原生改动没有修改 Manifest、版本、签名、通知/更新模块文件，没有执行提交或推送。
