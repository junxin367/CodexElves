[CmdletBinding()]
param(
    [ValidateSet("List", "Request", "Read", "Collect")]
    [string]$Action = "List",
    [string]$DeviceId = "",
    [ValidateRange(0, 60)]
    [int]$WaitSeconds = 30,
    [string]$OutputPath = ""
)
$ErrorActionPreference = "Stop"
$stateDirectory = Join-Path ([Environment]::GetFolderPath("UserProfile")) ".codex-session-delete"
$endpointPath = Join-Path $stateDirectory "remote-access-service.json"
if ($Action -ne "List" -and [string]::IsNullOrWhiteSpace($DeviceId)) {
    throw "请先用 -Action List 查看设备，再指定 -DeviceId。"
}
function Invoke-DiagnosticsCommand([hashtable]$Payload) {
    $endpoint = Get-Content -Raw -LiteralPath $endpointPath | ConvertFrom-Json
    if ($endpoint.protocolVersion -ne 2 -or $endpoint.port -lt 1 -or $endpoint.port -gt 65535 -or
        $endpoint.token -notmatch "^[a-f0-9]{64}$") {
        throw "本机远程后台状态无效。"
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
