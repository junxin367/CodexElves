[CmdletBinding()]
param(
    [ValidateSet("Debug", "Release", "All")]
    [string]$Configuration = "Debug",
    [string]$JavaHome = $env:JAVA_HOME,
    [string]$AndroidHome = $(if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { $env:ANDROID_SDK_ROOT }),
    [string]$GradlePath = "",
    [switch]$SkipLint
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$androidProject = Join-Path $repositoryRoot "apps\codex-elves-android"
$cargoToml = Join-Path $repositoryRoot "Cargo.toml"
$outputDirectory = Join-Path $repositoryRoot "dist\android"
$gradleVersion = "8.11.1"
$gradleSha256 = "f397b287023acdba1e9f6fc5ea72d22dd63669d59ed4a289a29b1a76eee151c6"

if ([string]::IsNullOrWhiteSpace($JavaHome)) {
    throw "必须通过 -JavaHome 或 JAVA_HOME 提供 JDK 17。"
}
if ([string]::IsNullOrWhiteSpace($AndroidHome)) {
    throw "必须通过 -AndroidHome、ANDROID_HOME 或 ANDROID_SDK_ROOT 提供 Android SDK。"
}

$resolvedJavaHome = (Resolve-Path -LiteralPath $JavaHome).Path
$resolvedAndroidHome = (Resolve-Path -LiteralPath $AndroidHome).Path
$javaExecutable = Join-Path $resolvedJavaHome "bin\java.exe"
if (-not (Test-Path -LiteralPath $javaExecutable -PathType Leaf)) {
    throw "JavaHome 中找不到 bin\java.exe：$resolvedJavaHome"
}
if (-not (Test-Path -LiteralPath (Join-Path $resolvedAndroidHome "platforms\android-35") -PathType Container)) {
    throw "Android SDK 缺少 platforms\android-35：$resolvedAndroidHome"
}

$env:JAVA_HOME = $resolvedJavaHome
$env:ANDROID_HOME = $resolvedAndroidHome
$env:ANDROID_SDK_ROOT = $resolvedAndroidHome

function Resolve-GradleExecutable {
    if (-not [string]::IsNullOrWhiteSpace($GradlePath)) {
        return (Resolve-Path -LiteralPath $GradlePath).Path
    }

    $wrapper = Join-Path $androidProject "gradlew.bat"
    $wrapperJar = Join-Path $androidProject "gradle\wrapper\gradle-wrapper.jar"
    if ((Test-Path -LiteralPath $wrapper -PathType Leaf) -and
        (Test-Path -LiteralPath $wrapperJar -PathType Leaf)) {
        return $wrapper
    }

    $installed = Get-Command gradle.bat -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -ne $installed) {
        return $installed.Source
    }

    $toolRoot = Join-Path $repositoryRoot "temp\android-gradle"
    $distributionRoot = Join-Path $toolRoot "gradle-$gradleVersion"
    $download = Join-Path $toolRoot "gradle-$gradleVersion-bin.zip"
    $downloadUrl = "https://services.gradle.org/distributions/gradle-$gradleVersion-bin.zip"
    $downloadedGradle = Join-Path $distributionRoot "bin\gradle.bat"
    if (-not (Test-Path -LiteralPath $downloadedGradle -PathType Leaf)) {
        New-Item -Path $toolRoot -ItemType Directory -Force | Out-Null
        if (-not (Test-Path -LiteralPath $download -PathType Leaf)) {
            Write-Host "下载 Gradle $gradleVersion 到仓库 temp 目录..."
            Invoke-WebRequest -Uri $downloadUrl -OutFile $download
        }
        $actualGradleHash = (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualGradleHash -ne $gradleSha256) {
            Remove-Item -LiteralPath $download -Force
            throw "Gradle 下载摘要不匹配，已删除：$actualGradleHash"
        }
        Expand-Archive -LiteralPath $download -DestinationPath $toolRoot -Force
    }
    return (Resolve-Path -LiteralPath $downloadedGradle).Path
}

