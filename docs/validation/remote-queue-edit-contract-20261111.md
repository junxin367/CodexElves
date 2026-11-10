# 新版 Codex 队列编辑兼容修复

## RCA 结论

- 状态：已确认当前运行版本的兼容拒绝原因；真实用户队列未用于写入验收，手机操作需加载修复后复验。
- 现象：手机点击编辑排队消息，提示“当前 Codex 版本的队列取消输入合成合同尚未核准”。
- 直接原因：开始编辑时，取消恢复预检对原生输入序列化函数的源码摘要做核准检查；实际摘要不在已有兼容名单中，返回 503。
- 根因：Rspack 分支只核准旧序列化函数摘要，没有覆盖当前已加载模块的序列化及合并函数。原生模块仍可工作，但旧名单不能证明它与原取消恢复合同兼容，因而主动拒绝。
- 系统性根因：原有完整队列夹具冻结于旧 `app-initial`，`nativeModuleSnapshot` 没有 `rspack:true`，且函数识别被替身绕过；新增 Rspack 分支后没有对应的队列合同回归。
- 关键因果链：`beginQueuedEdit` → `prepareNativeQueuedCancel` → `queuedEditInputs` → 当前 serializer 的 SHA-256 不匹配旧名单 → 503 → 手机 toast。检查发生在 `removeQueuedMessage` 之前，原队列尚未移除。

## 证据

| 判断 | 类型 | 证据 | 来源 |
| --- | --- | --- | --- |
| 当前模块与已有核准值不同 | 事实 | 运行桥 137；唯一 serializer 为 `YLui.pJ`，SHA-256 `fe8f00e5…`；旧名单要求 `2a2d0476…` | 当前页面已执行模块缓存、`native-contract-before.json` |
| 能独立复现同一错误 | 事实 | 将原产品 `queuedEditInputs` 连接到实际模块，使用合成上下文，得到同文案 503 | `native-proof.json.before` |
| 当前合并函数可核对 | 事实 | 同模块导出 `YLui.rc`，SHA-256 `98eae70d…`；逐分支对照旧 `w8t`，文本保留、图片元数据、隐藏项追加及 mention 过滤结构一致 | 实际原生函数源码、旧冻结夹具 |
| 中间校验没有改写 input | 事实 | 当前 `Od9H.o` 仅在 Aeon 引用带图片时抛错，不改写输入 | `native-proof-after.json.dependencies.Od9H` |
| 修复可通过实际输入合同 | 事实 | 当前实际 serializer/merge 配合产品取消预检，纯文本、图文、纯图片、多行文本均通过；每类覆盖两种 durable 值及 steer/start/admission start 三条准备输入 | `native-proof-after.json.cancelCases` |
| 原隐藏上下文未重复 | 事实 | 上述合成用例中的 skill、mention 按原顺序各一项；图片原 detail 保留，完整输入逐字段等价 | 同上 |
| 原回归漏掉新版路径 | 事实 | 旧 snapshot 仅提供 module/source，无 rspack；旧 nativeFunction 替身不执行 predicate | 修复前 `cdp_bridge.rs` 队列夹具 |

启动状态记录的安装目录为 `OpenAI.Codex_26.1103.7773.0`，当前页面入口为 `app://-/assets/348410.792d1c9584.js`。调查读取已执行模块，未执行未加载工厂，未打印用户队列正文。运行证明使用合成文本、图片引用、skill 和 mention；准备器与 owner 检查是内存替身，不能宣称实际向用户会话派发成功。

完整核准摘要：

- serializer：`fe8f00e53e1baed72cc3ece6d4b3d18208a186f66aa78d4d626522a1e6bed70c`
- merger：`98eae70dc9b22ff289fb4e9abead809020472c357647075277716e21c68d6c97`

