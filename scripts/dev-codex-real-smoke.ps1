param(
    [int]$Port = 51555,
    [int[]]$ReservedProxyPorts = @(45221),
    [string]$RunRoot = "",
    [string]$SourceCodexHome = "",
    [string]$SourceSettingsPath = "",
    [string]$ProviderId = "custom",
    [string]$Model = "deepseek-v4-pro",
    [string]$ClaudeModel = "claude-sonnet-4-6",
    [string]$GptModel = "gpt-5.5",
    [switch]$IncludeClaude,
    [switch]$IncludeGptControl,
    [string]$ScenariosPath = "",
    [string]$ExtraScenarioJson = "",
    [string[]]$Scenario = @(),
    [switch]$ListScenarios,
    [switch]$SkipBuild,
    [switch]$KeepHelper
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($RunRoot)) {
    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $RunRoot = Join-Path $repoRoot "temp\dev-codex-smoke-run\$timestamp"
}

function Format-PortList {
    param([int[]]$Ports)
    if (($null -eq $Ports) -or ($Ports.Count -eq 0)) {
        return "(none)"
    }
    return (($Ports | ForEach-Object { $_.ToString() }) -join ", ")
}

function Test-PortReserved {
    param(
        [int]$CandidatePort,
        [int[]]$ReservedPorts
    )
    if (($null -eq $ReservedPorts) -or ($ReservedPorts.Count -eq 0)) {
        return $false
    }
    foreach ($reservedPort in $ReservedPorts) {
        if ($CandidatePort -eq $reservedPort) {
            return $true
        }
    }
    return $false
}

function Assert-DevPortIsIsolated {
    param(
        [int]$CandidatePort,
        [int[]]$ReservedPorts
    )
    if (Test-PortReserved $CandidatePort $ReservedPorts) {
        $reservedText = Format-PortList $ReservedPorts
        throw "Port $CandidatePort is reserved for installed local proxy. Reserved ports: $reservedText. Use -Port for the dev helper or update -ReservedProxyPorts."
    }
}

function Assert-NoReservedProxyPortReference {
    param(
        [string]$Path,
        [int[]]$ReservedPorts
    )
    if (($null -eq $ReservedPorts) -or ($ReservedPorts.Count -eq 0)) {
        return
    }
    foreach ($reservedPort in $ReservedPorts) {
        $pattern = "(127\.0\.0\.1|localhost|\[::1\]):$reservedPort"
        if (Select-String -LiteralPath $Path -Pattern $pattern -Quiet) {
            throw "Isolated CODEX_HOME config still references reserved local proxy port $reservedPort`: $Path. Update -ReservedProxyPorts if the installed proxy moved, or remove stale provider entries from the copied config."
        }
    }
}

function Assert-PathIgnoredByGit {
    param(
        [string]$RepoRoot,
        [string]$Path
    )
    $repoFullPath = [System.IO.Path]::GetFullPath($RepoRoot).TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    $targetFullPath = [System.IO.Path]::GetFullPath($Path)
    $repoPrefix = $repoFullPath + [System.IO.Path]::DirectorySeparatorChar
    if ($targetFullPath.Equals($repoFullPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        $relativePath = "."
    }
    elseif ($targetFullPath.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        $relativePath = $targetFullPath.Substring($repoPrefix.Length)
    }
    else {
        return
    }

    $relativePath = $relativePath.Replace([System.IO.Path]::DirectorySeparatorChar, '/')
    git -C $repoFullPath check-ignore -q -- $relativePath
    if ($LASTEXITCODE -ne 0) {
        throw "RunRoot is inside the repository but is not ignored by git: $targetFullPath. Add it to .gitignore before copying real credentials."
    }
}

function Assert-FileContainsPatterns {
    param(
        [string]$Path,
        [string[]]$Patterns,
        [string]$Label
    )
    if (($null -eq $Patterns) -or ($Patterns.Count -eq 0)) {
        return
    }
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "$Label file not found for expectation check: $Path"
    }
    foreach ($pattern in $Patterns) {
        if ([string]::IsNullOrWhiteSpace($pattern)) {
            continue
        }
        if (-not (Select-String -LiteralPath $Path -Pattern $pattern -Quiet)) {
            throw "$Label expectation not found in $Path`: $pattern"
        }
    }
}

Assert-DevPortIsIsolated $Port $ReservedProxyPorts

