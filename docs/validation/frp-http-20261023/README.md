# frp 地址兼容、自动恢复与 HTTP 验收

日期：2026-10-23。本文件记录源码和隔离验证结果；Windows 安装包由仓库标准构建入口交付。未更新当前已安装程序、未构建 APK、未做 Android 真机验收。

## 实现范围

- 服务器地址保存前提取域名，兼容 `http://`、`https://`；粘贴后台网址的端口不覆盖独立的服务端口。
- 固定公网地址默认自动生成 HTTP Origin，只读并可复制，删除重复的「当前公网地址」区域。已有 HTTPS 配置保持兼容。
- HTTP 入口通过回源身份和匿名访问检查后可应用为主链路；Cookie、设备 Origin、退出及 ngrok 第二通道边界按协议正确处理。
- 端口冲突时，只处理受管程序、状态目录、服务器、服务端口、Token 和单一公网映射端口均符合的旧客户端。通过进程创建时间及持有的进程句柄避免误杀复用 PID。释放后重试一次，端口未获许可与 Token 错误分别报告。升级前的 0.54.0 程序额外校验固定 SHA-256。
- 补充非安全上下文的 SHA-256 计算与复制兼容。Android 接受显式 HTTP 主地址，保留同源导航、资源、下载及 HTTPS 第二通道校验，不忽略证书错误。

## 验证结果

| 检查 | 结果及边界 |
| --- | --- |
| 远程访问 Rust 回归 | 91 通过、3 按原标记跳过；包括登录、来源校验、图片缓存、双通道及事件测试 |
| 最终 frp 专项 | 11 通过；包括下载进度状态、地址规范化、映射归属、HTTP 登录、原图/缩略图、304、视频封面、Range 206、HEAD、越界 416、退出后媒体 401 |
| 最终静态检查 | `cargo check --workspace`、管理器 `npm run check`、6 个 JavaScript 文件语法检查、`git diff --check` 通过 |
| 真实映射自动恢复 | 同版本检查通过：制造本机同配置端口冲突，旧 PID 53728 被自动释放，新映射启动及真实 HTTP 回源检查成功；该临时测试入口已从源码删除 |
| 新旧客户端兼容 | 0.71.1 客户端成功释放 0.54.0 旧映射 PID 70688，在服务商 0.54.0 服务端重建映射，并通过真实 HTTP 回源验证；测试端口 39402，临时测试函数已删除 |
| SHA-256 回退 | 14 个样本与 Node 原生 SHA-256 对照通过，包括空串、Unicode、填充分块边界、百万字符 |
| Android 地址/身份 | 18 项 Java 验证通过；使用轻量 Uri 测试桩，不代表 WebView 真机验证 |
| 管理器实际 React 组件 | 带协议、端口和路径的网址正确保存为域名；服务端口保持 39397；自动地址、只读/复制、HTTP 应用、80px 端口和重复区域删除通过 |
| 真实 HTTP 浏览器媒体 | 使用购买服务的 39401 测试端口与合成数据；确认 `isSecureContext=false`、`SubtleCrypto=false` 后，原图/缩略图解码、图片组件处理及上传请求、图片指纹、视频播放/拖到 2.5 秒/关闭、Range/HEAD、WebSocket 101 及收消息通过 |

真实线路浏览器页使用合成图片、四秒视频和测试授权，并复用生产 `attachments.js`、`channels.js`、`sha256.js`；不是用户真实会话或模型交互验收。实际后端的鉴权、媒体响应和退出边界由 Rust 专项验证。

证据：`browser-checks.json`、`media-requests.json`、`manager-checks.json`、`http-media.png`、`manager-http.png`。

## 限制与清理

- HTTP 普通浏览器仍受安全上下文限制，不提供 Service Worker/PWA 离线安装；在线媒体访问已测试。
- Android 源码已适配，尚未构建、安装或实机播放。没有触发 Android 发版或替换用户已有正式签名。
- 测试清理命令被自动审批拒绝，仅返回 `blocked by policy`，没有具体原因；未改换执行途径绕过拒绝。
- 拒绝时遗留本轮合成媒体 frpc PID 78192、Node PID 20544，测试映射为 39401；本轮测试目录在仓库 `temp/frp-http-validation-20261023/`，私有测试配置在用户状态目录 `remote-access-secrets/frp-http-validation-ac69ad30-0853-41b1-9cce-860060a0fda7/`。
- 新旧版本兼容测试的两个进程均随测试退出；其隔离文件目录为 `remote-access-secrets/frp-upgrade-validation-0fa4b1d8-a079-4fa9-a71c-f6891add83ee/`，一次失败的早期夹具目录为 `remote-access-secrets/frp-upgrade-validation-ZdoSCu/`。已将这两个准确目录加入同一清理脚本。
- 已准备 `temp/frp-http-validation-20261023/cleanup.ps1` 供用户执行。脚本核对进程创建时间与程序路径后停止进程，并核对解析后的绝对路径再删除这两个测试目录。临时媒体、测试桩和私有凭据未加入版本管理。
