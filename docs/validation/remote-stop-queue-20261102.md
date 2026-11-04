# 普通停止、原队列保留和单次显式恢复：独立复核

日期：2026-11-02，Asia/Shanghai。

**限定子场景通过：第二次尝试在准确命令工具仍运行时发出一次普通 stop，原 T 随后为 interrupted；原 Q 的完整输入保留，用户一次显式引导后，原 client 仅有一份正式接受并完成。** 最终主会话30轮、队列为空，原前28轮与控制会话的持久8轮保持，五个截点的两会话设置和权限均未变。最终截图真实显示未发送草稿 `STOP_LIVE_2_UNSENT_DRAFT_保留`。

第一次尝试已经自然完成，必须记为**未命中停止窗口**，不计停止通过。本例也不是自然 FIFO、真实上游失败、长期 unknown、120原生适配器、安装包或完整三轮通过。

本次只读取既有原始 JSON、运行脚本、AX文本和截图，进行逐字段比较与摘要复算，仅新增本文件。没有修改代码、测试、旧报告或原件，没有连接浏览器／RPC、执行模型请求、操作现场或派生代理。

## 1. 原件与运行身份

原件根目录为 `dist/validation/remote-acceptance-20261102/stop-queue-live/`，下文文件名均相对此目录。

| 原件 | 本次实际核对内容 |
| --- | --- |
| `baseline.json` | 原主会话完整26轮、控制会话已加载6轮及持久8轮、原设置／权限／状态、脚本身份 |
| `before-stop.json` | 第一次尝试已成为完整28轮、两条新增轮次均completed、队列空 |
| `before-stop-2.json` | 主会话完整29轮、准确活动T和运行命令item、原Q完整输入 |
| `after-stop-2.json` | 同一T中断、同一命令item结束、原Q完整保持 |
| `after-steer-2.json` | 完整30轮、准确原client唯一正式接受、原Q与请求为空 |
| `responses.json` | 第二次尝试四份原始响应正文及浏览器network ID |
| `network.json` | 归档窗口内逐项请求、响应状态、动作次数与时间 |
| `final-dom.txt`、`final.png` | 真实“已停止”、原Q正文和完成回复、最终输入区实际展示 |
| `native-runtime.js` | 实际归档运行脚本文本与SHA-256 |

五份快照均记录相同身份：

- fixture：`61b3c80f-2248-4a29-b908-c5e853e4321b`；target：`DC2A82A8EFAA78FEC87FAE0B59E1E229`。
- epoch：`1f26a046-3ad0-4f02-9a9f-85c62627b91a`；scriptId：`93170`。
- fixture声明 `sourceVersion=119`、`compatibilityVersion=106`；实际 `adapterVersion=106`，归档脚本也暴露 `version:106`。
- 网页摘要字段均为 `53b01a37e501d1e6b617b23d18e959bee9474d876bea353c664e0166eebc7b22`。本目录没有另存网页源码全文，本次核对五份字段一致，未重新连接网页获取脚本。
- `native-runtime.js` 全文实际SHA-256为 `5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749`，与五份 `/scriptSha256` 一致。
- fixture内的 `runtimeSourceSha256=312103909fc1c62a6b783268566eccb856aaa7b9352ba25c8d1024b0d00277ed` 是另一摘要范围：去掉末尾调试 `sourceURL` 行并保留一个LF后，实算恰好匹配。不能把带调试尾注的完整脚本与这个字段直接混比。

因此，本例归属于**119业务来源／106兼容运行标记配合上述网页摘要**，没有120原生运行证据。此前60项前端回归是源码验证，不代替本例真实动作，也不能据正常成功链把unknown分支标通过。

## 2. 第一次尝试：未命中停止窗口

`baseline.json` 采集于北京时间23:24:23.582，主会话完整26轮、idle、队列空。

`before-stop.json` 采集于23:28:23.839，已是完整28轮、idle、队列空。新增两轮为：

| 轮次 | 原生结果 |
| --- | --- |
| `01a19cde-fdfc-75f2-8445-3cda50b88f17` | completed；唯一50秒等待工具 `call_uM6WuSIBpTav1b7wlilog70H` completed／exitCode0，记录时长50,193ms；最终回复 `STOP_SOURCE_DONE` |
| `01a19ce0-17c4-71e2-973f-a63d6e1baf22` | completed；正式用户消息client为 `9f75b27d-07c2-4091-8768-f9ade29ce05e`；最终回复 `STOP_QUEUE_OK` |