本机原件位于 `C:\Users\junes\.codex-session-delete\remote-access-runtime\queue-edit-contract-20261111-r26\`。

## 已排除解释

- 不是取消按钮才会触发：开始编辑就会预先验证能否取消恢复。
- 不是移除队列 API 的报错：源码链路与失败测试都证明拒绝发生在 remove 之前，消息保持原位。
- 不是原生接口缺失或多候选：当前 serializer、merger 各唯一，实际纯函数可调用。
- 不是仅由公网或网页缓存产生：同一当前原生模块与旧产品函数在隔离上下文中能够复现。
- 旧版测试通过不足以证明新版可用：旧夹具没有进入 Rspack 合同分支。

## 异常状态溯源

- 适用约束：远程编辑必须先证明能安全恢复原输入，且只能接受已核准原生合同；当当前已加载接口有可核准的实现时，应按其实际行为完成兼容。
- 约束依据：用户修复要求、既有队列恢复/未知回执验收、`assertNativeQueuedCancelInput` 及编辑移除前预检；已确认。
- 前提命中：命中。当前模块提供唯一 serializer 与 merger，旧产品检查因摘要拒绝。
- 值判断：新摘要本身不是非法输入；旧名单缺少当前合同，属于消费者兼容能力缺口。
- 来源与传播：实际加载函数 `toString` → SHA-256 → Rspack 旧单值比较 → 503；不是队列内容或网络层改变了摘要。
- 首次约束违反点：原生函数可调用，但产品尚无对应已验证的兼容绑定；不能通过删除检查修复。
- 调查边界：原生第三方模块实现及其未来变化；本次只核准已观察的一对函数，未知组合继续拒绝。

## 修复与防复发

- 保留旧版本合同，为当前版本核准 serializer/merger **成对**摘要，并要求同属一个已加载模块记录；真歧义、缺失函数、未知摘要仍在移除前拒绝。
- 同时对 serializer 输出和实际 prepare 的 steer、start、admission start 输入调用当前原生 merger，分别验证 durable=false/true，合成结果须与原队列完整输入等价。
- 实际准备和派发仍由原生协调器完成，不将预演得到的完整 input 塞回准备器，避免隐藏项被再次追加。
- 保留消息身份、原文、图片基线、owner/scope、暂停状态、位置锚点和未知回执不重放检查；完成取消前重新验证。
- 明确拒绝不能仅凭 input 证明无损恢复的非空 additionalContext、写作块上下文、评论/MCP 被动上下文及多用户 sender；空对象、空数组和 null 仍兼容普通消息。
- 新增当前原始函数 fixture，校验真实 `Function.toString` 摘要；Rspack 回归使用真实谓词识别、模块配对和 pJ/rc。平台、存储及辅助序列化依赖仍为明确标注的内存替身。
- 覆盖核准失败、跨模块拼接、缓存隔离、合成漂移、旧 mention 丢失、非空隐藏上下文拒绝，以及新版/旧版保存、取消、FIFO 和未知回执恢复。
- 35 项队列编辑回归通过；完整桥接检查 254 项通过。1 项已知受 Windows 套接字权限限制的 IPv6 本机连接检查按既有环境限制排除，未计入通过。
- 226 项网页状态检查、前端资源 / 类型检查、Rust 格式与差异检查通过。

验证命令：

```powershell
cargo test -p codex-elves-core --test cdp_bridge queued_edit -- --test-threads=1
cargo test -p codex-elves-core --test cdp_bridge -- --test-threads=1 --skip list_targets_can_query_ipv6_loopback_cdp_endpoint
node --test scripts/remote-access-ui-state.test.mjs
npm --prefix apps/codex-elves-manager run check
cargo fmt --all -- --check
git diff --check
```

## 未闭环项与下一步

- Android 真机点击编辑、保存及取消尚未作为本轮写入验收；修复后由用户在自己的队列上复验。未使用用户真实排队消息做删除、重发或测试改稿。
- 完整第三方上下文和未来未知构建不自动放开；不能证明恢复等价的消息继续引导到桌面处理。

## 构建与热更新

- 主工作区同期出现独立的日志 / 文档预览改动。本次以 `694607ee48c53837515a34de9035650565a5573f` 为基线，在独立工作树中仅加入队列修复及其回归、版本标识和验证记录；主工作区的其他改动保留。
- 独立构建第一次因忽略目录中的管理端 `dist` 缺失失败；确认管理端源码与基线相同后，补齐既有前端产物，`release-fast` 后台构建通过。
- 2026-11-11 00:46:23（Asia/Shanghai）完成热切换，服务 PID 5336，构建标识 `diag-20261111-queue-edit-contract-r26`，原生桥运行读回确认 138。
- 网页资源编号 `2026111102`。本次主要修复电脑端原生桥，网页文件内容未变，资源内容摘要仍为 `099f5299a842f005e4e4238bf5cd4f50ed2bfe5e636faf9ced1ab491e5709ee6`。
- 本机及公网各 26 项资源与预期清单一致，设备授权、配置摘要及受保护桌面进程保持不变。
- 切换验收时独立服务已连接，原生连接在预热后再次确认正常，公网入口 ready；服务模式 CDP 尝试计数保持 0。
- 后台 SHA-256：`ea4acb1fb3e15dacd95c060dc1b6d57b5f770469334834f8a5a261f872da2204`；r25 后台已备份可回退。
- 核准源码与主工作区对应文件的 SHA-256 一致，见 `isolated-source-verification.json`；实际运行函数及桥版本见 `native-contract-deployed.json`。
- 独立工作树被工具标记为当前会话主工作区，归档时因主工作区保护被拒绝；已保留原件，未使用文件命令强制删除。
