# 123 原生入口发现修复与真实读取恢复

2026-11-03。结论：**入口发现缺口已修复，受影响的真实原生读取已恢复；完整安装后验收仍待用户重启后继续。**

13:20:34原生文档重新加载后，HTML入口仍为同源`index-*.js`，但预加载列表和Resource Timing没有initial或原算法接受的根入口。模块实际已加载，安装包仍为26.1027.3384.0；内存initial哈希差异由现有Fast v3转换精确解释。原算法没有收集`document.scripts`，也不接受index根，因此模块发现失败并进入CLI兜底。谁触发重载未确认，不影响这项已观察到的入口缺口。

修复仅在`nativeResource`补入实际脚本入口和index根，继续使用原来的同源限制、依赖遍历、深度及唯一性检查。内部适配器升123，产品版本保持0.6.0。新增行为回归在旧源码报准确initial模块错误，修复后通过；完整桥接232/232通过。没有增加并发协调、权限层或重试机制。

| 证据 | 结果与边界 |
| --- | --- |
| `goal-storage-runtime/module-diagnosis/` | 当前真实index与安装包逐字相同；原finder根列表为空；仅补两处后可沿index→app-main→app-initial唯一解析；包含完整诊断和Fast转换对照 |
| `native-entry-123-live/ready-summary.json`、`read-verification.json` | 临时123/106一次安装成功，实际脚本SHA-256为`e0e9292f4b1cf7af9027b43b3057e5280ca72b87705ab9164dd67dd1a2042aa7`，epoch为`5a615fbb-6ada-4555-96f8-22ae08f446ba`；initial、primary、侧边和子代理模块解析成功 |
| `metadata-history-goal-queue-native-returns.json`、`related-native-returns.json` | M54/C8；M idle、测试目标保持paused；请求和队列为空，业务写0；关联会话读取成功。不能把canCreateSideThread=true当作实际创建验收 |
| `browser/automatic-read-observation.json` | 原测试页面随后显示准确A目标，普通稿保持；没有据离散采样声称准确首显耗时或稳定性能 |
| `remote-delivery-20261103/delivery-index.json` | 主代理独立比较，M原54轮和C原8轮完整内容不变；记录当前源码及最终安装包摘要 |

当前产品native源码SHA-256为`3b1010ce2782cc3399fd0050af1c73a4a9b5356c9ba427febd77ee1ae193ce85`；前端仍为`64a074804f84e11821d8c08f3fda050d00a6bb164b537d14844285bfedf2d387`。临时运行仅调整兼容标记并限制自动命名至测试名单，不修改用户安装包。

用户随后要求构建两端安装包后暂停。收尾实查发现82740/45430测试服务已退出、原浏览器标签已不存在、9229调试端口拒绝连接；45222当前由另一PID83208监听。未重启测试环境，未执行已失去前提的旧119或当前core恢复脚本，也未向新实例注入。原测试目标M/createdAt1793683117最后确认为paused，旧B回执及浏览器取消证据已归档，续作时先核对清理；不把这段现场变化写成主动恢复成功。原件见`environment-before-pause.json`及`browser/old-tabs-unavailable.json`。
