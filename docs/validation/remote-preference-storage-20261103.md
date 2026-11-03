# 侧栏偏好 localStorage 配额失败：独立复核

日期：2026-11-03，Asia/Shanghai。

**PASS，限定为单客户端 localStorage 侧栏偏好。** 原生 `setItem` 真实触发配额错误后，偏好仅在本页生效、未落盘，页面记录明确失败提示且普通稿保持；移除测试填充键后，重试成功写入 `mode=project`，最后恢复测试前的 0 个 localStorage 键。图文／IndexedDB 配额场景为 **not_run**，全量存储故障覆盖仍为部分完成。

原件为 [storage-failure](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/storage-failure/)，下文简称 B。[verification.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/storage-failure/verification.json) 保存 14 份来源摘要及 18 项独立核对。本次只新增这两份审计文件，未操作业务、浏览器、原生 API、服务或配置，未修改源码、主表及原件，未重跑或派发代理。

| 阶段／北京时间 | 直接原件事实 |
| --- | --- |
| 13:45:15，`preferences-before.json` | 测试 origin 为 45430，storageLength=0、preferences=[]，普通稿为 `GOAL_STALE_A_DRAFT_保留` |
| 13:46:20，`local-storage-filled.json` | `setItemSource="function setItem() { [native code] }"`；24 次尝试中记录 10 次 `QuotaExceededError`；仅存在测试键 `__codexElvesAcceptanceQuota_20261103`，最终字符串长度 5242844。该长度不冒称字节数 |
| `preference-write-failed-dom.txt`／UI JSON／截图 | DOM 的 status 和 UI JSON 均记录“侧栏偏好已在本页生效，但浏览器本地保存失败，刷新后可能丢失。”；preferences=[]、仅剩测试填充键。DOM 切换按钮为“切换到项目视图”，支持本页已切到最近会话视图 |
| 13:47:27，`quota-filler-removed.json` | 测试填充键已移除，storageLength=0 |
| 13:47:28，`preference-retry-ui.json`／DOM | 明确新建 `codex-elves-sidebar-preferences-v1:<scope>` 记录，值为 `{"version":1,"preferences":{"mode":"project"}}`，storageLength=1；DOM 按钮为“切换到最近会话”，与项目视图一致 |
| 13:47:53，`local-storage-restored.json` | fillerPresent=false、storageLength=0，与原空键空间相同 |

上述失败、重试及恢复 JSON 中的普通稿均与基线逐字相同。本次直接查看 `preference-write-failed.png`，确认最近会话布局和底部原稿可见；**截图本身未直接显示失败提示文字**，提示依据为实际 DOM status 和 UI JSON，不以截图补造该视觉证据。该结果也不等于验证普通稿自身的持久化。

`preference-write-failed-network.json` 有界窗口为 sequence=1774—1845、cursor=1845，truncated=false／hasMore=false，包含 12 次请求和对应响应／结束事件，**零 `/api/action/`**。其中 6 次 POST 都是 `/api/read/sidebar` 或 `/api/read/relatedThreads`，不能写成“零 POST”。这一窗口支持未见业务写入；没有后置原生历史快照，不据此推定整个测试期间所有业务／原生状态绝对未变。

网络原件的 source.tabId 均为 `1283922995`，与 `../goal-stale/a-identity.json` 相同。该 tab 于 13:19:29 的实际 Debugger 身份对应 `a-app-runtime.js`，本次全文重算 SHA-256 为 `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`，匹配身份及 scriptParsed 摘要，isLiveEdit=false。这是同 tab 的实际执行身份采样；本场景结束未新增执行全文采样，不宣称已经取得起止两份身份。

`quota-interface-blocked.json` 明示 `Storage.getUsageAndQuota` 返回 `This method is not supported through raw CDP.`，quotaMutations=0、imageUploaded=false、verdict=not_run。原计划的图片／IndexedDB 配额不足保稿、发送阻止及恢复后唯一图文提交均未执行；`quota-test.png` 的存在不能补成上传或验收通过。未验证刷新后偏好加载、跨设备、并发修改或全量存储故障，没有追加新的必测门禁。