该归档时间区间有两次send、两次markRead，**没有stop或steerQueued请求**。不能将自然结束后的idle、空队列或 `STOP_QUEUE_OK` 计为停止成功；本次只保留它们作为后续准确28轮基线。

## 3. 第二次尝试：普通stop命中活动T

主会话M为 `01a19a01-ebb4-7ee1-a8e1-90d180656ac1`。

`before-stop-2.json` 于23:31:22.961记录：

| 对象 | 准确原件值 |
| --- | --- |
| 活动T：`/threads/0/history/turns/28` | `01a19ce2-bf9a-7352-9f5e-9d2f9b8a50b5`，`status=inProgress` |
| T的客户端身份 | `f9ae6d3b-09f5-4956-8119-9dd42634424a` |
| 运行工具：该T的`items/3` | `call_96R9x91Ywatyq50Zq82AIvB8`，`type=commandExecution/status=inProgress` |
| 实际命令 | 原生pwsh包装的 `Start-Sleep -Seconds 50` |
| processId／开始时间 | 字符串 `74545`；`startedAtMs=1793633482423` |
| 此刻工具结果 | `exitCode=null/durationMs=null/completedAtMs=null` |
| 会话与队列 | `threadRuntimeStatus.type=active`；原Q仍在server队列；原Q的client尚无正式接受 |

这证明停止前确有正在运行的命令，不是仅凭提示正文中的“等待50秒”判断活动状态。

`network.json` 在23:31:39.271记录唯一 `POST /api/action/stop`，network ID为 `53184.3200`。该事件的 `body` 精确为 `{"threadId":"01a19a01-ebb4-7ee1-a8e1-90d180656ac1"}`，请求侧 `receiptId` 与下述响应A相同。`responses.json` 中同network ID的原响应为：

- 原回执A：`e8d5e489-a1ce-47d7-9f90-8f1ee96e9be1`，`state=completed/result.ok=true`。
- 回执准确绑定M，返回 `backgroundTerminals={data:[],nextCursor:null}`，不是审批 `approve/decision=cancel`。
- 本次回执没有 `cleanupPending=true`，不能拿它作为终端清理失败分支的真实通过。

`after-stop-2.json` 于23:32:23.659记录同一T为 `interrupted`、会话idle；同一命令item为 `failed/exitCode=-1`，`completedAtMs=1793633499629`、记录时长17,205ms，没有最终 `STOP_SOURCE_2_DONE` 回复。最终30轮原件中该T保持此状态，只有这一项commandExecution，没有继续新增第二、第三条等待命令。

**该命令项的failed/-1是本次停止后的工具结果，不是“真实上游失败”验收。** 原件没有独立OS进程存活检查；这里只确认原生准确工具已结束及回执返回的后台终端列表为空，不扩张为所有进程均已核验清理。

## 4. 原Q保留、显式恢复与唯一接受

原Q在 `before-stop-2.json` 与 `after-stop-2.json` 的 `/threads/0/queue` **整个对象逐字段相同**，包括单项data及 `nextCursor=null`：

| 身份／输入 | 实际值 |
| --- | --- |
| 物理Q | `01a19ce2-c4f2-7ad0-ae6b-f5fbd663f933` |
| 原client | `85c2e0bb-312b-403a-b90e-d8b6069d8b4f` |
| input | 仅一项text，含下述原文、末尾一个换行和 `text_elements=[]` |

```text
[remote-e2e:R3-06:stop-live-2:queued] 这是普通停止后的原队列消息。只回复 STOP_QUEUE_2_OK，不调用工具，不读取或修改文件。
```

原Q的网页send回执A为 `21977be3-ba5d-434a-ba7d-a636f85725bf`，completed，`confirmed=true/deliveryState=queued`，返回上述原physical和client。原POST `body.clientUserMessageId` 也为上述原client，`body.prompt` 与原Q文本相比仅少原生接纳时追加的末尾换行。停止后29轮历史中该client仍没有正式接受，原Q没有消失或被另加一份。

随后23:32:53.021出现唯一 `POST /api/action/steerQueued`，network ID `53184.3222`；其 `body` 精确为原M和原Q的 `{threadId,messageId}`，请求侧 `receiptId` 与原响应A `3ac85a5c-a0fa-4db3-9e37-cc477e8d23e4` 相同。响应completed／ok，绑定**原Q**，返回新T：

