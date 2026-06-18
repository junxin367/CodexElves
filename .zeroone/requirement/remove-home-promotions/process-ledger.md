# 过程证据台账 — 去除首页推广与 Zed 相关功能

- 关联澄清档案：remove-home-promotions
- 轮次：Round-001
- 最后更新：2026-06-17
- Ledger 状态：进行中
- 当前阶段：GATE

## 输入来源消费记录

| ID | 来源类型 | 父来源 | 重要性 | 位置 / 链接 | 本 skill 处理状态 | 获取时间 | 备注 |
|---|---|---|---|---|---|---|---|
| SRC-001 | 来源归一结果 |  | 必读 | `.zeroone/requirement/remove-home-promotions/source-prd.md` | 已消费 | 2026-06-17 | REQ-001~REQ-004 已进入规格与技术设计 |
| SRC-002 | 对话输入 |  | 必读 | chat | 已消费 | 2026-06-17 | 用户要求去除概览顶部官方中转站推荐、推荐内容、Zed 相关功能、请作者喝咖啡功能 |

## 需求登记表

| ID | 来源 | 类型 | 需求条目 | 处置 | 追踪 |
|---|---|---|---|---|---|
| REQ-001 | SRC-001 | 显式 | 去除概览页顶部官方中转站推荐 | 已覆盖 | `spec.md`；`technical-design.md` |
| REQ-002 | SRC-001 | 显式 | 去除推荐内容 | 已覆盖 | `spec.md`；`technical-design.md` |
| REQ-003 | SRC-001 | 显式 | 去除 Zed 相关功能 | 已覆盖 | `spec.md`；`technical-design.md` |
| REQ-004 | SRC-001 | 显式 | 去除请作者喝咖啡功能 | 已覆盖 | `spec.md`；`technical-design.md` |

### 模糊术语

- “去除”：本轮按“当前 UI 不展示、接口不暴露、注入菜单不可触达、文档不推荐”处理。

### 产品暂定前提

- TEMP-001：历史 `CHANGELOG.md` 是发布事实记录，不作为当前功能入口，默认不清理。

### 来源中的工程建议

- 当前无来源工程建议。

## Evidence Ledger / 证据台账

| Evidence ID | 轮次 | 证据类型 | 原词 / Token | 规范化理解 | 来源位置 | 证据路径 | 支撑项 | 必须保留的精确 token | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| EV-001 | Round-001 | UI 入口 | 官方中转站 | 概览顶部推荐卡片 | SRC-001 | `apps/codex-plus-manager/src/App.tsx` | REQ-001 | 官方中转站 | 已落点 |
| EV-002 | Round-001 | 功能入口 / API | 推荐内容 / `/ads` | 管理端推荐页和注入菜单远程推荐 | SRC-001 | `apps/codex-plus-manager/src/App.tsx`; `assets/inject/renderer-inject.js`; `crates/codex-plus-core/src/routes.rs` | REQ-002 | `/ads` | 已落点 |
| EV-003 | Round-001 | 功能入口 / API | Zed / `/zed-remote/*` | Zed Remote 打开、项目记录、设置项与后端桥接 | SRC-001 | `assets/inject/renderer-inject.js`; `crates/codex-plus-core/src/routes.rs`; `apps/codex-plus-manager/src-tauri/src/commands.rs` | REQ-003 | `/zed-remote/*` | 已落点 |
| EV-004 | Round-001 | 资源 / UI | 请作者喝咖啡 / `__CODEX_PLUS_SPONSOR_IMAGES__` | 赞赏 tab、二维码资源和 README 展示 | SRC-001 | `assets/inject/renderer-inject.js`; `crates/codex-plus-core/src/assets.rs`; `README.md` | REQ-004 | `__CODEX_PLUS_SPONSOR_IMAGES__` | 已落点 |

## Landing Matrix / 落点矩阵

