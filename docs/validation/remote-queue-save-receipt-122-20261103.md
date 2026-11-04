# 122 队列保存丢回包与刷新恢复：独立复核

日期：2026-11-03，Asia/Shanghai。

**PASS，限本次普通文本、单条队列的完整故障链。** 保存已经在服务端真实成功；网页响应被中断后，刷新仍保留准确 A/E、修改稿和普通稿备份，新文档只读原 A 恢复，回答原同步题后原 client 自然唯一消费。不是服务端长期 unknown，不扩大为全部回归通过。

原件为 [failure-chain-122](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/failure-chain-122/)，下文简称 B；[verification.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/failure-chain-122/verification.json) 保存 38 份来源摘要、25 项独立比较及准确消息／事件身份。本次仅新增这两份审计文件，无业务、浏览器、原生 API、桌面、服务、配置操作；未修改原件、产品或主表，未重跑、未派发代理。清理收尾不属本次范围。

## 1. 身份与直接执行证据

M=`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，C=`01a1996d-0b7a-71d0-b9ad-9c521d224ad9`；runId=`RA-20261103-failure-chain-122`。原生 sourceVersion=122／compatibilityVersion=106，fixture=`3c2a4f9f-6177-445f-b873-e05778df9b0b`，epoch=`5aac1c58-2cfc-4489-a9dc-95a5afa71dd2`。独立服务为 45430，准备记录 PID=74160。

同一 tab `1283922987` 的 `app-runtime-before.js` 与 `app-runtime-after-refresh.js` 逐字相同，直接重算 SHA-256 均为 `64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`，匹配两份实际 Debugger script 身份，刷新前后 executionContextId=2→21、scriptId=7→151。起始实际原生全文 `runtime/native-engine-before.js` 摘要为 `225519d4a7c48f5caf108a3dcdb7329cd1bc5492d625b3c4a237f945756db157`，匹配起始执行身份；冻结集没有结束实际原生全文，不以同 fixture／epoch 替代原生起止源码逐字相同的证明。

| 业务对象 | 准确身份 |
| --- | --- |
| 保存原回执 A | `4e45d55a-229c-4ca1-a5c4-d96fcfdf9e8d` |
| 编辑事务 E | `a9787469-e4db-43d8-9f4f-a5d01f0b4196` |
| 原 client | `c473308e-94c7-47e6-9ed6-a93d4df0a0c8` |
| 原 Q → 保存后 Q′ | `01a19fbe-c6da-7a63-82da-382fb01e3d84` → `01a19fbf-ad79-7af0-a2ad-502cb0748d08` |
| 原同步问答 T53 | `01a19fbd-ec7d-7541-a5e1-98aca2d1954f`，requestId=14、题目 `crossflow_color` |
| 自然消费 T54／正式消息 | `01a19fc6-489e-7050-b87a-26631eb396b1`／`01a19fc6-48e7-7671-aa1c-5b7e8f7da5ed` |

## 2. 原成功正文、刷新未决与只查原 A

`network-ledger-final.json` 精确等于 11 份分段的事件并集；752 个已筛选事件，sequence=254—1813、cursor=1813，各段 truncated=false／hasMore=false。以下结论有界于该归档窗口，不宣称所有 CDP／原生事件连续。

| 原事件 | 独立核对 |
| --- | --- |
| seq392，`72904.108` | 唯一 `/api/action/send`：原 Q／client，HTTP 200、deliveryState=queued |
| seq431，`72904.114` | 唯一 beginQueuedEdit，准确 E／原 Q／client，HTTP 200、editing；原生编辑事务的 originalInput 与原 Q 完整 input 相同 |
| seq494，`72904.123` | 唯一 editQueued，幂等键为 A、正文为编辑稿 |
| seq497，Fetch 响应暂停 | HTTP 200；`original-save-success.json.rawBody` 的 Base64 直接解码后与 receipt 对象完全相同：A completed、E committed、返回 Q′及原 client |
| seq596，同一 `72904.123` | canceled=true／`net::ERR_ABORTED`，与 `fault-controls-start.json` 中准确拦截项的 Fetch.failRequest 对应；故障发生在成功响应之后 |
| seq1005，Page.frameNavigated | loader 从 `7B0F60F6CFA495A289366108B896CF58` 切换为 `8E621383DED4541F9E54E440247810FA` |
| seq1443／1446／1448，`72904.303` | **新 loader** GET 原 A，HTTP 200、正常 loadingFinished；响应正文与最初暂停保留的成功正文逐字段相同 |

`pending-resumed-before-refresh.json` 与 `refreshed-held-ui.json` 的 pending、edits 整体分别相同：保留原 A、action=editQueued、E、原 client、修改稿和普通稿 backup，输入保持禁用，页面明确结果待确认。不能把新文档中可见 Q′当作旧保存已经被网页确认。解除回执读取拦截后，`recovered-ui.json` 显示 pending=null、edits=[]、输入恢复可用、普通稿恢复；`recovered.png` 直接可见原题仍待回答、Q′及底部普通稿。

归档内共有 send=1、beginQueuedEdit=1、editQueued=1、answer=1、markRead=2；没有第二次保存、重发 Q 或 steerQueued。原同步题起始由原生准备，不计成这份 HTTP 窗口中的 send。17 次回执 GET 全部只查同一 A，旧 loader 11 次、新 loader 6 次；这是读取重试，不是业务重发。17 个暂停网络请求均有 canceled=true 的终止事件：原保存和旧 loader 的 11 次回执 GET 全部在导航 seq1005 前结束，最后旧 GET `72904.209` 于 seq997 终止；新 loader 的 5 次受阻 GET 也均已取消，随后 `72904.303` 成功。没有把旧挂起请求留存推定为新的业务执行。

## 3. 自然唯一消费、完整输入与普通稿

`recovered-before-answer-native.json` 中 T53 仍等待原 request=14，原生队列准确为 Q′／原 client／编辑后 input，E 已 committed。随后网页唯一 answer 请求 `72904.320`（seq1547）回答蓝色；loaded 原轮的 userInputResponse 确认实际答案，T53 于 12:58:54 completed，回复 `CROSSFLOW_ANSWER:蓝色`。同秒 T54 自然开始，12:59:01 completed，最终回复 **`FAILURE_CHAIN_EDITED`**。

本次直接遍历完整持久历史，原 client 只对应 T54 的上述一项正式 userMessage；loaded 正式消息的 ID／完整 content 也相同，没有该 client 的 accepted steeringUserMessage。原 Q input 为一个 text 元素、text_elements=[]、含尾换行；仅将正文替换为编辑稿并保留换行后，整个数组分别等于恢复后 Q′ input、最终 loaded 正式 userMessage.content 和持久正式 content。自然消费轮的 loaded params.input 本身为空，本报告没有把这个空投影当作丢输入，也没有伪称它保存了完整发送参数。本次无图片、Skill 或隐藏引用。

六份阶段原生快照的前 52 个持久 turn 对象均与 baseline 原 52 轮逐字段相同；**整个 C control 对象**也分别相同，C 保持 8 轮。M 仅新增 T53／T54，最终持久 54 轮、idle、requests=[]、queue=[]。loaded 53→55 的额外既有无 turnId 本地记录不另计轮次。

普通稿 `恢复前标签编辑：FAILURE_CHAIN_122_UNSENT_保留` 在保存前 backup、刷新未决 backup、恢复后和消费后逐字相同。最终完整正式历史无该稿正文；`consumed.png` 直接可见编辑后 Q、准确回复及底部普通稿。结论是普通稿恢复且未被发送，未扩为跨设备或附件草稿验收。

## 4. 确切边界与摘要

本故障链没有阻塞 PASS 的业务证据缺口。未覆盖的范围为服务端真正长期 unknown、原生重启、复杂输入、两条 FIFO、其他失败分支及全部回归；原生结束实际执行全文未在冻结集，清理／模式／119 恢复／停服由主代理另行处理，不等待或补写这些收尾为本次完成。

| 关键原件 | 本次 SHA-256 |
| --- | --- |
| `network-ledger-final.json` | `7c187740c4e8678ff9d8c05516d65b840086a953260b608788fb7fc0baf90a9d` |
| `original-save-success.json` | `f0620435606c7b92e02ca22066a9ad6d43354ef2c85808834a703102af5d4f64` |
| `refreshed-held-ui.json` | `99c67775f995dc6db319136af02b267e168ea90db2cd5380cd29d14934bf7e00` |
| `recovered-receipt-72904-303.json` | `f90a943e683ec04a967678364568a2aa0ae9997ed2c692fe88c68729675b80f7` |
| `baseline-native.json` | `8ce6fc1b709c9b8c0072f1938016be5fb66e39e930e3202dc7de6b153f78de02` |
| `consumed-native.json` | `3cdef57a2f05f235cfd0ece2d134a9946361c2d0d813e8c21c30caa30076dac8` |

## 5. 主代理后续收尾

上述独立复核冻结后，主代理补存 `runtime/native-engine-after.js` 及实际执行身份。起止原生全文逐字相同，SHA-256 均为 `225519d4a7c48f5caf108a3dcdb7329cd1bc5492d625b3c4a237f945756db157`，因此结束执行源码缺口已由后续原件补齐；没有修改独立复核的冻结集或原结论。

`restored-mode-native.json` 与原基线的完整 settings、collaborationMode、currentPermissions、前52轮及C逐字段相同，最终M为54轮、idle、请求/队列为空。普通测试稿通过原输入框清空，`clean-refresh-ui.json`、DOM及实际截图共同确认刷新后仍为空，pending=null、edits=[]；自建标签已关闭。

`runtime/shutdown-verification.json` 记录14项收尾比较均成立。119恢复全文SHA-256为 `5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`，与测试前基线相同，fixture仅runtimeEpoch更新为 `9120600a-4314-44f4-b73a-0a7c115e1893`；既有终态编辑保留。74160已退出、45430不再监听，1432/45222原进程保持。浏览器拦截规则已清空，没有桌面窗口操作，也没有提交、构建、安装或发布。

临时目录 `temp/remote-failure-chain-122-20261103/` 的递归删除被自动审批以 `blocked by policy` 拒绝；260个文件原样保留，零次换命令重试，见 `runtime/cleanup-retained.json`。服务停止和运行时恢复不受影响，不能宣称全部临时文件已清理。
