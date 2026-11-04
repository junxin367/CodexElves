# 安装重启后复核：网页更新与冷会话目标

2026-11-03，Asia/Shanghai。目标重新激活后复核安装后的实际运行环境。**Windows 安装文件、原生 123 和最新网页身份已核实；另发现并修复冷会话的持久目标没有传到远程页面，已用同一测试目标完成真实页面对照。** 产品源码内部标记升为124，公开版本不变。没有发送消息、恢复旧请求或操作桌面窗口；验收结束仅清除了本轮遗留的准确测试目标。

原件位于 `dist/validation/remote-resume-123-20261103/`。测试会话 M 为 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`，对照会话 C 为 `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。

## E68：安装文件与正常网页更新

注册表 `HKCU/Software/CodexElves/InstallDir` 指向 `D:/foWork/CodexElves`。该目录的 launcher、manager、task-board 的大小和 SHA-256 均与本轮正式安装包中的副本一致：

| 文件 | SHA-256 |
| --- | --- |
| `codex-elves.exe` | `10432cd1cd462c2d44962ebb070893b4808891ed335bd9ed29b657cf57949324` |
| `codex-elves-manager.exe` | `0b22d238541dbc24b814665e302e86274a20f23939ce6808152651f302c069ba` |
| `codex-elves-task-board.exe` | `00551b703b59632d72b6a93d7db06219c6f5513f42cf0c5ed3f37b887b4b4435` |

管理器进程在 14:58 启动，45222 由 PID 97428 监听。系统未返回该进程的可执行路径，不能只以进程名证明它执行了上述文件；另以实际 HTTP 资源清单、网页执行源码及原生脚本补充运行证据。9229 对应 Codex 26.1027.3384.0，当前原生目标为 `B30E1269994B80554EC71F6F5969552D`。

| 阶段 | 实际结果 | 原件 |
| --- | --- | --- |
| 新开测试页 | `/api/session` 为 200，`readOnly=false`、`nativeConnected=true`、`allowApprovals=true`；实际 `app.js` 仍为旧 `e6849237…4fdb` | `browser-initial-state.json`、`browser-app-runtime.js` |
| 检查更新 | 旧 worker `51c61c93…d27` 控制页面，新 worker `220f0547…1fb4` 已 installed/waiting；服务清单 `/app.js` 为当前 `64a07480…d387` | `pwa-initial-state.json` |
| 现有设置入口 | 明确显示“新版网页资源已就绪，关闭现有页面后重新打开即可启用” | `old-page-settings.txt`、`old-page-settings.png` |
| 关闭本轮测试页再打开 | 当前唯一同源测试页关闭后，新 worker 正常 activated，waiting 为空；实际执行脚本逐字等于当前源码 | `reopened-summary.json`、`reopened-app-runtime.js`、`reopened-ui.txt`、`reopened.png` |

这里没有清除缓存、注销设备、强制激活 worker 或绕过 Service Worker；正常关闭重开完成更新。该行为符合 `docs/remote-access.md` 的既有更新约定，不据此增加更新协议或自动刷新逻辑。它证明本次浏览器的正常更新流程，不替代 Android 真机更新验收。

## E69：冷会话目标缺失，限定场景已确认并修复

预期：用户已保存且未完成的当前目标，在 Codex 重启后从远程打开同一会话时仍应按原生状态展示；不能要求先发送消息才能看见。完成或清除的目标仍不能恢复为当前目标。

| 事实 | 独立证据 |
| --- | --- |
| 当前原生为 123，无测试 fixture；实际脚本包含完整、逐字一致的当前 `native.js` | `before-summary.json`、`before-native-runtime.js` |
| 15:11 原生 `thread/goal/get` 返回 M 的暂停目标，创建时间 `1793683117`，正文为本轮甲端测试目标；C 返回 null | `before-native-returns.json` |
| 同一会话远程 `/api/read/thread` 为 200，却返回 `goal=null`、`status.type=notLoaded`、`requestsKnown=false` | `reopened-thread-response.json` |
| 15:16 再读原生，同一目标仍存在；M 的原生会话对象为空，适配缓存为空且没有 goalState，已建订阅不会自动把它补成完整会话 | `cold-recheck-native-returns.json`、`cold-recheck-cache-state.json` |
| 两次原生代次均为 `94e993d6-0821-40f2-8184-993a2725ed85`，没有换页或换注入解释该差异 | 两份 `*-summary.json` |

直接原因已定位到 `refreshRequestClientConversation → runtimeThread`：原生 `getConversation` 对未物化会话返回 null，缓存也保持 null；`runtimeThread` 只从该会话对象取 `threadGoal`，将没有读取到的目标转成 null。独立 `thread/goal/get` 已证明持久目标存在。这不是旧网页导致的显示差异，也不是原生目标被删除。

