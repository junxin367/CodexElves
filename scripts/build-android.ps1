[CmdletBinding()]
param(
    [ValidateSet("Debug", "Release", "All")]
    [string]$Configuration = "Release",
    [string]$JavaHome = $env:JAVA_HOME,
    [string]$AndroidHome = $(if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { $env:ANDROID_SDK_ROOT }),
    [string]$GradlePath = "",
    [switch]$SkipLint
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "android-build-common.ps1")
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$androidProject = Join-Path $repositoryRoot "apps\codex-elves-android"
$outputDirectory = Join-Path $repositoryRoot "dist\android"
$gradleVersion = "8.11.1"
$gradleSha256 = "f397b287023acdba1e9f6fc5ea72d22dd63669d59ed4a289a29b1a76eee151c6"
$androidVersion = Get-AndroidVersion -RepositoryRoot $repositoryRoot
$releaseRequested = $Configuration -in @("Release", "All")

if ($releaseRequested -and $SkipLint) {
    throw "正式 Android 构建必须执行 lint；-SkipLint 仅支持 Debug。"
}
$signingVariableNames = @(
    "CODEX_ELVES_ANDROID_KEYSTORE",
    "CODEX_ELVES_ANDROID_STORE_PASSWORD",
    "CODEX_ELVES_ANDROID_KEY_ALIAS",
    "CODEX_ELVES_ANDROID_KEY_PASSWORD"
)
$hasSigning = @($signingVariableNames | Where-Object {
    [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_))
}).Count -eq 0
if ($releaseRequested -and -not $hasSigning) {
    $signingHelper = if ($env:LOCALAPPDATA) {
        Join-Path $env:LOCALAPPDATA "CodexElves/signing/android/Build-Release.ps1"
    } else { "" }
    if (-not $signingHelper -or -not (Test-Path -LiteralPath $signingHelper -PathType Leaf) -or
        $env:CODEX_ELVES_ANDROID_SIGNING_HELPER_ACTIVE -eq "1") {
        throw "正式 APK 缺少签名配置，需完整设置 $($signingVariableNames -join ', ')，或提供现有本机正式签名辅助脚本。"
    }
    Write-Host "使用本机既有正式签名配置构建 Android Release。"
    $helperArguments = @("-NoProfile", "-File", $signingHelper, "-RepositoryRoot", $repositoryRoot)
    if ($PSBoundParameters.ContainsKey("JavaHome")) { $helperArguments += @("-JavaHome", $JavaHome) }
    if ($PSBoundParameters.ContainsKey("AndroidHome")) { $helperArguments += @("-AndroidHome", $AndroidHome) }
    if ($PSBoundParameters.ContainsKey("GradlePath")) { $helperArguments += @("-GradlePath", $GradlePath) }
    $previousHelperFlag = $env:CODEX_ELVES_ANDROID_SIGNING_HELPER_ACTIVE
    try {
        $env:CODEX_ELVES_ANDROID_SIGNING_HELPER_ACTIVE = "1"
        & (Join-Path $PSHOME "pwsh.exe") @helperArguments
        $helperExitCode = $LASTEXITCODE
        if ($helperExitCode -ne 0) { throw "Android 正式签名辅助构建失败，退出码：$helperExitCode" }
        $helperDecision = Get-AndroidBuildDecision -RepositoryRoot $repositoryRoot
        if ($helperDecision.NeedsBuild) { throw "Android 正式签名辅助构建未生成有效基线：$($helperDecision.Reason)" }
    } finally {
        $env:CODEX_ELVES_ANDROID_SIGNING_HELPER_ACTIVE = $previousHelperFlag
    }
    if ($Configuration -eq "Release") { return }
    $Configuration = "Debug"
    $releaseRequested = $false
}

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

$inputState = Get-AndroidBuildInputs -RepositoryRoot $repositoryRoot

