# CodexElves Android APK 构建与发布

Android 使用 `apps/codex-elves-android/version.properties` 独立版本，与桌面管理器的 `Cargo.toml` 版本分开。当前为 `0.0.1`、`versionCode=2`；每次正式发布必须增加 `versionCode` 并沿用正式签名证书。

## 自动运行与手动运行

| 触发条件 | 检查与产物 | 是否发布 |
| --- | --- | --- |
| PR 修改 Android 相关文件 | `pr-build.yml` 执行 Debug 构建和 lint，不读取签名 secrets；无关 PR 跳过 Android job | 否 |
| `main` 推送 Android 相关文件 | `android-apk.yml` 正式签名 Release、lint、APK 元信息与摘要校验，上传 Actions Artifacts | 否 |
| 推送 `android-v<versionName>` 标签 | 正式构建后校验标签、版本和递增的 `versionCode`，发布独立 Android Release | 是 |
| 手动 Debug | 指定 ref 或当前 workflow ref 构建、lint，上传 Actions Artifacts | 否 |
| 手动 Release，tag 留空 | 当前 workflow ref 正式构建、lint，上传 Actions Artifacts | 否 |
| 手动 Release，填写 Android tag | 按该标签正式构建、lint 并创建或更新对应 Android Release | 是 |

Android 相关文件包括 Android 项目、内置 `assets/remote-access` 网页资源、Android 专用 PowerShell 脚本、根构建入口及对应工作流；构建目录、Gradle 缓存和本机配置不参与自动路径判断。PR 检查使用普通 `pull_request` 事件，不向 fork PR 提供签名材料。

在仓库的 **Actions → Android APK → Run workflow** 中选择：

- `configuration = Debug`：仅生成测试 APK。
- `configuration = Release`：仅使用正式签名。`tag` 留空时仅生成产物；填写时必须与独立版本对应，例如 `versionName=0.0.1` 使用 `android-v0.0.1`。

Release 模式要求仓库预先配置以下 Actions secrets：

| Secret | 内容 |
| --- | --- |
| `CODEX_ELVES_ANDROID_KEYSTORE_BASE64` | 发布 keystore 文件的 Base64 内容 |
| `CODEX_ELVES_ANDROID_STORE_PASSWORD` | keystore 密码 |
| `CODEX_ELVES_ANDROID_KEY_ALIAS` | 签名 key alias |
| `CODEX_ELVES_ANDROID_KEY_PASSWORD` | 签名 key 密码 |

使用现有正式签名 keystore 配置这些 secrets，不创建新签名证书。工作流会把 keystore 解码到 runner 的唯一临时文件，构建结束后无论成功失败均清理；签名文件不写入仓库。Release 模式缺少任一 secret 时会在构建前明确失败，不生成 Debug APK 代替正式包。代码变更本身不配置 secrets，也不触发远程构建或发布。

## APK 下载位置

| 类型 | 文件名 | 下载位置 | 用途 |
| --- | --- | --- | --- |
| Debug | `CodexElves-<版本>-android-debug.apk` | Actions 对应运行记录的 **Artifacts** | 测试和验收，不作为正式发布包 |
| Release 分支构建 | `CodexElves-<versionName>-android.apk`、`android-update.json` | Actions 对应运行记录的 **Artifacts** | 正式签名构建检查 |
| Release 标签发布 | `CodexElves-<versionName>-android.apk`、`android-update.json` | `android-v<versionName>` 对应 GitHub Release 的附件 | 正式下载安装及应用内更新 |

清单采用 `schemaVersion=1`，包含 `applicationId`、`versionName`、`versionCode`、`minSdk` 和 APK 的 `name`、`size`、`sha256`，不包含下载 URL。大小和 SHA-256 从签名后的实际 APK 生成。完整格式见 [Android 版本与应用更新](android-update.md)。

标签发布前会检查当前成功构建、准确标签和已发布 Android 清单；新 `versionCode` 必须大于其他已发布 Android 版本。新 Release 先创建为草稿，上传 APK 和清单并重新下载校验后才公开，上传失败时保持草稿。同标签已发布且附件完全一致时跳过重复发布；APK 或清单不同则拒绝覆盖，要求提升独立版本及 `versionCode`。

Android Release 显式设置 `make_latest=false`（`gh --latest=false`），不会覆盖桌面 Latest 标记，也不生成或覆盖桌面 `latest.json`。桌面 `release-assets.yml` 在入口排除 `android-v` 标签，后续 Windows、macOS 和 `latest.json` job 均跳过。

## PWA 与运行时资源

APK 包含 Android 原生外壳和首版网页资源；PWA 页面和 Android 后续更新的静态资源由用户配置的 HTTPS 服务域名提供。GitHub Release 分发 APK 和应用更新清单；应用内更新与网页静态资源更新互相独立。

先更新电脑端 CodexElves，使用户服务域名提供 PWA 清单和 Android 资源清单，再通过 APK 连接该域名。Android 构建环境和本地构建参数见 [Android 构建与运行](android.md)。
