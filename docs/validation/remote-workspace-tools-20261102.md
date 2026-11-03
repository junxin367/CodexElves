# 文件与变更页面：真实原件独立复核

日期：2026-11-02，Asia/Shanghai。原件目录：`dist/validation/remote-acceptance-20261102/workspace-git-final/`，下文文件名均相对于该目录。

**结论：通过本文列出的正常子场景；R1-30、R2-16 整条仍为 partial。** 真实 Git status、Git diff、浏览器 HTTP 和界面记录共同支持：无 HEAD 仓库中暂存/未暂存/未跟踪内容分离，详情条件读取，19 字节同长度修改后的内容版本更新，以及两个真实根目录的正常往返、同名内容和筛选隔离、非空普通草稿保持。

本轮不能包装成同一版119完整验收：初始A变更使用安装适配器106，B文件及返回A文件使用118插桩，最后B变更及返回A变更使用119插桩。工作区Rust模块本轮未修改，但不同原生/网页运行阶段仍分别保留。非零滚动、拒绝路径、撤权、迟到响应竞争、所有Git状态、DOM对象复用与图片请求复用均未因此通过。

本子任务只读归档和必要源码，只新增本报告；没有操作实际夹具目录、Git、RPC、浏览器或业务状态，没有运行产品测试、提交或发布。

## 1. 产品依据与核验方式

预期来自 `docs/remote-session-tools.md`，不是从实现输出反推：

- “变更”要求当前真实worktree、未提交三分组、staged比较HEAD与index、unstaged比较index与工作目录、untracked显示新增内容；详情与列表分级返回。
- “文件”要求当前项目内的目录和内容、Markdown默认排版、相对路径搜索、返回保留状态；Git项目使用当前worktree根，非Git使用实际cwd。
- “App缓存”和接口约定要求按授权、会话/项目、分组和路径隔离；未变化以 `ifRevision` 小型确认，每次详情独立核对内容版本，不能因为列表仍显示M就认定内容没变。

实现与既有测试只用于确认可复核的处理方式：`workspace.rs` 的 `Repository::discover`、`detail`、`revision_payload`，以及 `workspace_git_empty_head_index_and_literal_paths`、`workspace_details_use_index_worktree_images_deletions_and_binary`、`workspace_revisions_track_visible_lists_and_same_length_content`。本次没有重跑这些测试。

独立复算没有使用 `verification.json` 的true值作为通过依据，具体执行了：

1. 解析原始 `a-status.txt`，将Git的XY状态独立展开为分组条目，与真实接口列表逐项比较。
2. 将接口中的三个文本diff与对应Git自身生成的 `.diff` 比较；只统一归档文本的CRLF/LF，不改正文、路径、hunk或Git对象标识。
3. 对八份完整响应移除 `revision`，按归档响应原字段顺序紧凑序列化为UTF-8后重新计算SHA-256，均与返回revision一致；没有自行改成键排序后的其他序列化。
4. 对九份HTTP归档分别按准确network requestId核对唯一工作区请求、对应HTTP200、`loadingFinished`、`Cache-Control: no-store`，并核对声明的Content-Length与配对响应原文件的UTF-8字节数。
5. 直接重算 `work-A/work-B`、两份Markdown和 `next-A` 的UTF-8内容摘要，与原始摘要记录、diff或文件内容交叉核对。
6. 比较实际UI文本、AX中的当前会话URL/路径/页面容器、筛选实际值与非空编辑器值，不用文件名或汇总 `matches:true` 替代界面内容。
7. 对20:38后补的 `git-final-hashes.json`，按label和完整root绑定初始 `git-baselines.json`，逐项比较index和Markdown原始SHA，并重新计算两份预期工作正文的SHA；没有用汇总true值代替。

## 2. 对象、夹具与运行阶段

| 对象 | 准确身份 | 初始独立资料 |
| --- | --- | --- |
| A | threadId `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`；根 `C:\Users\junes\Documents\Codex\2026-11-02\remote-e2e-ra-20261102-input-integrity` | `a-status.txt`、`a-staged.diff`、`a-unstaged.diff`、`git-baselines.json`、`source-a-before.txt` |
| B | threadId `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`；根 `C:\Users\junes\Documents\Codex\2026-11-02\remote-e2e-ra-20261102-queue-status` | `b-status.txt`、`b-staged.diff`、`b-unstaged.diff`、`git-baselines.json` |
| 公共相对路径 | `work/remote-acceptance-source.txt`、`remote-acceptance-shared.md` | 同名不代表同一对象；A/B的原生会话、根、内容标记和revision分别核对 |