`01a19ce4-5ca3-7a80-a34b-94c40e27a82b`

回执同时为 `queueSyncPending=true`。该标记本身不证明完成，本次继续核对 `after-steer-2.json`：

1. 完整原生历史30轮中，原client仅匹配一个正式 `userMessage`，ID为 `01a19ce4-5cbc-7fa0-88dd-5a6024da63f3`，位于上述新T。
2. 该userMessage的完整 `content` 与原Q的完整 `input` 逐字段相同；新T的 `params.input` 也相同。三种投影不重复算接受次数。
3. 正式持久历史 `/threads/0/persisted/thread/turns` 也只有同一client的一份正式userMessage，准确消息ID、T和完整input均吻合。
4. 新T completed，最终回复仅为 `STOP_QUEUE_2_OK`；队列 `data=[]/nextCursor=null`，requests为空，会话idle。

原T此前已经interrupted，因此这是**显式恢复原Q后另起新轮**，不是向原活动T追加steer，也不是自然FIFO。本例只记录所观察的“停止后保留、随后显式恢复”，不规定其他停止／失败场景必须采用相同暂停策略。

## 5. 历史、控制会话与设置保护

五份主会话原生历史均声明 `isComplete=true`，数量依次为26、28、29、29、30；对应正式持久读取数量相同。

- 原26轮在所有后续原生历史及持久历史中均逐字段保持。
- 第二次尝试以前的准确28轮，以 `before-stop.json` 为基线，在 `before-stop-2.json`、`after-stop-2.json`、`after-steer-2.json` 的两种历史来源中均逐字段保持。
- 最终30轮仅比该28轮新增本次interrupted的T及原Q的completed恢复轮，没有第三份补偿消息。
- 五个截点M的 `/metadata/latestThreadSettings`、`/metadata/currentPermissions` 整个对象分别与baseline相同。

控制会话C为 `01a1996d-0b7a-71d0-b9ad-9c521d224ad9`：

| 来源 | 本次比较结果 |
| --- | --- |
| `/threads/1/history` | 五份均已加载6轮、`isComplete=false`，整个对象相同；不能把这6轮说成完整8轮 |
| `/threads/1/persisted/thread/turns` | 正式持久读取的8轮在五份原件中逐字段相同 |
| `/threads/1/metadata/latestThreadSettings`、`currentPermissions` | 五份原件的完整对象分别相同 |
| C的runtime、queue、persisted.readState | 五个截点均相同，C保持idle、队列空 |

C在基线已经idle，本例没有证明另一个同时运行的控制任务不受影响，也不以两个会话的设置一致推断全局配置、浏览器或服务已完成收尾。

## 6. HTTP动作次数与原回执

`network.json` 为 `from=17005/to=17906/truncated=false`，保存278条事件。本次只按 `Network.requestWillBeSent` 的唯一network ID统计派发；同请求的响应不再次计数。

| 时间区间 | send | stop | steerQueued | markRead |
| --- | ---: | ---: | ---: | ---: |
| 第一次尝试：第二次base发送以前 | 2 | 0 | 0 | 2 |
| 第二次尝试：从sequence17501开始至归档结束 | 2 | 1 | 1 | 2 |
| 本归档全窗口 | 4 | 1 | 1 | 4 |

第二次尝试可逐项关联的原件如下，时间为2026-11-02北京时间：

| 动作 | network ID／请求时间 | 响应正文中的原回执A | 实际结果 |
| --- | --- | --- | --- |
| 发送活动T任务 | `53184.3183`／23:31:07.397 | `31d987aa-6d9d-4097-9d91-71bf3d492312` | completed／accepted，准确原T/client |
| 原Q入队 | `53184.3190`／23:31:09.230 | `21977be3-ba5d-434a-ba7d-a636f85725bf` | completed／queued，准确原Q/client |
| 普通停止 | `53184.3200`／23:31:39.271 | `e8d5e489-a1ce-47d7-9f90-8f1ee96e9be1` | completed／ok，绑定M |
| 显式恢复原Q | `53184.3222`／23:32:53.021 | `3ac85a5c-a0fa-4db3-9e37-cc477e8d23e4` | completed／ok，绑定原Q及实际恢复T |

