# 问答卡避让队列与旧页面刷新草稿：独立复核

日期：2026-11-03，Asia/Shanghai。

**两个限定子场景通过：问答卡不再覆盖原队列菜单按钮；同scope下未编辑的旧空／旧非空页面刷新不再删除或覆盖另一页的新普通文本草稿，主动清空仍有效。** 本报告只覆盖以下实际对象和采样，不将新复现反推为所有历史丢稿的原因，也不扩为附件、跨设备或真机通过。

原件目录为`dist/validation/remote-acceptance-20261103/queue-dequeue-race/`，下文简称B。本次只读原件及必要源码，只新增本报告，没有操作现场、改产品／主表、重跑测试或提交代码。

## 1. 问答卡遮挡原队列菜单

准确Q为`01a19dce-595d-7453-a881-8b25559523ff`，前后均对应`queued-menu-<Q>`，没有另造队列替代原Q。直接查看两张截图并比较[before JSON](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-dequeue-race/question-queue-overlap-before.json)和[after JSON](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-dequeue-race/question-queue-overlap-after.json)：

| 北京时间 | 问答卡与队列几何 | 按钮中心实际命中 |
| --- | --- | --- |
| 03:56:22.095，修复前 | 卡片top=474.03125、bottom=727；队列top=702、bottom=736 | SECTION，class=`approval-card question-card`，`hitInsideApproval=true/hitIsQueueButton=false` |
| 04:04:09.526，修复后 | 卡片top=440.03125、bottom=693，上移34px且高度保持252.96875；队列位置保持 | `hitInsideApproval=false/hitIsQueueButton=true` |

命中检查与同一Q的截图支持遮挡解除；主代理现场点击已成功打开真实队列菜单。`B/page-after-layout-identity.json`还记录`originalQueueReused=true`。本结论限本问答卡、队列和视口，不把两张截图的普通草稿值差异当作草稿保留或丢失原因证据。

页面来源记录先为`641b08ac6f4571ba59dd71e8c94df279c0ae5ece85a22f4885e83ea34c00881b`，布局修复后为`f1a832e3f4030dc5fdc319b3addaafbc5d0eb98f95e0eb10b4057f0c3c7f69da`。必要源码定位为`assets/remote-access/app.js:3346`的`fitApprovalArea`：原先仅避让composer anchor，现在还读取其前面可见、非零尺寸内容的顶部，计算问答卡可用高度与上移距离。本轮未新增CSS或轮询。

## 2. 旧空页面刷新删除共享草稿的真实复现

准确会话M为`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`。旧A/B的tabId分别为`1283922820`／`1283922821`，使用相同设备`9274b4d8-e65e-4a3c-8eef-9d738a730e55`、createdAtMs=`1793459870085`；recoveryScope与DB owner相同，DB generation均为`6d74e565-5ccf-43e2-8bfb-2eb4bcfedea0`，recoveryGeneration均0。

[draft-pair-after-edit-cleanup.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-dequeue-race/draft-pair-after-edit-cleanup.json)于04:22:24记录：A的prompt、内存draft和持久记录均为`QUEUE_EDIT_UNSENT_AFTER_REFRESH_保留`；B的prompt为空、无本页draft，但读取同一DB记录仍见A原稿。两页均`write=null/writeInFlight=false/draftStorageError=""`，不是仍在等待A写完的截点。

[stale-blank-reload-reproduction.json](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-dequeue-race/stale-blank-reload-reproduction.json)的独立受控步骤仅刷新B：04:24:46.817—04:24:47.057的beforeA/beforeB仍满足上述条件；04:24:47.671的afterA中，A内存和prompt仍是原文，**共享DB record已经变为null**，scope、owner、generation均未变，仍无在途写或存储错误。这确认本次旧空页面刷新可破坏已落盘草稿，不据此解释所有旧丢稿。

## 3. 最新脚本的双页普通文本验证

`B/app-draft-fixed.js`为实际脚本全文，本次重算SHA-256为`a4d9c02b2b222608cd73df59ccea6d99b6e2c99d7130fb035df9f074f3a37c55`，同时匹配`draft-fixed-empty-baseline.json`中A、B两项实际来源摘要。修复后的A仍为`1283922820`，B为新测试页`1283922823`，并非原B的同一tabId；授权scope、owner、DB generation保持。

