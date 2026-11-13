# Windows 正式远程服务更新

适用于修改远程网页资源或兼容现有管理协议的服务代码后，直接更新正在使用的正式入口。
当前网页资源通过 Rust `include_str!` / `include_bytes!` 内嵌，更新需要短暂切换远程服务进程。
脚本只编译服务宿主 `codex-elves-manager.exe`，通过原 launcher 拉起新后台，保留正在运行的管理器和本地代理。
无需生成或执行安装包。

脚本入口为 [`scripts/update-remote-service.ps1`](../scripts/update-remote-service.ps1)，要求 Windows、PowerShell 7；
默认构建还需要现有 Rust 工具链。安装目录、正式服务 PID、启动器、网页端口与公网入口均从运行中的正式服务读取，
不写死本机的盘符、进程号或端口。

## 常用流程

在仓库根目录执行。网页资源有变化时，先递增 `assets/remote-access/resource-revision.txt`，
再做与改动相关的检查；小改动无需附带完整回归或安装包构建。

```powershell
$updatePlan = Join-Path (Get-Location) (
    "temp/remote-service-updates/{0}/plan.json" -f (Get-Date -Format "yyyyMMdd-HHmmss")
)

# 默认动作是 Prepare：构建、校验、备份并生成计划，正式服务继续运行。
.\scripts\update-remote-service.ps1 -PlanPath $updatePlan

# 核对计划后应用；只有此动作会切换正式远程服务。
.\scripts\update-remote-service.ps1 -Action Apply -PlanPath $updatePlan
```

用户已明确要求更新正式远程服务时，完成准备和计划核对后即可执行 `Apply`。
仅要求编写脚本、准备更新或读取状态时，执行对应动作即可。
独立验收端口的更新不能代替正式入口更新。

`Prepare` 的输出包含计划的绝对路径、目标资源号和是否需要切换服务。
不指定 `-PlanPath` 时，会自动创建带时间和随机后缀的新记录目录。
计划包含正式及目标程序 SHA-256、服务与受保护进程身份、资源清单、配置和授权摘要。
不保存访问密钥、管理令牌、设备 Cookie 或隧道凭据明文。

已有本次构建程序时，可以跳过编译：

```powershell
.\scripts\update-remote-service.ps1 -Action Prepare -PlanPath $updatePlan `
    -SkipBuild -ProgramPath ".\target\release-fast\codex-elves-manager.exe"