这四项原POST的 `body` 均已读取，其请求侧 `receiptId` 分别与配对响应正文的 `requestId` 相同；四项均有HTTP200的responseReceived及可解析的完整响应正文文件。所读窗口没有其他写动作路径，没有第二次stop／steer，也没有删除、重排、编辑、审批决定或普通send补偿原Q的动作。

**记录边界：** 请求正文存于 `body`，不是 `postData`；请求标识存于独立字段 `receiptId`。归档没有原始headers或loadingFinished，因此本次核对的是已保存请求标识与A相等，不追加原始 `Idempotency-Key` header逐字复核或完整底层传输终态结论。`responses.json` 只含第二次尝试四份正文，第一次两次send的回包正文未在该文件保存。派发次数只对本归档窗口负责，不代表全服务所有线程的总数。

## 7. 草稿与真实展示

已直接查看 `final.png`：准确第二次T下显示“已停止”，后续原Q正文和 `STOP_QUEUE_2_OK` 回复可见，底部输入区实际显示：

```text
STOP_LIVE_2_UNSENT_DRAFT_保留
```

最终完整原生历史中没有该草稿标记，本归档四次send的原POST正文也均不含该标记。`final-dom.txt` 同样保留准确第二次T/Q正文、“已停止”和完成回复，但其第501行只有 `textbox "消息输入"`，**没有导出该输入的Value**。本次草稿展示结论由实际截图支持，没有把AX缺少Value当作空稿或伪造一份DOM值。

原件没有单独保存填稿时点的输入值、草稿存储内容或刷新前后比较，因此本例只确认**最终仍真实展示该非空未发送稿**，不扩张为跨刷新持久化、图片／Skill草稿或全部异常恢复通过。

## 8. 未闭环边界与时间差

1. **跨来源的毫秒顺序存在差异。** 原T记录的 `turnStartedAtMs=1793633468202` 加 `durationMs=31018` 为23:31:39.220，比浏览器stop请求的 `wallTime` 23:31:39.271早51ms；同一命令项完成时间又为23:31:39.629。原因未在本次原件中解释。本报告按准确T/item的前后状态及唯一stop原回执确认上述限定链路，不将不同时间字段拼成严格全序，也不据此报告精确停止延迟。
2. 本例没有长期pending／unknown、原回执丢失、派发后断线、自然出队与编辑／删除竞争、真实供应商失败，不能新增这些分支通过。
3. 原生仍为119来源／106兼容运行，本例不验证120原生删除结果分类修复；正常成功回执也不能替代前端unknown防重复的真实故障验收。
4. 仅一条纯文本Q；未验多Q顺序、图片／Skill／隐藏输入、停止与后继轮启动竞争、全部后台终端、同时运行的控制会话、Android或安装包。
5. 归档终态截止23:33:08.737的原生快照及随后最终截图／AX。本次没有查询当前现场或服务收尾，不报告全局环境已恢复。

## 9. 本次复算的原件摘要

| 文件 | SHA-256 |
| --- | --- |
| `baseline.json` | `4c948780cfdd43dcde8bff1fa9264e7f0f6a5d87cdf2d04ed1956421d75a1c94` |
| `before-stop.json` | `c165fe2ee57882b9ef28f0e0f650365ffb6dc0feac9afdfde46610ac56882067` |
| `before-stop-2.json` | `48c3295ba5d6c66a2865d5ecf300368a404cba5bab80ba2f56b8901499c1de36` |
| `after-stop-2.json` | `a75adc78b404658dc77ca8706a4143c93e61a3a3d6f9bf096e486a7146f61aaa` |
| `after-steer-2.json` | `b3ea3d66d6a096a25783fe009900c3452150757a23653bc94b5e5300ef904047` |
| `network.json` | `7fcfbd4d35bcd18810ce9d0bdea7f1088e44106aab47d8cb51409d3354491666` |
| `responses.json` | `a2cf6f308594675960b7dd078b38abef572db3e4ab96a4cbc5b675f5957f3ed0` |
| `native-runtime.js` | `5b73d16ff0b6477f77c39f9513e4c45b5268cf27f4a24651fa25cc358d238749` |
| `final-dom.txt` | `d2e580f8383bc5fc8027c9dbf1ecf3cb9949db3a08f2c38a0b1b2022ad33f7b9` |
| `final.png` | `b1ef51b43a1caf9910f97137160411ed4a907a439845835f3a32ac9c0aceccaa` |
