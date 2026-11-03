# 目标旧请求迟到：尝试记录与范围调整

日期：2026-11-03。状态：**未完成该竞争分支，不判通过，也没有据此新增并发协调逻辑。**

用户在执行中明确要求“双远程会话同时操作的并发可简单处理，正常来说不会这样使用”。后续仅保留已有的过期请求拒绝、不自动重发及草稿保护，不继续展开双设备同时提交、原子协调或组合竞态验收。

原件目录：`dist/validation/remote-acceptance-20261103/goal-stale/`。测试对象仅为 M=`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`；C只读，用户工作会话未操作。原生准备的是暂停目标，不属于目标创建入口验收。

| 阶段 | 原件及观察 |
| --- | --- |
| 基线 | `before-fixture-native.json`：M54/C8、idle、请求/队列为空、goal=null。`fixture-created.json`：创建时间1793683117、status=paused、正文为本轮baseline标记 |
| 双页编辑 | `a-before-save.json`、`b-before-save.json`：不同目标编辑稿及普通稿均记录；实际网页全文分别保存为`a-app-runtime.js`、`b-app-runtime.js` |
| 乙先提交 | `b-request-held.json`：Request阶段暂停准确updateGoal，原回执B=`e62825c7-6a78-42b1-a0fb-e3fe72ca5a0f`；expectedCreatedAt/Objective/Status均来自原目标 |
| 甲完成 | `a-ledger-final.json`：原回执A=`1e0c404b-4a30-47cb-b85d-de553dc501f6`返回completed，正文为甲稿、status=paused、createdAt保持；甲页面也显示该正文 |
| 乙未实际放行 | 暂停请求`75676.87`已有Network.loadingFailed、canceled=true、ERR_ABORTED。后续continueRequest返回Invalid InterceptionId，不能作为成功放行，也不能算服务端409拒绝；拦截规则已清空，未重提 |
| 编辑稿 | `b-after-canceled-request.json`及实际截图：乙稿和普通稿仍在，结果未知锁仍保留。没有点击“已在桌面核对”或强制清除原回执 |

放行前后，独立原生读取先出现manager为空，随后出现“当前 Codex 版本的原生关联会话模块不可用（initial）”。原生最终目标及完整历史的独立对照因此未取得，不能只凭甲的HTTP成功回执宣布竞争正确。该单客户端可受影响的模块问题另行诊断，不把它归因于双端并发。

独立契约审查确认现有实现为`thread/goal/get → 比较创建时间/正文/状态 → thread/goal/set`；安装版本调用未提供原子CAS字段。Request阶段暂停也不能证明这两次原生调用之间具有原子性。此项仅记录能力边界，不主动增加锁、重试或新协议。

后续已确认原生文档于13:20:34.802重新加载，fixture消失且epoch变化；实际安装版本未变化。对应模块发现缺口已由123源码最小修复，真实恢复仍另项验证。本轮临时取证脚本已将epoch/fixture核对移到管理器与业务读取之前；`old-context-rejected-context-changed.json`记录其在新文档准确拒绝沿用122旧身份，nativeBusinessReads=0。该改动仅改善验收失败分类，不作为产品恢复证据。