`git-baselines.json` 分别记录两目录HEAD检查退出码1；`fixture-state.json` 登记没有创建HEAD。归档staged补丁从 `/dev/null` 添加index内容，符合该无提交夹具。这里没有把“已有HEAD下的暂存修改”与“无HEAD下的暂存新增”混为同一覆盖。

| 运行阶段（主代理明确登记） | 本目录对应材料 | 本次保留的边界 |
| --- | --- | --- |
| 安装适配器106 | A初始列表、staged/unstaged/untracked详情、重复详情及同长度更新：`a-changes-*`、`a-staged-*`、`a-unstaged-*`、`a-untracked-*`、`a-repeat-*`、`a-changed-*` | 不写成119下执行，也不把插桩版本当成已安装版本 |
| 118插桩 | `b-shared-*`、`a-shared-return-*`，B原文件页与返回A原文件页 | 这些文件读取与初始A的Git详情属于不同阶段 |
| 119插桩 | `b-changes-ui.txt`、`b-unstaged-*`、`a-return-diff-ui.txt`、`a-filter-restored-ui.txt` | 最后返回A差异有UI原件，但本目录没有独立的该次A详情HTTP对 |

阶段原生版本来自主代理本次明确登记；本目录未包含各阶段独立的引擎全文或完整运行摘要，本报告不自行补造。HTTP记录证明实际工作区请求与返回，不能单独证明全套原生脚本身份。

本次读取的工作区源码SHA-256为 `868c8dc22f46734670f29a0e6f88006f41398a60cbf35cfd6bf6b0e2da53f059`；产品文档SHA-256为 `184c31dae5358b9758a1635b07f3e1f54525e81dd398bf14d9858ab767cece47`。源码摘要用于定位本次静态核对，不替代运行二进制身份。

## 3. 真实Git与页面内容对照

### 3.1 三分组与准确比较对象

`a-status.txt` 的原始 `AM work/remote-acceptance-source.txt` 展开为：

- staged：同一路径、status A。
- unstaged：同一路径、status M。

另有四个 `??` 文件，分别为PNG、binary、large文本和Markdown，展开为四条untracked/A。独立解析得到的**六条完整条目及顺序**与 `a-changes-response.json /files` 一致，`truncated=false`。UI的六项变更、暂存1、未暂存1及未跟踪项与接口相符。

| 对象／分区 | Git原始预期 | 接口及UI结果 | 原件 |
| --- | --- | --- | --- |
| A，staged/A | 旧侧不存在；新增 `index-A` 与 `shared-line` | `before.exists=false`，after20字节，`+2/−0`；不含工作目录的 `work-A` | `a-staged.diff`、`a-staged-response.json /diff`、`a-staged-ui.txt` |
| A，unstaged/M | `-index-A`、`+work-A`，保留上下文 `shared-line` | 与Git补丁逐字一致；before20字节、after19字节，UI `+1/−1` | `a-unstaged.diff`、`a-unstaged-response.json`、`a-unstaged-ui.txt` |
| A，untracked/A | `remote-acceptance-shared.md` 为新增Markdown，三行含空行及 `ROOT_A_VERSION_1` | 旧侧不存在，after32字节，UI `+3/−0`，标记与正文一致 | `a-untracked-response.json`、`a-untracked-ui.txt` |
| B，unstaged/M | `-index-B`、`+work-B`，保留 `shared-line` | 与 `b-unstaged.diff` 逐字一致；没有借用A内容，UI `+1/−1` | `b-unstaged-response.json`、`b-unstaged-ui.txt` |

A、B staged原生Git补丁分别指向blob前缀 `ab686f7`、`19b7e0b`；工作内容分别为 `b766ce2`、`b06a3bc`。它们是Git自身输出中的不同身份，不能只因路径相同省略根目录与分区。

PNG、binary和large文件本轮仅在列表出现，**没有对应详情/图片解码/限额原件**，因此不把它们计为图片前后预览、二进制说明或4MiB截断验收。

