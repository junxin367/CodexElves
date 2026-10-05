# 2026-10-05 持续排查与修复记录

目标：在当前工作区完成 10 轮问题确认、必要修复与验证。保留此前未提交修复；只有复现或调用链证据支持的问题才修改。每轮可以确认没有发现新问题，不以制造改动作为完成标准。

## 轮次进度

| 轮次 | 检查范围 | 结论与修复 | 验证 |
| --- | --- | --- | --- |
| 1 | 工具与插件新增、删除的校验与保存失败路径 | 校验失败仍保存、保存失败仍返回配置，调用方继续同步；改为失败提示后返回空结果，停止后续同步 | 从实际 App.tsx 提取函数：原先 6 个失败场景复现，修复后 8/8 通过 |
| 2 | 本地代理日志详情的异步竞争 | 旧请求覆盖新选中项、关闭或清空后旧响应回填、同 ID 重复请求错误结束加载；增加请求序号并在关闭时失效；清空失败保留当前详情 | 5/5：乱序、关闭、清空、重复 ID、清空失败 |
| 3 | live 工具配置同步及插件刷新失败 | 失败响应中的空列表覆盖已有数据，同步失败仍关闭编辑器；仅成功时更新数据和关闭编辑器 | 9/9：三个读取/同步动作的成功与失败，以及编辑器同步成功、失败、调用异常 |
| 4 | 后端设置修改命令的互斥 | 重置全部设置、重置图片覆盖、激活皮肤、删除皮肤绕过已有写锁；四个命令统一加锁，全部设置重置改为异步命令 | 四个确定性持锁测试修复前全部失败；修复后 commands::tests 42/42 通过 |
| 5 | 缓存租约错误响应识别 | SSE error 事件或无 type 的错误 JSON 后接 DONE 会误建有效租约；统一检查明确错误类型、错误字段、失败状态，并识别 SSE 错误事件 | 两个新增表格测试先复现失败；compaction_cache::tests 13/13 通过，涵盖 8 类错误的 JSON/SSE 路径、分块 CRLF 事件以及正常输出中的错误词和 null error |
| 6 | 设置队列中的重置、皮肤切换与旧快照 | 虽然请求顺序正确，后续完整保存仍带旧字段撤销前面的重置/切换；将已确认结果合入排队快照和当前表单中未再次编辑的字段；失败结果不更新已保存元数据 | 首批 7 个队列用例中 5 个修复前失败，修复后全部通过；补充正在编辑的原始字段不被归一化改写的边界验证 |
| 7 | 切换聚合配置后旧请求结果的隔离 | 不同聚合或成员配置已变化时，旧请求失败仍推进新 selector；普通请求失败也会清空聚合状态；只向匹配的当前聚合配置记录结果 | 两个新增测试先复现失败，修复后 relay_rotation 17/17、relay_switch 9/9 通过 |
| 8 | 加权轮转的资源占用和边界 | 每次选择/探测按权重展开 String 向量；权重为 u32 且没有上限校验，最大值可能展开数十亿项。改用成员索引和已服务次数，保持原有连续重复顺序 | relay_rotation 集成 19/19，最大计数边界单测 1/1；包含最大权重、零权重、多轮顺序、重复探测。未在旧代码执行最大权重分配以避免耗尽内存 |
| 9 | 任务看板与 Checkpoint 存储完整性 | 核对锁、版本冲突、原子替换、坏文件保留、迁移目标限制和提交失败回滚；本轮未发现新增问题 | task_board_store/create/attach/move/delete/boards 共 60 项；workspace_checkpoint 17 项，合计 77/77 |
| 10 | 最终回归与完成核对 | 复核改动、补充未提交草稿保持原样的边界测试，确认前九轮修复未破坏协议及配置行为 | 最终前端 30/30；core 库 484 通过、2 原有忽略；六组 core 集成 528/528；manager 库 60/60；工作区检查、严格 TypeScript 检查、Vite 构建、格式及 diff 检查通过 |

## 最终验证清单

以下测试按目标去重，Rust 共 1149 项通过，另有 2 项原有忽略；前端函数级用例 30 项通过。

| 测试目标 | 通过数 |
| --- | --- |
| codex-elves-core --lib | 484 |
| launcher 集成 | 68 |
| protocol_proxy 集成 | 285 |
| relay_config 集成 | 117 |
| relay_rotation 集成 | 19 |
| relay_switch 集成 | 9 |
| responses_websocket 集成 | 30 |
| task_board_store/create/attach/move/delete/boards 集成 | 60 |
| workspace_checkpoint 集成 | 17 |
| codex-elves-manager --lib | 60 |

最终执行的主要命令：

```powershell
npm --prefix apps/codex-elves-manager run check -- --noUnusedLocals --noUnusedParameters
npm --prefix apps/codex-elves-manager run vite:build
cargo check --workspace
cargo test -p codex-elves-core --lib --test protocol_proxy --test responses_websocket --test relay_rotation --test relay_switch --test relay_config --test launcher -- --test-threads=1 --quiet
cargo test -p codex-elves-core --test task_board_store --test task_board_create --test task_board_attach --test task_board_move --test task_board_delete --test task_board_boards --test workspace_checkpoint -- --test-threads=1 --quiet
cargo test -p codex-elves-manager --lib --bin codex-elves-task-board -- --test-threads=1 --quiet
cargo fmt --check
git diff --check
```

`codex-elves-task-board` 二进制目标本身包含 0 个测试，未计入上述通过数；看板逻辑由列出的 core 集成测试及 manager 库测试覆盖。

## 验证边界

- 前端测试从实际源码 AST 提取函数，使用可控延迟和状态模拟；不是浏览器或安装版端到端测试。
- 第 1–3 轮 22 个用例，加上第 6 轮及最终补充的 8 个队列用例，共 30 个。
- 临时前端测试脚本验证后已清理；新增 Rust 回归用例保留在现有测试模块中，使用临时目录及现有进程状态隔离。
- 2 项忽略测试分别需要真实的两个三分钟恢复周期和本地图片失败请求 fixture，本次未启用。
- Vite 构建成功，主 JS 分块约 660 kB（gzip 约 196 kB），触发默认 500 kB 体积提示。
- 本次完成源码修复和验证，没有打包、安装、提交或推送。
