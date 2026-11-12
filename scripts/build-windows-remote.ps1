param(
    [string]$Version = "",
    [ValidateSet("release-fast", "release")]
    [string]$Profile = "release-fast",
    [switch]$SkipBuild,
    [string]$BinaryDirectory = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$workspace = Get-Content -LiteralPath (Join-Path $RepoRoot "Cargo.toml") -Raw
$versionMatch = [regex]::Match($workspace, '(?m)^\s*version\s*=\s*"([^"]+)"')
if (!$versionMatch.Success) { throw "Unable to resolve workspace version." }
$WorkspaceVersion = $versionMatch.Groups[1].Value
$ResolvedVersion = if ($Version.Trim()) { $Version.Trim() } else { $WorkspaceVersion }
if ($ResolvedVersion -notmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$') {
    throw "Invalid artifact version: $ResolvedVersion"
}
if ($BinaryDirectory -and !$SkipBuild) {
    throw "-BinaryDirectory is only supported with -SkipBuild."
}

# 用户要求远程会话新增独立 Windows EXE；此入口只构建客户端，不启动管理器、不构建 Android。
if (!$SkipBuild) {
    $cargo = Get-Command "cargo.exe" -CommandType Application -ErrorAction Stop | Select-Object -First 1
    Push-Location $RepoRoot
    try {
        & $cargo.Path build --locked --profile $Profile -p codex-elves-remote --bin codex-elves-remote
        if ($LASTEXITCODE -ne 0) { throw "Remote client build failed: $LASTEXITCODE" }
    }
    finally { Pop-Location }
}

if ($BinaryDirectory) {
    $BuildDirectory = [System.IO.Path]::GetFullPath(
        $(if ([System.IO.Path]::IsPathRooted($BinaryDirectory)) { $BinaryDirectory } else { Join-Path $RepoRoot $BinaryDirectory })
    )
}
else {
    $targetRoot = if ($env:CARGO_TARGET_DIR) {
        [System.IO.Path]::GetFullPath(
            $(if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot $env:CARGO_TARGET_DIR })
        )
    } else { Join-Path $RepoRoot "target" }
    if ($env:CARGO_BUILD_TARGET) { $targetRoot = Join-Path $targetRoot $env:CARGO_BUILD_TARGET }
    $BuildDirectory = Join-Path $targetRoot $Profile
}
$Executable = Join-Path $BuildDirectory "codex-elves-remote.exe"
if (!(Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "Remote client executable was not created: $Executable"
}

# 校验实际 PE 机器类型与 GUI 子系统，不能把其他目标产物命名为 Windows x64 或带控制台的客户端。
$image = [System.IO.File]::ReadAllBytes($Executable)
if ($image.Length -lt 256 -or $image[0] -ne 0x4d -or $image[1] -ne 0x5a) { throw "Invalid PE executable." }
$peOffset = [BitConverter]::ToInt32($image, 0x3c)
if ($peOffset -lt 0 -or $peOffset + 96 -ge $image.Length) { throw "Invalid PE header offset." }
if ([BitConverter]::ToUInt32($image, $peOffset) -ne 0x4550 -or
    [BitConverter]::ToUInt16($image, $peOffset + 4) -ne 0x8664 -or
    [BitConverter]::ToUInt16($image, $peOffset + 24 + 68) -ne 2) {
    throw "Remote client must be a Windows x64 GUI executable."
}
$DistDirectory = Join-Path $RepoRoot "dist\windows"
New-Item -ItemType Directory -Path $DistDirectory -Force | Out-Null
$Destination = Join-Path $DistDirectory "CodexElves-Remote-$ResolvedVersion-windows-x64.exe"
$sourceHasher = [Security.Cryptography.SHA256]::Create()
try { $sourceDigest = [BitConverter]::ToString($sourceHasher.ComputeHash($image)).Replace("-", "").ToLowerInvariant() }
finally { $sourceHasher.Dispose() }
# 用户可能正在使用便携客户端；相同字节无需覆盖，避免常规安装包编译因运行中的同版 EXE 被锁定而失败。
if (!(Test-Path -LiteralPath $Destination -PathType Leaf) -or
    (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sourceDigest) {
    # 编译时保留正在使用的客户端：先写入完整新文件，再轮换旧产物，不停止用户窗口。
    $artifactSuffix = [Guid]::NewGuid().ToString("N")
    $temporaryArtifact = Join-Path $DistDirectory "remote-$artifactSuffix.tmp"
    $previousArtifact = "$Destination.previous-$artifactSuffix"
    [IO.File]::WriteAllBytes($temporaryArtifact, $image)
    try {
        if (Test-Path -LiteralPath $Destination -PathType Leaf) {
            Move-Item -LiteralPath $Destination -Destination $previousArtifact -ErrorAction Stop
        }
        try { Move-Item -LiteralPath $temporaryArtifact -Destination $Destination -ErrorAction Stop }
        catch {
            if (!(Test-Path -LiteralPath $Destination) -and (Test-Path -LiteralPath $previousArtifact)) {
                Move-Item -LiteralPath $previousArtifact -Destination $Destination -ErrorAction Stop
            }
            throw
        }
    }
    finally { Remove-Item -LiteralPath $temporaryArtifact -Force -ErrorAction SilentlyContinue }
}
$digest = (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant()
if ($digest -ne $sourceDigest) { throw "Staged remote client does not match the validated executable." }
$info = [ordered]@{
    product = "CodexElves Remote"
    version = $WorkspaceVersion
    artifactVersion = $ResolvedVersion
    platform = "windows-x64"
    profile = $Profile
    executable = [System.IO.Path]::GetFileName($Destination)
    bytes = (Get-Item -LiteralPath $Destination).Length
    sha256 = $digest
    builtAt = [DateTimeOffset]::Now.ToString("o")
}
$InfoPath = Join-Path $DistDirectory "CodexElves-Remote-$ResolvedVersion-windows-x64-build-info.json"
$text = ($info | ConvertTo-Json -Depth 4) + "`n"
[System.IO.File]::WriteAllText($InfoPath, $text, [System.Text.UTF8Encoding]::new($false))
Write-Output "[OK] Remote client: $Destination"
Write-Output "[OK] SHA256: $digest"
Write-Output "[OK] Build info: $InfoPath"
