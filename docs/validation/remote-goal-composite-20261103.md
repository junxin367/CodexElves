# R1-20 目标标签与复合草稿续验

**一项 Skill＋一张测试图片的本地复合草稿子场景通过；R1-20 整行仍为 partial。** 本次只操作独立 Chrome 标签 `1283922810`、C 会话 `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`，准确 URL 为 `http://127.0.0.1:1432/#thread=01a1996d-0b7a-71d0-b9ad-9c521d224ad9`。北京时间 2026-11-03 01:31–01:41，原件时间使用 UTC。开始时正文、Skill、图片和目标模式均为空，pending、发送请求、消息/队列编辑均为 null，队列为空；`baseline.json` 保留原草稿。

实际执行的 `app.js` 已用 `Debugger.getScriptSource` 保存为 `app-runtime.js`，SHA-256 为 `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`，与本次源码及恢复刷新后脚本一致。期间主代理通知原生由 119/106 临时切到 121/106、前端不变；本项只验客户端草稿，不将跨阶段数据写成同版原生目标创建验收。所有证据位于 `dist/validation/remote-acceptance-20261103/goal-composite/`。

| 真实操作 | 核对结果／原件 |
| --- | --- |
| 输入 `/root-cause`，点击既有 `Evidence-Closed RCA`，逐字输入两行测试正文；从加号菜单的文件选择器选图 | Skill 为 `root-cause-analysis`，路径 `C:\Users\junes\.agents\skills\root-cause-analysis\SKILL.md`，初始 `_offset=0`。图片是从 E29 `queued-before-edit.json` 精确恢复的 `perf-blue.png`，24×24、172 字节，SHA-256 `63db222d0886ab1a22d1a675af881b00a9aed9ea8ef69c8dbef2909aee8a06d4`；未使用私人图片。见 `fixture-identity.json`、`prepared-composite.json/jpg` |
| 在已有 Skill 和图片时，从“＋”选择“目标” | 正文、完整 Skill 对象和图片逐字段不变，目标标签恰好一个，按钮变为“创建目标”；未点击按钮。见 `goal-selected.json/jpg` |
| `Ctrl+Home → ArrowRight`，逐字键入 `标签后真实输入：` | 实测选区为 `#prompt` 的折叠 offset 1，即目标标签后、Skill 前。正文前插 8 字，Skill 身份不变、偏移正确变为 8，图片字节保持。见 `caret-after-goal.json`、`goal-typed.json/jpg` |
| `Ctrl+Home → Delete`，只删目标标签 | 目标模式恢复 null、标签数为 0、按钮恢复“发送消息”；正文、Skill 全字段（含 `_offset=8`）、图片逐字段及解码 SHA-256 与删除前相同。见 `caret-before-delete.json`、`goal-removed.json/jpg` |
| 正常全选删除测试正文/Skill，点击移除测试图片；刷新确认后关闭自己的标签 | 原空草稿逐字段恢复，刷新后仍空，目标/队列为空、无未决写；模型保持 `gpt-6.1-terra / Max`。观察断点已移除，独立标签已关闭，主代理 `1283922800` 仍在。见 `restored-before-reload.json`、`restored-after-reload.json/jpg`、`cleanup-before-close.json`、`tab-closed.json` |

网络证据从 cursor 528 连续保存至 2095，动作窗口分页均无截断；最初脚本发现缓冲曾截断，与后续动作取证窗口分开记录。草稿准备、选目标、键盘输入、删标签和恢复空稿期间 `/api/action/*` 为 **0**，全记录中 `setGoal/updateGoal/send/create/answer` 均为 **0**，服务器目标写入为 **0**。用于确认空稿持久化的刷新在 `2026-11-02T17:38:07.584Z` 自动触发一次 `markRead`（网络请求 `87568.333`，回执 `8009027f-46be-443c-946f-9ce9242e1e5f`，completed/ok）；因此**包含恢复刷新的全流程不是零 action**，不能把只读 POST 或这次自动已读隐藏。原件为 `network-events.json`、`reload-auto-mark-read.json`，逐字段断言结果为 `verification.json`。

