#requires -Version 7.0
# 针对服务切换与回滚的隔离检查：全部进程、端口、启动和网络操作均使用夹具，不触碰正式服务。
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "update-remote-service.ps1")
$script:testRoot = Join-Path ([IO.Path]::GetTempPath()) ("codex-elves-remote-update-test-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $script:testRoot | Out-Null
$script:realSwitch = ${function:Switch-RemoteUpdateProgram}
$script:realProtected = ${function:Assert-RemoteUpdateProtected}
$script:passed = 0

function Assert-Test([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "断言失败：$Message" }
}
function Assert-Fails([scriptblock]$Run, [string]$Message) {
    $failed = $false
    try { & $Run | Out-Null } catch { $failed = $true }
    Assert-Test $failed $Message
}
function Complete-Test([string]$Name) {
    $script:passed++
    Write-Output "PASS $Name"
}

# 点源不执行主入口；这些替身只存在于当前测试脚本作用域。
function Initialize-RemoteUpdateNative {}
function Assert-RemoteUpdateStopAccess([int]$ProcessId) {}
function Assert-RemoteUpdateProtected($Plan) {
    if ($script:fixture.protectedChanged) { throw "夹具：管理器或启动器身份改变。" }
}
function Get-RemoteUpdateProcess([int]$ProcessId) {
    if ($ProcessId -eq 22) { return $script:fixture.plan.launcher }
    if ($ProcessId -eq 33 -and $script:fixture.manager) { return $script:fixture.manager }
    if (-not $script:fixture.alive -or $ProcessId -ne $script:fixture.service.processId) { throw "夹具进程已退出。" }
    return $script:fixture.service
}
function Get-RemoteUpdateLive([string]$Directory) {
    [pscustomobject]@{ service = (Get-RemoteUpdateProcess $script:fixture.service.processId); status = @{} }
}
function Get-CimInstance {
    [CmdletBinding()]
    param([string]$ClassName, [string]$Filter)
    if ($script:fixture.alive -and $Filter -eq "ProcessId = $($script:fixture.service.processId)") {
        return [pscustomobject]@{ ProcessId = $script:fixture.service.processId }
    }
}
function Get-NetTCPConnection {
    [CmdletBinding()]
    param([string]$State, [int]$LocalPort)
    [pscustomobject]@{ OwningProcess = $(if ($LocalPort -eq 33330) { 22 } else { $script:fixture.service.processId }) }
}
function Stop-RemoteUpdateService($Plan, $Service, [string]$Retired) {
    $script:fixture.stops++
    Assert-Test ($Service.processId -eq $script:fixture.service.processId) "只能停止夹具正式服务"
    $script:fixture.alive = $false
}
function Request-RemoteUpdateStart($Plan) {
    $script:fixture.startRequests++
    $newProgram = (Get-RemoteUpdateHash $Plan.installedProgram) -eq $Plan.newSha256
    if ($script:fixture.failStart -and $newProgram) { throw "夹具：新版无法启动。" }
    if (-not $script:fixture.alive) {
        $script:fixture.service = [pscustomobject]@{
            processId = $script:fixture.service.processId + 1; parentProcessId = 22
            imagePath = $Plan.installedProgram; createdFileTime = [string]([long]$script:fixture.service.createdFileTime + 1)
        }
        $script:fixture.alive = $true
        Write-RemoteUpdateJson (Join-Path $Plan.stateDirectory "remote-access-service.json") @{
            processId = $script:fixture.service.processId; port = 33332; protocolVersion = 5; token = ("f" * 64)
        }
    }
}
function Test-RemoteUpdateResources([string]$Origin, $Expected, [string[]]$Paths) {
    if ($script:fixture.failPublic -and $Origin -eq $script:fixture.plan.publicOrigin) { throw "夹具：公网不可达。" }
    if ($script:fixture.failLocal -and $Expected.revision -eq 2) { throw "夹具：新版资源不符。" }
    $expectedHash = if ($Expected.revision -eq 1) { $script:fixture.plan.oldSha256 } else { $script:fixture.plan.newSha256 }
    Assert-Test ((Get-RemoteUpdateHash $script:fixture.plan.installedProgram) -eq $expectedHash) "核验的资源版本必须对应磁盘程序"
    Assert-Test (@($Paths | Where-Object { $_ -cnotin $Expected.files.path }).Count -eq 0) "回滚不得验证仅新版存在的资源"
}
function Switch-RemoteUpdateProgram($Plan, [string]$Program, [string]$Hash, [string]$Suffix) {
    $retired = & $script:realSwitch $Plan $Program $Hash $Suffix
    if ($script:fixture.alive -and $script:fixture.service.imagePath -eq $Plan.installedProgram) {
        $script:fixture.service.imagePath = $retired
    }
    if ($script:fixture.failAfterRename -and $Suffix -eq "apply") { throw "夹具：文件替换后、停止服务前失败。" }
    return $retired
}

function New-TestFixture {
    $directory = Join-Path $script:testRoot ([Guid]::NewGuid().ToString("N"))
    $install = Join-Path $directory "installed"
    $state = Join-Path $directory "state"
    $records = Join-Path $directory "records"
    foreach ($folder in @($install, $state, "$records/prepared", "$records/backup")) {
        New-Item -ItemType Directory -Path $folder -Force | Out-Null
    }
    $installed = Join-Path $install "codex-elves-manager.exe"
    $prepared = Join-Path $records "prepared/codex-elves-manager.exe"
    $backup = Join-Path $records "backup/codex-elves-manager.exe"
    [IO.File]::WriteAllText($installed, "fixture old program")
    [IO.File]::WriteAllText($prepared, "fixture new program")
    Copy-Item -LiteralPath $installed -Destination $backup
    Write-RemoteUpdateJson (Join-Path $state "settings.json") @{ example = "unchanged" }
    Write-RemoteUpdateJson (Join-Path $state "remote-access-devices.json") @(
        @{ id = "fixture-device"; name = "test"; tokenHash = "synthetic"; createdAtMs = 1; expiresAtMs = 999999; lastSeenAtMs = 2 }
    )
    Write-RemoteUpdateJson (Join-Path $state "remote-access-service.json") @{
        processId = 11; port = 33332; protocolVersion = 5; token = ("f" * 64)
    }
    $service = [pscustomobject]@{ processId = 11; parentProcessId = 22; imagePath = $installed; createdFileTime = "100" }
    $oldFiles = @("/app.js", "/style.css") | ForEach-Object {
        [pscustomobject]@{ path = $_; contentType = "text/plain"; size = 1; sha256 = "old" }
    }
    $newFiles = @($oldFiles) + @([pscustomobject]@{ path = "/new-module.js"; contentType = "text/plain"; size = 2; sha256 = "new" })
    $plan = [pscustomobject]@{
        schemaVersion = 1; id = [Guid]::NewGuid().ToString("N"); stateDirectory = $state
        installedProgram = $installed; preparedProgram = $prepared; backupProgram = $backup
        oldSha256 = Get-RemoteUpdateHash $backup; newSha256 = Get-RemoteUpdateHash $prepared
        serviceBefore = $service
        launcher = [pscustomobject]@{ processId = 22; parentProcessId = 33; imagePath = (Join-Path $install "codex-elves.exe"); createdFileTime = "90" }
        protectedManagers = @(); helperPort = 33330; localOrigin = "http://127.0.0.1:33331"
        publicOrigin = "https://fixture.invalid"; stateBefore = Get-RemoteUpdateStateStamp $state
        resourcesBefore = [pscustomobject]@{ revision = 1; files = @($oldFiles) }
        targetResources = [pscustomobject]@{ revision = 2; files = @($newFiles) }
        verifyPaths = @("/app.js", "/style.css", "/new-module.js")
    }
    $script:fixture = @{
        plan = $plan; path = (Join-Path $records "plan.json"); service = $service; alive = $true
        stops = 0; startRequests = 0; protectedChanged = $false; failStart = $false
        failLocal = $false; failPublic = $false; failAfterRename = $false
    }
    Write-RemoteUpdateJson $script:fixture.path $plan
}
function Invoke-TestAction([string]$Operation) {
    Invoke-RemoteServiceUpdate -Action $Operation -PlanPath $script:fixture.path -TimeoutSeconds 0 | Out-Null
}

try {
    New-TestFixture
    $devices = Read-RemoteUpdateJson (Join-Path $script:fixture.plan.stateDirectory "remote-access-devices.json")
    $devices.lastSeenAtMs = 50
    Write-RemoteUpdateJson (Join-Path $script:fixture.plan.stateDirectory "remote-access-devices.json") @($devices)
    Assert-Test ((Assert-RemoteUpdateState $script:fixture.plan.stateBefore $script:fixture.plan.stateDirectory) -eq 1) "正常访问不使授权摘要失效"
    Complete-Test "授权摘要忽略 lastSeenAtMs"

    New-TestFixture
    $managerBefore = [pscustomobject]@{ processId = 33; parentProcessId = 44; imagePath = $script:fixture.plan.installedProgram; createdFileTime = "80" }
    $script:fixture.plan.protectedManagers = @($managerBefore)
    $managerRetired = Join-Path (Split-Path -Parent $script:fixture.plan.installedProgram) "codex-elves-manager.remote-before-$($script:fixture.plan.id)-apply.exe"
    Copy-Item -LiteralPath $script:fixture.plan.backupProgram -Destination $managerRetired
    $script:fixture.manager = [pscustomobject]@{ processId = 33; parentProcessId = 44; imagePath = $managerRetired; createdFileTime = "80" }
    & $script:realProtected $script:fixture.plan
    $script:fixture.manager.createdFileTime = "81"
    Assert-Fails { & $script:realProtected $script:fixture.plan } "允许路径变化仍须拒绝 PID 复用"
    Complete-Test "管理器映像随本次重命名变化仍保留创建时间校验"

    New-TestFixture
    Invoke-TestAction "Apply"
    Assert-Test ($script:fixture.stops -eq 1 -and $script:fixture.startRequests -eq 1) "更新只切换一次服务"
    Invoke-TestAction "Apply"
    Assert-Test ($script:fixture.stops -eq 1) "重复 Apply 不重启已更新服务"
    Invoke-TestAction "Rollback"
    Assert-Test ((Get-RemoteUpdateHash $script:fixture.plan.installedProgram) -eq $script:fixture.plan.oldSha256) "回滚恢复原程序"
    Invoke-TestAction "Verify"
    Complete-Test "正常更新、重复应用、回滚及回滚后核验"

    foreach ($failure in @("failLocal", "failStart", "failAfterRename")) {
        New-TestFixture
        $script:fixture[$failure] = $true
        Assert-Fails { Invoke-TestAction "Apply" } "失败必须报告非成功"
        $result = Read-RemoteUpdateJson (Join-Path (Split-Path -Parent $script:fixture.path) "result.json")
        Assert-Test $result.previousVersionRestored "本地失败应恢复上一版本：$failure"
        Assert-Test ((Get-RemoteUpdateHash $script:fixture.plan.installedProgram) -eq $script:fixture.plan.oldSha256) "原程序已恢复"
        if ($failure -eq "failAfterRename") {
            Assert-Test ($script:fixture.stops -eq 0) "文件阶段失败保留原服务进程"
            Invoke-TestAction "Verify"
        }
        Complete-Test "自动恢复 $failure"
    }

    New-TestFixture
    $script:fixture.failPublic = $true
    Assert-Fails { Invoke-TestAction "Apply" } "公网失败应报告待核验"
    Assert-Test ($script:fixture.stops -eq 1) "公网失败不能再重启服务"
    Assert-Test ((Get-RemoteUpdateHash $script:fixture.plan.installedProgram) -eq $script:fixture.plan.newSha256) "保留已健康的新版本地服务"
    $script:fixture.failPublic = $false
    Invoke-TestAction "Verify"
    Assert-Test ($script:fixture.stops -eq 1) "重试 Verify 只能读取"
    Complete-Test "公网失败保留本地服务，Verify 可重试"

    foreach ($failure in @("candidate", "settings", "protected", "parent")) {
        New-TestFixture
        switch ($failure) {
            "candidate" { [IO.File]::WriteAllText($script:fixture.plan.preparedProgram, "tampered") }
            "settings" { Write-RemoteUpdateJson (Join-Path $script:fixture.plan.stateDirectory "settings.json") @{ example = "changed" } }
            "protected" { $script:fixture.protectedChanged = $true }
            "parent" { $script:fixture.service.parentProcessId = 999 }
        }
        Assert-Fails { Invoke-TestAction "Apply" } "前置条件改变必须拒绝应用"
        Assert-Test ($script:fixture.stops -eq 0) "拒绝后不能停止任何进程"
        Assert-Test ((Get-RemoteUpdateHash $script:fixture.plan.installedProgram) -eq $script:fixture.plan.oldSha256) "拒绝后程序不变"
        Complete-Test "应用前拒绝变化 $failure"
    }

    New-TestFixture
    $retired = Join-Path (Split-Path -Parent $script:fixture.plan.installedProgram) "codex-elves-manager.remote-before-$($script:fixture.plan.id)-apply.exe"
    Move-Item -LiteralPath $script:fixture.plan.installedProgram -Destination $retired
    Write-RemoteUpdateJson (Join-Path (Split-Path -Parent $script:fixture.path) "transaction.json") @{
        planId = $script:fixture.plan.id; action = "Apply"; phase = "before-switch"; retiredProgram = $retired
    }
    $script:fixture.alive = $false
    Invoke-TestAction "Rollback"
    Assert-Test ((Get-RemoteUpdateHash $script:fixture.plan.installedProgram) -eq $script:fixture.plan.oldSha256) "中断后缺少标准映像也能恢复"
    Complete-Test "文件重命名后中断可用 Rollback 恢复"

    Write-Output "全部 $script:passed 项针对性检查通过；未操作真实进程或网络。"
} finally {
    $resolvedTestRoot = (Resolve-Path -LiteralPath $script:testRoot).Path
    $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    Assert-RemoteUpdateChildPath $resolvedTestRoot $temporaryRoot
    if ([IO.Path]::GetFileName($resolvedTestRoot) -notlike "codex-elves-remote-update-test-*") { throw "拒绝清理意外目录。" }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
}
