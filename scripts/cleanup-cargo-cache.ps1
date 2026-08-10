param(
    [string]$RepoRoot = "",
    [double]$MaxSizeGiB = 20,
    [string]$CargoPath = "",
    [switch]$Background
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    if ($RepoRoot.Trim()) {
        if (!(Test-Path -LiteralPath $RepoRoot)) {
            throw "Repository root does not exist: $RepoRoot"
        }
        return (Resolve-Path -LiteralPath $RepoRoot).Path
    }

    $scriptDir = Split-Path -Parent $PSCommandPath
    return (Resolve-Path -LiteralPath (Join-Path $scriptDir "..")).Path
}

function Resolve-CargoPath {
    if ($CargoPath.Trim()) {
        if (!(Test-Path -LiteralPath $CargoPath)) {
            throw "Configured cargo path does not exist: $CargoPath"
        }
        return (Resolve-Path -LiteralPath $CargoPath).Path
    }

    $command = Get-Command "cargo.exe" -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $command) {
        throw "Unable to find cargo.exe"
    }
    return $command.Path
}

function Write-CleanupStatus {
    param(
        [string]$TargetRoot,
        [string]$Message
    )

    New-Item -ItemType Directory -Path $TargetRoot -Force | Out-Null
    $timestamp = Get-Date -Format "yyyy-MM-ddTHH:mm:ssK"
    $logPath = Join-Path $TargetRoot "cargo-cache-cleanup.log"
    Set-Content -LiteralPath $logPath -Value "[$timestamp] $Message" -Encoding UTF8
}

function Start-BackgroundCleanup {
    param(
        [string]$ResolvedRepoRoot
    )

    $powerShellPath = (Get-Command "powershell.exe" -CommandType Application |
            Select-Object -First 1).Path
    $arguments = @(
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ('"{0}"' -f $PSCommandPath),
        "-RepoRoot",
        ('"{0}"' -f $ResolvedRepoRoot),
        "-MaxSizeGiB",
        $MaxSizeGiB.ToString([System.Globalization.CultureInfo]::InvariantCulture)
    )
    if ($CargoPath.Trim()) {
        $resolvedCargoPath = Resolve-CargoPath
        $arguments += @("-CargoPath", ('"{0}"' -f $resolvedCargoPath))
    }

    $process = Start-Process `
        -FilePath $powerShellPath `
        -ArgumentList $arguments `
        -WorkingDirectory $ResolvedRepoRoot `
        -WindowStyle Hidden `
        -PassThru
    if ($process.HasExited -and $process.ExitCode -ne 0) {
        throw "Cargo cache cleanup process exited immediately with code $($process.ExitCode)"
    }

    Write-Output "[INFO] Cargo cache cleanup scheduled in background"
}

if ($MaxSizeGiB -le 0) {
    throw "MaxSizeGiB must be greater than zero"
}

$ResolvedRepoRoot = Resolve-RepoRoot
if ($Background) {
    Start-BackgroundCleanup -ResolvedRepoRoot $ResolvedRepoRoot
    return
}

$TargetRoot = Join-Path $ResolvedRepoRoot "target"
$DebugTarget = Join-Path $TargetRoot "debug"
if (!(Test-Path -LiteralPath $DebugTarget)) {
    Write-Output "[SKIP] Cargo debug target does not exist"
    return
}

New-Item -ItemType Directory -Path $TargetRoot -Force | Out-Null
$LockPath = Join-Path $TargetRoot ".cargo-cache-cleanup.lock"
$lockStream = $null
try {
    try {
        $lockStream = [System.IO.File]::Open(
            $LockPath,
            [System.IO.FileMode]::OpenOrCreate,
            [System.IO.FileAccess]::ReadWrite,
            [System.IO.FileShare]::None
        )
    }
    catch [System.IO.IOException] {
        Write-Output "[SKIP] Cargo cache cleanup is already running"
        return
    }

    $measure = Get-ChildItem -LiteralPath $DebugTarget -Force -Recurse -File -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum
    $sizeBytes = [double]$measure.Sum
    $limitBytes = $MaxSizeGiB * 1GB
    $sizeGiB = [math]::Round($sizeBytes / 1GB, 2)

    if ($sizeBytes -le $limitBytes) {
        Write-CleanupStatus -TargetRoot $TargetRoot -Message "skipped: debug cache ${sizeGiB} GiB <= limit ${MaxSizeGiB} GiB"
        Write-Output "[SKIP] Cargo debug cache is ${sizeGiB} GiB; limit is ${MaxSizeGiB} GiB"
        return
    }

    $resolvedCargoPath = Resolve-CargoPath
    $cargoArguments = @("clean", "--profile", "dev", "--target-dir", $TargetRoot)
    Push-Location $ResolvedRepoRoot
    try {
        & $resolvedCargoPath @cargoArguments
        $cargoExitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }
    if ($cargoExitCode -ne 0) {
        throw "cargo clean failed with exit code $cargoExitCode"
    }

    Write-CleanupStatus -TargetRoot $TargetRoot -Message "cleaned: debug cache was ${sizeGiB} GiB; limit ${MaxSizeGiB} GiB"
    Write-Output "[OK] Cleaned Cargo debug cache (${sizeGiB} GiB)"
}
catch {
    $originalError = $_
    try {
        Write-CleanupStatus -TargetRoot $TargetRoot -Message "failed: $($originalError.Exception.Message)"
    }
    catch {
        Write-Warning "Unable to write Cargo cache cleanup status: $($_.Exception.Message)"
    }
    throw $originalError
}
finally {
    if ($null -ne $lockStream) {
        $lockStream.Dispose()
    }
}