$gradleExecutable = Resolve-GradleExecutable

& $javaExecutable -version
if ($LASTEXITCODE -ne 0) {
    throw "JDK smoke test 失败，退出码：$LASTEXITCODE"
}

& $gradleExecutable --version
if ($LASTEXITCODE -ne 0) {
    throw "Gradle smoke test 失败，退出码：$LASTEXITCODE"
}

$cargoContent = Get-Content -LiteralPath $cargoToml -Raw
$workspacePackageMatch = [regex]::Match(
    $cargoContent,
    '(?ms)^\[workspace\.package\]\s*$(.*?)(?=^\[|\z)'
)
if (-not $workspacePackageMatch.Success) {
    throw "根 Cargo.toml 缺少 [workspace.package]。"
}
$versionMatch = [regex]::Match(
    $workspacePackageMatch.Groups[1].Value,
    '(?m)^\s*version\s*=\s*"([^"]+)"\s*$'
)
if (-not $versionMatch.Success) {
    throw "无法从根 Cargo.toml 读取 workspace.package.version。"
}
$productVersion = $versionMatch.Groups[1].Value

$tasks = [System.Collections.Generic.List[string]]::new()
if ($Configuration -eq "Debug" -or $Configuration -eq "All") {
    $tasks.Add("assembleDebug")
    if (-not $SkipLint) {
        $tasks.Add("lintDebug")
    }
}

$signingVariableNames = @(
    "CODEX_ELVES_ANDROID_KEYSTORE",
    "CODEX_ELVES_ANDROID_STORE_PASSWORD",
    "CODEX_ELVES_ANDROID_KEY_ALIAS",
    "CODEX_ELVES_ANDROID_KEY_PASSWORD"
)
$hasSigning = $true
foreach ($variableName in $signingVariableNames) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($variableName))) {
        $hasSigning = $false
        break
    }
}
if ($Configuration -eq "Release" -or $Configuration -eq "All") {
    if (-not $hasSigning) {
        throw "正式 APK 需要完整设置：$($signingVariableNames -join ', ')"
    }
    $tasks.Add("assembleRelease")
    if (-not $SkipLint) {
        $tasks.Add("lintRelease")
    }
}

Push-Location $androidProject
try {
    & $gradleExecutable @tasks --stacktrace
    $gradleExitCode = $LASTEXITCODE
} finally {
    Pop-Location
}
if ($gradleExitCode -ne 0) {
    throw "Android 构建失败，退出码：$gradleExitCode"
}

New-Item -Path $outputDirectory -ItemType Directory -Force | Out-Null
$artifacts = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
if ($Configuration -eq "Debug" -or $Configuration -eq "All") {
    $debugSource = Join-Path $androidProject "app\build\outputs\apk\debug\app-debug.apk"
    $debugTarget = Join-Path $outputDirectory "CodexElves-$productVersion-android-debug.apk"
    Copy-Item -LiteralPath $debugSource -Destination $debugTarget -Force
    $artifacts.Add((Get-Item -LiteralPath $debugTarget))
}
if ($Configuration -eq "Release" -or $Configuration -eq "All") {
    $releaseSource = Join-Path $androidProject "app\build\outputs\apk\release\app-release.apk"
    $releaseTarget = Join-Path $outputDirectory "CodexElves-$productVersion-android.apk"
    Copy-Item -LiteralPath $releaseSource -Destination $releaseTarget -Force
    $artifacts.Add((Get-Item -LiteralPath $releaseTarget))
}

$results = foreach ($artifact in $artifacts) {
    $hash = Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256
    [pscustomobject]@{
        Path = $artifact.FullName
        Bytes = $artifact.Length
        SHA256 = $hash.Hash.ToLowerInvariant()
    }
}
$results | Format-Table -AutoSize
