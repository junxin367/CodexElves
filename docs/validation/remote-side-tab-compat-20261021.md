# 侧边会话缺失修复与验收

日期：2026-10-21（北京时间）。

## RCA 结论

- 状态：已确认。代码缺陷、本机接口及公网接口验证完成；Android 真机页面未直接验收。
- 现象：主会话 `01a152f0-b78a-7f63-8801-36d9c09d536e` 的桌面右侧存在“侧边聊天”，远程关联会话接口却返回 `sideThreads:[]`。
- 直接原因：读取侧边标签时拿不到会话 ID，标签被筛掉。
- 根因：远程适配器固定读取 `tab.props.conversationId`；当前 Codex 原生侧边标签已使用 `tab.props.content.conversationId`，适配器没有兼容内容包装结构。
- 系统性根因：对原生内部标签结构的适配依赖单一布局，既有侧边同步验收记录针对 `26.1013`，没有覆盖当前 `26.1016` 内容包装结构。
- 关键因果链：原生创建侧边会话 → 标签保存 `props.content` → 适配器从旧字段读取到 `undefined` → 未取得内存会话 → 侧边条目被过滤 → Android 共用的远程接口返回空列表。

## 证据

| 判断 | 类型 | 证据 | 来源 |
| --- | --- | --- | --- |
| 桌面侧边标签存在 | 事实 | `local-side-chat` 标签标题为“侧边聊天”，`props.content.conversationId` 为 `01a15d79-24d2-78b2-ad21-e8e6dcbbedec` | 运行中 Codex 的原生面板状态 |
| 父关系成立 | 事实 | 同 ID 内存会话为 `sideConversation:true`、`ephemeral:true`，`forkedFromId` 等于目标主会话 | 原生 `getConversation` |
| 旧适配器遗漏条目 | 事实 | 桥版本 `58` 的 `relatedThreads` 返回空侧边列表；桌面标签同时存在 | 修复前真实只读调用 |
| 原生结构变化 | 事实 | 安装路径版本为 `OpenAI.Codex_26.1016.6573.0`；当前原生创建函数明确使用 `props:{content:g}` | launcher 状态与当前运行模块的创建函数 |
| 单一旧结构缺少兼容 | 结论 | 旧字段读取失败可复现；仅兼容嵌套字段后，同一真实会话及历史可以读取 | `assets/remote-access/native.js`、真实状态对照和临时回归脚本 |
| 既有验收覆盖不足 | 事实 | 上次原生同步验收针对 `OpenAI.Codex 26.1013.4894.0` | `docs/validation/remote-native-side-sync-20261021.md` |

## 已排除解释

- 会话已关闭：原生标签和内存会话均存在，修复后同 ID 可直接读取。
- 误把子代理当作侧边会话：原生 `threadSource:user`，父关系与主会话一致；子代理识别和排除条件未改变。
- 仅为手机缓存问题：修复前原生桥直接返回空列表；修复未改变网页资源版本，同一本机及公网接口随后返回正确条目。
- 缺少持久化登记：该会话按既定设计为原生临时会话，原生标签和内存状态足以确认关系，无需另建登记。

## 异常状态溯源

- 适用约束：当当前本机主会话存在未关闭的原生侧边标签，且对应内存会话确认属于该主会话时，`relatedThreads.sideThreads` 必须包含同一会话。
- 约束依据及状态：`docs/remote-session-tools.md` 中“两端同步列表”与“桌面原生侧边标签与远程菜单显示同一批会话”；已确认，无证据冲突。
- 本次运行前提命中判定：命中。
- 场景命中证据：上述目标侧边标签、内存父关系及临时会话属性同时成立。
- 值判断：空侧边列表违反约束；新版标签使用内容包装结构本身不构成非法状态。
- 值的实际来源与传播路径：原生创建函数将 ID 放入 `props.content`；远程枚举器读取旧字段，未取得内存会话并过滤条目，形成空列表。
- 最后满足约束的状态 → 首次约束违反点：原生标签和父子身份完整 → 远程枚举器使用旧字段过滤有效侧边条目。
- 调查边界与未知上游：已定位当前安装版的生产和消费边界；没有推断未来 Codex 版本的内部结构，也没有将 Android 真机展示描述为已验证。

