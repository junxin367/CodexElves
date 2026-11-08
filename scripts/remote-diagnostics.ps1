[CmdletBinding()]
param(
    [ValidateSet("List", "Request", "Read", "Collect", "Execution", "VerifyService")]
    [string]$Action = "List",
    [string]$DeviceId = "",
    [ValidateRange(0, 60)]
    [int]$WaitSeconds = 30,
    [string]$OutputPath = "",
    [string]$CodexExecutable = ""
)
$ErrorActionPreference = "Stop"
if ($Action -eq "VerifyService") {
    if ([string]::IsNullOrWhiteSpace($CodexExecutable) -or -not (Test-Path -LiteralPath $CodexExecutable -PathType Leaf)) {
        throw "请通过 -CodexExecutable 指定实际 Codex CLI 可执行文件。"
    }
    $codexExecutablePath = (Resolve-Path -LiteralPath $CodexExecutable).Path
    & $codexExecutablePath --version
    if ($LASTEXITCODE -ne 0) { throw "Codex CLI 单次检查失败。" }
    $repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
    $reportPath = if ($OutputPath) { [IO.Path]::GetFullPath($OutputPath) } else {
        Join-Path $repositoryRoot ("temp/service-without-cdp-{0}.json" -f [Guid]::NewGuid().ToString("N"))
    }
    if (Test-Path -LiteralPath $reportPath) { throw "验收报告已存在，请指定新的 -OutputPath。" }
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($reportPath)) -Force | Out-Null
    # 用户要求真实断开验收：仅本次测试进程禁用 CDP、隔离桌面 IPC；不修改常驻服务或系统环境变量。
    $testEnvironment = @{
        CODEX_ELVES_DISABLE_CDP = "1"
        CODEX_ELVES_SERVICE_CHECK_EXE = $codexExecutablePath
        CODEX_ELVES_SERVICE_CHECK_REPORT = $reportPath
        CODEX_ELVES_TEST_IPC_PIPE = "\\.\pipe\codex-elves-isolation-$([Guid]::NewGuid().ToString('N'))"
    }
    $previousEnvironment = @{}
    foreach ($name in $testEnvironment.Keys) {
        $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
    }
    Push-Location -LiteralPath $repositoryRoot
    try {
        foreach ($name in $testEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $testEnvironment[$name], "Process")
        }
        & cargo test -p codex-elves-core --lib remote_access::tests::harness_real_app_server_without_cdp -- --ignored --exact --test-threads=1 --nocapture
        if ($LASTEXITCODE -ne 0) { throw "无 CDP 验收失败，不能将现有服务的正常状态作为通过依据。" }
        if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) { throw "验收未生成结果文件。" }
        Write-Output "无 CDP 验收报告：$reportPath"
    } finally {
        foreach ($name in $testEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], "Process")
        }
        Pop-Location
    }
    return
}
$stateDirectory = Join-Path ([Environment]::GetFolderPath("UserProfile")) ".codex-session-delete"
$endpointPath = Join-Path $stateDirectory "remote-access-service.json"
if ($Action -notin @("List", "Execution") -and [string]::IsNullOrWhiteSpace($DeviceId)) {
    throw "请先用 -Action List 查看设备，再指定 -DeviceId。"
}
function Invoke-DiagnosticsCommand([hashtable]$Payload) {
    $endpoint = Get-Content -Raw -LiteralPath $endpointPath | ConvertFrom-Json
    if ($endpoint.protocolVersion -lt 2 -or $endpoint.port -lt 1 -or $endpoint.port -gt 65535 -or
        $endpoint.token -notmatch "^[a-f0-9]{64}$") {
        throw "本机远程后台状态无效。"
    }
    if ($Payload.mode -eq "execution" -and $endpoint.protocolVersion -lt 5) {
        throw "执行通道诊断需要管理协议 5，请先更新并重新启动远程访问后台。"
    }
    $result = Invoke-RestMethod -Uri "http://127.0.0.1:$($endpoint.port)/admin/diagnostics" `
        -Method Post -ContentType "application/json" -Headers @{ Authorization = "Bearer $($endpoint.token)" } `
        -Body ($Payload | ConvertTo-Json -Compress) -TimeoutSec 15
    if ($result.status -ne "ok") { throw "本机远程后台未确认诊断操作。" }
    return $result
}
function Save-DiagnosticsReport($Report) {
    if ($null -eq $Report) { throw "该设备尚未返回日志；App 离线时可在手机上导出。" }
    $target = $OutputPath
    if ([string]::IsNullOrWhiteSpace($target)) {
        $exportDirectory = Join-Path $stateDirectory "remote-client-diagnostics-export"
        New-Item -Path $exportDirectory -ItemType Directory -Force | Out-Null
        $target = Join-Path $exportDirectory ("android-diagnostics-{0}-{1}.json" -f `
            [DateTimeOffset]::Now.ToString("yyyyMMdd-HHmmss"), [Guid]::NewGuid().ToString("N").Substring(0, 8))
    }
    $target = [IO.Path]::GetFullPath($target)
    if (Test-Path -LiteralPath $target) { throw "输出文件已存在，请指定新路径。" }
    $json = $Report | ConvertTo-Json -Depth 30
    [IO.File]::WriteAllText($target, $json, [Text.UTF8Encoding]::new($false))
    [pscustomobject]@{
        Status = "received"
        DeviceId = $Report.deviceId
        RequestId = $Report.requestId
        EventCount = @($Report.bundle.events).Count
        ReceivedAtMs = $Report.receivedAtMs
        File = $target
    } | ConvertTo-Json
}
if ($Action -eq "List") {
    Invoke-DiagnosticsCommand @{ mode = "list" } | ConvertTo-Json -Depth 15
    return
}
if ($Action -eq "Execution") {
    # 服务模式验收读取真实后端和 CDP 拦截计数，不改变设备运行模式。
    Invoke-DiagnosticsCommand @{ mode = "execution" } | ConvertTo-Json -Depth 15
    return
}
if ($Action -eq "Read") {
    $result = Invoke-DiagnosticsCommand @{ mode = "read"; deviceId = $DeviceId }
    Save-DiagnosticsReport $result.report
    return
}
$request = Invoke-DiagnosticsCommand @{ mode = "request"; deviceId = $DeviceId }
if ($Action -eq "Request") {
    $request | ConvertTo-Json -Depth 10
    return
}
$deadline = [DateTimeOffset]::UtcNow.AddSeconds($WaitSeconds)
do {
    $result = Invoke-DiagnosticsCommand @{ mode = "read"; deviceId = $DeviceId }
    if ($result.report.requestId -eq $request.requestId) {
        Save-DiagnosticsReport $result.report
        return
    }
    if ([DateTimeOffset]::UtcNow -ge $deadline) { break }
    Start-Sleep -Milliseconds 1000
} while ($true)
[pscustomobject]@{
    Status = "pending"
    DeviceId = $DeviceId
    RequestId = $request.requestId
    ExpiresAtMs = $request.expiresAtMs
    Message = "尚未收到本次日志。请打开 App 保持连接，稍后用 -Action Read 获取；离线时可直接在手机导出。"
} | ConvertTo-Json
