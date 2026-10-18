[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$JavaHome = "",
    [string]$AndroidHome = "",
    [string]$GradlePath = "",
    [switch]$CheckOnly
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "android-build-common.ps1")
$RepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$decision = Get-AndroidBuildDecision -RepositoryRoot $RepositoryRoot
if ($CheckOnly) {
    $decision
    return
}
if (-not $decision.NeedsBuild) {
    Write-Host "跳过 Android：$($decision.Reason)。"
    return
}

Write-Host "同步构建 Android Release：$($decision.Reason)。"
$buildParameters = @{ Configuration = "Release" }
if ($JavaHome) { $buildParameters.JavaHome = $JavaHome }
if ($AndroidHome) { $buildParameters.AndroidHome = $AndroidHome }
if ($GradlePath) { $buildParameters.GradlePath = $GradlePath }
& (Join-Path $RepositoryRoot "scripts/build-android.ps1") @buildParameters
$result = Get-AndroidBuildDecision -RepositoryRoot $RepositoryRoot
if ($result.NeedsBuild) {
    throw "Android 构建后校验未通过：$($result.Reason)"
}
