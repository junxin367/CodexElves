#requires -Version 7.0
<#
.SYNOPSIS
准备、应用、核验或回滚 Windows 正式远程服务更新。
.DESCRIPTION
默认只准备程序和更新计划。Apply / Rollback 仅切换已核对的远程服务，
通过原 launcher 的本机管理接口拉起后台，保留管理器、本地代理和授权。
用法及中断恢复见 docs/remote-service-update.md。点源加载只定义函数，供隔离检查使用。
#>
[CmdletBinding()]
param(
    [ValidateSet("Prepare", "Apply", "Verify", "Rollback")]
    [string]$Action = "Prepare",
    [string]$PlanPath = "",
    [string]$ProgramPath = "",
    [string]$StateDirectory = "",
    [ValidateRange(0, 65535)]
    [int]$HelperPort = 0,
    [switch]$SkipBuild,
    # 60 秒覆盖服务启动和隧道恢复；单个 HTTP 请求另有更短超时，不无限等待。
    [ValidateRange(10, 180)]
    [int]$TimeoutSeconds = 60
)

function Initialize-RemoteUpdateNative {
    if ("CodexElves.RemoteUpdateNative" -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
namespace CodexElves {
    public static class RemoteUpdateNative {
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern IntPtr OpenProcess(uint access, bool inherit, int id);
        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder path, ref int size);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool GetProcessTimes(IntPtr process, out long created, out long exited, out long kernel, out long user);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool TerminateProcess(IntPtr process, uint exitCode);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
        [DllImport("kernel32.dll")]
        static extern bool CloseHandle(IntPtr handle);
        static IntPtr Open(int id, uint access) {
            var handle = OpenProcess(access, false, id);
            if (handle == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot open process " + id);
            return handle;
        }
        static string Image(IntPtr handle) {
            var path = new StringBuilder(32768);
            int size = path.Capacity;
            if (!QueryFullProcessImageName(handle, 0, path, ref size))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            return path.ToString();
        }
        static long Created(IntPtr handle) {
            long created, exited, kernel, user;
            if (!GetProcessTimes(handle, out created, out exited, out kernel, out user))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            return created;
        }
        public static string[] Identity(int id) {
            var handle = Open(id, 0x1000);
            try { return new[] { Image(handle), Created(handle).ToString() }; }
            finally { CloseHandle(handle); }
        }
        public static void CheckTerminateAccess(int id) {
            var handle = Open(id, 0x1001);
            CloseHandle(handle);
        }
        // 用同一进程句柄核对创建时间和映像后再停止，避免 PID 复用或按名称误停管理器。
        public static void Stop(int id, long created, string firstPath, string secondPath) {
            var handle = Open(id, 0x101001);
            try {
                var image = Image(handle);
                if (Created(handle) != created ||
                    !(String.Equals(image, firstPath, StringComparison.OrdinalIgnoreCase) ||
                      String.Equals(image, secondPath, StringComparison.OrdinalIgnoreCase)))
                    throw new InvalidOperationException("Process identity changed; nothing was stopped.");
                if (!TerminateProcess(handle, 0))
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                // 最多等 10 秒确认这个已核对的服务退出；超时即报错，不扩大停止范围。
                if (WaitForSingleObject(handle, 10000) != 0)
                    throw new InvalidOperationException("Remote service did not exit in 10 seconds.");
            } finally { CloseHandle(handle); }
        }
        public static bool Contains(byte[] binary, byte[] source) {
            return source.Length > 0 && binary.AsSpan().IndexOf(source) >= 0;
        }
    }
}
'@
}

function Read-RemoteUpdateJson([string]$Path) {
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Write-RemoteUpdateJson([string]$Path, $Value) {
    $temporaryPath = "$Path.writing"
    [IO.File]::WriteAllText($temporaryPath, ($Value | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporaryPath -Destination $Path -Force
}

function Get-RemoteUpdateHash([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-RemoteUpdateChildPath([string]$Path, [string]$Directory) {
    $absolutePath = [IO.Path]::GetFullPath($Path)
    $absoluteDirectory = [IO.Path]::GetFullPath($Directory).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $absolutePath.StartsWith($absoluteDirectory, [StringComparison]::OrdinalIgnoreCase)) {
        throw "路径不在预期目录内：$absolutePath"
    }
}

function Get-RemoteUpdateProcess([int]$ProcessId) {
    $process = Get-CimInstance Win32_Process -Filter "ProcessId = $ProcessId"
    if (-not $process) { throw "进程 $ProcessId 已退出，请重新 Prepare。" }
    # WMI 的 ExecutablePath 可能为空；有限查询权限即可读取真实路径，无需重启或提权管理器。
    $identity = [CodexElves.RemoteUpdateNative]::Identity($ProcessId)
    [pscustomobject]@{
        processId = $ProcessId
        parentProcessId = [int]$process.ParentProcessId
        imagePath = $identity[0]
        createdFileTime = $identity[1]
    }
}

function Assert-RemoteUpdateProcess($Expected, [string[]]$AllowedImages = @()) {
    $actual = Get-RemoteUpdateProcess $Expected.processId
    if ($actual.createdFileTime -ne $Expected.createdFileTime -or
        $actual.imagePath -notin (@($Expected.imagePath) + $AllowedImages) -or $actual.parentProcessId -ne $Expected.parentProcessId) {
        throw "进程 $($Expected.processId) 身份已变化，请重新 Prepare。"
    }
}

function Assert-RemoteUpdateStopAccess([int]$ProcessId) {
    [CodexElves.RemoteUpdateNative]::CheckTerminateAccess($ProcessId)
}

# 单次静态资源读取默认最多 10 秒；与外层默认 60 秒重试窗口分开，避免一次请求无限挂起。
function Invoke-RemoteUpdateHttp([string]$Url, [string]$Method = "GET", [string]$Token = "", [int]$Seconds = 10) {
    $handler = [Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $false
    if ([uri]$Url -and ([uri]$Url).IsLoopback) { $handler.UseProxy = $false }
    $client = [Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($Seconds)
    $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::new($Method), $Url)
    $request.Headers.CacheControl = [Net.Http.Headers.CacheControlHeaderValue]::new()
    $request.Headers.CacheControl.NoCache = $true
    $request.Headers.AcceptEncoding.ParseAdd("identity")
    if ($Token) { $request.Headers.Authorization = [Net.Http.Headers.AuthenticationHeaderValue]::new("Bearer", $Token) }
    if ($Method -eq "POST") { $request.Content = [Net.Http.StringContent]::new("{}", [Text.Encoding]::UTF8, "application/json") }
    $response = $null
    try {
        # 不设置 Origin，不禁用 TLS 校验，不携带网页 Cookie，也不把本机管理令牌发往公网。
        if ($Token -and -not ([uri]$Url).IsLoopback) { throw "管理凭据仅允许用于回环地址。" }
        $response = $client.SendAsync($request).GetAwaiter().GetResult()
        if (-not $response.IsSuccessStatusCode) { throw "HTTP $([int]$response.StatusCode)" }
        return ,($response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult())
    } catch {
        # 仅报告方法和地址；不序列化完整请求、响应或含凭据的状态对象。
        throw "$Method $Url 请求失败：$($_.Exception.GetBaseException().Message)"
    } finally {
        if ($response) { $response.Dispose() }
        $request.Dispose()
        $client.Dispose()
    }
}

function Invoke-RemoteUpdateJson([string]$Url, [string]$Method = "GET", [string]$Token = "", [int]$Seconds = 10) {
    [Text.Encoding]::UTF8.GetString((Invoke-RemoteUpdateHttp $Url $Method $Token $Seconds)) | ConvertFrom-Json
}

function Get-RemoteUpdateEndpoint([string]$Directory) {
    $endpoint = Read-RemoteUpdateJson (Join-Path $Directory "remote-access-service.json")
    if ($endpoint.processId -le 0 -or $endpoint.port -lt 1 -or $endpoint.port -gt 65535 -or
        $endpoint.protocolVersion -le 0 -or $endpoint.token -notmatch '^[a-fA-F0-9]{64}$') {
        throw "正式服务端点记录无效。"
    }
    return $endpoint
}

function Get-RemoteUpdateLive([string]$Directory) {
    $endpoint = Get-RemoteUpdateEndpoint $Directory
    $base = "http://127.0.0.1:$($endpoint.port)"
    # 本机健康探测只读取小响应，限 3 秒；公网可用性另行核验，不能用它代替服务身份。
    $health = Invoke-RemoteUpdateJson "$base/health" "GET" $endpoint.token 3
    if ($health.status -ne "ok" -or $health.processId -ne $endpoint.processId -or
        $health.protocolVersion -ne $endpoint.protocolVersion) { throw "正式服务健康身份不一致。" }
    $status = Invoke-RemoteUpdateJson "$base/admin/status" "POST" $endpoint.token
    if ($status.status -ne "ok" -or -not $status.running -or $status.serviceProcessId -ne $endpoint.processId) {
        throw "正式远程服务未处于运行状态。"
    }
    [pscustomobject]@{
        service = (Get-RemoteUpdateProcess $endpoint.processId); status = $status
        managementProtocolVersion = $health.protocolVersion
    }
}

function Get-RemoteUpdateStateStamp([string]$Directory) {
    # 用户要求保留设置、访问密钥及原有授权；只记录摘要，lastSeenAtMs 正常变化不算授权改变。
    $files = foreach ($name in @("settings.json", "remote-access-key.json", "remote-access-zrok.json",
        "remote-access-ngrok.json", "remote-access-primary.json", "remote-access-frp/draft.json", "remote-access-frp/active.json")) {
        $path = Join-Path $Directory $name
        [pscustomobject]@{ name = $name; sha256 = $(if (Test-Path -LiteralPath $path) { Get-RemoteUpdateHash $path } else { $null }) }
    }
    $devicePath = Join-Path $Directory "remote-access-devices.json"
    $devices = if (Test-Path -LiteralPath $devicePath) { @(Read-RemoteUpdateJson $devicePath) } else { @() }
    $records = foreach ($device in $devices) {
        $stable = [ordered]@{}
        foreach ($field in @("id", "name", "tokenHash", "createdAtMs", "expiresAtMs", "keyId", "publicOrigin", "clientIdHash")) {
            $stable[$field] = if ($field -in @("createdAtMs", "expiresAtMs")) { [uint64]$device.$field } else { [string]$device.$field }
        }
        $bytes = [Text.Encoding]::UTF8.GetBytes(($stable | ConvertTo-Json -Compress))
        [pscustomobject]@{ id = $device.id; sha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant() }
    }
    [pscustomobject]@{ files = @($files); devices = @($records | Sort-Object id) }
}

function Assert-RemoteUpdateState($Expected, [string]$Directory) {
    $actual = Get-RemoteUpdateStateStamp $Directory
    foreach ($file in $Expected.files) {
        $match = @($actual.files | Where-Object name -EQ $file.name)
        if ($match.Count -ne 1 -or $match[0].sha256 -ne $file.sha256) { throw "配置已变化：$($file.name)；未覆盖该文件。" }
    }
    foreach ($device in $Expected.devices) {
        $match = @($actual.devices | Where-Object id -EQ $device.id)
        if ($match.Count -ne 1 -or $match[0].sha256 -ne $device.sha256) { throw "原有设备授权已变化；未恢复或覆盖授权。" }
    }
    return $actual.devices.Count
}

function Get-RemoteUpdateSource([string]$Repository) {
    $source = Get-Content -LiteralPath (Join-Path $Repository "crates/codex-elves-core/src/remote_access.rs") -Raw
    $assetRoot = Join-Path $Repository "assets/remote-access"
    $constants = @{}
    $includes = [regex]::Matches($source, 'const\s+(\w+):\s*[^=]+=\s*include_(?:str|bytes)!\("\.\./\.\./\.\./assets/remote-access/([^"]+)"\);')
    foreach ($include in $includes) { $constants[$include.Groups[1].Value] = Join-Path $assetRoot $include.Groups[2].Value }
    $start = $source.IndexOf("fn web_assets()")
    if ($start -lt 0) { throw "无法识别网页资源定义。" }
    $end = $source.IndexOf("struct AppResourcesManifest", $start)
    if ($start -lt 0 -or $end -le $start) { throw "无法识别网页资源定义。" }
    $routes = [regex]::Matches($source.Substring($start, $end - $start),
        '"(/[^"]*)"\s*,\s*StaticWebAsset::new\(\s*"([^"]+)"\s*,\s*(\w+)(?:\.as_bytes\(\))?\s*,?\s*\)')
    $files = foreach ($route in $routes) {
        $path = $constants[$route.Groups[3].Value]
        if (-not $path) { throw "网页资源缺少源码映射：$($route.Groups[1].Value)" }
        [pscustomobject]@{
            path = $route.Groups[1].Value; contentType = $route.Groups[2].Value
            size = (Get-Item -LiteralPath $path).Length; sha256 = Get-RemoteUpdateHash $path
        }
    }
    if (@($files).Count -lt 3 -or "/app.js" -notin $files.path -or "/style.css" -notin $files.path) {
        throw "网页资源映射不完整。"
    }
    $revisionPath = Join-Path $assetRoot "resource-revision.txt"
    $revision = [uint64](Get-Content -LiteralPath $revisionPath -Raw).Trim()
    if ($revision -le 0) { throw "资源号必须为正整数。" }
    $serviceSource = Get-Content -LiteralPath (Join-Path $Repository "crates/codex-elves-core/src/remote_access/service.rs") -Raw
    $protocol = [regex]::Match($serviceSource, 'const PROTOCOL_VERSION:\s*u32\s*=\s*(\d+);')
    if (-not $protocol.Success) { throw "无法识别服务管理协议。" }
    [pscustomobject]@{
        revision = $revision; files = @($files | Sort-Object path)
        embeddedPaths = @(@($constants.Values) + @($revisionPath) | Sort-Object -Unique)
        managementProtocolVersion = [int]$protocol.Groups[1].Value
    }
}

function Assert-RemoteUpdateManifest($Expected, $Actual) {
    if ($Actual.revision -ne $Expected.revision -or @($Actual.files).Count -ne @($Expected.files).Count) {
        throw "资源号或清单文件数不一致，预期 $($Expected.revision)，实际 $($Actual.revision)。"
    }
    foreach ($file in $Expected.files) {
        $match = @($Actual.files | Where-Object path -CEQ $file.path)
        if ($match.Count -ne 1 -or $match[0].sha256 -ne $file.sha256 -or
            $match[0].size -ne $file.size -or $match[0].contentType -ne $file.contentType) {
            throw "资源清单与准备记录不一致：$($file.path)"
        }
    }
}

function Test-RemoteUpdateResources([string]$Origin, $Expected, [string[]]$Paths) {
    $manifest = Invoke-RemoteUpdateJson "$Origin/app-resources.json"
    Assert-RemoteUpdateManifest $Expected $manifest
    foreach ($path in $Paths) {
        if ($path -notmatch '^/[^?#\\]*$' -or $path.Contains("..") -or $path.StartsWith("//")) { throw "不支持的资源路径。" }
        $expectedFile = @($Expected.files | Where-Object path -CEQ $path)
        if ($expectedFile.Count -ne 1) { throw "核验路径不在资源清单中：$path" }
        $bytes = Invoke-RemoteUpdateHttp "$Origin$path"
        $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
        if ($bytes.Length -ne $expectedFile[0].size -or $hash -ne $expectedFile[0].sha256) {
            throw "实际资源字节与准备记录不一致：$path"
        }
    }
}

function Assert-RemoteUpdatePlan($Plan, [string]$Path) {
    if ($Plan.schemaVersion -ne 1 -or $Plan.id -notmatch '^[a-f0-9]{32}$') { throw "不支持的更新计划。" }
    if ([IO.Path]::GetFileName($Plan.installedProgram) -ne "codex-elves-manager.exe") { throw "安装程序路径无效。" }
    $directory = Split-Path -Parent $Plan.installedProgram
    if ($Plan.launcher.imagePath -ne (Join-Path $directory "codex-elves.exe")) { throw "启动器与安装目录不匹配。" }
    foreach ($program in @($Plan.preparedProgram, $Plan.backupProgram)) {
        Assert-RemoteUpdateChildPath $program (Split-Path -Parent $Path)
        if ((Get-RemoteUpdateHash $program) -ne $(if ($program -eq $Plan.preparedProgram) { $Plan.newSha256 } else { $Plan.oldSha256 })) {
            throw "准备程序或回滚备份摘要不一致，请重新 Prepare。"
        }
    }
    foreach ($origin in @($Plan.localOrigin, $Plan.publicOrigin) | Where-Object { $_ }) {
        $uri = [uri]$origin
        if (-not $uri.IsAbsoluteUri -or $uri.Scheme -notin @("http", "https") -or
            $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -ne "/") { throw "入口必须为 HTTP/HTTPS 根地址。" }
    }
    if (-not ([uri]$Plan.localOrigin).IsLoopback) { throw "正式本地入口必须为回环地址。" }
}

function Assert-RemoteUpdateProtected($Plan) {
    Assert-RemoteUpdateProcess $Plan.launcher
    foreach ($process in $Plan.protectedManagers) {
        $aliases = @()
        $retired = Join-Path (Split-Path -Parent $Plan.installedProgram) "codex-elves-manager.remote-before-$($Plan.id)-apply.exe"
        # 管理器与服务可能映射同一个文件；本次重命名允许映像路径变为旧文件名，但 PID、创建时间和父进程必须保持。
        if ($process.imagePath -eq $Plan.installedProgram -and (Test-Path -LiteralPath $retired) -and
            (Get-RemoteUpdateHash $retired) -eq $Plan.oldSha256) { $aliases = @($retired) }
        Assert-RemoteUpdateProcess $process $aliases
    }
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $Plan.helperPort -ErrorAction Stop)
    if ($Plan.launcher.processId -notin $listeners.OwningProcess -or
        @($listeners | Where-Object OwningProcess -NE $Plan.launcher.processId).Count -gt 0) {
        throw "本机管理端口已不归原启动器所有，请重新 Prepare。"
    }
}

function Request-RemoteUpdateStart($Plan) {
    # 必须由原 launcher 调用 ensure_running，才能保持 Windows Job 的所属生命周期；不能直接 Start-Process。
    # launcher 的现有启动预算是 8 秒，这里给本机请求 15 秒，包含状态读取余量。
    $status = Invoke-RemoteUpdateJson "http://127.0.0.1:$($Plan.helperPort)/remote-access/status" "POST" "" 15
    if ($status.status -ne "ok") { throw "原启动器未确认服务启动，请检查服务诊断日志。" }
}

function Wait-RemoteUpdateLocal($Plan, $Expected, [int]$Seconds, $RetainedService = $null) {
    if (-not $RetainedService) { $RetainedService = $Plan.serviceBefore }
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    $lastFailure = ""
    do {
        try {
            $live = Get-RemoteUpdateLive $Plan.stateDirectory
            $retained = $RetainedService -and $live.service.processId -eq $RetainedService.processId -and
                $live.service.createdFileTime -eq $RetainedService.createdFileTime -and
                (Split-Path -Parent $live.service.imagePath) -eq (Split-Path -Parent $Plan.installedProgram)
            if ($live.service.parentProcessId -ne $Plan.launcher.processId -or
                ($live.service.imagePath -ne $Plan.installedProgram -and -not $retained)) {
                throw "新服务未归属原启动器或映像路径不一致。"
            }
            $listeners = @(Get-NetTCPConnection -State Listen -LocalPort ([uri]$Plan.localOrigin).Port -ErrorAction Stop)
            if ($live.service.processId -notin $listeners.OwningProcess) { throw "正式网页端口不属于本次服务。" }
            $paths = @($Plan.verifyPaths | Where-Object { $_ -cin $Expected.files.path })
            Test-RemoteUpdateResources $Plan.localOrigin $Expected $paths
            return $live.service
        } catch { $lastFailure = $_.Exception.Message }
        # 500ms 只轮询本次目标服务就绪，不反复停止进程或发送业务请求。
        Start-Sleep -Milliseconds 500
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "正式本地服务未通过核验：$lastFailure"
}

function Switch-RemoteUpdateProgram($Plan, [string]$Program, [string]$Hash, [string]$Suffix) {
    $directory = Split-Path -Parent $Plan.installedProgram
    $pending = Join-Path $directory "codex-elves-manager.remote-pending-$($Plan.id)-$Suffix.exe"
    $retired = Join-Path $directory "codex-elves-manager.remote-before-$($Plan.id)-$Suffix.exe"
    foreach ($path in @($pending, $retired)) {
        Assert-RemoteUpdateChildPath $path $directory
        if (Test-Path -LiteralPath $path) { throw "切换目标已存在：$path；请先按文档核对中断记录。" }
    }
    Copy-Item -LiteralPath $Program -Destination $pending
    if ((Get-RemoteUpdateHash $pending) -ne $Hash) { throw "待切换程序复制校验失败。" }
    # Windows 管理器仍可能映射同一个 exe；重命名旧映像后换入新文件，不覆盖或删除被占用的旧映像。
    Move-Item -LiteralPath $Plan.installedProgram -Destination $retired
    try { Move-Item -LiteralPath $pending -Destination $Plan.installedProgram } catch {
        Move-Item -LiteralPath $retired -Destination $Plan.installedProgram
        throw
    }
    return $retired
}

function Stop-RemoteUpdateService($Plan, $Service, [string]$Retired) {
    if ($Service.processId -eq $Plan.launcher.processId -or $Service.processId -in $Plan.protectedManagers.processId -or
        $Service.parentProcessId -ne $Plan.launcher.processId) { throw "拒绝停止未确认归属的进程。" }
    [CodexElves.RemoteUpdateNative]::Stop($Service.processId, [long]$Service.createdFileTime, $Plan.installedProgram, $Retired)
}

function Restore-RemoteUpdatePrevious($Plan, $Previous, [string]$Program, [string]$Hash, $Manifest, [int]$Seconds) {
    Assert-RemoteUpdateProtected $Plan
    $retired = ""
    if (-not (Test-Path -LiteralPath $Plan.installedProgram)) {
        # 中断可能恰好发生在旧映像重命名之后；安装目录为空时，先按已核验备份恢复标准文件名。
        Copy-Item -LiteralPath $Program -Destination $Plan.installedProgram
        if ((Get-RemoteUpdateHash $Plan.installedProgram) -ne $Hash) { throw "中断恢复的程序摘要不一致。" }
    } elseif ((Get-RemoteUpdateHash $Plan.installedProgram) -ne $Hash) {
        $retired = Switch-RemoteUpdateProgram $Plan $Program $Hash ("restore-" + [Guid]::NewGuid().ToString("N"))
    }
    $endpointPath = Join-Path $Plan.stateDirectory "remote-access-service.json"
    $endpoint = if (Test-Path -LiteralPath $endpointPath) { Get-RemoteUpdateEndpoint $Plan.stateDirectory } else { $null }
    $process = if ($endpoint) { Get-CimInstance Win32_Process -Filter "ProcessId = $($endpoint.processId)" } else { $null }
    if ($process) {
        $service = Get-RemoteUpdateProcess $endpoint.processId
        if ($service.processId -ne $Previous.processId -or $service.createdFileTime -ne $Previous.createdFileTime) {
            # 新服务已经起来但未通过核验时，仅停止本安装目录、原 launcher 所属的服务。
            if ($service.imagePath -notin @($Plan.installedProgram, $retired)) { throw "回滚遇到未识别映像，已停止自动操作。" }
            Stop-RemoteUpdateService $Plan $service $retired
        }
        # 原服务尚未停止的失败发生在文件切换阶段：恢复磁盘程序即可，不再停止原服务。
    }
    Request-RemoteUpdateStart $Plan
    Wait-RemoteUpdateLocal $Plan $Manifest $Seconds $Previous
}

function New-RemoteUpdatePlan([string]$Path, [string]$Repository, [string]$Directory, [int]$Port,
    [string]$Candidate, [switch]$ReuseBuild) {
    if (Test-Path -LiteralPath $Path) { throw "更新计划已存在，请指定新的 -PlanPath。" }
    $live = Get-RemoteUpdateLive $Directory
    $service = $live.service
    $installed = $service.imagePath
    if ([IO.Path]::GetFileName($installed) -ne "codex-elves-manager.exe") { throw "正式服务映像不是标准安装程序。" }
    $installDirectory = Split-Path -Parent $installed
    $launcher = Get-RemoteUpdateProcess $service.parentProcessId
    if ($launcher.imagePath -ne (Join-Path $installDirectory "codex-elves.exe")) { throw "正式服务并非由安装目录的 launcher 管理。" }
    if (-not $Port) { $Port = (Read-RemoteUpdateJson (Join-Path $Directory "latest-status.json")).helper_port }
    if ($Port -lt 1 -or $Port -gt 65535) { throw "无法识别本机管理端口，请指定 -HelperPort。" }
    $managers = foreach ($process in Get-CimInstance Win32_Process -Filter "Name LIKE 'codex-elves-manager%'") {
        if ($process.ProcessId -eq $service.processId) { continue }
        $identity = Get-RemoteUpdateProcess $process.ProcessId
        if ((Split-Path -Parent $identity.imagePath) -eq $installDirectory) { $identity }
    }
    $origin = "http://127.0.0.1:$($live.status.settings.port)"
    $before = Invoke-RemoteUpdateJson "$origin/app-resources.json"
    $stamp = Get-RemoteUpdateStateStamp $Directory
    $source = Get-RemoteUpdateSource $Repository
    if ($source.managementProtocolVersion -ne $live.managementProtocolVersion) {
        throw "管理协议已变化，原 launcher 无法管理目标服务；请更新安装侧程序，不能仅切换远程服务。"
    }
    $artifactDirectory = Split-Path -Parent $Path
    foreach ($protectedDirectory in @($installDirectory, $Directory)) {
        if ($artifactDirectory -eq $protectedDirectory -or
            $artifactDirectory.StartsWith($protectedDirectory.TrimEnd('\') + "\", [StringComparison]::OrdinalIgnoreCase)) {
            throw "更新记录不能写入安装或运行状态目录。"
        }
    }
    New-Item -ItemType Directory -Path $artifactDirectory -Force | Out-Null
    if (-not $ReuseBuild) {
        if ($Candidate) { throw "-ProgramPath 必须与 -SkipBuild 一起使用。" }
        Push-Location -LiteralPath $Repository
        try {
            # 用户要求优先更新正式远程服务：只编译 manager 服务宿主，不运行安装包或附带 Android 构建。
            $buildLines = [Collections.Generic.List[string]]::new()
            & cargo build --profile release-fast -p codex-elves-manager --bin codex-elves-manager --message-format=json-render-diagnostics 2>&1 |
                ForEach-Object {
                    $line = $_.ToString()
                    $buildLines.Add($line)
                    $message = try { $line | ConvertFrom-Json -ErrorAction Stop } catch { $null }
                    if ($message.reason -eq "compiler-artifact" -and $message.target.name -eq "codex-elves-manager" -and $message.executable) {
                        # 直接取 Cargo 的真实产物位置，兼容 CARGO_TARGET_DIR 与显式目标配置，避免误用旧目录中的 exe。
                        $Candidate = $message.executable
                    } elseif ($message.reason -eq "compiler-message") { Write-Host $message.message.rendered }
                    elseif (-not $message) { Write-Host $line }
                }
            [IO.File]::WriteAllLines((Join-Path $artifactDirectory "build.log"), $buildLines)
            if ($LASTEXITCODE -ne 0) { throw "远程服务程序构建失败。" }
            if (-not $Candidate) { throw "Cargo 未返回本次 manager 可执行文件。" }
        } finally { Pop-Location }
    } elseif (-not $Candidate) { throw "-SkipBuild 时必须指定 -ProgramPath。" }
    $sourceAfterBuild = Get-RemoteUpdateSource $Repository
    Assert-RemoteUpdateManifest $source $sourceAfterBuild
    if ($sourceAfterBuild.managementProtocolVersion -ne $source.managementProtocolVersion) { throw "准备期间管理协议已变化，请重新 Prepare。" }
    $binary = [IO.File]::ReadAllBytes([IO.Path]::GetFullPath($Candidate))
    foreach ($embeddedPath in $source.embeddedPaths) {
        if (-not [CodexElves.RemoteUpdateNative]::Contains($binary, [IO.File]::ReadAllBytes($embeddedPath))) {
            throw "候选程序内嵌资源不符：$([IO.Path]::GetFileName($embeddedPath))；请重新构建。"
        }
    }
    $binary = $null
    if ($source.revision -lt $before.revision) { throw "目标资源号低于正式版本，不能作为正常更新应用。" }
    if ($source.revision -eq $before.revision) {
        try { Assert-RemoteUpdateManifest $source $before } catch { throw "资源已变更但编号未递增，请更新 resource-revision.txt 后重新构建。" }
    }
    $preparedDirectory = Join-Path $artifactDirectory "prepared"
    $backupDirectory = Join-Path $artifactDirectory "backup"
    foreach ($folder in @($preparedDirectory, $backupDirectory)) {
        New-Item -ItemType Directory -Path $folder -Force | Out-Null
    }
    $prepared = Join-Path $preparedDirectory "codex-elves-manager.exe"
    $backup = Join-Path $backupDirectory "codex-elves-manager.exe"
    if ((Test-Path -LiteralPath $prepared) -or (Test-Path -LiteralPath $backup)) { throw "记录目录已有程序副本，请使用新的目录。" }
    Copy-Item -LiteralPath $Candidate -Destination $prepared
    Copy-Item -LiteralPath $installed -Destination $backup
    $changed = @("/app.js", "/style.css") + @($source.files | Where-Object {
        $file = $_
        -not @($before.files | Where-Object { $_.path -ceq $file.path -and $_.sha256 -eq $file.sha256 }).Count
    } | ForEach-Object path)
    $plan = [pscustomobject]@{
        schemaVersion = 1; id = [Guid]::NewGuid().ToString("N"); preparedAt = [DateTime]::UtcNow.ToString("o")
        stateDirectory = $Directory; installedProgram = $installed; preparedProgram = $prepared; backupProgram = $backup
        managementProtocolVersion = $source.managementProtocolVersion
        oldSha256 = Get-RemoteUpdateHash $backup; newSha256 = Get-RemoteUpdateHash $prepared
        serviceBefore = $service; launcher = $launcher; protectedManagers = @($managers); helperPort = $Port
        localOrigin = $origin; publicOrigin = ([string]$live.status.primary.publicUrl).TrimEnd("/")
        stateBefore = $stamp; resourcesBefore = $before
        targetResources = [pscustomobject]@{ revision = $source.revision; files = $source.files }
        verifyPaths = @($changed | Sort-Object -Unique); requiresServiceRestart = $true
    }
    $plan.requiresServiceRestart = $plan.oldSha256 -ne $plan.newSha256
    Assert-RemoteUpdatePlan $plan $Path
    Assert-RemoteUpdateProtected $plan
    Assert-RemoteUpdateProcess $service
    $null = Assert-RemoteUpdateState $stamp $Directory
    if ((Get-RemoteUpdateHash $installed) -ne $plan.oldSha256) { throw "准备期间正式程序已变化，请重新 Prepare。" }
    if ($plan.requiresServiceRestart) { Assert-RemoteUpdateStopAccess $service.processId }
    Write-RemoteUpdateJson $Path $plan
    return $plan
}

function Invoke-RemoteUpdateVerification($Plan, $Manifest, [int]$Seconds) {
    Assert-RemoteUpdateProtected $Plan
    $service = Wait-RemoteUpdateLocal $Plan $Manifest $Seconds
    $count = Assert-RemoteUpdateState $Plan.stateBefore $Plan.stateDirectory
    $publicVerified = $null
    $publicError = ""
    if ($Plan.publicOrigin) {
        $publicVerified = $false
        $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
        do {
            try {
                $paths = @($Plan.verifyPaths | Where-Object { $_ -cin $Manifest.files.path })
                Test-RemoteUpdateResources $Plan.publicOrigin $Manifest $paths
                $publicVerified = $true
                break
            } catch { $publicError = $_.Exception.Message }
            Start-Sleep -Milliseconds 500
        } while ([DateTime]::UtcNow -lt $deadline)
    }
    [pscustomobject]@{
        success = ($publicVerified -ne $false); localVerified = $true; publicVerified = $publicVerified
        publicError = $publicError; revision = $Manifest.revision; service = $service
        managerAndLauncherPreserved = $true; settingsAndKeyPreserved = $true; originalAuthorizationsPreserved = $true
        deviceCount = $count; verifiedAt = [DateTime]::UtcNow.ToString("o")
    }
}

function Invoke-RemoteServiceUpdate {
    [CmdletBinding()]
    param([string]$Action = "Prepare", [string]$PlanPath = "", [string]$ProgramPath = "",
        [string]$StateDirectory = "", [int]$HelperPort = 0, [switch]$SkipBuild, [int]$TimeoutSeconds = 60)
    $ErrorActionPreference = "Stop"
    if (-not $IsWindows) { throw "正式服务更新脚本仅支持 Windows PowerShell 7。" }
    Initialize-RemoteUpdateNative
    $repository = Split-Path -Parent $PSScriptRoot
    if ($Action -eq "Prepare" -and -not $PlanPath) {
        $PlanPath = Join-Path $repository ("temp/remote-service-updates/{0}-{1}/plan.json" -f
            [DateTime]::Now.ToString("yyyyMMdd-HHmmss"), [Guid]::NewGuid().ToString("N").Substring(0, 8))
    }
    if (-not $PlanPath) { throw "$Action 必须指定 Prepare 生成的 -PlanPath。" }
    $PlanPath = [IO.Path]::GetFullPath($PlanPath)
    $artifactDirectory = Split-Path -Parent $PlanPath
    if ($Action -eq "Prepare") {
        if (-not $StateDirectory) { $StateDirectory = Join-Path ([Environment]::GetFolderPath("UserProfile")) ".codex-session-delete" }
        $plan = New-RemoteUpdatePlan $PlanPath $repository ([IO.Path]::GetFullPath($StateDirectory)) $HelperPort $ProgramPath -ReuseBuild:$SkipBuild
        Write-Output "准备完成，尚未切换正式服务。目标资源：$($plan.targetResources.revision)"
        Write-Output "更新计划：$PlanPath"
        Write-Output "需要切换服务：$($plan.requiresServiceRestart)"
        return
    }
    $plan = Read-RemoteUpdateJson $PlanPath
    Assert-RemoteUpdatePlan $plan $PlanPath
    $manifest = $plan.targetResources
    $expectedHash = $plan.newSha256
    $installedHash = if (Test-Path -LiteralPath $plan.installedProgram) { Get-RemoteUpdateHash $plan.installedProgram } else { "" }
    if ($Action -eq "Rollback" -or ($Action -eq "Verify" -and $installedHash -eq $plan.oldSha256 -and $installedHash -ne $plan.newSha256)) {
        $manifest = $plan.resourcesBefore
        $expectedHash = $plan.oldSha256
    }
    if ($Action -eq "Verify") {
        if ($installedHash -ne $expectedHash) { throw "正式程序摘要与预期版本不一致。" }
        $verification = Invoke-RemoteUpdateVerification $plan $manifest $TimeoutSeconds
        Write-RemoteUpdateJson (Join-Path $artifactDirectory "verification.json") $verification
        if (-not $verification.success) { throw "正式本地服务通过，但公网核验失败；未重启已健康的本地服务。可重试 Verify。" }
        Write-Output "核验通过：资源 $($manifest.revision)，服务 PID $($verification.service.processId)，授权 $($verification.deviceCount) 条。"
        return
    }
    # 同一安装目录只允许一个更新事务；不使用服务的锁文件，也不删除原服务端点。
    $lockPath = Join-Path (Split-Path -Parent $plan.installedProgram) ".remote-service-update.lock"
    $updateLock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        Assert-RemoteUpdateProtected $plan
        $null = Assert-RemoteUpdateState $plan.stateBefore $plan.stateDirectory
        $currentHash = if (Test-Path -LiteralPath $plan.installedProgram) { Get-RemoteUpdateHash $plan.installedProgram } else { "" }
        if ($Action -eq "Rollback") {
            $journalPath = Join-Path $artifactDirectory "transaction.json"
            $journal = if (Test-Path -LiteralPath $journalPath) { Read-RemoteUpdateJson $journalPath } else { $null }
            if ($currentHash -notin @($plan.oldSha256, $plan.newSha256) -and
                -not (-not $currentHash -and $journal.planId -eq $plan.id -and $journal.action -eq "Apply")) {
                throw "正式程序与本计划不匹配，拒绝回滚其他更新。"
            }
            $resultPath = Join-Path $artifactDirectory "rollback-result.json"
            try {
                $null = Restore-RemoteUpdatePrevious $plan $plan.serviceBefore $plan.backupProgram $plan.oldSha256 $plan.resourcesBefore $TimeoutSeconds
                $result = Invoke-RemoteUpdateVerification $plan $plan.resourcesBefore $TimeoutSeconds
                Write-RemoteUpdateJson $resultPath $result
            } catch {
                Write-RemoteUpdateJson $resultPath ([pscustomobject]@{ success = $false; failure = $_.Exception.Message })
                throw "回滚未完成；未覆盖设置或授权。详情：$resultPath"
            }
            if (-not $result.success) { throw "本地已回滚，公网尚未通过核验；可重试 Verify。详情：$resultPath" }
            Write-Output "回滚完成：资源 $($result.revision)，结果记录：$resultPath"
            return
        }
        $live = Get-RemoteUpdateLive $plan.stateDirectory
        if ($live.service.parentProcessId -ne $plan.launcher.processId -or $live.service.imagePath -ne $plan.installedProgram) {
            throw "正式服务归属已变化，请重新 Prepare。"
        }
        $program = $plan.preparedProgram
        $previousProgram = $plan.backupProgram
        $previousHash = $plan.oldSha256
        $previousManifest = $plan.resourcesBefore
        if ($currentHash -ne $expectedHash) {
            Assert-RemoteUpdateProcess $plan.serviceBefore
        }
        if ($currentHash -ne $expectedHash -and $currentHash -ne $previousHash) { throw "正式程序已经变化，拒绝应用过期计划。" }
        $resultPath = Join-Path $artifactDirectory "result.json"
        if ($currentHash -ne $expectedHash) {
            Assert-RemoteUpdateStopAccess $live.service.processId
            $journalPath = Join-Path $artifactDirectory "transaction.json"
            $journal = [pscustomobject]@{
                planId = $plan.id; action = $Action; phase = "before-switch"; serviceBefore = $live.service
                desiredSha256 = $expectedHash; previousSha256 = $previousHash
                retiredProgram = Join-Path (Split-Path -Parent $plan.installedProgram) "codex-elves-manager.remote-before-$($plan.id)-apply.exe"
                startedAt = [DateTime]::UtcNow.ToString("o")
            }
            Write-RemoteUpdateJson $journalPath $journal
            try {
                $journal.retiredProgram = Switch-RemoteUpdateProgram $plan $program $expectedHash ($Action.ToLowerInvariant())
                $journal.phase = "program-replaced"
                Write-RemoteUpdateJson $journalPath $journal
                Stop-RemoteUpdateService $plan $live.service $journal.retiredProgram
                $journal.phase = "service-stopped"
                Write-RemoteUpdateJson $journalPath $journal
                Request-RemoteUpdateStart $plan
                $null = Wait-RemoteUpdateLocal $plan $manifest $TimeoutSeconds
                Assert-RemoteUpdateProtected $plan
                $null = Assert-RemoteUpdateState $plan.stateBefore $plan.stateDirectory
            } catch {
                $failure = $_.Exception.Message
                $recovered = $false
                $recoveryError = ""
                try {
                    $null = Restore-RemoteUpdatePrevious $plan $live.service $previousProgram $previousHash $previousManifest $TimeoutSeconds
                    $recovered = $true
                } catch { $recoveryError = $_.Exception.Message }
                Write-RemoteUpdateJson $resultPath ([pscustomobject]@{
                    success = $false; failure = $failure; previousVersionRestored = $recovered; recoveryError = $recoveryError
                })
                $journal.phase = $(if ($recovered) { "previous-version-restored" } else { "recovery-required" })
                Write-RemoteUpdateJson $journalPath $journal
                throw "更新未通过本地核验。上一版本已恢复：$recovered。详情：$resultPath"
            }
            $journal.phase = "local-verified"
            Write-RemoteUpdateJson $journalPath $journal
        }
        # 公网故障不触发程序回滚：先保留已核验的本地服务，单独记录公网结果，之后 Verify 可重试。
        $result = Invoke-RemoteUpdateVerification $plan $manifest $TimeoutSeconds
        Write-RemoteUpdateJson $resultPath $result
        if (-not $result.success) { throw "本地更新已通过，公网核验未通过。保留当前服务，请重试 Verify；详情：$resultPath" }
        Write-Output "$Action 完成：资源 $($manifest.revision)，服务 PID $($result.service.processId)，授权 $($result.deviceCount) 条。"
        Write-Output "结果记录：$resultPath"
    } finally { $updateLock.Dispose() }
}

if ($MyInvocation.InvocationName -ne ".") {
    Invoke-RemoteServiceUpdate @PSBoundParameters
}
