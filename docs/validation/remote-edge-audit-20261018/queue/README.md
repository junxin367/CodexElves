# 压缩与消息队列边界复核

2026-10-18（Asia/Shanghai）。功能定向验证 **PASS：26 通过，0 失败**；临时源码清理 **PARTIAL**，自动审批拒绝原因见末节。仅修改 `assets/remote-access/native.js` 的普通发送、入队确认及提交身份检查，适配器 guard/export 同步至 `46`；更新 `docs/remote-access.md` 对应合同。未修改通知分支、`app.js`、Rust、产品显示版本或 Android 版本。

## RCA 结论

- 状态：已确认。下述链路均已用生产桥接及当前安装包函数取得修复前、后对照；结论限于这里列明的隔离环境。
- 现象：普通空闲发送的网页 ID 没有进入原生请求；最后一次桥接检查之后才开始的压缩被普通发送引导；follower 的本地 `pending` 被提前报告为成功；准备期间连接更换或会话已删除后仍能进入 server add。
- 直接原因：普通发送绕过既有入队身份保护；原生 `Pln` 在内部异步恢复后固定使用 `send-now` 并生成新 ID；原生 follower 的 `queued` 返回不保证本地记录已离开 `pending`；server 分支在异步准备后继续读取全局 manager。
- 根因：桥接把“空闲发送”和“排队发送”分为两条不同的准备与身份路径，又把 coordinator 的本地接受返回等同于最终入队；异步操作未绑定原始 manager 和删除状态。
- 系统性根因：这些约束分别存在于发送分流、恢复核对和生命周期缓存中，原生内部等待及 follower 的本地接受没有落入同一提交检查。既有 63 项记录未证明这几条具体分支。
- 关键因果链：空闲快路 → `Pln.resumeConversation` 等待 → 压缩开始 → 原生 `send-now` 引导；网页 ID → 原生另生成 ID → `sendState` 查不到；follower 接受 pending → 返回 queued → 桥接成功回执；旧 prepare 等待 → 全局 manager 更换/删除记录到达 → 未重查身份 → server add。

## 证据

| 判断 | 类型 | 证据 |
| --- | --- | --- |
| 普通发送丢失 stable-client，准备期间压缩被 steer | 事实 | `before-native-send.json`：实际 `Pln`、`ifn`、`TurnCoordinator`；记录为 `generated-1`，稳定 ID 核对 unknown |
| follower pending 不能证明最终排队 | 事实 | `before-follower-pending.json`：20 通过、2 条语义断言失败，真实 coordinator 返回 queued 时记录仍 pending |
| 旧 prepare 可借新 manager 执行 add；已删除会话也继续 add | 事实 | `before-owner-deletion.json`：22 通过、2 条语义断言失败，分别观察到一次本不应发生的 add |
| send-only 在异步恢复或准备中开始普通活动轮时会直接 steer | 事实 | `before-send-only-active.json`：24 通过、2 条语义断言失败；实际 `ifn` 的 send-only 模式不会等待普通活动轮 |
| 修复后阻断上述链路并保留普通发送兼容 | 事实 | `after-native-send.json`：26 通过、0 失败；记录最终 `native.js` SHA-256 及实际函数 SHA-256 |

安装包来源为当前 `OpenAI.Codex_26.1007.9446.0` 的 `app.asar`，实际函数 `ifn`、`ufn`、`TurnCoordinator`、`Dln` 的摘要与此前压缩验收一致。本次直接从 ASAR 内存读取，未重建此前的 app-server fixture。`Pln` 参与修复前对照；修复后普通发送不再调用这条会丢失 ID 的入口。

## 修复与防复发