### 3.2 同长度修改与详情条件读取

初始A工作正文为 `work-A\nshared-line\n`，19字节，SHA-256为：

`2256e23604d10d4323a993a1358b41cb2469083dbfa930974b1f818f45bccde9`

再次打开同一unstaged详情，原请求确实携带初始revision，响应为：

```json
{
  "ok": true,
  "notModified": true,
  "revision": "eb107f9ae5ab26edfe5f2871a5738db866c27a03b1a658e518522591ca950e6c",
  "root": "\\\\?\\C:\\Users\\junes\\Documents\\Codex\\2026-11-02\\remote-e2e-ra-20261102-input-integrity"
}
```

该确认响应恰有四个字段，没有diff、before、after或文件列表；保存原件211字节，对应HTTP Content-Length也为211。重复后的界面仍显示准确 `index-A→work-A`。原件为 `a-repeat-http.json`、`a-repeat-response.json`、`a-unstaged-repeat-ui.txt`。

随后经专用夹具操作将正文改为 `next-A\nshared-line\n`，仍为19字节。独立重算SHA-256为：

`16a5499a3a2bc3914e4b82c326803c8e2f1ed099c2dcaca9fb43f89a30c5c3e7`

与 `a-same-length-mutation.json /afterSha256` 一致。后续请求仍携带旧revision，但返回全量新详情：

- 初始：`eb107f9ae5ab26edfe5f2871a5738db866c27a03b1a658e518522591ca950e6c`。
- 修改后：`212f3d0140d54065d98516372f096f6fc89c39e8cb3da4b44fa03100bd0cb63f`。
- 新diff为 `-index-A/+next-A`，after仍19字节；新Git工作blob前缀为 `4d90171`。
- `a-changed-ui.txt` 和最后的 `a-return-diff-ui.txt` 均显示新正文，没有退回 `work-A`。

这证明该次**详情内容版本**发现了同长度更新，不能扩大为目录列表、变更列表的notModified、图片缓存、所有更新竞争或持久缓存恢复通过。目录内没有对应这些分支的原请求/响应。

## 4. HTTP原始关联与revision复算

以下每组 `*-http.json` 均为其独立采样窗口：恰一个工作区请求、`truncated=false/hasMore=false`，同network ID有HTTP200及完成事件，响应头 `Cache-Control: no-store`。每组配对 `*-response.json` 的UTF-8字节数都与该请求响应的Content-Length相等。

| 原件前缀 | 准确network requestId | 请求对象 | 响应字节 | 独立复核 |
| --- | --- | --- | ---: | --- |
| `a-changes` | `88792.143` | A，`/api/workspace/changes` | 695 | 六条与Git status一致，完整响应revision复算一致 |
| `a-staged` | `88792.149` | A，source路径，section=staged | 684 | 原生Git diff、完整响应revision一致 |
| `a-unstaged` | `88792.159` | A，source路径，section=unstaged | 714 | 原生Git diff、完整响应revision一致 |
| `a-untracked` | `88792.168` | A，shared.md，section=untracked | 563 | 新增Markdown全文、完整响应revision一致 |
| `a-repeat` | `88792.184` | A，unstaged，原ifRevision | 211 | 同revision、四字段notModified，未用确认体重新计算完整内容hash |
| `a-changed` | `88792.196` | A，unstaged，仍携带旧ifRevision | 714 | 全量新正文、新revision，独立SHA-256一致 |
| `b-shared` | `88792.324` | B，`/api/workspace/file`，shared.md | 300 | 原文件文本为ROOT_B，完整响应revision一致 |
| `a-shared-return` | `88792.681` | A，`/api/workspace/file`，shared.md | 303 | 原文件文本为ROOT_A，完整响应revision一致 |
| `b-unstaged` | `88792.759` | B，source路径，section=unstaged | 711 | B原生Git diff、完整响应revision一致 |

以上是九个归档窗口的检查，不是覆盖整个测试期间的连续请求账本，不能据此声称整个阶段只有九次读取、完全没有其他请求或零业务写入。

原响应root中Git详情采用Windows规范化路径 `\\?\C:\...`，文件内容采用 `C:/...`。比较时统一分隔符并去除合法扩展路径前缀，二者仍对应准确A/B目录，未把文本表现不同误判为串根。

## 5. 跨根、文件页面与返回状态