function Resolve-DefaultCodexHome {
    if (-not [string]::IsNullOrWhiteSpace($SourceCodexHome)) {
        return $SourceCodexHome
    }
    if (-not [string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        return $env:CODEX_HOME
    }
    return Join-Path $env:USERPROFILE ".codex"
}

function Resolve-DefaultSettingsPath {
    if (-not [string]::IsNullOrWhiteSpace($SourceSettingsPath)) {
        return $SourceSettingsPath
    }
    if (-not [string]::IsNullOrWhiteSpace($env:APPDATA)) {
        $candidate = Join-Path $env:APPDATA "CodexElves\settings.json"
        if (Test-Path -LiteralPath $candidate) {
            return $candidate
        }
    }
    return Join-Path $env:USERPROFILE "AppData\Roaming\CodexElves\settings.json"
}

function Copy-PathIfExists {
    param(
        [string]$Source,
        [string]$Destination
    )
    if (Test-Path -LiteralPath $Source) {
        $parent = Split-Path -Parent $Destination
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
        Copy-Item -LiteralPath $Source -Destination $Destination -Recurse -Force
    }
}

function Set-TomlRootString {
    param(
        [string]$Path,
        [string]$Key,
        [string]$Value
    )
    $lines = [System.Collections.Generic.List[string]]::new()
    if (Test-Path -LiteralPath $Path) {
        foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
            $lines.Add($line)
        }
    }

    $pattern = "^\s*$([regex]::Escape($Key))\s*="
    $replaced = $false
    for ($index = 0; $index -lt $lines.Count; $index++) {
        $trimmed = $lines[$index].TrimStart()
        if ($trimmed.StartsWith("[")) {
            break
        }
        if ($lines[$index] -match $pattern) {
            $lines[$index] = "$Key = `"$Value`""
            $replaced = $true
            break
        }
    }
    if (-not $replaced) {
        $lines.Insert(0, "$Key = `"$Value`"")
    }
    [System.IO.File]::WriteAllLines($Path, $lines)
}

function Set-TomlTableString {
    param(
        [string]$Path,
        [string]$Table,
        [string]$Key,
        [string]$Value
    )
    $lines = [System.Collections.Generic.List[string]]::new()
    if (Test-Path -LiteralPath $Path) {
        foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
            $lines.Add($line)
        }
    }

    $tableHeader = "[$Table]"
    $keyPattern = "^\s*$([regex]::Escape($Key))\s*="
    $tableIndex = -1
    for ($index = 0; $index -lt $lines.Count; $index++) {
        if ($lines[$index].Trim() -eq $tableHeader) {
            $tableIndex = $index
            break
        }
    }

    if ($tableIndex -lt 0) {
        if ($lines.Count -gt 0 -and -not [string]::IsNullOrWhiteSpace($lines[$lines.Count - 1])) {
            $lines.Add("")
        }
        $lines.Add($tableHeader)
        $lines.Add("$Key = `"$Value`"")
        [System.IO.File]::WriteAllLines($Path, $lines)
        return
    }

    $insertIndex = $lines.Count
    for ($index = $tableIndex + 1; $index -lt $lines.Count; $index++) {
        if ($lines[$index].TrimStart().StartsWith("[")) {
            $insertIndex = $index
            break
        }
        if ($lines[$index] -match $keyPattern) {
            $lines[$index] = "$Key = `"$Value`""
            [System.IO.File]::WriteAllLines($Path, $lines)
            return
        }
    }

    $lines.Insert($insertIndex, "$Key = `"$Value`"")
    [System.IO.File]::WriteAllLines($Path, $lines)
}

function Write-HelperProject {
    param(
        [string]$HelperDir,
        [string]$RepoRoot
    )

    $srcDir = Join-Path $HelperDir "src"
    New-Item -ItemType Directory -Force -Path $srcDir | Out-Null
    $corePath = (Resolve-Path -LiteralPath (Join-Path $RepoRoot "crates\codex-elves-core")).Path
    $corePath = $corePath.Replace("\", "/")
    @"
[package]
name = "codex-elves-dev-helper-smoke"
version = "0.1.0"
edition = "2024"

[workspace]

[dependencies]
anyhow = "1"
codex-elves-core = { path = "$corePath" }
tokio = { version = "1", features = ["full"] }
"@ | Set-Content -LiteralPath (Join-Path $HelperDir "Cargo.toml") -Encoding UTF8

    @'
use std::path::PathBuf;

use codex_elves_core::launcher::LaunchHooks;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let port = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing port"))?
        .parse::<u16>()?;
    let settings_path = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("missing settings path"))?,
    );
    let diagnostic_log_path = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("missing diagnostic log path"))?,
    );

    codex_elves_core::paths::set_settings_path_for_tests(Some(settings_path));
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(diagnostic_log_path));

    let hooks = codex_elves_core::launcher::DefaultLaunchHooks::default();
    hooks.start_helper(port).await?;
    println!("dev helper listening http://127.0.0.1:{port}/v1");
    std::future::pending::<()>().await;
    Ok(())
}
'@ | Set-Content -LiteralPath (Join-Path $srcDir "main.rs") -Encoding UTF8
}

function Invoke-CodexSmoke {
    param(
        [string]$Name,
        [string]$CaseModel,
        [string]$Prompt,
        [string]$CodexHome,
        [string]$CodexCommand,
        [string]$WorkDir,
        [string]$OutputDir,
        [string]$DiagnosticLogPath = "",
        [string[]]$ExpectedJsonlPatterns = @(),
        [string[]]$ExpectedLastPatterns = @(),
        [string[]]$ExpectedDiagnosticPatterns = @()
    )

    $safeName = $Name -replace '[^A-Za-z0-9_.-]', '_'
    $outPath = Join-Path $OutputDir "codex-$safeName.jsonl"
    $errPath = Join-Path $OutputDir "codex-$safeName.err.log"
    $lastPath = Join-Path $OutputDir "codex-$safeName.last.txt"

    $previousCodexHome = $env:CODEX_HOME
    try {
        $env:CODEX_HOME = $CodexHome
        & $CodexCommand exec --json -m $CaseModel -C $WorkDir -s danger-full-access -o $lastPath $Prompt 1> $outPath 2> $errPath
        $exitCode = $LASTEXITCODE
    }
    finally {
        if ($null -ne $previousCodexHome) {
            $env:CODEX_HOME = $previousCodexHome
        }
        else {
            Remove-Item Env:\CODEX_HOME -ErrorAction SilentlyContinue
        }
    }

    if ($exitCode -ne 0) {
        throw "codex smoke '$Name' failed with exit code $exitCode. See $outPath and $errPath"
    }
    if (-not (Test-Path -LiteralPath $lastPath)) {
        throw "codex smoke '$Name' did not write final message file: $lastPath"
    }
    $lastText = Get-Content -LiteralPath $lastPath -Raw
    if ([string]::IsNullOrWhiteSpace($lastText)) {
        throw "codex smoke '$Name' final message is empty: $lastPath"
    }
    Assert-FileContainsPatterns $outPath $ExpectedJsonlPatterns "codex smoke '$Name' JSONL"
    Assert-FileContainsPatterns $lastPath $ExpectedLastPatterns "codex smoke '$Name' final message"
    if (-not [string]::IsNullOrWhiteSpace($DiagnosticLogPath)) {
        Assert-FileContainsPatterns $DiagnosticLogPath $ExpectedDiagnosticPatterns "codex smoke '$Name' diagnostic log"
    }
    Write-Output "[OK] $Name -> $($lastText.Trim())"
}

function New-SmokeScenario {
    param(
        [string]$Name,
        [string]$CaseModel,
        [string]$Prompt,
        [bool]$Enabled = $true,
        [string[]]$ExpectedJsonlPatterns = @(),
        [string[]]$ExpectedLastPatterns = @(),
        [string[]]$ExpectedDiagnosticPatterns = @()
    )
    return [pscustomobject]@{
        Name = $Name
        Model = $CaseModel
        Prompt = $Prompt
        Enabled = $Enabled
        ExpectedJsonlPatterns = $ExpectedJsonlPatterns
        ExpectedLastPatterns = $ExpectedLastPatterns
        ExpectedDiagnosticPatterns = $ExpectedDiagnosticPatterns
    }
}

function Get-ObjectPropertyValue {
    param(
        [object]$Object,
        [string]$Name,
        [object]$Default = $null
    )
    if ($null -eq $Object) {
        return $Default
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $Default
    }
    return $property.Value
}

function ConvertTo-StringArray {
    param([object]$Value)
    if ($null -eq $Value) {
        return @()
    }
    if ($Value -is [string]) {
        if ([string]::IsNullOrWhiteSpace($Value)) {
            return @()
        }
        return @($Value)
    }
    if ($Value -is [System.Array]) {
        $items = @()
        foreach ($item in $Value) {
            if ($null -ne $item -and -not [string]::IsNullOrWhiteSpace($item.ToString())) {
                $items += $item.ToString()
            }
        }
        return $items
    }
    return @($Value.ToString())
}

function Get-ScenarioPatternProperty {
    param(
        [object]$Object,
        [string[]]$Names
    )
    foreach ($name in $Names) {
        $value = Get-ObjectPropertyValue $Object $name $null
        if ($null -ne $value) {
            return ConvertTo-StringArray $value
        }
    }
    return @()
}

function Resolve-ScenarioModel {
    param([string]$ScenarioModel)
    if ([string]::IsNullOrWhiteSpace($ScenarioModel)) {
        return $Model
    }
    $normalized = $ScenarioModel.Trim().ToLowerInvariant()
    switch ($normalized) {
        "default" { return $Model }
        "model" { return $Model }
        '$model' { return $Model }
        "claude" { return $ClaudeModel }
        "claudemodel" { return $ClaudeModel }
        '$claudemodel' { return $ClaudeModel }
        "gpt" { return $GptModel }
        "gptmodel" { return $GptModel }
        '$gptmodel' { return $GptModel }
        default { return $ScenarioModel }
    }
}

function ConvertTo-SmokeScenario {
    param(
        [object]$Value,
        [string]$Source
    )
    $name = [string](Get-ObjectPropertyValue $Value "name" "")
    $caseModel = [string](Get-ObjectPropertyValue $Value "model" "default")
    $prompt = [string](Get-ObjectPropertyValue $Value "prompt" "")
    $enabledValue = Get-ObjectPropertyValue $Value "enabled" $true
    $expectedJsonlPatterns = Get-ScenarioPatternProperty $Value @("expectedJsonlPatterns", "expected_jsonl_patterns")
    $expectedLastPatterns = Get-ScenarioPatternProperty $Value @("expectedLastPatterns", "expected_last_patterns")
    $expectedDiagnosticPatterns = Get-ScenarioPatternProperty $Value @("expectedDiagnosticPatterns", "expected_diagnostic_patterns")

    if ([string]::IsNullOrWhiteSpace($name)) {
        throw "Scenario from $Source is missing required field 'name'."
    }
    if ([string]::IsNullOrWhiteSpace($prompt)) {
        throw "Scenario '$name' from $Source is missing required field 'prompt'."
    }

    return New-SmokeScenario $name (Resolve-ScenarioModel $caseModel) $prompt ([bool]$enabledValue) $expectedJsonlPatterns $expectedLastPatterns $expectedDiagnosticPatterns
}

function Read-SmokeScenariosFromJsonText {
    param(
        [string]$JsonText,
        [string]$Source
    )
    if ([string]::IsNullOrWhiteSpace($JsonText)) {
        return @()
    }
    $parsed = $JsonText | ConvertFrom-Json
    if ($null -eq $parsed) {
        return @()
    }

    $items = @()
    if ($parsed -is [System.Array]) {
        $items = $parsed
    }
    else {
        $scenarios = Get-ObjectPropertyValue $parsed "scenarios" $null
        if ($null -ne $scenarios) {
            $items = @($scenarios)
        }
        else {
            $items = @($parsed)
        }
    }

    $result = [System.Collections.Generic.List[object]]::new()
    foreach ($item in $items) {
        $result.Add((ConvertTo-SmokeScenario $item $Source)) | Out-Null
    }
    return $result.ToArray()
}

function Get-DefaultSmokeScenarios {
    $items = [System.Collections.Generic.List[object]]::new()
    $items.Add((New-SmokeScenario "normal-$Model" $Model "Only answer OK. Do not call tools." $true @() @("^OK\\.?$") @())) | Out-Null
    $items.Add((New-SmokeScenario "pal-version-$Model" $Model "Call mcp__pal version tool, then answer one Chinese sentence with the version." $true @('"type":"mcp_tool_call"', '"server":"pal"', '"tool":"version"') @() @())) | Out-Null
    $items.Add((New-SmokeScenario "tool-search-$Model" $Model "Use tool_search to find pal mcp, then answer one Chinese sentence with the namespace you found." $true @("tool_search") @("mcp__pal") @())) | Out-Null
    $items.Add((New-SmokeScenario "web-search-$Model" $Model "You must call web_search to search pal mcp GitHub, then answer one Chinese sentence summarizing the result." $true @('"type":"mcp_tool_call"', 'tavily_search|web_search_exa|exa_search') @() @())) | Out-Null

    if ($IncludeClaude) {
        $items.Add((New-SmokeScenario "web-search-$ClaudeModel" $ClaudeModel "You must call web_search to search pal mcp GitHub, then answer one Chinese sentence summarizing the result." $true @('"type":"mcp_tool_call"', 'tavily_search|web_search_exa|exa_search') @() @("finishReason.*stop|finish_reason.*stop"))) | Out-Null
    }
    if ($IncludeGptControl) {
        $items.Add((New-SmokeScenario "web-search-$GptModel" $GptModel "You must call web_search to search pal mcp GitHub, then answer one Chinese sentence summarizing the result." $true @("web_search_call|mcp_tool_call") @() @())) | Out-Null
    }

    return $items.ToArray()
}

function Select-SmokeScenarios {
    param([object[]]$AllScenarios)
    if (($null -eq $Scenario) -or ($Scenario.Count -eq 0)) {
        return @($AllScenarios | Where-Object { $_.Enabled })
    }

    $requestedNames = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($name in $Scenario) {
        if (-not [string]::IsNullOrWhiteSpace($name)) {
            $requestedNames.Add($name.Trim()) | Out-Null
        }
    }

    $selected = @($AllScenarios | Where-Object { $requestedNames.Contains($_.Name) })
    $selectedNames = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($item in $selected) {
        $selectedNames.Add($item.Name) | Out-Null
    }
    $missingNames = @()
    foreach ($name in $requestedNames) {
        if (-not $selectedNames.Contains($name)) {
            $missingNames += $name
        }
    }
    if ($missingNames.Count -gt 0) {
        throw "Requested scenario not found: $($missingNames -join ', ')"
    }
    return $selected
}

$allScenarios = [System.Collections.Generic.List[object]]::new()
foreach ($defaultScenario in (Get-DefaultSmokeScenarios)) {
    $allScenarios.Add($defaultScenario) | Out-Null
}
if (-not [string]::IsNullOrWhiteSpace($ScenariosPath)) {
    if (-not (Test-Path -LiteralPath $ScenariosPath)) {
        throw "Scenarios file not found: $ScenariosPath"
    }
    $scenarioJson = Get-Content -LiteralPath $ScenariosPath -Raw
    foreach ($loadedScenario in (Read-SmokeScenariosFromJsonText $scenarioJson $ScenariosPath)) {
        $allScenarios.Add($loadedScenario) | Out-Null
    }
}
if (-not [string]::IsNullOrWhiteSpace($ExtraScenarioJson)) {
    foreach ($extraScenario in (Read-SmokeScenariosFromJsonText $ExtraScenarioJson "ExtraScenarioJson")) {
        $allScenarios.Add($extraScenario) | Out-Null
    }
}

$selectedScenarios = @(Select-SmokeScenarios $allScenarios.ToArray())
if ($selectedScenarios.Count -eq 0) {
    throw "No smoke scenarios selected."
}

if ($ListScenarios) {
    foreach ($item in $allScenarios) {
        Write-Output "$($item.Name) model=$($item.Model) enabled=$($item.Enabled)"
    }
    exit 0
}

$sourceCodexHomePath = Resolve-DefaultCodexHome
$sourceSettingsFile = Resolve-DefaultSettingsPath

if (-not (Test-Path -LiteralPath $sourceCodexHomePath)) {
    throw "Source CODEX_HOME not found: $sourceCodexHomePath"
}
if (-not (Test-Path -LiteralPath $sourceSettingsFile)) {
    throw "Source settings.json not found: $sourceSettingsFile"
}

$cargo = (Get-Command cargo).Path
$codex = (Get-Command codex).Path

New-Item -ItemType Directory -Force -Path $RunRoot | Out-Null
Assert-PathIgnoredByGit $repoRoot $RunRoot
$codexHome = Join-Path $RunRoot "codex-home"
$settingsDir = Join-Path $RunRoot "settings"
$helperDir = Join-Path $RunRoot "helper"
$diagnosticLog = Join-Path $RunRoot "helper-diagnostic.log"
$helperOut = Join-Path $RunRoot "helper.out.log"
$helperErr = Join-Path $RunRoot "helper.err.log"

New-Item -ItemType Directory -Force -Path $codexHome | Out-Null
New-Item -ItemType Directory -Force -Path $settingsDir | Out-Null

Copy-PathIfExists (Join-Path $sourceCodexHomePath "config.toml") (Join-Path $codexHome "config.toml")
Copy-PathIfExists (Join-Path $sourceCodexHomePath "auth.json") (Join-Path $codexHome "auth.json")
Copy-PathIfExists (Join-Path $sourceCodexHomePath "codex-elves-model-catalog.json") (Join-Path $codexHome "codex-elves-model-catalog.json")
Copy-PathIfExists (Join-Path $sourceCodexHomePath "plugins") (Join-Path $codexHome "plugins")
Copy-PathIfExists (Join-Path $sourceCodexHomePath "skills") (Join-Path $codexHome "skills")
$settingsPath = Join-Path $settingsDir "settings.json"
Copy-Item -LiteralPath $sourceSettingsFile -Destination $settingsPath -Force

$configPath = Join-Path $codexHome "config.toml"
if (-not (Test-Path -LiteralPath $configPath)) {
    New-Item -ItemType File -Force -Path $configPath | Out-Null
}

Set-TomlRootString $configPath "model_provider" $ProviderId
Set-TomlRootString $configPath "model" $Model
Set-TomlTableString $configPath "model_providers.$ProviderId" "base_url" "http://127.0.0.1:$Port/v1"
Set-TomlTableString $configPath "model_providers.$ProviderId" "wire_api" "responses"
Assert-NoReservedProxyPortReference $configPath $ReservedProxyPorts
Assert-NoReservedProxyPortReference $settingsPath $ReservedProxyPorts

Write-HelperProject $helperDir $repoRoot
$helperCargo = Join-Path $helperDir "Cargo.toml"
if (-not $SkipBuild) {
    & $cargo build --manifest-path $helperCargo
    if ($LASTEXITCODE -ne 0) {
        throw "helper build failed"
    }
}

$helperExe = Join-Path $helperDir "target\debug\codex-elves-dev-helper-smoke.exe"
if (-not (Test-Path -LiteralPath $helperExe)) {
    throw "helper executable not found: $helperExe"
}

$helperProcess = $null
try {
    $helperProcess = Start-Process -FilePath $helperExe -ArgumentList @($Port.ToString(), $settingsPath, $diagnosticLog) -RedirectStandardOutput $helperOut -RedirectStandardError $helperErr -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 2
    if ($helperProcess.HasExited) {
        throw "dev helper exited early. See $helperOut and $helperErr"
    }

    Write-Output "[OK] helper started on http://127.0.0.1:$Port/v1"
    Write-Output "[INFO] run root: $RunRoot"
    Write-Output "[INFO] reserved proxy ports: $(Format-PortList $ReservedProxyPorts)"
    Write-Output "[INFO] selected scenarios: $((@($selectedScenarios | ForEach-Object { $_.Name })) -join ', ')"

    foreach ($smokeScenario in $selectedScenarios) {
        Invoke-CodexSmoke $smokeScenario.Name $smokeScenario.Model $smokeScenario.Prompt $codexHome $codex $repoRoot $RunRoot $diagnosticLog $smokeScenario.ExpectedJsonlPatterns $smokeScenario.ExpectedLastPatterns $smokeScenario.ExpectedDiagnosticPatterns
    }

    $badFiles = @()
    $badFiles += Get-ChildItem -LiteralPath $RunRoot -Filter "codex-*.jsonl" -File
    if (Test-Path -LiteralPath $diagnosticLog) {
        $badFiles += Get-Item -LiteralPath $diagnosticLog
    }
    $badMatches = @()
    if ($badFiles.Count -gt 0) {
        $badMatches = @(Select-String -Path ($badFiles | ForEach-Object { $_.FullName }) -Pattern 'unsupported|stream disconnected|response\.failed|"type":"error"' -ErrorAction SilentlyContinue)
    }
    if ($badMatches.Count -gt 0) {
        $badMatches | ForEach-Object { Write-Output "[BAD] $($_.Path):$($_.LineNumber): $($_.Line)" }
        throw "smoke logs contain failure markers"
    }

    Write-Output "[OK] no failure markers found"
    Write-Output "[OK] smoke completed"
}
finally {
    if ($helperProcess -and -not $helperProcess.HasExited -and -not $KeepHelper) {
        Stop-Process -Id $helperProcess.Id
        Write-Output "[OK] helper stopped"
    }
    elseif ($helperProcess -and -not $helperProcess.HasExited) {
        Write-Output "[INFO] helper kept running, pid=$($helperProcess.Id)"
    }
}
