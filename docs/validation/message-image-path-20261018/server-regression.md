# 会话图片路径兼容回归

结论：服务端修复与回归 **PASS**。生产源码仅改动 `crates/codex-elves-core/src/remote_access.rs` 中的 `eligible_image_path`，增加 5 行 Windows 条件分支。浏览器渲染与预览由主代理独立验证。

## 复现与修复

实际输入来自会话日志 `C:\Users\junes\.codex\sessions\2026\10\18\rollout-2026-10-18T00-06-17-01a14a9d-2d95-7100-9ac3-87adb37d0454.jsonl` 第 6908 行，消息 ID 为 `msg_020ba1caa3c75e08016ad45ac7a4d887d09a2e03f56f4379a1`：

```markdown
![手机登录页效果](/E:/code/junes/github/CodexPlusPlus/docs/validation/mobile-login-center-20261018/mobile-login.jpg)
```

修复前，生产 `canonical_item_image_sources` 提取出完整 `/E:/...` 引用，但 Windows `PathBuf::is_absolute()` 和本机盘符前缀检查均为 `false`。生产 `attach_item_images` 最终注册数为 0，输出没有 `remoteImages`。同一图片去掉首个 `/` 后路径可识别，文件存在，证明故障发生于路径兼容检查。

先运行了修复前复现测试，并保存生产函数输出至 `baseline-item.json`；随后运行要求该消息注册图片的回归测试，按预期失败于 `None != Some(1)`。修复只让 Windows 下严格匹配 `/` + ASCII 字母盘符 + `:` + 根分隔符（`/` 或 `\`）的路径在构造 `PathBuf` 时去掉首个 `/`。`remoteImages.source` 仍为 Markdown 原始引用，正文未改动。

修复后，生产 `attach_item_images` 输出已保存至 `repaired-item.json`。`read_registered_image` 和鉴权 HTTP `/api/images/{id}` 均返回原始 JPEG 的完整字节：

- 大小：15,680 字节。
- MIME：`image/jpeg`。
- SHA-256：`bb5f71e1dd2bec43a7af48fb5245e16872ffddc2882c78044df1db1c89bd5a40`。
- 最终源码 SHA-256：`3515abb8324ae43e10c1bbffd7260beb8aebb641aa8709e2ca6580641b429b3a`。

## 验证结果

临时测试作为 `remote_access.rs` 的测试子模块运行，直接调用生产函数，使用独立 controller、独立临时状态目录与回环随机端口。原始消息从上述日志读取；未手工拼接前端图片映射，未使用真实授权凭证，也未调用真实原生会话。

| 项目 | 结果 |
| --- | --- |
| 原 `/E:/...`、普通 `E:/...`、`E:\...`、`/E:\...` | 4 种写法均可注册、保留原 source，并读到原 JPEG |
| 非法盘符、缺根分隔符、双/三前导斜杠、UNC、设备路径、相对路径、`file:`、HTTP(S)、不支持的扩展名、URL 编码盘符、空值和 NUL | 24 种输入均被拒绝 |
| 本机固定磁盘检查 | 实际图片通过；普通 UNC 和 verbatim UNC 均拒绝 |
| 伪装为 JPG 的非图片内容 | 415 |
| 文件大小超过 25 MiB | 413 |
| 宽度超过 16,384 的 PNG | 415，保持原有尺寸读取限制 |
| 现有 JPEG data URL | 可注册并读回相同字节 |
| HTTP 无凭证、无效凭证、撤销设备后访问 | 均为 401 |
| HTTP 合法凭证 | 200；字节摘要一致，`image/jpeg`、`Content-Length: 15680`、`no-store`、`same-origin`、`nosniff` 均正确 |
| HTTP 未知/非法图片 ID | 404 |
| HTTP 非法 Origin、跨站请求 | 403 |
| HTTP 伪装图片、过大图片 | 分别 415、413 |
| HTTP `/api/read/history` | 200，生产附加后的实际消息与 `repaired-item.json` 一致 |

实际执行命令及退出码：

```text
cargo test -p codex-elves-core --lib message_image_path_baseline_reproduces_rejection -- --exact remote_access::message_image_path_regression::message_image_path_baseline_reproduces_rejection --nocapture
  修复前：exit 0，1 passed，已复现注册数为 0。

cargo test -p codex-elves-core --lib message_image_path_registration_read_and_http_auth -- --nocapture
  修复前：exit 1，预期红灯，图片注册断言失败。
  修复后：exit 0，1 passed，28 项路径输入、11 项 HTTP 行为和字节/格式/大小/尺寸检查通过。

rustfmt --edition 2024 --check --config skip_children=true temp/message-image-path-20261018/eligible-image-path.rs
  exit 0，从最终源码提取的完整修改函数格式检查通过。

git diff --check -- crates/codex-elves-core/src/remote_access.rs
  exit 0，最终生产源码差异检查通过。

cargo check -p codex-elves-core --lib
  exit 0，删除临时测试入口后的最终生产源码检查通过。
```

## 边界与清理

未改动图片注册、读取或权限函数，未开放网络图片、`file://`、UNC 或设备路径，也没有加入 URL 解码和路径猜测。非 Windows 分支由条件编译保持原逻辑；未在非 Windows 系统运行测试。

临时测试源码、用于格式检查的函数副本、`temp/message-image-path-20261018/`、临时 controller 状态与测试模块入口均已删除，隔离 HTTP listener 已停止，没有保留新增专项测试文件。临时回归源码 SHA-256 为 `9a654eac24b459c6865d9602287c12e0fafa4412e8cf48af98341e2f9beec51d`。详细路径和 HTTP 用例保存在 `server-regression.json`。

未提交、构建安装包、安装或重启生产服务；实际生产服务仍需使用后续构建部署才能获得该修复。
