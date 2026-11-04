# 2026-11-03 最新本地安装包交付：适配器124

两端正式本地包均已构建并校验，版本未递增。Windows包新增重启后冷会话目标补读；Android源码资源与上一包相同，但按用户要求重新执行Release构建及完整lint。没有自动安装、提交、推送、创建发布标签或公开发布。

| 平台 | 产物 | 校验结果 |
| --- | --- | --- |
| Windows 0.6.0 | `dist/windows/CodexElves-0.6.0-windows-x64-setup.exe`，46,559,388字节，2026-11-03 15:47:28生成 | 专用入口退出码0；launcher/manager/task-board各三处副本一致；manager内嵌准确124原生源码及当前网页 |
| Android 0.1.0 / code24 | `dist/android/CodexElves-0.1.0-android.apk`，2,633,917字节 | Release、完整lint退出码0；既有正式签名核验通过；26/26内置网页资源与源码及清单一致 |

- Windows SHA-256：`af7c3a4be3cc722602037b5ebc4d6cb17d224c202570d5740ba3fff51393e230`
- Android SHA-256：`4bc3db1ca8dc373d2a4eae3ae884dd0804f451584acafb7e8c8ddb046ee239d4`
- Android正式证书SHA-256：`7b15895625b40e03792fc250a1fc489df11fe03a25f7e8f1e177d2442fe9dc2d`
- 原生124源码SHA-256：`02723af0e9144f7d39ea30a5cbc8a41c9911fd73ec8603f69eb2dd90ece0efdd`
- 网页`app.js` SHA-256：`64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`

执行入口为`.\scripts\build-windows-installer.ps1`及`.\scripts\build-android.ps1 -Configuration Release`。原始构建日志、退出码和校验位于`dist/validation/remote-resume-123-20261103/delivery/windows/`与`android/`。Android完整lint任务正常执行，分析/报告命中既有Gradle缓存，报告0错误、12警告；没有使用SkipLint或Debug替代。Windows仅跳过独立Cargo缓存清理，不影响构建步骤。

## 修复和实际验收

冷会话持久目标读取已完成真实浏览器对照：原生仍未物化会话时，同一paused目标恢复显示；刷新保持；按测试授权清除后，目标栏移除且刷新不复活。M54/C8完整历史保持；235/235桥接回归通过。过程及运行恢复见`remote-installed-resume-123-20261103.md`。

测试结束准确恢复安装中的123原生脚本并关闭测试页；124仅在新安装包中，不能把临时验证当成已替用户安装新程序。Android APK与上一包内容相同，这次新增冷目标修复在电脑端；如此前已安装同摘要APK，重复安装不会增加该项修复。旧网页显示“新版网页资源已就绪”时，按既有提示关闭本站页面再打开即可启用。

## 后续验收

两端交付后曾按此前要求暂停，用户随后明确选择“保持目标运行”，本轮继续验收。整个回归目标仍未完成，不标记为complete；先核对实际加载身份，再继续普通发送未决跨刷新、单客户端问答恢复、旧读隔离和CLI局部兜底的剩余边界。目标卡片如仍暂停，须用户点击继续目标恢复自动续跑，现有工具不能切回active。安卓真机及Windows闪窗继续由用户验收，桌面窗口自动操作仍停止。

本轮临时脚本清理被自动审批以`blocked by policy`拒绝；两个脚本已归档并保留在`temp/remote-resume-123-20261103/`，未绕过拒绝。此项不影响安装包校验和原生运行恢复。
