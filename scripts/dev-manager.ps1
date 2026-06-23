param(
    [switch]$NoElevate,
    [int]$GuardPort = 45229
)

$ErrorActionPreference = "Stop"

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

$scriptPath = $PSCommandPath
if (-not $NoElevate -and -not (Test-Administrator)) {
    $pwsh = (Get-Command pwsh.exe).Path
    $arguments = @(
        "-NoExit",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $scriptPath,
        "-NoElevate",
        "-GuardPort",
        $GuardPort.ToString()
    )
    Start-Process -FilePath $pwsh -ArgumentList $arguments -Verb RunAs -WindowStyle Normal
    exit 0
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$managerDir = Join-Path $repoRoot "apps\codex-elves-manager"

$env:CODEX_ELVES_MANAGER_DEV = "1"
$env:CODEX_ELVES_MANAGER_GUARD_PORT = $GuardPort.ToString()

Set-Location -LiteralPath $managerDir
npm run dev
