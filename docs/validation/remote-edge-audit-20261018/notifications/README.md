# Android 通知边界复查

2026-10-18，状态：**PASS（本轮定向源码验证）**。只修改 `NotificationController.java` 和 `NotificationSettings.java`。没有修改服务、事件协议、通知发布器、版本、APK 或用户偏好，没有运行真实权限、通知或网络服务；完整 Gradle 编译及 lint 由主代理统一记录。

## 发现与修复

**保存设置或记录权限请求遇到慢盘时，页面替换与 Activity 销毁会等待磁盘提交。** 两个 writer 入口原先在 `pageLock` 内调用 `commit()`；`onPageChanged()`、`onDestroy()` 及 `Request.current()` 同样获取该锁。主线程因此可能间接等待异步写盘。使用实际生产 Controller 和 Settings、阻塞的 SharedPreferences 替身，修复前五个场景均不能在磁盘屏障释放前返回，观察窗口为 250ms；这证明项目锁阻塞，不等于设备 ANR 实测。

修复位置：`NotificationController.java:91,189` 将两个写入口交给带请求锁和身份检查的 Settings 方法；`NotificationSettings.java:108,145` 保留全局串行写锁，但磁盘 I/O 不持有页面锁。只在提交前校验、成功后确认发布时短暂获取页面锁；若页面或身份已失效，沿已有回滚路径恢复旧键值，仍不发布新 `Confirmed`。原 `save(Context)` 和 `markPermissionRequested(Context)` 重载保留。没有新增存储层、协议字段或事务机制。

## 定向验证

JDK `17.0.20.1`，复用现有 Android JSON jar。实际生产 `NotificationController`、`NotificationSettings` 编译并执行；Activity、Handler、SharedPreferences、Service 和 Publisher 使用进程内替身。修复前 37 条复现断言通过；修复后 **142 条断言通过**，编译与运行退出码均为 0。

| 场景 | 结果 |
| --- | --- |
| 设置与权限标志正在 `commit()`，页面替换、销毁或身份改变 | 页面锁不等待磁盘屏障；旧确认值保持；提交完成后回滚失效写入，不向旧页面回复、不弹权限框 |
| 写入排队时页面或身份失效 | 不调用 `commit()`，不改变设置或权限标志 |
| 已成功确认发布，主线程回调前页面或身份失效 | 保留已成功确认值；旧页面不收到回复或 changed，不再发起权限框 |
| 当前请求写入成功/失败 | 成功才发布确认值；设置失败只回复固定错误，不发送成功 changed；权限失败不显示权限框 |
| 回滚的异步磁盘写入失败 | 替身独立保留未回滚磁盘值；当前进程 `Confirmed` 读者仍保持最后确认状态，旧页面没有成功推送 |
| 原 Controller 写盘中替换为新身份 Controller | 全局写锁使旧回滚先于新写入完成；新设置最终生效，旧回滚不覆盖它 |
| 两个旧重载、成功权限结果回调 | 保持原调用兼容和正常成功回复 |

执行命令（临时源已按要求删除）：

```powershell
& "C:/Users/junes/AppData/Local/Programs/Python/Python312/python.exe" `
  temp/remote-edge-audit-20261018/notifications/boundary_check.py before
& "C:/Users/junes/AppData/Local/Programs/Python/Python312/python.exe" `
  temp/remote-edge-audit-20261018/notifications/boundary_check.py after
```

`before-results.json`、`after-results.json` 保留断言和观察结果，两个 `*-manifest.json` 记录当时实际源码与夹具 SHA-256；`*-compile.log`、`*-execution.log` 保留编译/运行诊断。该夹具从给定源码编译后执行，没有复刻被修复的保存或页面有效性决策函数。`before-results.json` 的列表为初版 Android JSON 库转成的字符串，`after-results.json` 已显式使用 JSONArray。

## 边界

服务启动/停止、权限返回、流重连、快照补读、点击/划掉与身份隔离已逐路径复查，没有确认第二个需要修改的缺陷；未重跑已有 100 条策略/传输流程，也没有把此前结果当成本轮设备证据。

本轮重新读取 Android 官方 `SharedPreferences.Editor` 文档：`commit()` 同步落盘；`apply()` 立即更新内存并异步落盘，磁盘失败不会通知调用者；系统组件切换仍可能等待未完成的 `apply()`。因此本修复只消除本项目 `pageLock` 跨磁盘等待，不承诺消除 Android 框架自身的所有慢盘等待。双重磁盘失败后的跨进程重启恢复没有设备验证，当前进程读者不接纳失败值的边界已验证。

官方来源（读取于 2026-10-18）：
`https://developer.android.com/reference/android/content/SharedPreferences.Editor`

没有真机/模拟器上的系统权限、声音、DeleteIntent 实际回调、冷启动导航或后台存活验证；未构建正式 APK、安装、提交、推送。17 份临时 Python/Java 源文件已用 `apply_patch` 逐文件删除；源码残留为 0。自动审批对递归清理和单文件 `Remove-Item` 均返回 `blocked by policy`，没有提供更具体原因；因此未继续尝试其它 Shell 删除手段，仍有 27 个本次生成的 `.class` 位于 `temp/remote-edge-audit-20261018/notifications/classes/`，清理状态为部分完成，见 `cleanup.json`。未触碰旧临时目录。