| 子场景 | 独立核对结果 | 证据边界 |
| --- | --- | --- |
| A文件变更根 | `a-changes-ui.txt` 的当前会话URL/路径及变更顶部均指向A；真实请求threadId也为A | 原生cwd的初始来源登记在夹具材料，本次未再次调用原生读取 |
| B文件内容 | `b-shared-http.json` 请求B；响应root为B、path为同名shared.md，正文为 `# 工作区 B\n\nROOT_B_VERSION_1\n`；`b-shared-ui.txt` 显示B完整路径和Markdown排版 | 是真正 `/api/workspace/file`，不是把Git diff当原文件内容 |
| 返回A文件内容 | `a-shared-return-http.json` 请求A；响应root为A、正文为ROOT_A；完整AX中 `session-files-content`、A路径及“工作区A”对应 | `a-shared-return-ui.txt` 同时保存当前A会话URL和非空普通草稿 |
| A/B同名内容摘要 | 对两份文件响应的Markdown文本重新计算SHA-256，分别与 `git-baselines.json /markdownHash` 一致；均32字节，但内容与摘要不同 | 证明各自文件读取时内容与初始基线一致，不代表全部文件结束状态已审计 |
| B变更→返回A变更 | `b-changes-ui.txt` 顶部是B，B未暂存详情为index-B→work-B；`a-return-diff-ui.txt` 是index-A→next-A，符合A更新后的原件 | 最后A返回差异仅有UI文本，没有独立配对的该次HTTP原件，不能推断是否再次获取了全量内容 |
| 筛选隔离 | A返回完整AX中筛选实际值为 `remote-acceptance-source`；B完整AX搜索为空，`b-filter.json /value` 也为空；回A后仍为原筛选并只显示source两个分区 | 比较实际AX/值，不仅采用 `filter-isolation.json /matches`。没有非零滚动或展开状态反例 |
| 普通草稿 | 初始 `a-changes-ui.txt` 与返回A文件页的完整AX均有 `GIT_TOOLS_DRAFT_20261102：仅保留，不发送。`；`a-draft-return.json /draft` 同文 | `a-draft-return-ui.txt` 本身是“AX无变化”的增量，不能单独证明编辑器值；采用前述完整AX与实际值交叉核对 |

这里的首次A Markdown原件是**Git未跟踪差异页**；随后明确归档的原文件页是B→A。因此可以确认正常往返中的同名内容不串根，不能写成“三次A→B→A均在同一个文件页面层级取得完整原件”。源码/Markdown切换按钮存在，但没有独立执行切换的原件。

本轮notModified是服务端小型确认的真实证据；界面内容保持也有记录。但没有同DOM对象引用、MutationObserver、图片请求前后对照，**不能声称客户端未重建DOM或未重新下载图片的分支已通过**。

## 6. 只读性及收尾的独立边界

### 6.1 index与工作文件摘要

`git-baselines.json` 保存的初始index SHA-256，与后补结束状态的比较如下：

| 根 | 初始与后补结束 `.git/index` SHA-256 | 后补采集时间（Asia/Shanghai） |
| --- | --- | --- |
| A | `9b49f2c790592213636a88448308c667634263f167680b73f73e68c4fe304ae0`，两次一致 | 2026-11-02 20:38:42.5177882 |
| B | `274c2b5a9e6dc75ed0a6bab7c4d98243f273063ecfdbb923a4718762e21a568a`，两次一致 | 2026-11-02 20:38:42.5587196 |

初审只读到20:17:38的 `verification.json /files/*/indexUnchanged=true` 汇总，当时缺少结束原始摘要，故未将index只读性升级为独立通过。随后主代理于上述20:38时间执行真实只读采集并归档 `git-final-hashes.json`，没有修改夹具或增加UI操作。本次按label及完整root逐一绑定初始记录，直接比较两份index原始摘要，均一致；两目录的结束HEAD检查也均为 `headExit=1`。

结束内容另经独立重算核对：