$tasks = [System.Collections.Generic.List[string]]::new()
if ($Configuration -eq "Debug" -or $Configuration -eq "All") {
    $tasks.Add("assembleDebug")
    if (-not $SkipLint) {
        $tasks.Add("lintDebug")
    }
}

if ($releaseRequested) {
    if (-not (Test-Path -LiteralPath $env:CODEX_ELVES_ANDROID_KEYSTORE -PathType Leaf)) {
        throw "正式签名 keystore 文件不存在。"
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
    $debugTarget = Join-Path $outputDirectory "CodexElves-$($androidVersion.VersionName)-android-debug.apk"
    Copy-Item -LiteralPath $debugSource -Destination $debugTarget -Force
    $artifacts.Add((Get-Item -LiteralPath $debugTarget))
}
if ($releaseRequested) {
    $releaseSource = Join-Path $androidProject "app\build\outputs\apk\release\app-release.apk"
    $buildTools = Join-Path $resolvedAndroidHome "build-tools/35.0.0"
    $aapt = Join-Path $buildTools "aapt.exe"
    $apksigner = Join-Path $buildTools "apksigner.bat"
    if (-not (Test-Path -LiteralPath $aapt -PathType Leaf) -or
        -not (Test-Path -LiteralPath $apksigner -PathType Leaf)) {
        throw "正式 APK 校验需要 Android SDK build-tools 35.0.0 的 aapt 和 apksigner。"
    }
    $signatureOutput = @(& $apksigner verify --verbose --print-certs $releaseSource 2>&1)
    if ($LASTEXITCODE -ne 0 -or
        -not ($signatureOutput -match 'certificate SHA-256 digest: [0-9a-fA-F]{64}') -or
        ($signatureOutput -match 'certificate DN:.*CN=Android Debug')) {
        throw "正式 APK 签名校验失败，或 APK 使用了 Debug 签名。"
    }
    $badging = @(& $aapt dump badging $releaseSource 2>&1)
    if ($LASTEXITCODE -ne 0) { throw "无法读取正式 APK 元信息。" }
    $packageMatch = [regex]::Match(($badging -join "`n"), "(?m)^package: name='([^']+)' versionCode='([^']+)' versionName='([^']+)'")
    $sdkMatch = [regex]::Match(($badging -join "`n"), "(?m)^sdkVersion:'([0-9]+)'")
    if (-not $packageMatch.Success -or -not $sdkMatch.Success -or
        $packageMatch.Groups[1].Value -cne $androidVersion.ApplicationId -or
        $packageMatch.Groups[2].Value -cne [string]$androidVersion.VersionCode -or
        $packageMatch.Groups[3].Value -cne $androidVersion.VersionName -or
        $sdkMatch.Groups[1].Value -cne [string]$androidVersion.MinSdk) {
        throw "实际签名 APK 的包名、版本或 minSdk 与 Android 独立版本合同不一致。"
    }
    $releaseTarget = Join-Path $outputDirectory $androidVersion.ApkName
    Copy-Item -LiteralPath $releaseSource -Destination $releaseTarget -Force
    $releaseArtifact = Get-Item -LiteralPath $releaseTarget
    $manifest = [ordered]@{
        schemaVersion = 1
        applicationId = $packageMatch.Groups[1].Value
        versionName = $packageMatch.Groups[3].Value
        versionCode = [int]$packageMatch.Groups[2].Value
        minSdk = [int]$sdkMatch.Groups[1].Value
        apk = [ordered]@{
            name = $releaseArtifact.Name
            size = $releaseArtifact.Length
            sha256 = (Get-FileHash -LiteralPath $releaseTarget -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
    $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputDirectory "android-update.json") -Encoding utf8
    Write-AndroidBuildBaseline -RepositoryRoot $repositoryRoot -Version $androidVersion -Inputs $inputState
    $artifacts.Add($releaseArtifact)
    Write-Host "Android 更新清单与成功构建基线已保存。"
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
