# 队列引导移除全量历史前置读取

## 修改范围

用户要求先优化已知过重的历史防重检查。此前单次只读采样中，`includeTurns: true` 返回 20 轮、1544 项、约 1382 万字符，耗时约 3.15 秒；该采样不能解释截图中整个引导过程的耗时。

`steerQueued` 的准备前及原生锁内最终准入均改为轻量检查：

- 检查已加载用户消息的逻辑 ID、已知接受记录和未确认提交，不请求历史、不读取正文或工具输出。
- 按原生 manager 隔离，仅保存队列物理 ID、核准的逻辑 ID 与发送状态。同一调用允许多次原生准入，成功或未知结果阻止再次派发；只有明确未发送才释放记录。
- 服务端核准的逻辑 ID 优先于恢复前的 `submissionOptions`。保留原生 owner、队列版本、发送锁、完整准备输入和目标轮次校验。
- 本次调用在原生协调器中产生的 `pending/sending` 状态及临时输入占位不会被误判成重复发送。
- 队列编辑、取消恢复、重排的历史确认逻辑保持原样。

已加载快照是已接受的正面证据，不再被当作所有旧历史均未接受的完整证明。发送记录随当前 manager 生命周期保留；本次不宣称覆盖桥重建前所有历史副本，也不以超时释放未知结果。

## 验证

- 引导专项：10 项通过，包含图文身份、目标轮次、准备竞态、临时会话及工作树恢复前检查。
- 20 轮、1544 项夹具将正文属性设为不可读取，并令完整历史接口失败；引导成功且历史请求为 0。
- 固定独立历史快照，覆盖原生成功、回执丢失、已确认成功后的快照错误；重复物理 ID 和恢复后的同逻辑 ID 均不再次派发。
- 覆盖并发点击、自身多次准入、明确未发送及 deferred 后重试，确保拒绝的并发调用不能清掉首个调用的记录。
- 桥接回归：257 项通过；既有 Windows IPv6 套接字权限限制用例单独排除，未计入通过数。
- 网页状态：226 项通过。初次运行发现桥版本断言仍为 138，更新为 139 后通过。
- JavaScript 语法、Rust 格式及差异空白检查通过。

```powershell
cargo test -p codex-elves-core --test cdp_bridge request_client_steer
cargo test -p codex-elves-core --test cdp_bridge -- --test-threads=1 --skip list_targets_can_query_ipv6_loopback_cdp_endpoint
node --test scripts/remote-access-ui-state.test.mjs
node --check assets/remote-access/native.js
cargo fmt --all -- --check
git diff --check
```

原生桥版本为 139，资源编号为 `2026111105`，诊断构建标识为 `diag-20261111-queue-steer-light-r29`。未使用真实用户队列做发送验收；端到端引导耗时仍须在后续真实操作中观察，不能将移除一次历史请求直接等同于整个延迟问题已解决。

## 本机热更新

- 从主仓库 `main` 的 `e14eb0d7` 加本次未提交改动构建，`release-fast` 后台构建通过。
- 2026-11-11 01:55:15（Asia/Shanghai）完成热切换，后台 PID 11804。设备授权、配置及受保护桌面进程校验通过。
- 本机与公网各 26 项资源均与构建清单匹配。资源编号递增，网页文件内容摘要保持 `7a794581f1447e34c441e9a8041dd8bc504dc9679443d3554dd55ead7335f4a6`；本次行为修改在电脑端原生桥。
- 只读检查当前运行桥为 139，`steerQueued.toString()` 与主仓库源码摘要完全一致，函数中已无全量历史防重调用。
- 后台 SHA-256：`9a3156e58fbf6887c14f91b3ffb55175b11b54b3c0f85f8e91a276738876a63f`。
- 运行引导函数 SHA-256：`84eadc6538e7d2ff818e2c56bd429ff15806f0eda8467ab4ccf2507bde5fe038`。
- 切换及回滚材料、资源验证和运行读回记录保存在 `C:\Users\junes\.codex-session-delete\remote-access-runtime\queue-steer-light-20261111-r29\`。未创建安装包；提交与推送按用户后续明确授权执行。