## 修复与防复发

- 直接修复与根因修复：优先读取 `tab.props.content.conversationId`，兼容旧版 `tab.props.conversationId`；原生桥版本从 `58` 递增到 `59`，确保运行页面加载新逻辑。
- 身份约束：继续要求 `local-side-chat` 标签、原生侧边标记、内存父关系一致，并排除子代理。
- 系统性防线：本次验证覆盖新旧结构和真实缺失会话快照。升级 Codex 后应继续用实际原生标签与远程返回对照；不把单个历史版本的验收推广为未来版本兼容保证。
- 临时验证：10 项通过，覆盖新旧标签、右侧和底部顺序、创建中及无内存会话、主会话及其他父会话排除、子代理排除、非聊天标签、标题变化、关闭事件和父关系变化。按项目规则清理本次临时测试文件。
- 静态检查：`node --check assets/remote-access/native.js`、`git diff --check` 通过。
- 构建：`cargo build -p codex-elves-manager --bin codex-elves-manager --release` 成功，并核对程序完整嵌入修复后的适配器。
- 既有服务回归：`cargo test -p codex-elves-core --release --lib remote_access:: -- --test-threads=1` 通过，76 项成功、0 项失败、3 项按既有标记跳过（两个隔离进程入口、一个需另行授权的 ngrok 网络验证）。

## 服务交付与真实接口验收

- 原远程后台 PID `22220`，通过 Windows UAC 授权切换至新后台 PID `40908`；Codex 和 launcher 未重启。
- 运行程序：`C:\Users\junes\.codex-session-delete\remote-access-runtime\side-tab-compat-20261021-142a3edccf3c\codex-elves-manager.exe`。
- 程序大小：`59138560` 字节；SHA-256：`142a3edccf3cfc830204e638eb36bb033915ab899112e99b19bb3f058f91dd69`。
- 原程序保留于 `D:\foWork\CodexElves\codex-elves-manager.exe`，用于回退。安装目录尚未替换，本轮运行版本为独立验收后台。
- 运行桥版本为 `59`；目标侧边会话身份为 `viewKind:side`，父会话正确，保持 `ephemeral:true`。
- 本机 `127.0.0.1:45222` 和公网 `junxin367.shares.zrok.io` 的 `relatedThreads` 均返回目标“侧边聊天”，历史首屏均读取到 3 轮、19 条内容。
- 两处匿名 `/api/session` 均返回 401；原有 3 台设备授权及设置文件摘要保持一致，临时验收设备已撤销。
- 本机与公网资源版本均为 `e1e2fbb64ac1fbd2b28e0e9588480e032f805f4959d8b00a5e9f56b33bd41845`。本轮只修改后台原生适配器，网页资源版本无需变化。
- 公网初次验证出现 TLS 握手中断；随后使用系统 `curl.exe` 并限制重试后，真实鉴权读取、历史和资源核对通过。没有修改隧道或系统网络设置。
- 运行目录保存 `startup-result.json`、`native-verification.json`、`service-verification.json`，不包含访问密钥或设备 Cookie。

## 未闭环项与下一步

- Android 真机菜单和消息页面没有直接操作验收；本轮完成的是同一服务接口的真实公网验证。手机重新打开该主会话的“会话工具”菜单可触发读取。
- 本次未发送模型消息，未操作用户侧边会话的关闭或改名。
- 安装目录和安装包未更新；需要将修复纳入正式安装版本时另行构建交付。当前运行后台已经生效，重启后若重新使用旧安装目录启动服务，需要使用本次修复程序或更新安装版本。
- 本次未编译 Android、未提交、未推送、未创建发布标签或发布 Release。
