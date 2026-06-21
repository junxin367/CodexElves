param(
    [string]$Version = "",
    [string]$MakensisPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSCommandPath
$BuildScript = Join-Path $RepoRoot "scripts\build-windows-installer.ps1"

& $BuildScript -Version $Version -MakensisPath $MakensisPath
exit $LASTEXITCODE