根因是读取路径把“桌面尚未物化会话”与“没有当前目标”混为一谈，缺少该状态下的目标读取。现有会话刷新回归的目标快照使用非空 conversation，既有目标验收也没有证明重启后的冷会话。对同一真实目标补入只读路径后，原生会话仍为空，远程已经准确显示目标，构成修复前后的机制对照；没有通过先发送消息或强制加载会话掩盖缺口。

修复由subagent完成，仅改`native.js`及已有桥接测试。`thread`详情只对request-client、持久主会话、非只读且conversation为空的对象补读`thread/goal/get`；暖会话不增加RPC。目标最后已知值保存在原有有界会话记录中，沿用订阅revision及读取代次保护，失败不清已知目标，真实null/complete及新状态不被旧结果覆盖。未新增轮询、锁、自动重试或权限层。

| 修复与复验 | 结果及原件 |
| --- | --- |
| 源码行为回归 | 既有`cdp_bridge.rs`新增3项长期回归；最终完整235/235通过。升124后的首次检查只因两条旧123标记断言失败，更新断言后重跑通过；两次原日志分别保留在`cdp-124-tests.log`与`cdp-124-final-tests.log` |
| 受控真实运行 | 切换前五项写状态集合为空，无未决队列编辑；暂时执行准确修复源码，兼容标记保留123以免安装中的服务反复替换。`install-before.json`、`install-result.json`记录身份和边界；没有测试fixture或伪造原生返回 |
| 普通刷新入口 | `fixed-response-bodies.json`保存真实HTTP 200，目标字段逐项等于同一原生G，状态仍notLoaded、readOnly仍false；`fixed-ui.txt`与`fixed-goal.png`证明目标可见 |
| 整页刷新 | `fixed-reloaded-stable-ui.txt`、`fixed-reloaded-goal.png`继续显示同G，输入为空、模型/上下文保持。首次刷新记录为加载过渡，另存而未用于稳定态判定 |
| 独立原生复查 | `fixed-native-returns.json`中M54/C8及目标均未变；`fixed-cache-state.json`中M会话仍为空，证明没有resume或伪造物化对象 |
| 测试目标清理 | 核对准确创建时间、正文、paused状态及历史后，仅一次`thread/goal/clear`，原生返回`cleared=true`且复读为null；`clear-test-goal-cleanup.json`保存完整结果 |
| 清除后的页面 | `cleared-ui-before-refresh.txt`中目标栏已移除，整页刷新后仍无目标；这不是桌面菜单清除验收 |
| 运行恢复 | 原生准确恢复原123源码，epoch为`90b75122-1a2e-433e-a851-b0bbaa2bcf29`；`restored-summary.json`证明原文一致，两会话目标为空、历史54/8与队列保持；本轮测试页已关闭 |

`verification.json`独立核对：真实测试源码SHA-256为`1993ef8133be4cf0b7947ba1c3e175f163c50de9a8eeda254e3d32aff388b78a`；最终124源码为`02723af0e9144f7d39ea30a5cbc8a41c9911fd73ec8603f69eb2dd90ece0efdd`，仅两个版本标记不同，业务文本完全一致。首次修复刷新网络窗口完整且没有action；从后续刷新到清理的485条网络记录已截断，不据此声称全程HTTP唯一或全程零写。完整原生历史证明没有新增消息轮次，测试清除单独按一次业务写记录。旧B回执`e62825c7-6a78-42b1-a0fb-e3fe72ca5a0f`未重放或强行解除。

真实读取失败、迟到订阅/读取及连接更换由行为回归覆盖，不冒充本次现场故障验收；本次现场证明冷目标读取、刷新保持和真实清除后不复活。

## 历史与交付边界

`bootstrap-history-verification.json`将安装复核时的完整持久历史与暂停前E67原件逐字段比较：M54轮、C8轮完全保持，目标对象保持，两会话队列为空，业务写0。后续`verification.json`再次比较修复中及恢复后的全历史，均保持；唯一业务写为上述测试目标清理。

Windows0.6.0与Android0.1.0/code24已按原专用入口重新打包并校验，版本和正式签名材料保持；见`remote-delivery-124-20261103.md`及本轮`delivery/`原件。Android安装和实际设备状态未核实。整体目标仍未完成，E69只增加本次限定场景的通过证据。

本轮两个临时取证脚本清理被自动审批拒绝，原因仅为`blocked by policy`，没有换方式删除。脚本已复制到证据目录并核对摘要，原`temp/remote-resume-123-20261103/`保留；`cleanup.json`记录此边界。没有触碰此前被拒绝清理的临时目录。