- 普通空闲发送复用既有 `queueMessage` 身份合并、prepare 和 coordinator。空闲分支跳过手工 server add 快路，仍由原生立即开始；压缩或旧 local 消息在准备期间出现时交由原生保序。
- 保留模型、effort、图片、Skill 及原生重放输入。`send-only` 只在空闲普通发送时直接提交，使用原生提交前校验阻止准备期间开始的压缩、普通活动轮或已出现的队列；显式排队继续拒绝。
- 本地 queued 返回继续核对已有 `sendState`。只有真实 queued/accepted 或原生明确的 server 接受才成功；pending 未确认时保留 unknown，不再次提交。
- 捕获原始 manager，在读取、设置、准备及真正提交前核对身份与删除状态。已经开始提交后的结果仍走原有未知回执保护。
- 复用已有去重 Map，没有新增持久消息队列。成功、明确失败及已确认的终态 unknown 释放记录；未知未确认保留记录；实际接受但 promise 仍在途时查询不会提前释放。对应测试覆盖同 ID 并发、冲突、丢回包、迟到确认及重查不重发。

## 异常状态溯源

- 适用约束：在网页普通发送仍受压缩保护时，不允许向正在压缩的轮次直接 send-now/steer；在只有本地 pending/sending/outcome-unknown 占位时，不得确认最终排队；原始 manager 已失效或会话已删除时，不得开始新的提交。
- 依据及状态：`docs/remote-access.md` 压缩、回执及删除合同；本次场景全部命中，约束已确认。
- 值的来源与首次违反：网页 `clientUserMessageId` 进入普通发送快路后被 `Pln` 新 ID 替代；follower pending 被桥接的 `ok:true` 放行；旧 coordinator 准备完成后借全局新 manager 进行写操作。
- 调查边界：manager、存储、附件准备与传输为内存夹具，真实安装包的发送和协调器算法参与执行；没有据此宣称真实账号、跨窗口 IPC、图片物化或正式部署全部验收。

## 已排除解释

- 不是后端自动按客户端 ID 去重：既有真实 app-server 证据已证明不会；本次在桥接层保留原身份及在途合并。
- 不是网页重复点击导致：仅一次生产 `call(send)` 就能复现原生内部等待后绕过压缩。
- 不是原生已经排队但 UI 没显示：follower 失败样本的本地状态明确仍是 pending；修复继续保留原请求核对。
- 不是所有普通发送都改成 server queue：server queue 可用的空闲样本仍只有一次原生 start，零 server add。

## 未闭环项与验收边界

- 本次没有运行 app-server、真实用户聊天、上游模型请求或桌面窗口操作。此前手动/自动压缩的真实 app-server 调度证据按未修改的原生算法范围复用，未计入这 26 项。
- 附件准备、manager 映射和跨窗口传输为夹具。已验证传入原生准备器的字段、实际 `Dln` 合并结果与调度选择，未验证真实图片文件物化或跨窗口 IPC 的网络时序。
- 删除覆盖已知删除状态到达后的未提交操作；不将已经开始的原生请求伪报为可取消。
- 本次未新增全量历史读取。未加载历史而且原生队列中也暂时没有可靠同 ID 证据时，仍保留未知结果并只读核对，不自动重发。
- JavaScript 语法及目标文件差异检查通过。Rust 未修改，不重复追加 Cargo 整套；浏览器与 Android 由主代理独立验收。

## 清理

临时 ASAR 读取器和生产函数夹具仅创建于 `temp/remote-edge-audit-20261018/queue`。清理命令先解析并验证绝对目标处于该目录内，再尝试删除本次已知文件；自动审批拒绝整条命令，仅返回 `blocked by policy`，未提供更具体原因。没有改用其他工具、脚本语言或执行方式绕过。

本次仍保留 `temp/remote-edge-audit-20261018/queue/inspect-native.cjs` 和 `temp/remote-edge-audit-20261018/queue/queue-boundaries.cjs`，不作为长期回归测试交付。`after-native-send-initial.json` 为早期夹具缺少 steer input 的诊断结果，不计入产品结论；`before-manager-replacement.json` 是后来合并进 owner/deletion 对照的中间证据。这两个中间文件与临时源码的删除同样包含在被拒命令内，因此仍保留。

未启动后台进程，未触碰旧 app-server-fixture、旧被拒缓存或其他任务临时文件。最终静态检查、源码摘要匹配及残留清单见 `checks.json`；未构建、安装、提交或推送。
