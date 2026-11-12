param(
    # 47136 是独立验收默认端口，避开正式远程端口；与 Rust example 和文档同步。
    [ValidateRange(1024, 65535)]
    [int]$Port = 47136,
    [string]$StateDirectory = "",
    [switch]$Stop
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $StateDirectory) {
    $StateDirectory = Join-Path $repoRoot "temp\remote-acceptance"
}
$StateDirectory = [System.IO.Path]::GetFullPath($StateDirectory)
$program = Join-Path $repoRoot "target\debug\examples\remote_access_acceptance.exe"
$manifestPath = Join-Path $StateDirectory "acceptance.json"
$manifest = $null
if (Test-Path -LiteralPath $manifestPath) {
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
}
$ownedProcess = $null
if ($manifest -and -not $manifest.stopped) {
    $candidate = Get-CimInstance Win32_Process -Filter "ProcessId = $($manifest.processId)"
    if ($candidate -and $candidate.ExecutablePath -eq $program) {
        $ownedProcess = $candidate
    }
}
if ($Stop) {
    if (-not $ownedProcess) {
        Write-Output "这个独立验收后台已经停止。"
        exit 0
    }
    # 只通知已核对的验收进程，不按进程名关闭现有管理器、launcher 或代理。
    Set-Content -LiteralPath (Join-Path $StateDirectory "stop-request") -Value "stop" -Encoding utf8
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ((Get-Process -Id $ownedProcess.ProcessId -ErrorAction SilentlyContinue) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
    }
    if (Get-Process -Id $ownedProcess.ProcessId -ErrorAction SilentlyContinue) {
        throw "验收后台未及时退出；未强制关闭任何现有程序。"
    }
    Write-Output "独立验收后台已停止，记录保留在 $StateDirectory"
    exit 0
}
if ($ownedProcess) {
    Write-Output "独立验收入口已经运行：$($manifest.url)"
    Write-Output "登录密钥文件：$($manifest.loginKeyFile)"
    exit 0
}
# 用户要求保留当前本地代理；仅构建 core 的专用 example，不构建或启动 manager / launcher。
Push-Location -LiteralPath $repoRoot
try {
    cargo build -p codex-elves-core --example remote_access_acceptance
    if ($LASTEXITCODE -ne 0) { throw "独立验收后台编译失败：$LASTEXITCODE" }
} finally {
    Pop-Location
}
# 目标目录由 Rust 完成隔离核对后创建；启动日志固定放仓库 temp，不能提前写入用户指定目录。
$bootDirectory = Join-Path $repoRoot "temp\remote-acceptance-boot"
New-Item -ItemType Directory -Path $bootDirectory -Force | Out-Null
$stdoutPath = Join-Path $bootDirectory "$Port.stdout.log"
$stderrPath = Join-Path $bootDirectory "$Port.stderr.log"
$process = Start-Process -FilePath $program -ArgumentList @(
    "--port", $Port.ToString(), "--state-directory", ('"{0}"' -f $StateDirectory)
) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
$deadline = [DateTime]::UtcNow.AddSeconds(75)
while ([DateTime]::UtcNow -lt $deadline) {
    if ($process.HasExited) {
        throw "独立验收后台退出，详情见 $stderrPath"
    }
    try {
        $health = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/acceptance/health" -TimeoutSec 2
        if ($health.mode -eq "claude-acceptance" -and $health.processId -eq $process.Id) {
            Write-Output "新版 Claude 验收入口：http://127.0.0.1:$Port/"
            Write-Output "原项目数：$($health.projectCount)"
            Write-Output "登录密钥文件：$(Join-Path $StateDirectory 'login-key.txt')"
            # 用户要求保留既有验收入口；自定义目录的停止提示必须绑定本次实例，不能回到默认目录关闭另一后台。
            $stopDirectory = $StateDirectory.Replace("'", "''")
            Write-Output "停止命令：.\scripts\start-remote-acceptance.ps1 -Stop -Port $Port -StateDirectory '$stopDirectory'"
            exit 0
        }
    } catch {
        # 只等待本次新进程就绪，不尝试修复或重启占用端口的原服务。
    }
    Start-Sleep -Milliseconds 500
}
throw "验收入口未就绪，详情见 $stderrPath；未停止或替换现有程序。"
