param(
    [string]$Version = "",
    [string]$MakensisPath = "",
    [string]$AndroidJavaHome = "",
    [string]$AndroidSdkHome = "",
    [string]$AndroidGradlePath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSCommandPath
$BuildScript = Join-Path $RepoRoot "scripts\build-windows-installer.ps1"

& $BuildScript -Version $Version -MakensisPath $MakensisPath
$windowsExitCode = $LASTEXITCODE
if ($windowsExitCode -ne 0) {
    exit $windowsExitCode
}

& (Join-Path $RepoRoot "scripts\sync-android-build.ps1") `
    -JavaHome $AndroidJavaHome -AndroidHome $AndroidSdkHome -GradlePath $AndroidGradlePath
exit 0