```

`-ProgramPath` 必须指向实际候选程序，并与 `-SkipBuild` 一起使用。
脚本会核对候选程序内嵌的全部远程资源和资源号，拒绝使用网页资源与当前源码不一致的旧产物。
默认构建从 Cargo 的 `compiler-artifact.executable` 读取真实产物位置，兼容共享 `CARGO_TARGET_DIR` 或显式目标配置。

其他参数：

| 参数 | 用途 |
| --- | --- |
| `-StateDirectory` | 仅在 Prepare 时指定正式状态目录；默认当前用户的 `.codex-session-delete` |
| `-HelperPort` | 仅在 Prepare 时覆盖自动识别的本机管理端口；仍会核对归属原 launcher |
| `-TimeoutSeconds` | 服务及公网核验的重试窗口，默认各 60 秒，可设 10～180 秒 |

同一个记录目录不能用于新的 Prepare。正式进程、配置或原授权变化时，重新生成计划；
不要手改计划中的 PID、摘要或路径来绕过核对。

## 应用与核验

`Apply` 按以下顺序操作：

1. 独占本次安装目录的更新锁，核对候选及备份摘要、正式服务身份、原 launcher 和管理器创建时间、
   本机管理端口归属，以及准备时的配置和原授权摘要。
2. 在安装目录写入并校验待更新程序，将旧标准程序重命名为本次专属的 `remote-before-…exe`，
   再换入新标准程序。旧映像仍供原管理器使用，因此不删除它。
3. 用同一 Windows 进程句柄再次核对服务映像和创建时间，只停止已确认的远程服务。
   不按程序名批量停止进程。
4. 向原 launcher 的回环管理接口发送 `POST /remote-access/status`，请求体为 `{}`，不发送 `Origin`。
   由现有 `ensure_running` 创建新后台，继续归属原 launcher 的 Windows Job。
   不直接运行独立的 `--remote-access-service` 进程。
5. 核对新服务父进程、正式网页监听端口、本地资源号和完整资源清单；
   下载并校验 `app.js`、`style.css` 及本次改变的其他资源字节。
6. 再检查管理器、launcher、设置、密钥和原授权是否保留，并对实际公网入口执行相同资源核验。

原管理器也会自动恢复退出的远程后台，可能在切换期间抢先于 launcher 启动服务。
Apply / Rollback 仅在原管理器身份未变、健康端点确认服务 PID 与创建时间、
映像属于本事务且摘要匹配时，停止这个服务子进程并请求原 launcher 接续；
本地就绪检查最多处理两次这样的竞争，不停止管理器或代理。
最终仍要求服务归属原 launcher；`Verify` 始终只读，不执行接续。

配置核对包含 `settings.json`、访问密钥、zrok、ngrok、主入口选择及 FRP 的 draft/active 配置。
原授权按稳定字段生成摘要，忽略正常访问变化的 `lastSeenAtMs`；新增授权可以保留，
原授权的删除、撤销或身份变化会被报告。脚本不登录网页，不占用新的设备授权名额。

同一程序已在正式位置时，重复 `Apply` 只做核验，不再次停止服务。
服务管理协议有变化时，Prepare 会拒绝仅更新服务，因为仍在运行的旧 launcher 无法管理新协议；
此时需要更新安装侧程序。

仅重试核验：

```powershell
.\scripts\update-remote-service.ps1 -Action Verify -PlanPath $updatePlan
```

`Verify` 不启动或停止服务。根据安装程序摘要核对当前目标版或本计划的原版，输出实际资源号；
它也可用于确认回滚结果。程序摘要不属于这两个版本时会拒绝核验。
没有配置公网入口时，结果中的 `publicVerified` 为 `null`，只证明正式本地入口通过。
有公网入口时，只有本地与公网都通过才报告更新成功。

## 失败和回滚

本地启动、资源或保留项核验失败时，`Apply` 自动恢复切换前的程序并通过原 launcher 恢复服务。
失败发生在停止旧服务之前时，保留旧服务进程。
回滚只更换程序，不恢复旧设置、密钥或设备文件，不恢复已撤销的授权。

如果本地新版健康但公网核验失败，脚本保留本地新版并返回失败状态，
在结果中分别记录 `localVerified: true`、`publicVerified: false`。
先检查公网连通性，再执行 `Verify`；无需为了重试公网检查再次切换服务。

主动回滚：

```powershell
.\scripts\update-remote-service.ps1 -Action Rollback -PlanPath $updatePlan
```

`Rollback` 也支持脚本在文件重命名或停止服务后中断的情况：
从计划中的备份恢复标准程序名，核对端点中仍存活服务的归属，只停止本计划安装目录和原 launcher 所属的新版服务，
然后通过原 launcher 恢复原版并核验。标准程序名缺失时，还必须有匹配本计划的更新事务记录。
管理器或 launcher 已更换、配置已变化、安装程序属于另一次更新时会拒绝自动操作，
保留记录用于人工判断。

查看这些记录即可定位阶段，无需重新登录网页或重复发送业务消息：

| 文件 | 内容 |
| --- | --- |
| `plan.json` | 原版与目标版程序、资源及进程基线 |
| `prepared/codex-elves-manager.exe` | 本次候选程序 |
| `backup/codex-elves-manager.exe` | 切换前程序，用于回滚 |
| `build.log` | 默认构建的 Cargo 输出；跳过构建时没有该文件 |
| `transaction.json` | 文件切换前即落盘的事务、旧映像路径和当前阶段 |
| `result.json` | 应用结果；失败时包含原版是否自动恢复 |
| `rollback-result.json` | 主动回滚的结果 |
| `verification.json` | 最近一次只读核验结果 |

不要删除仍供管理器使用的旧映像；保留至该进程自然退出后再清理。
需要保留回滚能力时，同时保留计划、原版备份和本次候选程序。

## 客户端验收和适用边界

切换会短暂断开远程连接，应在当前交互完成后进行。
服务模式的临时侧边正文只在服务内存保存，无法跨服务重启恢复，详见
[服务模式](remote-service-mode.md)。
脚本不发送、取消或重放会话消息，不修改运行中的 Claude 原生后台。

服务与公网资源通过后，重新打开正式页面检查本次实际界面或功能。
HTTPS/PWA 中仍停留在旧页面的标签可能保有旧资源，应关闭同源旧页面后重开。
服务端清单正确不等同于已经完成 Android 或浏览器的界面验收。

程序回滚可能降低服务端资源号；Android 不接受静态资源降级。
已经缓存新版资源的客户端，需要在兼容性确认后使用资源号更高的修复构建更新，
不能仅凭服务程序回滚就宣称客户端资源也已回滚。
涉及 launcher 管理协议、安装结构、桌面管理器界面或 Android 原生能力的变更，
按对应安装或发布流程处理。

## 脚本验证

```powershell
.\scripts\update-remote-service.test.ps1
```

该检查使用隔离的临时文件和进程、端口、网络替身，覆盖更新、重复应用、主动及自动回滚、
原管理器映像重命名、创建时间变化、公网故障、文件切换中断，以及管理器抢先恢复服务后的
launcher 接续和拒绝错误身份，不操作正式服务。

2026-11-11 已使用实际正式服务完成 `Prepare -SkipBuild` 和 `Verify`：
本地及公网资源 `2026111124` 通过，服务仍为 PID `34208`，原管理器和 launcher 创建时间未变，
授权记录仍为 2。未再次执行 Apply、编译或安装。
