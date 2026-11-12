# PC 文件与变更多标签面板验收（2026-11-12）

## 实现

- PC 网页和 Windows 远程 EXE 共用右侧多标签阅读面板。文件列表、变更列表及各文件、差异详情可同时保留，聊天仍可操作。
- 同一文件路径重复打开复用标签；差异按仓库、分区、路径分别识别。Markdown 内的文件引用也打开独立标签。
- 每个标签保留自己的 DOM、阅读位置和读取任务；关闭后释放资源，切换会话、项目或授权时清空旧标签，旧回调不能在新目标中创建标签。
- 拖动左侧分隔线调宽，也可用方向键、Home、End；仅用户调整保存宽度，窗口临时变窄不覆盖偏好。
- 761px 以下及 Android 容器沿用原整页导航。桌面窗口临时变窄时保留原标签，恢复桌面宽度后再次显示。
- 独立 EXE 按用户后续要求移除“连接”“窗口”菜单栏，标准标题栏、托盘操作及加载失败后的地址恢复继续保留。

相关实现：

- `assets/remote-access/session-tools.js`
- `assets/remote-access/session-files.js`
- `assets/remote-access/style.css`
- `apps/codex-elves-remote/src-tauri/src/desktop.rs`

## 检查结果

执行：

```powershell
node --test scripts/remote-access-ui-state.test.mjs scripts/workspace-tabs.test.mjs
npm --prefix apps/codex-elves-manager run check
cargo test --locked -p codex-elves-remote --lib
cargo build --locked --profile release-fast -p codex-elves-remote --bin codex-elves-remote
.\scripts\build-windows-remote.ps1 -SkipBuild
git diff --check
```

前端 301 项测试通过，包括新增的 5 项标签生命周期、关闭与键盘操作、调宽、上下文隔离和移动端兼容测试。资源检查确认 23 个 JS 的语法、引用及服务路由有效，TypeScript 检查通过。独立客户端 7 项测试通过，Windows EXE 编译及 PE 校验成功。

浏览器使用真实工具页组件和合成项目数据验收，没有调用正式会话接口：

- 同时保留文件列表、README、变更列表、差异和源码共 5 个标签。
- Markdown 文件链接新开源码标签，聊天输入框仍可编辑。
- 实际拖动使面板从 480px 变为 580px，没有整页横向溢出。
- 源码滚动到约 1189px，切到 README 后返回仍保持原位置。
- 390px 窄屏打开原整页文件列表；恢复默认 1243px 桌面宽度后原标签恢复，移动端页面隐藏。
- 切换会话后标签数归零，聊天草稿保留。浏览器无脚本错误。

脱敏效果图为本机忽略文件 `temp/workspace-tabs-after.jpg`，未写入仓库。临时验收页已关闭，验收服务器已停止。

## 交付范围

验收结束时工作区网页资源版本为 `2026111240`。本次完成源码调整和本地验证，没有热更新正式远程服务；正式服务使用新网页资源后，PC 网页与 EXE 的右侧标签行为同时生效。

移除顶部菜单栏的独立客户端已生成：

- `dist/windows/CodexElves-Remote-0.7.0-windows-x64.exe`
- 大小：9,085,952 字节。
- SHA-256：`7608f8859b88952db5095c712710f827ba0ea9689b71aa80271136c697f9f9c8`。

EXE 菜单栏调整经过编译验证；本记录中的真实界面操作验证针对共用网页组件，不代表全部原生窗口及托盘行为均已重新实测。
