Set-StrictMode -Version Latest

function Get-AndroidVersion {
    param([Parameter(Mandatory)][string]$RepositoryRoot)

    $versionPath = Join-Path $RepositoryRoot "apps/codex-elves-android/version.properties"
    $values = @{}
    foreach ($line in Get-Content -LiteralPath $versionPath -Encoding utf8 -ErrorAction Stop) {
        if ($line -match '^\s*(?:#.*)?$') { continue }
        if ($line -notmatch '^\s*(versionName|versionCode)\s*=\s*(\S+)\s*$') {
            throw "Android 独立版本配置包含无效行。"
        }
        if ($values.ContainsKey($Matches[1])) {
            throw "Android 独立版本配置包含重复字段：$($Matches[1])"
        }
        $values[$Matches[1]] = $Matches[2]
    }
    if (-not $values.ContainsKey("versionName") -or
        $values.versionName -cnotmatch '^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$') {
        throw "Android versionName 必须是三段数字版本，例如 0.0.1。"
    }
    $versionCode = 0
    if (-not $values.ContainsKey("versionCode") -or
        $values.versionCode -notmatch '^[1-9][0-9]*$' -or
        -not [int]::TryParse($values.versionCode, [ref]$versionCode) -or
        $versionCode -lt 2 -or $versionCode -gt 2100000000) {
        throw "Android versionCode 必须在 2 到 2100000000 之间，并在每次正式发布时递增。"
    }
    [pscustomobject]@{
        VersionName = $values.versionName
        VersionCode = $versionCode
        ApplicationId = "com.junxin367.codexelves"
        MinSdk = 26
        ApkName = "CodexElves-$($values.versionName)-android.apk"
        Tag = "android-v$($values.versionName)"
    }
}

function Get-AndroidBuildInputs {
    param([Parameter(Mandatory)][string]$RepositoryRoot)

    $root = [System.IO.Path]::GetFullPath($RepositoryRoot)
    $files = [System.Collections.Generic.List[string]]::new()
    $excludedDirectories = @("build", ".gradle", ".idea", ".git", "node_modules", ".pnpm", "target", "dist", "temp", "test-results", "toolchains")
    foreach ($relativeDirectory in @("apps/codex-elves-android", "assets/remote-access")) {
        $directory = Join-Path $root $relativeDirectory
        if (-not (Test-Path -LiteralPath $directory -PathType Container)) {
            throw "Android 构建输入目录不存在：$relativeDirectory"
        }
        $pending = [System.Collections.Generic.Stack[string]]::new()
        $pending.Push($directory)
        while ($pending.Count -gt 0) {
            foreach ($entry in Get-ChildItem -LiteralPath $pending.Pop() -Force -ErrorAction Stop) {
                if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                    throw "Android 构建输入不允许符号链接或目录联接：$($entry.FullName)"
                }
                if ($entry.PSIsContainer) {
                    if ($entry.Name -notin $excludedDirectories) { $pending.Push($entry.FullName) }
                } elseif ($entry.Name -ne "local.properties" -and $entry.Name -notmatch '\.(?:jks|keystore|log)$') {
                    $files.Add($entry.FullName)
                }
            }
        }
    }
    foreach ($script in Get-ChildItem -LiteralPath (Join-Path $root "scripts") -Filter "*android*.ps1" -File) {
        $files.Add($script.FullName)
    }
    $files.Add((Join-Path $root "build.ps1"))
    $entries = @(
        foreach ($path in $files) {
            $relativePath = [System.IO.Path]::GetRelativePath($root, $path).Replace("\", "/")
            [pscustomobject]@{
                path = $relativePath
                sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()
            }
        }
    ) | Sort-Object -Property path -CaseSensitive
    $inputText = ($entries | ForEach-Object { "$($_.path)`0$($_.sha256)" }) -join "`n"
    $digest = [System.Security.Cryptography.SHA256]::HashData([System.Text.Encoding]::UTF8.GetBytes($inputText))
    [pscustomobject]@{
        sha256 = [Convert]::ToHexString($digest).ToLowerInvariant()
        files = @($entries)
    }
}

