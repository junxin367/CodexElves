# 远程问答状态接口歧义修复

日期：2026-11-10，北京时间。

## RCA 结论

- 状态：已确认当前版本的识别缺陷，修复已热更新；手机新问题的实际提交仍待用户验收。
- 现象：Android 点击“提交回答”，提示“当前 Codex 版本的原生问答状态接口无法唯一识别”。
- 直接原因：`nativeAsyncQuestionState` 的选题函数匹配得到两个候选，`unique` 在发送答案前返回 503。
- 根因：定位器用三个源码标记判断选题函数唯一性。当前原生模块同时导出了切换当前问题和重新打开问题面板的函数，两者均合法且都含有这些标记；文本相似不能证明它们提供同一种状态读取行为。
- 系统性根因：隔离状态验证位于文本唯一性检查之后，无法区分多个候选；既有问答测试只提供一个选题函数，没有覆盖同模块中存在相似功能导出的情况。
- 关键因果链：手机回答请求 → 原生 `answerAsyncQuestion` → 加载原生模块 → 两个选题候选命中旧规则 → 提前拒绝 → 手机显示错误。该异常发生在设置提交记录和调用原生提交函数之前。

## 证据

| 判断 | 类型 | 证据 | 来源位置或命令 |
| --- | --- | --- | --- |
| 当前反馈确实触发了原生兼容错误 | 事实 | 手机报告有 400 条事件、8 次回答 HTTP 完成记录；桌面日志在 19:07:59—19:08:16 记录了 8 次 `action: answer` 的同文案错误 | `manual-1794308973841-286`；`codex-elves.log` 的 `renderer.renderer.remote_native_compatibility_failed` |
| 手机已使用当前网页资源 | 事实 | Android 0.1.1、versionCode 25；资源版本为 `3d8224a629bbfb5a8f888b910dc35392d0228ed581f79a87aab183d188993077`，与服务资源清单一致 | 手机报告及部署资源验收 |
| 原生模块存在，提交函数仍可用 | 事实 | 从当前页面入口 `app://-/assets/348410.792d1c9584.js` 读取 9151 个已执行模块；草稿、跳过、关闭面板和提交函数各一个，选题候选两个 | `native-before.json` |
| 旧规则可以复现截图错误 | 事实 | 对同一原生模块执行 HEAD 版本识别函数，返回相同 503；错误栈指向选题函数的 `unique` 检查 | `native-before.json` |
| 行为验证能够区分候选 | 事实 | 切换问题函数依次读取面板与逐题状态；重开面板函数先读逐题状态，在空状态下直接返回 | 原生导出源码、隔离读取探针 |
| 修复后原生状态操作可用 | 事实 | 相同模块识别成功；虚拟 scope 中打开面板、修改草稿、跳过、关闭面板均通过 | `native-after.json`、`native-deployed.json` |
| 提交函数可安全到达取消守卫 | 事实 | 使用实际原生提交函数，`isSubmissionCurrent: false` 导致拒绝；守卫 1 次，桌面状态读取和写入均 0 次 | `native-deployed.json` |
| 热更新已生效 | 事实 | 后台 PID 7120，构建标识 `diag-20261110-question-state-r19`，当前页面原生桥 137 | `switch-result.json`、`verification.json`、`native-deployed.json` |

手机日志只记录 HTTP 状态，HTTP 200 不代表回答被原生接受。本次用桌面兼容错误和同版本隔离复现确认失败位置，没有把 HTTP 200 当作提交成功证据。

上述 JSON 证据保存在本机运行目录：