- A工作文件仍为本轮明确修改后的 `next-A\nshared-line\n`，19字节；SHA-256为 `16a5499a3a2bc3914e4b82c326803c8e2f1ed099c2dcaca9fb43f89a30c5c3e7`，与20:38原始采集值及先前同长度修改记录一致。不能将它误称为恢复了初始work-A或夹具准备前的index-A正文。
- B工作文件仍为 `work-B\nshared-line\n`，19字节；SHA-256为 `02834c364020dd9b8b4f5d6fdd74eb298ce5ed559e69fe828e37a776d8cda051`，与初始及结束原始记录一致。
- A/B的Markdown结束SHA分别为 `4e2ad8366f08a5497fb8b62e9faa7d21a02efeb34341b290910bb7ae3a05708b`、`dbbb64f0b8eef29a5616b44d0770d8582bede93056137f84f42ea456f3e8d750`，均与各自初始摘要及预期32字节正文一致。

因此，**初始与后补结束状态之间的index摘要一致、工作文件符合明确预期、Markdown未变及无HEAD，现已独立闭环**。这是20:38的后补结束状态证据，不回填20:17汇总或更早HTTP/UI的采集时间，也不代表期间每个瞬间都留有连续文件状态记录。它不改变下节夹具清理受阻的事实，不能据此宣称测试目录已恢复原状。

### 6.2 已记录的收尾与剩余物

- `draft-cleanup.json` 登记原普通草稿为空，收尾当前草稿也为空。
- `browser-cleanup.json` 登记测试标签1283922798已关闭、缓存覆盖已恢复。
- `service-cleanup.json` 登记已核对身份并停止1432/PID84288，时间2026-11-02 20:26:33。
- `fixture-cleanup.json` 明确记录2026-11-02 20:28:58的清理被自动审批拒绝：命令包含递归删除测试Git元数据，整条命令 `executed=false/status=blocked_by_policy`，没有更换命令重试。

因此只能报告浏览器/调试服务已有各自收尾记录；**夹具清理未执行，不能宣称Git元数据和测试工作文件已复原或删除**。本次未重新检查实际进程、浏览器库存或夹具目录，也没有执行清理。

## 7. 基线增量与未完成项

| 基线 | 本轮可加入的真实子场景 | 整体状态 |
| --- | --- | --- |
| R1-30 文件／变更／浏览器入口 | 当前Git根的三分组列表与各分区文本详情；文件内容页的Markdown显示；分级返回后筛选与非空草稿保持；同名文件正常跨根不串内容 | partial。未覆盖图片/二进制/超限详情、非Git、错误提示和Android浏览器，不能将入口列表中的文件当作已预览 |
| R2-16 菜单／文件缓存隔离 | 两真实根的同名差异和文件读取；正常切换时筛选隔离；详情ifRevision确认及同长度更新返回新正文/新revision | partial。未覆盖迟到竞争、授权/路径失效、网络失败缓存标识、DOM/图片复用、非零滚动和持久恢复 |

后续应分别执行，不能继承本轮通过：

以下仅指本报告尚未取证的分支；同日 `workspace-tools-final/` 已完成普通文件图片／二进制／超限和非Git场景，汇总见当前验收表E31，不再作为完全未执行项重复验收。

1. 在足够长列表上记录**非零**滚动，详情→列表、列表→会话→返回后检查筛选/展开/位置与普通草稿。
2. 非Git正常场景沿用E31的独立原件；路径失效或错误缓存等新增分支需要另行取证，测试目录仍须确实不在其他Git根之内。
3. 对专用对象验证 `../`、项目外绝对路径、`.git`、符号链接/重解析点拒绝，旧revision不能绕过路径和授权校验。
4. 对已获准测试授权验证撤销、旧响应晚到、新scope不能接收旧内容；正常A/B往返不能代替。
5. 保持状态字母不变时继续核内容版本，另验列表/目录的notModified、DOM/图片复用及真实网络失败下的缓存状态。
6. Git图片／二进制差异、差异限额，以及删除、重命名、冲突等其他Git状态分别留原始输入、Git独立预期、HTTP和UI材料；普通文件预览的既有E31证据不替代这些Git详情分支。
7. 结束index及工作文件摘要的原始证据缺口已由20:38后补采集和独立比较关闭；夹具恢复仍受自动审批拒绝影响，后续如执行恢复，应另记准确操作与恢复后状态，不能把摘要一致当成清理完成。

本报告只整理上述证据增量，不修改旧验收报告的历史状态，不将混合106/118/119阶段计为同版完整三轮，也不改变用户已接管的Android真机及Windows闪窗验收分工。
