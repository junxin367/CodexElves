# 2026-11-03 本地安装包交付

**后续交付已更新：** 15:47重新生成的Windows安装包已包含124冷会话目标修复，当前同名文件摘要变为`af7c3a4be3cc722602037b5ebc4d6cb17d224c202570d5740ba3fff51393e230`；Android重新正式构建校验，内容摘要不变。最新记录见`remote-delivery-124-20261103.md`。下文保留14:45的123历史构建事实，不再代表当前Windows产物内容。

用户要求：完成最近优化后编译Windows管理器和Android正式包，随后暂停目标，待用户安装并重启Codex后手动继续。没有要求提交、推送、发布或代为安装。

| 平台 | 产物 | 校验 |
| --- | --- | --- |
| Windows 0.6.0 | `dist/windows/CodexElves-0.6.0-windows-x64-setup.exe`，46,561,834字节 | 专用构建入口退出码0；launcher、manager、task-board三处副本一致，manager内嵌准确native123及app.js |
| Android 0.1.0 / code24 | `dist/android/CodexElves-0.1.0-android.apk`，2,633,917字节 | 正式Release及完整lint退出码0；既有正式签名一致，APK内26项网页资源与源码/清单匹配 |

两端版本均未递增。状态均为**已构建未发布、未由代理安装**。主代理已重新计算最终产物摘要：

- Windows：`6e57edf6d749cab26400b93bdfc399dcf540d0497a91bd143b1a2c1f896effd9`
- Android：`4bc3db1ca8dc373d2a4eae3ae884dd0804f451584acafb7e8c8ddb046ee239d4`
- Android正式证书：`7b15895625b40e03792fc250a1fc489df11fe03a25f7e8f1e177d2442fe9dc2d`

构建日志、退出码、二进制和网页资源核对在`dist/validation/remote-delivery-20261103/windows/`及`android/`；总索引为`delivery-index.json`。本次未发布新Release、标签或下载服务。

本次包包含原生页面重载后的入口发现最小修复；桥接232/232通过，真实M/C读取恢复证据见`remote-native-entry-123-20261103.md`。这不是全部73条验收完成。双远程端并发按用户要求从简；安卓真机、Windows闪窗由用户验收；桌面窗口操作仍停止。单客户端问答、重连、队列恢复及CLI剩余边界保留在`remote-acceptance-current-20261102.md`。

恢复目标后的第一步：确认用户实际安装包与当前运行源码，再核对本轮暂停的测试目标和旧回执记录；不重放旧请求，不沿用已失效的119或临时123运行身份。
