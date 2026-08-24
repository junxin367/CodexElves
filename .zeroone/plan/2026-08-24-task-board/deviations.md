# 执行期偏差登记

关联计划：`.zeroone/plan/2026-08-24-task-board/plan.md`
计划持有方：主控会话，L2 以上转用户
分级规则：见 `plan.md` 偏差处置区

## 偏差清单

### DEV-001：移除 W1 mutation 占位断言

- 日期：2026-08-24
- 分级：L0
- 状态：已解决
- 触发：T-001 的 `task_board_store` 测试暂时断言 create/move 返回 `Unavailable`，但 T-002/T-003 的冻结目标正是启用这两个 mutation；计划未把该占位断言的退场分配给 W2 写入范围。
- 处置：在 W2 波次整合时删除过期占位测试及其专用 import；创建和移动行为分别由 `task_board_create`、`task_board_move` 的完整测试闭包接管。
- 影响：仅调整测试衔接，不改变 CT-001/CT-002、生产代码、任务目标或回滚边界。

### DEV-002：将 W2 实机绑定记录并入 W3 端到端门

- 日期：2026-08-24
- 分级：L0
- 状态：执行中
- 触发：W2 源码、自动化测试与复审均已通过，但当前运行中的 launcher 是修改前二进制；单独重启它只为取得 W2 read-only Debug 记录会中断现有 Codex 使用。
- 处置：不降低 W2 自动化门；把 snapshot/catalog 真实 binding、modal DOM/交互记录并入 W3 的同一次 launcher 重建与端到端验证，同时覆盖真实 catalog/create/move/store 链路。
- 影响：仅调整实机证据采集顺序；CT-001–CT-005、W3 进入实现的代码依赖、验收内容和发布边界均不变。

### DEV-003：更新 W2 mutation 路由占位断言

- 日期：2026-08-24
- 分级：L0
- 状态：已解决
- 触发：T-006 的路由注册测试在 W2 期间用空对象断言 create/move 占位 handler 返回 `task_board_unavailable`；T-007/T-008 启用真实强类型 handler 后，空对象按 CT-001 应返回 `invalid_input`。
- 处置：W3 整合时只把该注册测试的预期码更新为 `invalid_input`；create/move 的完整行为分别由新 route test target 验收。
- 影响：仅移除过期占位预期，不改变路由、DTO、错误码契约或生产实现。

## 延迟修复清单

（执行期填写。）

## 熔断升级记录

（执行期填写。）
