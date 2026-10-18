# Android 通知授权原生修复验证

2026-10-18。状态：**PASS（本轮原生定向验证）**。实际生产类参与的 47 条定向断言通过；完整 Android Gradle 编译/lint 由主代理统一执行，本记录不宣称已经完成系统权限或真机验收。

## 修改范围

只修改以下四个生产文件，保留本轮开始前已有的工作区改动：

- `NotificationSettings.java`：增加本 App 单进程、同名私有偏好的已确认不可变快照。读者不获取写盘锁；保存进行中继续读取旧快照，提交成功后才通过 volatile 引用原子发布新值。首次直接保存也先初始化旧值。提交失败以原键值或 `remove` 恢复 SharedPreferences 内存；权限请求标志采用相同确认/回滚规则。
- `NotificationController.java`：保存失败只回复固定失败包，不发送可能把暂态当作成功的 changed；成功仍刷新服务并推送。复用可选 `message` 字段提示已启用分类里受系统关闭的频道，保留全局权限、身份授权和服务不支持提示的优先级。
- `NotificationPublisher.java`：为当前身份、已知 pending 账本、匹配会话的有效点击记录静默身份，并取消可能已经竞态重发的同 ID 提醒。原 `dismissed:<identity>` 持久键保持兼容，含义扩展为“用户已划掉或已查看的 pending 提醒”。pending 恢复依赖这条明确记录；仅完成/失败通知继续使用原 missing-active 静默判断。
- `MainActivity.java`：沿原有目标、当前身份和总开关校验，在设置待导航目标前记录有效点击。冷启动原有 `handleNotificationIntent()` 位于恢复页面之前；没有增加网页或会话写操作。

不改前端、Manifest、版本、构建脚本、通知协议或通知传输实现。N1 中性文案和前端错误队列处理由主代理负责，不属于本子任务验证范围。

## 定向验证

证据：`targeted-results.json`。结果为 47 条断言通过，测试编译输出的 61 个类全部位于内存。

| 范围 | 已执行验证 |
| --- | --- |
| 保存失败 | SharedPreferences 替身先更新进程内 Map，再在屏障后返回 false；实际 `NotificationSettings.read()` 在屏障中保持旧值，300ms 读超时检查通过；实际 `NotificationService` 的连接 `active()` 判断仍有效；失败后原键值恢复 |
| 保存成功 | 同一屏障期间保持旧值；提交成功后读取到新不可变实例；实际服务活动判断此时才随关闭设置变为无效 |
| 首次/旧值 | 未先读取就直接保存、缺失设置键、旧损坏 JSON、缺失权限标志、已有 false/true 标志的失败回滚；成功标志才发布 |
| Controller | 实际异步 Controller 的保存失败只有错误回复且不启动服务；成功仍回复、刷新并发送 changed |
| pending 恢复 | 实际 `NotificationEventState.snapshot()` 与 Publisher 在 API 26 模拟“频道移除通知、程序未观察到关闭”后恢复当前 pending |
| 点击与划掉 | 实际通知仍使用 autoCancel；显式调用实际点击处理方法，覆盖 Activity 回调前相同 ID 已被重发、点击记录后立即清理、后续重同步静默；实际 `NotificationDismissReceiver` 继续处理 DeleteIntent；跨分类开关、权限新基线及策略持久化恢复保持静默；同会话的新 pending 身份正常提醒 |
| 身份边界 | 非当前摘要、错误会话、未知通知 ID 和非法 URI 不能污染已知 pending 静默记录 |
| 历史通知 | 完成/失败的 previouslySeen + missing-active 保持静默；权限恢复的新快照不回放历史完成/失败 |
| 频道提示 | 只列已启用且被系统关闭的分类；不改全局授权结果；应用内禁用分类不误列；全局拒权、身份授权及服务不支持提示不会被覆盖 |
| Activity 集成 | 对真实 `MainActivity.java` 做源码顺序检查：点击记录位于原校验之后、导航之前，冷启动记录位于恢复页面之前 |

内存编译包含实际生产文件：`NotificationSettings`、`NotificationController`、`NotificationPublisher`、`NotificationEventState`、`NotificationIdentity`、`NotificationService`、`NotificationStreamClient`、`NotificationDismissReceiver`、`SessionStore`、`ServiceOrigin`、`ZrokCookiePolicy`。其中存储、Controller、Publisher、快照、服务活动判断、DeleteIntent 接收器和身份路径实际执行；没有运行 StreamClient 的网络循环。MainActivity 未在替身环境编译或实例化，仅做上述真实源码集成检查。

测试只替代 Android 平台对象和系统副作用，使用现有 `android-json-0.0.20131108.vaadin1.jar`；没有复刻本次生产决策函数。首轮曾误用普通 `org.json` 主机库，因其 `JSONException` 为 RuntimeException，与 Android 的 checked exception 签名不一致而编译失败；替换为现有 Android JSON 库后通过，没有为迁就测试修改生产代码。

使用现有 Microsoft JDK 17 的 Java 源文件模式启动，JavaCompiler 使用内存 JavaFileManager；未调用 `javac -d`，也未运行 Gradle。执行形式：

```powershell
& "<existing-jdk-17>/bin/java.exe" -Dfile.encoding=UTF-8 `
  -cp "<existing-maven-cache>/android-json-0.0.20131108.vaadin1.jar" `
  temp/android-permissions-20261018/native-fixes/MemoryNativeCheck.java `
  E:\code\junes\github\CodexPlusPlus `
  E:\code\junes\github\CodexPlusPlus\docs\validation\android-permissions-20261018\native-fixes\targeted-results.json
```

## 验证边界与清理

- API 26 的系统撤下、autoCancel 和 DeleteIntent 时序由 Android 替身表达；官方系统源码依据见上级 `notifications-audit.md` 的 A7/A8。没有把它们写成设备权限或系统回调实测。
- 没有正式打包、签名、安装、真实通知、真实权限弹窗、网络请求、服务重启、Git 提交或推送。
- 本轮只创建两份临时 Java 源文件，均位于 `temp/android-permissions-20261018/native-fixes/`。验证完成后通过 `apply_patch` 逐文件删除；没有创建磁盘 `.class` 或测试二进制，也没有触碰旧 haptics 临时文件。
- `source-hashes.json` 记录最终四个生产文件的 SHA-256；临时验证源的清理前摘要记录在 `temporary-source-hashes.json`，结果和本文保留为验证证据。