| Evidence ID | 主落点 | 落点 ID / 章节 | 用户可读摘录 | 原词保真 | 处理结果 |
|---|---|---|---|---|---|
| EV-001 | spec / technical-design | REQ-001 | 概览顶部官方中转站推荐下线 | 原词+解释 | 已落点 |
| EV-002 | spec / technical-design | REQ-002 | 推荐内容入口和接口下线 | 原词+解释 | 已落点 |
| EV-003 | spec / technical-design | REQ-003 | Zed 相关功能下线 | 原词+解释 | 已落点 |
| EV-004 | spec / technical-design | REQ-004 | 请作者喝咖啡功能下线 | 原词+解释 | 已落点 |

## Landing Queue / 本轮落点缓冲

| ID | 发现轮次 | Evidence ID | 发现原词 | 基础对象 | 限定词类型 | 证据路径 | 触发来源 | 候选影响 | 影响等级 | 要求落点 | 状态 | 处理结果 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| LQ-001 | Round-001 | EV-001 | 官方中转站 | 概览 | 场景 | `apps/codex-plus-manager/src/App.tsx` | REQ-001 | 当前展示 | P1 | spec / technical-design | 已落点 | 删除当前展示 |
| LQ-002 | Round-001 | EV-002 | 推荐内容 | 推荐 | 行为 | 多处 | REQ-002 | 当前入口 / API | P1 | spec / technical-design | 已落点 | 删除当前入口与 API |
| LQ-003 | Round-001 | EV-003 | Zed | 编辑器集成 | 行为 | 多处 | REQ-003 | 当前入口 / API | P1 | spec / technical-design | 已落点 | 删除 Zed 专用能力，保留通用 SSH 解析 |
| LQ-004 | Round-001 | EV-004 | 请作者喝咖啡 | 赞赏 | 资产角色 | 多处 | REQ-004 | 当前展示 / 资源 | P1 | spec / technical-design | 已落点 | 删除赞赏入口与二维码注入 |

## 待分类分支

| Evidence ID | 分支 | 触发点 | 当前归类 | 阻塞级别 | 证据 / 下一步 |
|---|---|---|---|---|---|
| EV-004 | 历史发布记录是否清理 | 来源缺口 | 暂定 | P3 | CONFIRM-01；默认不清理 `CHANGELOG.md` |

## 已决策项

当前无 DEC。

## 暂定项

| ID | Evidence ID | 暂定内容 | 依据 | 被推翻影响 | 当前落点 |
|---|---|---|---|---|---|
| TEMP-001 | EV-004 | 不清理历史 `CHANGELOG.md` | 用户只要求去除当前功能和展示 | 如用户要求历史也清理，再单独改文档 | clarification / spec / technical-design |

## 待验证项

| ID | Evidence ID | 验证事项 | 类型 | 阻塞级别 | 关联项 | 下一步 |
|---|---|---|---|---|---|---|
| VERIFY-001 | EV-002~EV-004 | 格式检查、前端检查、核心路由测试不再引用推荐、赞赏、Zed | 实现期 | P1 | REQ-002~REQ-004 | 运行验证命令 |

## 风险项

| ID | Evidence ID | 风险 | 触发条件 | 影响 | 缓解 / 后续 | 关联项 |
|---|---|---|---|---|---|---|
| RISK-001 | EV-003 | 上游 worktree 复用了 Zed 模块里的 SSH 解析 | 直接删除整个模块 | 破坏非目标功能 | 抽出通用 `remote_ssh` 模块 | REQ-003 |

## GROUND 复核记录

| 复核ID | 复核方式 | 输入范围 | 失败项 | 结论 | 恢复动作 |
|---|---|---|---|---|---|
| REVIEW-001 | inline | source-prd / rg 命中 | 无 | 通过 | 进入实现 |

## Validator 结果

| Validator ID | Gate ID | 检查范围 | 必须满足的断言 | 结果 | 失败对象 | 修复动作 | 重跑结果 |
|---|---|---|---|---|---|---|---|
| VAL-001 | GA-08 | Round-001 | 当前功能下线范围无 P0/P1 产品阻塞，历史记录缺口按 TEMP-001 处理 | 通过 | 无 | 无 | 通过 |
