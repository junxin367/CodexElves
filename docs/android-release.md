# CodexElves Android APK 手动构建与发布

Android APK 通过 GitHub Actions 的 `Android APK` 工作流手动构建。工作流只响应 `workflow_dispatch`，不会自动运行，也不会创建 GitHub Release 或触发 Windows、macOS 桌面包发布。

## 手动运行

在仓库的 **Actions → Android APK → Run workflow** 中选择：

- `configuration = Debug`：`tag` 可留空，使用当前 workflow ref；填写时按指定 ref 构建。产物上传到本次 workflow run 的 **Artifacts**，供测试下载。
- `configuration = Release`：必须填写一个已有 GitHub Release 对应的 tag。工作流先检查 Release 存在，再使用该 tag 构建并签名 APK，最后把 APK 上传到对应 Release。此工作流不会新建 Release。

Release 模式要求仓库预先配置以下 Actions secrets：

| Secret | 内容 |
| --- | --- |
| `CODEX_ELVES_ANDROID_KEYSTORE_BASE64` | 发布 keystore 文件的 Base64 内容 |
| `CODEX_ELVES_ANDROID_STORE_PASSWORD` | keystore 密码 |
| `CODEX_ELVES_ANDROID_KEY_ALIAS` | 签名 key alias |
| `CODEX_ELVES_ANDROID_KEY_PASSWORD` | 签名 key 密码 |

使用现有正式签名 keystore 配置这些 secrets。工作流会把 keystore 解码到 runner 的唯一临时文件，构建结束后清理；不会把签名文件写入仓库。Release 模式缺少任一 secret 时会在构建前失败，不会生成 debug APK 代替正式包。

## APK 下载位置

| 类型 | 文件名 | 下载位置 | 用途 |
| --- | --- | --- | --- |
| Debug | `CodexElves-<版本>-android-debug.apk` | Actions 对应运行记录的 **Artifacts** | 测试和验收，不作为正式发布包 |
| Release | `CodexElves-<版本>-android.apk` | 所选 GitHub Release 的附件 | 正式签名 APK，供用户手动下载 |

再次向同一 Release 发布同名 APK 时，工作流会替换该附件。

## PWA 与运行时资源

APK 包含 Android 原生外壳和首版网页资源；PWA 页面和 Android 后续更新的静态资源由用户配置的 HTTPS 服务域名提供。GitHub Release 发布 APK，供用户手动下载安装。

先更新电脑端 CodexElves，使用户服务域名提供 PWA 清单和 Android 资源清单，再通过 APK 连接该域名。Android 构建环境和本地构建参数见 [Android 构建与运行](android.md)。