| 原件与北京时间 | 逐字段核对结果 |
| --- | --- |
| `draft-fixed-empty-baseline.json`，04:33:53 | 两页prompt均空，DB均无record，无在途写或存储错误 |
| [stale-empty](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-dequeue-race/draft-fixed-stale-empty.json)，04:35:04—05 | A的新稿`DRAFT_CROSS_TAB_NEW_A_保留`已在内存与DB；B本页仍空。刷新B后，afterA的prompt、draft及整个DB record与beforeA相同 |
| [stale-nonempty](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-dequeue-race/draft-fixed-stale-nonempty.json)，04:35:23—24 | 先记录B的旧非空原稿`DRAFT_CROSS_TAB_NEW_A_保留`；A随后更新并落盘`DRAFT_CROSS_TAB_NEW_B_保留`。刷新B后，A及整个DB record仍为新稿B，没有被旧稿A覆盖 |
| [explicit-clear](E:/code/junes/github/CodexPlusPlus/dist/validation/remote-acceptance-20261103/queue-dequeue-race/draft-fixed-explicit-clear.json)，04:35:39—40 | A主动清空后prompt为空、record=null；旧非空B刷新后，afterA仍空、record仍null，没有复活旧稿 |
| `draft-fixed-final.json`，04:40:07—08 | A/B最终prompt均空、DB均无record，均无在途写或存储错误；A内存为显式空串、B无draft键，二者不混为同一内部状态 |

源码定位为`assets/remote-access/app.js:2798`的`rememberDraft`：比较本页当前输入与本页已记录草稿，未变化即返回、不调用storeDraft；实际改动和主动清空仍继续保存／删除。它阻止未编辑旧页在刷新离开时重写共享记录，不构成两页同时真实编辑的冲突合并策略。

最终a4脚本与实际旧641全文的差异仅在`fitApprovalArea`及`rememberDraft`两处。以旧rememberDraft替换最终脚本对应片段后，内存重建的布局单独变体摘要也匹配记录的f1；没有将此重建结果伪装成新采集原件。

## 4. 网络、原生历史及结论边界

`B/draft-fixed-network.json`中A窗口`truncated=false/hasMore=false`，可见57次请求、零`/api/action/`；**B窗口truncated=true**，可见140次请求且含一次markRead（`90484.236`）。因此不能声称双页全程零HTTP动作，也不以截断窗口证明所有写动作均不存在。

草稿段没有新增正式消息的依据是原生历史比较：`B/edit-timeout-ended.json`（04:20:00.015）与`B/before-remove.json`（04:41:25.631）的准确M完整加载／persisted均36轮、C完整persisted均8轮，前后历史整个对象及persisted逐字段相同；两者latestThreadSettings与currentPermissions也分别相同。C加载0轮且不完整，不用它代替持久8轮；nativeQueue诊断字段前后不同，不宣称整个thread对象完全相同，也不混入后续remove轮次。

本次真实输入仅普通文本，skills／attachments均为空、goalMode为null。未覆盖图片、Skill／隐藏引用、同时编辑冲突、跨设备、授权变化、存储失败或Android真机；卡片避让也未覆盖所有窗口、软键盘和页面组合。新复现及两项修复各按本报告范围成立，不把它们补记为整个队列竞争或全部历史草稿问题已解决。完成本报告后停止写入。

| 关键B原件 | 本次直接重算SHA-256 |
| --- | --- |
| `question-queue-overlap-before.json` | `102e6fbee917bc498636f1af2cbf8bf3297603e8af8f76b82a8261dbd2c183fa` |
| `question-queue-overlap-after.json` | `52155502848da27134ade90ea5028ba32dc1a522275dd8504ff6dbbf5f1d9861` |
| `stale-blank-reload-reproduction.json` | `3872ebd17e74bbad0ec183b49b1fb14b52a0919f1a0171940fee6adabc21057a` |
| `draft-fixed-stale-empty.json` | `4b6191d5b74ad3de1ef74b17d1bc26cf39149fdcf1d4ca6e4d41d64bdba411cb` |
| `draft-fixed-stale-nonempty.json` | `49937b1831da37a8f391f5242b2b841d841da6da83d1e3a6e540245a20a65d5c` |
| `draft-fixed-explicit-clear.json` | `58c9ab128130df2f0bb900258be696e5c779a0ab7ac3d867a36349f16e37a13b` |
