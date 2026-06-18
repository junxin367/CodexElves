# 技术设计

## 关联需求规格

- `spec.md`

## 功能归属

- 管理端：`apps/codex-plus-manager/src/App.tsx`、`apps/codex-plus-manager/src/styles.css`
- Tauri 命令层：`apps/codex-plus-manager/src-tauri/src/commands.rs`、`apps/codex-plus-manager/src-tauri/src/lib.rs`
- 核心桥接：`crates/codex-plus-core/src/routes.rs`
- 注入脚本：`assets/inject/renderer-inject.js`
- 文档与资源：`README.md`、`README_EN.md`、赞赏图片资源

## 现有能力扫描

- `apps/codex-plus-manager/src/App.tsx` 当前包含概览顶部官方中转站卡片、`zedRemote` 路由、`recommendations` 路由、Zed 设置项和推荐内容组件。
- `assets/inject/renderer-inject.js` 当前包含 `/ads` 拉取、推荐 / 赞赏 tab、`__CODEX_PLUS_SPONSOR_IMAGES__` 图片引用、Zed Remote open 运行时代码和 `/zed-remote/*` 调用。
- `crates/codex-plus-core/src/routes.rs` 当前暴露 `/ads` 和 `/zed-remote/*`。
- `crates/codex-plus-core/src/upstream_worktree/remote.rs` 复用 `crate::zed_remote::{SshTarget, resolve_ssh_target_for_host_id}`，需要抽成通用 SSH 解析模块。

## 证据闭环说明

本次是功能下线，不新增数据链路。需要切断当前入口到后端能力的链路：UI 不展示，注入脚本不触发，Tauri 命令不注册，桥接路由不处理，文档不推荐。

## Source-to-sink 固定骨架

- 推荐内容：导航 / 注入 tab -> `/ads` -> 远程广告列表，整条链路删除。
- Zed：UI / 注入菜单 -> `/zed-remote/*` 或 Tauri 命令 -> Zed 打开 / 项目记录，整条 Zed 专用链路删除。
- 请作者喝咖啡：注入 tab / README -> 赞赏二维码资源，当前展示链路删除。

## 状态规则执行图

无新增状态规则。

## 可复用资源

- 通用 SSH host 解析能力从 Zed 模块抽出，供 Upstream worktree 继续复用。

## 复用 / 扩展 / 抽象 / 新建判断

- 删除 `ads` 模块和 Zed 专用模块。
- 新建或保留中性 `remote_ssh` 模块，只承载 `SshTarget` 和 `resolve_ssh_target_for_host_id` 等与编辑器无关的能力。

## 备选方案与取舍

- 备选 A：只隐藏 UI，保留接口。放弃，因为用户要求去除功能，保留接口会留下可调用入口。
- 备选 B：删除专用入口和接口，保留通用 SSH 解析。采用，因为能满足 Zed 下线且不破坏 Upstream worktree。

## 数据设计判断

- 不新增数据结构。
- 旧设置字段可由 serde 忽略或保留未知字段，不做迁移。

## API 设计判断

- 删除 `/ads` 和 `/zed-remote/*` 当前桥接处理。
- 删除 Tauri `load_ads`、`list_zed_remote_projects`、`open_zed_remote`、`forget_zed_remote_project` 命令注册。

## 权限设计判断

无新增权限。

## 状态迁移与业务规则

无状态迁移。

## 异常、并发、幂等与失败处理

旧调用命中已删除桥接路径时按 unknown path 返回，不新增兼容层。

## 影响评估

- 前端导航项减少。
- README 中推广、推荐、赞赏和 Zed 功能描述减少。
- 测试需同步删除或改为不可用断言。

## 场景压力矩阵

| 场景 | 预期 |
|---|---|
| 打开管理端概览 | 不出现官方中转站推荐 |
| 打开管理端导航 | 不出现 Zed 远程项目、推荐内容 |
| 打开注入菜单 | 不出现推荐内容、请作者喝咖啡、Zed Remote open |
| 请求 `/ads` 或 `/zed-remote/status` | 返回 unknown path |
| 使用 Upstream worktree 远程项目 | 通用 SSH 解析仍可用 |

## 关键疑点覆盖检查

- 历史 `CHANGELOG.md` 是否清理：见 CONFIRM-01，默认不清理。
- Upstream worktree 依赖：通过通用 SSH 模块保留。

## 项目约定与约束

- 不使用 Git worktree。
- 不主动创建临时测试文件。
- 删除二进制资源时只删除本次明确关联的赞赏二维码资源。

## 已决策项引用

当前无 DEC。

## 风险

- RISK-001：误删通用 SSH 解析会破坏 Upstream worktree。

## 待确定项引用

- CONFIRM-01
