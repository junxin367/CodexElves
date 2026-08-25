param(
    [string]$Version = "",
    [string]$MakensisPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    $scriptDir = Split-Path -Parent $PSCommandPath
    return (Resolve-Path -LiteralPath (Join-Path $scriptDir "..")).Path
}

function Resolve-WorkspaceVersion {
    param([string]$RepoRoot)

    if ($Version.Trim()) {
        return $Version.Trim()
    }

    $cargoToml = Join-Path $RepoRoot "Cargo.toml"
    $content = Get-Content -LiteralPath $cargoToml -Raw
    $match = [regex]::Match($content, '(?m)^\s*version\s*=\s*"([^"]+)"')
    if (!$match.Success) {
        throw "Failed to read workspace version from Cargo.toml"
    }
    return $match.Groups[1].Value
}

function Resolve-ToolPath {
    param(
        [string]$Name,
        [string]$ExplicitPath,
        [string[]]$FallbackPaths
    )

    if ($ExplicitPath.Trim()) {
        if (!(Test-Path -LiteralPath $ExplicitPath)) {
            throw "Configured $Name path does not exist: $ExplicitPath"
        }
        return (Resolve-Path -LiteralPath $ExplicitPath).Path
    }

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Path
    }

    foreach ($fallback in $FallbackPaths) {
        if (Test-Path -LiteralPath $fallback) {
            return (Resolve-Path -LiteralPath $fallback).Path
        }
    }

    throw "Unable to find $Name. Install it or pass -MakensisPath for NSIS."
}

function Invoke-Checked {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory
    )

    Push-Location $WorkingDirectory
    try {
        & $FilePath @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
        }
    }
    finally {
        Pop-Location
    }
}

function Start-CargoCacheCleanup {
    param(
        [string]$RepoRoot,
        [double]$MaxSizeGiB
    )

    if ($env:CODEX_ELVES_SKIP_CARGO_CACHE_CLEANUP -eq "1") {
        Write-Output "[INFO] Cargo cache cleanup skipped by environment"
        return
    }

    $cleanupScript = Join-Path $RepoRoot "scripts\cleanup-cargo-cache.ps1"
    if (!(Test-Path -LiteralPath $cleanupScript)) {
        Write-Warning "Cargo cache cleanup script not found: $cleanupScript"
        return
    }

    try {
        & $cleanupScript -RepoRoot $RepoRoot -MaxSizeGiB $MaxSizeGiB -Background
    }
    catch {
        Write-Warning "Unable to schedule Cargo cache cleanup: $($_.Exception.Message)"
    }
}

$RepoRoot = Resolve-RepoRoot
$ResolvedVersion = Resolve-WorkspaceVersion -RepoRoot $RepoRoot
$ManagerDir = Join-Path $RepoRoot "apps\codex-elves-manager"
$DistAppDir = Join-Path $RepoRoot "dist\windows\app"
$InstallerScriptDir = Join-Path $RepoRoot "scripts\installer\windows"
$InstallerScript = Join-Path $InstallerScriptDir "CodexElves.nsi"
$InstallerPath = Join-Path $RepoRoot "dist\windows\CodexElves-$ResolvedVersion-windows-x64-setup.exe"
if ($env:CARGO_TARGET_DIR) {
    if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
        $CargoTargetDir = [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
    }
    else {
        $CargoTargetDir = [System.IO.Path]::GetFullPath(
            (Join-Path $ManagerDir $env:CARGO_TARGET_DIR)
        )
    }
}
else {
    $CargoTargetDir = Join-Path $RepoRoot "target"
}
$ReleaseDir = Join-Path $CargoTargetDir "release"

$NpmPath = Resolve-ToolPath -Name "npm.cmd" -ExplicitPath "" -FallbackPaths @()
$NsisPath = Resolve-ToolPath `
    -Name "makensis" `
    -ExplicitPath $MakensisPath `
    -FallbackPaths @(
        "C:\Program Files\NSIS\makensis.exe",
        "C:\Program Files\NSIS\Bin\makensis.exe",
        "C:\Program Files (x86)\NSIS\makensis.exe",
        "C:\Program Files (x86)\NSIS\Bin\makensis.exe"
    )

Write-Output "[INFO] Repo: $RepoRoot"
Write-Output "[INFO] Version: $ResolvedVersion"
Write-Output "[INFO] npm: $NpmPath"
Write-Output "[INFO] makensis: $NsisPath"
Write-Output "[INFO] Cargo target: $CargoTargetDir"

Invoke-Checked -FilePath $NpmPath -Arguments @("run", "build") -WorkingDirectory $ManagerDir

New-Item -ItemType Directory -Path $DistAppDir -Force | Out-Null
$LauncherExe = Join-Path $ReleaseDir "codex-elves.exe"
$ManagerExe = Join-Path $ReleaseDir "codex-elves-manager.exe"
$TaskBoardExe = Join-Path $ReleaseDir "codex-elves-task-board.exe"

foreach ($artifact in @($LauncherExe, $ManagerExe, $TaskBoardExe)) {
    if (!(Test-Path -LiteralPath $artifact)) {
        throw "Expected release artifact not found: $artifact"
    }
    Copy-Item -LiteralPath $artifact -Destination $DistAppDir -Force
}

Invoke-Checked `
    -FilePath $NsisPath `
    -Arguments @("/INPUTCHARSET", "UTF8", "/DVERSION=$ResolvedVersion", $InstallerScript) `
    -WorkingDirectory $InstallerScriptDir

if (!(Test-Path -LiteralPath $InstallerPath)) {
    throw "Installer was not created: $InstallerPath"
}

$InstallerItem = Get-Item -LiteralPath $InstallerPath
$Length = $InstallerItem.Length
$LastWriteTime = $InstallerItem.LastWriteTime
Write-Output "[OK] Installer: $InstallerPath"
Write-Output "[OK] Size: $Length bytes"
Write-Output "[OK] Updated: $LastWriteTime"

Start-CargoCacheCleanup -RepoRoot $RepoRoot -MaxSizeGiB 20