function Assert-AndroidUpdateManifest {
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)]$Version
    )

    $directory = Join-Path $RepositoryRoot "dist/android"
    $apkPath = Join-Path $directory $Version.ApkName
    $manifestPath = Join-Path $directory "android-update.json"
    $apk = Get-Item -LiteralPath $apkPath -ErrorAction Stop
    if ($apk.Length -le 0) { throw "正式 APK 为空。" }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    foreach ($number in @($manifest.schemaVersion, $manifest.versionCode, $manifest.minSdk, $manifest.apk.size)) {
        if ($number -isnot [int] -and $number -isnot [long]) {
            throw "Android 更新清单的版本、minSdk 和大小字段必须是 JSON 整数。"
        }
    }
    foreach ($text in @($manifest.applicationId, $manifest.versionName, $manifest.apk.name, $manifest.apk.sha256)) {
        if ($text -isnot [string]) { throw "Android 更新清单的文本字段必须是 JSON 字符串。" }
    }
    $hash = (Get-FileHash -LiteralPath $apkPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()
    if ($manifest.schemaVersion -cne 1 -or
        $manifest.applicationId -cne $Version.ApplicationId -or
        $manifest.versionName -cne $Version.VersionName -or
        $manifest.versionCode -cne $Version.VersionCode -or
        $manifest.minSdk -cne $Version.MinSdk -or
        $manifest.apk.name -cne $Version.ApkName -or
        $manifest.apk.size -cne $apk.Length -or
        $manifest.apk.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
        $manifest.apk.sha256 -cne $hash) {
        throw "Android 更新清单与当前版本或实际 APK 不匹配。"
    }
    $rootFields = @($manifest.PSObject.Properties.Name | Sort-Object)
    $apkFields = @($manifest.apk.PSObject.Properties.Name | Sort-Object)
    if (($rootFields -join ",") -cne "apk,applicationId,minSdk,schemaVersion,versionCode,versionName" -or
        ($apkFields -join ",") -cne "name,sha256,size") {
        throw "Android 更新清单字段不符合 schemaVersion=1 合同。"
    }
    $manifest
}

function Get-AndroidBuildDecision {
    param([Parameter(Mandatory)][string]$RepositoryRoot)

    $version = Get-AndroidVersion -RepositoryRoot $RepositoryRoot
    $inputs = Get-AndroidBuildInputs -RepositoryRoot $RepositoryRoot
    $baselinePath = Join-Path $RepositoryRoot "dist/android/build-state.json"
    $reason = "首次构建，缺少成功的正式构建基线"
    if (Test-Path -LiteralPath $baselinePath -PathType Leaf) {
        try {
            $baseline = Get-Content -LiteralPath $baselinePath -Raw -Encoding utf8 | ConvertFrom-Json -ErrorAction Stop
            if ($baseline.schemaVersion -cne 1 -or $baseline.lintPassed -cne $true) {
                $reason = "成功构建基线无效"
            } elseif ($baseline.inputSha256 -cne $inputs.sha256 -or
                $baseline.versionName -cne $version.VersionName -or
                $baseline.versionCode -cne $version.VersionCode) {
                $reason = "Android 源码、资源、版本或构建脚本已变化"
            } else {
                $manifest = Assert-AndroidUpdateManifest -RepositoryRoot $RepositoryRoot -Version $version
                if ($baseline.apk.name -cne $manifest.apk.name -or
                    $baseline.apk.sha256 -cne $manifest.apk.sha256 -or
                    $baseline.apk.size -cne $manifest.apk.size) {
                    $reason = "正式 APK 与成功构建基线不匹配"
                } else {
                    return [pscustomobject]@{ NeedsBuild = $false; Reason = "相关内容未变化，正式 APK 和更新清单校验有效"; Version = $version; Inputs = $inputs }
                }
            }
        } catch {
            $reason = "正式 APK、更新清单或成功基线缺失、损坏或不匹配"
        }
    }
    [pscustomobject]@{ NeedsBuild = $true; Reason = $reason; Version = $version; Inputs = $inputs }
}

function Write-AndroidBuildBaseline {
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)]$Version,
        [Parameter(Mandatory)]$Inputs
    )

    $manifest = Assert-AndroidUpdateManifest -RepositoryRoot $RepositoryRoot -Version $Version
    $currentInputs = Get-AndroidBuildInputs -RepositoryRoot $RepositoryRoot
    if ($currentInputs.sha256 -cne $Inputs.sha256) {
        throw "构建期间 Android 输入发生变化，不记录成功基线；请重新构建。"
    }
    $baseline = [ordered]@{
        schemaVersion = 1
        builtAtUtc = [DateTime]::UtcNow.ToString("o")
        versionName = $Version.VersionName
        versionCode = $Version.VersionCode
        inputSha256 = $Inputs.sha256
        lintPassed = $true
        apk = $manifest.apk
        files = $Inputs.files
    }
    $baselinePath = Join-Path $RepositoryRoot "dist/android/build-state.json"
    $temporaryPath = "$baselinePath.tmp"
    try {
        $baseline | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $temporaryPath -Encoding utf8
        Move-Item -LiteralPath $temporaryPath -Destination $baselinePath -Force
    } finally {
        if (Test-Path -LiteralPath $temporaryPath -PathType Leaf) { Remove-Item -LiteralPath $temporaryPath -Force }
    }
}
