# Android 更新集成审查

2026-10-18，独立只读审查结果：**PASS，无未解决阻断发现**。

范围包括原生更新来源、元数据、文件及签名校验，Activity 与安装来源权限流程，网页版本消息桥，独立版本、构建指纹与失败基线，以及 GitHub PR、分支构建和独立 Release 发布。

初审发现一项 P2：重跑已经公开的旧 Android 标签时，如果已有更高版本，发布脚本先执行跨版本递增检查，导致尚未进入同标签附件一致的幂等分支就失败。修复后先校验已公开同标签附件，相同内容直接无写入跳过；新发布或草稿转公开才检查递增版本。独立审查使用最新生产控制流确认旧标签重跑成功跳过、低版本草稿仍被拒绝。构建代理另有 7 项定向验证。

控制器在成功交给系统安装器后释放本实例的待安装状态，取消安装返回后可以重新检查更新。此调整已包含在最终审查快照中。

最终核心文件 SHA-256：

| 文件 | SHA-256 |
| --- | --- |
| `AppUpdatePolicy.java` | `c8f0c9db403ebf9e6824c9801f91a81bbfa936447fd464d313e6811e64e79877` |
| `AppUpdateClient.java` | `625efbc730f46767eae340c9def4ac80b28afd811ecc966720b132e296067a4f` |
| `AppUpdateController.java` | `fff0c9e8a6af88d0e0d4adff28046d60855fe1b4afda41ed626bde9d872a22f5` |
| `publish-android-release.ps1` | `ef18eaad4c2db7e4f53fc864a9721534109d169b5a06cefc539026b1ca1d7633` |

本审查不代表真实手机安装或 GitHub Actions 已运行；相应执行结果与限制分别记录。
