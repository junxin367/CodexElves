# 124普通发送未决状态跨刷新验收

2026-11-03，Asia/Shanghai。**限定子场景通过：发送回包被挂起直至客户端真实超时，原回执、消息身份和后写草稿跨整页刷新保持；人工放行新文档的原回执GET后解锁，没有重发。** 不认定预定的主动丢回包步骤完成，也不认定完全自动恢复或整个R3-01通过。

原件根目录为`dist/validation/remote-send-refresh-124-20261103/`。`verification.json`和独立subagent复核均以完整原生历史、实际网络事件、回执正文及页面记录判定，没有修改产品源码或安装包。

## 运行和隔离

| 对象 | 本轮身份与边界 |
| --- | --- |
| 服务 | 独立`127.0.0.2:45430`，验收进程PID82752；本轮EXE SHA-256为`c7b4359fa9532ea4de4e2c89e82507fe82c95747bcaf6172c48a88e35e8ce894`；用户45222保持PID97428 |
| 原生 | 124业务实现，实际兼容标记123，SHA-256为`1993ef8133be4cf0b7947ba1c3e175f163c50de9a8eeda254e3d32aff388b78a`，epoch为`9e7c8777-2760-4889-9bcf-d93fdf97014c`；起止全文一致，仅两处版本标记不同于正式124源码 |
| 网页 | Chrome测试tab1283923055；刷新前后实际`app.js`均为`64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`；刷新后按当前composer处理函数定位实际脚本135 |
| 授权 | 新回环主机在登录前无Cookie，与用户`127.0.0.1`隔离；测试设备`cbfb0fca-b354-4a9d-91b4-7d704cba2d93`，createdAtMs=1793698953146 |
| 测试范围 | 仅M=`01a19a01-ebb4-7ee1-a8e1-90d180656ac1`发送一条；C=`01a1996d-0b7a-71d0-b9ad-9c521d224ad9`只读；用户工作会话及原队列未操作 |

复制来的设备列表已有32条，首次登录返回真实设备上限错误，尚未发送业务消息。停止独立服务后，只将其状态副本初始化为空设备列表，再启动并正常登录；没有调整产品32条上限或撤销用户真实授权。`environment/isolated-authorization-initialized.json`及`final-state.json`证明用户key/devices/settings摘要保持。

## 实际故障与刷新恢复

| 阶段 | 原始事实与证据 |
| --- | --- |
| 单次发送 | A=`da8c2cee-acba-4847-9531-5fe3ab08f769`；client=`89c034b2-eccd-44fb-b680-1bf536a3b457`；原POST为`28428.147`。`browser/fault-v2/original-send-request.json`保存准确正文与身份 |
| 真实超时 | 原POST在sequence424进入HTTP200的Response暂停；临时后台观察循环已经报错。约91.023秒后sequence627为`net::ERR_ABORTED/canceled=true`。没有采集原POST响应正文，也没有主动执行该POST的`Fetch.failRequest` |
| 刷新前未决 | `unknown-before-reload.json`保留原A/client、recoveryRef及授权scope，发送锁定；后写的两行普通草稿完整存在 |
| 真实新文档 | loader从`FA8CE36C7F2156C3CE53783CB1C3C796`切到`0926FEAEDFACA494D088FD3DC3DD6055`，timeOrigin变化；`reloaded-held-state.json`的整个pending对象与刷新前一致，新草稿保持，发送仍禁用 |
| 只读原A恢复 | 新文档的GET`28428.392`在sequence1772发出；人工放行后sequence1792收到200、1794完成。`original-receipt-recovered.json`为同A的completed/ok，准确绑定原thread/client/turn |
| 解锁与保稿 | `recovered-state.json`中pending=null、发送可用，新草稿逐字保持；`recovered.png`有实际回复、新稿与可用发送按钮 |

这里曾点击页面实际“查询此消息的送达回执”按钮，但它发起的是`28428.395`，随后被暂停并超时。真正成功的`28428.392`由定时核对发起，启动栈指向`resolvePending`的重试回调。准确结论是**人工放行此前已发起的新文档原A GET后恢复**，不能将成功GET误归于按钮点击，也不能称完全自动恢复。

91.023秒是人为挂起响应后的客户端超时，不是正常业务发送耗时。事件、历史和队列读取继续正常，只有原A确认读取及原client备用确认路径设有测试闸门；本次sendState实际请求为0。

## 原生接受次数与历史

`native/before-send-native-returns.json`、`after-recovery-native-returns.json`、`final-before-restore-native-returns.json`和`restored-native-returns.json`独立对照：

- M从54轮变为55轮，原54个完整turn逐字段保持；C的整个持久history对象及8轮保持。
- 原历史没有该client，最终仅一条`userMessage.clientId`匹配，位于T=`01a1a0cd-a767-7252-81d8-81cccd23c70e`；与原回执的turnId一致。
- 完整用户输入恰为预期prompt加一个终止LF，`text_elements=[]`；轮次completed，模型回复准确为`SEND_REFRESH_124_OK`。
- 后写草稿标记在完整用户历史中出现0次；没有把草稿复制或补发成第二条消息。
- 队列及目标最终为空，M为空闲、无待答请求。

初始4条事件与前台分段按sequence合并为1250条，无重复，见`network-ledger-redacted.json`。该窗口send POST恰好1次、markRead1次，刷新后send0；26次回执GET只查询同一A，不是26次发送。各已保存分段均未报告truncated；`manual-network-08.json`没有hasMore字段，保留此记录缺口，不宣称每段完整性字段齐全。

## 验收逻辑校正与未覆盖范围

本轮有三项测试控制问题，均未包装成产品失败或成功：

1. 首次Fetch设置未明确Fetch/XHR类型，在发送前被浏览器拒绝；修正类型后才发生唯一业务发送。
2. 临时观察器在工具调用返回后继续后台读CDP，报`Browser Use could not read requirements`。后续改为前台分段读取原始事件；不能把观察器空输出当作零请求。未来使用该工具适配器时须先核对观察器执行边界。
3. 首次“退出”只打开确认弹窗，弹窗文字也含“访问密钥”；按这个词出现就判断已登录页是错误验收。收尾另行完成确认，以实际`/api/logout`200、登录输入框可见及认证Cookie消失验证退出。

未证明原POST的成功正文已经被采集、主动丢回包、完全自动恢复、真实后端长期unknown、附件/Skill、换授权、全通道断网、原生重启或Android真机。E70仅补齐普通文本未决锁跨真实刷新的本次边界，不替代这些分支。

## 收尾

本轮新草稿已通过正常输入入口清空，并在刷新后确认为空；原A已成功确认，没有强行解除锁。传输拦截撤回，两测试页关闭。实际退出证据在`browser/logout-verified.json`，早期`logout-ui.txt`和“loginVisible”布尔值仅为未完成尝试。

独立服务已停止，45430不再监听；用户45222和9229的PID保持。原生准确恢复原123源码，epoch为`14cdee44-7108-4f93-a544-68074d321746`；最终M55/C8历史保持。可复用的独立环境暂留供未完成目标的后续验收，未清理此前被自动审批拒绝的目录。当前没有提交、推送、发布、安装或重新打包。