`C:\Users\junes\.codex-session-delete\remote-access-runtime\question-state-20261110-r19\`

该目录只保留排障元数据、原生模块静态源码和部署记录，未保存用户问题或答案正文。

## 已排除解释

- 原生模块缺失或提交入口消失：实际模块可读取，状态操作及提交取消守卫均通过。
- 网页资源过旧：手机资源版本与当前服务一致。
- 公网连接直接产生本次错误：相同文案可在桌面页面内部独立复现；原生日志也明确记录了 `answer` 的接口识别异常。
- 两个候选代表非法原生状态：它们分别承担切换问题和重开面板行为，缺陷在消费方的识别规则。

## 异常状态溯源

- 适用约束：当当前原生模块提供可验证的问答状态及提交入口时，远程回答必须沿原生机制工作；不能仅因其他合法导出包含相同文本标记而拒绝。
- 约束依据及状态：用户修复要求，以及 `docs/validation/remote-answer-module-compat-20261021.md` 记录的原生问答功能边界；已确认。真正不能区分的接口仍须拒绝。
- 本次运行前提命中判定：命中。当前模块、逐题状态 family、面板状态 family 和提交入口均通过隔离验证。
- 值判断：两个文本候选本身合法；将文本候选数量当成接口行为唯一性的充分证据，导致错误拒绝。
- 值的实际来源与传播路径：当前已执行的原生模块导出 → `nativeRuntimeSnapshot` 按函数身份去重 → 旧选题源码标记命中两个不同函数 → `unique` 抛出 503 → 回答失败提示。
- 最后满足约束的状态 → 首次约束违反点：合法原生导出可用 → 消费方在行为验证之前按文本数量拒绝。
- 调查边界与未知上游：原生模块由当前安装的 Codex 提供，其内部导出结构属于第三方边界；本次不假定未来版本的源码标记或读取行为不再变化。

## 修复与防复发

- 在纯虚拟 scope 中先识别草稿所属的逐题 family，再验证选题候选是否依次读取同一会话面板及同一逐题 family；读取键必须准确匹配。
- 探针禁止写入，记录写入尝试；异常、错误 family、缺失候选或真正多个有效候选继续拒绝。
- 保留原有面板、跳过状态验证，以及目标问题身份、重复提交、未知结果不重发和原生协调器发送约束。
- 原生桥从 136 递增到 137，构建标识更新为 `diag-20261110-question-state-r19`。
- 在既有测试文件增加相似函数共存的完整回答和跳过回归，并覆盖真实歧义、错误 family、探针写入拒绝；同步原生桥版本自检。
- 9 项问答回归通过；其余桥接检查共 251 项通过。另有 1 项 IPv6 本机连接测试单独复核仍返回 Windows `os error 10013`，未计入通过。
- `npm run check`、`cargo fmt --all --check`、`git diff --check` 和后台 `release-fast` 构建通过。

验证命令：

```powershell
cargo test -p codex-elves-core --test cdp_bridge async_question -- --test-threads=1
cargo test -p codex-elves-core --test cdp_bridge -- --test-threads=1 --skip list_targets_can_query_ipv6_loopback_cdp_endpoint
cargo test -p codex-elves-core --test cdp_bridge list_targets_can_query_ipv6_loopback_cdp_endpoint -- --exact --test-threads=1
cargo build --profile release-fast -p codex-elves-manager --bin codex-elves-manager
# 在 apps/codex-elves-manager 下运行
npm run check
```

## 热更新验收

- 2026-11-10 19:22:06 完成独立后台切换；桌面管理器、启动器及 Codex 相关受保护进程保留。
- 本机、公网各 26 项网页资源大小和 SHA-256 校验通过；网页资源未变，revision 保持 `2026111002`。
- 原有两台设备授权及配置摘要一致，匿名 `/api/session` 继续返回 401。
- 原生连接、独立服务连接正常，`serviceCdpAttempts` 保持 0。
- 新后台程序 SHA-256：`0e9ebc204ff883987cf753d5f0cd7493f253fe04c8e5c67b290ccbc5f4f15251`；上一后台完整程序已备份可回退。
- 反馈会话可通过运行中的桥 137 正常读取。最新轮次中有 1 个问题及 1 条精确已接受的回答，当前待答请求为空；测试未发送业务答案。

## 未闭环项与下一步

- 手机端新问题的真实点击提交尚未验收；隔离状态操作和原生提交取消守卫不能替代该项。后续有效问题出现时，由用户按自己的选择提交。
- IPv6 本机连接测试受 Windows 套接字权限限制；当前实际 CDP 使用 `127.0.0.1` 且读取正常。本次未改动系统网络权限，不能将该检查计为通过。
- 本轮已完成源码修复和独立后台热更新；新安装包、Android APK 和正式发布不在本次验收范围。
