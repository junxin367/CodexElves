# 登录页左对齐与上三下七布局

2026-10-18，状态：PASS。网页登录页、连接恢复页及 Android 原生登录页已按同一布局要求调整：图标和完整标题作为一组与表单左边缘对齐；保留最小上下边距后，表单上方、下方的剩余高度按 3:7 分配。表单内容超过可用高度时，加权留白归零，保留原有边距并允许滚动。

网页 `assets/remote-access/style.css` 将共用 `.pair-page` 改为纵向 flex，通过 `::before`、`::after` 的 3、7 权重分配剩余高度；`.pair-panel` 保持内容高度，不收缩，取消上下自动外边距。`.pair-brand` 改为左对齐。原有安全区、响应式最小边距、420px 最大表单宽度、标题换行规则、图标尺寸和所有交互逻辑保持。

Android `view_login.xml` 保留根布局 `wrap_content`，仅水平居中表单；在表单前后添加高度为 `0dp`、权重分别为 3 和 7 的 `Space`。品牌行使用 `start` 对齐，标题使用 `start|center_vertical`。保留 24dp 最小上下边距、18dp 左右边距、420dp 最大表单宽度、40dp 图标、28sp 完整标题及 12dp 间距；没有修改 `LoginView.java`、输入、登录、权限或帮助展开逻辑。

## 验证

- 主代理通过 Chrome 扩展加载完整生产网页，在 390×844、320×640、1280×800、13px 和 20px 字号下验证左对齐、3:7 留白、完整标题及无横向溢出；连接恢复页同样通过。390×844 下表单顶部从 215.859px 上移到 139.109px，上下额外留白为 115.109px、268.594px。
- 展开长帮助内容及 320×360 短视口时，网页加权留白归零，底部可滚动到达。网页明细见 `browser-results.json`，前后截图见 `before-mobile.png`、`after-mobile.png`。
- 原生 XML/CSS 结构检查、Android SDK 35.0.0 AAPT2 资源编译、链接、编译后布局检查以及 JDK 17.0.20.1 编译现有 `LoginView.java` 与生成的 `R.java` 均通过。目标文件 `git diff --check` 通过。明细及源码 SHA-256 见 `native-validation.json`。
- 核对 Android 15 的 AOSP `ScrollView.onMeasure` 和 `LinearLayout.measureVertical`：`fillViewport` 在内容较短时将根布局按视口高度重新精确测量，权重随后分配剩余高度；内容较长时保持内容测量高度。此项是平台源码和布局结构核对。

## 验证边界

没有 ADB、设备或模拟器，因此没有 Android 真机截图、真实软键盘、系统字体缩放、OEM 字体或 TalkBack 运行验收。未运行 Gradle、APK 或 Windows 安装包构建，没有安装、提交、推送、部署或重启服务。

本次只修改上述两个布局源码文件，并增加本目录验证证据；保留已有脏工作区内容和既有 Rust 图片修复。

临时清理状态：PASS。批量删除操作曾被自动审批拦截；主代理核对目录内全部 23 个本任务文件、8 个子目录及无链接后，以明确文件清单逐项删除，再清理空目录。最终 `temp/login-alignment-37-20261018-agent` 已不存在，未删除其他来源文件。验证报告和截图保留。