本次未补双图、重复选择目标、能力不足、跨端/刷新复合草稿恢复，也未创建服务器目标；不扩大为 R1-20 或完整 R1/R2/R3 通过。未改产品或脚本、未重启服务、未调用原生写 RPC、未提交或打包。

## 双图续验（2026-11-03 02:03–02:08）

**双图与重复选择子场景已补齐，通过；能力不足继续未验。** 另建自己的 C 标签 `1283922814`，未操作主代理 `1283922800` 或 M。开始再次确认原空稿、无未决提交/编辑、队列和服务器目标为空。实际运行和刷新后的 `app.js` 均为上述 `53b01a37…bc7b22`，与源码逐字一致；主代理已通知原生恢复 119/106，本项仍只归属客户端草稿验收。新增原件全部位于同目录的 `two-images/`。

| 对象／操作 | 核对结果 |
| --- | --- |
| 两张不同图片 | 顺序为 `perf-blue.png` → `red.png`。蓝图复用上一轮 E29 原件，172 字节、SHA-256 `63db222d0886ab1a22d1a675af881b00a9aed9ea8ef69c8dbef2909aee8a06d4`；红图精确复制旧多图验收夹具 `continuation-sources/media/red.png`，64×64、178 字节、SHA-256 `04e4409b6cc1446ef075ac418fe4214fc65ce8f763c2bacf51c8d1d6d74f5e18`。两图均通过正常文件选择器添加，身份见 `fixture-identities.json` |
| Skill＋正文＋双图中从同一“＋→目标”入口连续选择两次 | 两次均恰好一个目标 token，草稿目标 ID 均为 `f42cb369-2640-4cfa-91f6-09759103233e`；正文、Skill 全字段和两图完整附件对象逐字段不变。见 `prepared.json`、`selected-once.json`、`selected-twice.json/jpg`、`repeat-menu.json` |
| 标签后真实输入，再只删除标签 | `Ctrl+Home → ArrowRight` 后实测选区为 `#prompt` 折叠 offset 1；逐字输入 `双图标签后输入：`，Skill 身份保持、偏移正确由 0 变为 8。随后 `Ctrl+Home → Delete`，仅目标模式变为 null、token 数归零；正文、含偏移的 Skill 对象和两图对象逐字段保持，两图名称、顺序、解码字节及各自 SHA-256 全部与基线一致。见 `caret-after-goal.json`、`typed.json/jpg`、`removed.json` |
| 实际绘制和收尾 | 即时 `removed-transition.jpg` 仍是删除前的合成过渡帧，保留但不用作最终显示证据；重新观察后的 `removed-rendered.json` 和 `removed.jpg` 显示目标已移除、Skill 与蓝红两图保留。最后通过正常编辑清空本轮草稿并移除两图，刷新后与原空稿逐字段相等，模型仍为 `gpt-6.1-terra / Max`；观察点全部移除，自己的标签已关闭，主代理标签仍在。见 `restored-before-reload.json`、`restored-after-reload.json/jpg`、`cleanup-before-close.json`、`tab-closed.json` |

新增连续网络范围为 cursor 150–1288，动作窗口各页无截断。双图准备、两次选择、输入、删标签及恢复空稿期间 `/api/action/*` 为 **0**；全记录 `setGoal/updateGoal/send/create/answer` 均为 **0**，服务器目标写入为 **0**。恢复刷新在 `2026-11-02T18:07:29.334Z` 自动触发 `markRead` 一次，网络请求 `78688.247`，原回执 `77bd3531-7a73-4edc-b4d3-dfe089b13d28` 为 completed/ok；仍不能将含刷新的整段写成零 action。见 `network-events.json`、`reload-auto-mark-read.json` 和逐字段断言结果 `verification.json`。

本续验未改产品、测试或模型，未发送或创建服务器目标，未人为修改服务端能力。双图和重复选择不再列为本报告剩余项；能力不足、非空复合草稿的跨端/刷新恢复仍未验，R1-20 整行保持 partial。
