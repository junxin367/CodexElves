[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Tag,
    [string]$Repository = "junxin367/CodexElves",
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [switch]$ValidateOnly
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "android-build-common.ps1")
if ($Repository -cne "junxin367/CodexElves") {
    throw "Android 正式发布仓库必须是 junxin367/CodexElves。"
}
$version = Get-AndroidVersion -RepositoryRoot $RepositoryRoot
if ($Tag -cne $version.Tag) {
    throw "Android 标签必须与独立版本一致，预期 $($version.Tag)，实际 $Tag。"
}
$manifest = Assert-AndroidUpdateManifest -RepositoryRoot $RepositoryRoot -Version $version
$decision = Get-AndroidBuildDecision -RepositoryRoot $RepositoryRoot
if ($decision.NeedsBuild) {
    throw "Android 发布需要当前内容的成功正式构建：$($decision.Reason)"
}
if ($ValidateOnly) {
    [pscustomobject]@{ Tag = $Tag; VersionCode = $manifest.versionCode; Apk = $manifest.apk.name; MakeLatest = $false }
    return
}

# 只读取固定仓库的 Release 和附件，拒绝回退 versionCode。
$releasePagesJson = @(& gh api "repos/$Repository/releases?per_page=100" --paginate --slurp)
if ($LASTEXITCODE -ne 0) { throw "读取既有 Android Release 失败。" }
$releasePages = ($releasePagesJson -join "`n") | ConvertFrom-Json -NoEnumerate -ErrorAction Stop
$releases = @(
    foreach ($page in $releasePages) {
        foreach ($release in $page) { $release }
    }
)
$currentReleases = @($releases | Where-Object { $_.tag_name -ceq $Tag })
if ($currentReleases.Count -gt 1) { throw "Android 标签对应多个 Release，拒绝发布。" }
$currentRelease = if ($currentReleases.Count -eq 1) { $currentReleases[0] } else { $null }

$temporaryBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { Join-Path $RepositoryRoot "temp" }
$verificationRoot = Join-Path $temporaryBase "android-release-verify-$([Guid]::NewGuid().ToString('N'))"
$downloadDirectory = Join-Path $verificationRoot "dist/android"
function Assert-RemoteAndroidRelease {
    New-Item -Path $downloadDirectory -ItemType Directory -Force | Out-Null
    & gh release download $Tag --repo $Repository --pattern $manifest.apk.name --pattern "android-update.json" --dir $downloadDirectory --clobber
    if ($LASTEXITCODE -ne 0) { throw "下载 Android Release 附件以验证发布结果失败。" }
    try {
        $remoteManifest = Assert-AndroidUpdateManifest -RepositoryRoot $verificationRoot -Version $version
        if ($remoteManifest.apk.sha256 -cne $manifest.apk.sha256 -or
            $remoteManifest.apk.size -cne $manifest.apk.size) {
            throw "APK 内容不同"
        }
    } catch {
        throw "同标签 Android Release 附件与本次 APK 不同或校验失败；请提升独立 versionName 和 versionCode，禁止覆盖已发布 APK。"
    }
}

try {
    if ($null -ne $currentRelease -and -not $currentRelease.draft) {
        if ($currentRelease.prerelease) { throw "同标签 Release 已标记为预发布，拒绝改写其状态。" }
        Assert-RemoteAndroidRelease
        Write-Host "Android Release 已发布且 APK、清单一致，跳过重复发布。"
        return
    }
    # 只有新增正式版本或将草稿公开时才要求 code 递增；旧版本原样重跑先走上面的幂等检查。
    foreach ($release in $releases) {
        if ($release.draft -or $release.prerelease -or
            -not $release.tag_name.StartsWith("android-v", [StringComparison]::Ordinal) -or
            $release.tag_name -ceq $Tag) { continue }
        $previousAssets = @($release.assets | Where-Object { $_.name -ceq "android-update.json" })
        if ($previousAssets.Count -ne 1) { throw "既有 Android Release 缺少唯一更新清单：$($release.tag_name)" }
        $previousJson = @(& gh api "repos/$Repository/releases/assets/$($previousAssets[0].id)" -H "Accept: application/octet-stream")
        if ($LASTEXITCODE -ne 0) { throw "读取既有 Android 更新清单失败：$($release.tag_name)" }
        $previous = ($previousJson -join "`n") | ConvertFrom-Json -ErrorAction Stop
        if ($previous.schemaVersion -cne 1 -or $previous.applicationId -cne $version.ApplicationId -or
            ($previous.versionCode -isnot [long] -and $previous.versionCode -isnot [int]) -or $previous.versionCode -lt 2) {
            throw "既有 Android 更新清单无效：$($release.tag_name)"
        }
        if ($manifest.versionCode -le $previous.versionCode) {
            throw "正式发布 versionCode=$($manifest.versionCode) 必须大于 $($release.tag_name) 的 $($previous.versionCode)。"
        }
    }
    if ($null -eq $currentRelease) {
        & gh release create $Tag --repo $Repository --verify-tag --draft --title "Android $($version.VersionName)" `
            --notes "CodexElves Android $($version.VersionName)，请下载正式 APK 安装或在应用中检查更新。" --latest=false
        if ($LASTEXITCODE -ne 0) { throw "创建 Android Release 草稿失败。" }
    }
    $apkPath = Join-Path $RepositoryRoot "dist/android/$($manifest.apk.name)"
    $manifestPath = Join-Path $RepositoryRoot "dist/android/android-update.json"
    & gh release upload $Tag $apkPath $manifestPath --repo $Repository --clobber
    if ($LASTEXITCODE -ne 0) { throw "上传 Android APK 和更新清单失败，Release 保持草稿。" }
    Assert-RemoteAndroidRelease
    & gh release edit $Tag --repo $Repository --draft=false --prerelease=false --latest=false
    if ($LASTEXITCODE -ne 0) { throw "发布已校验的 Android Release 失败，Release 保持草稿。" }
} finally {
    # 仅删除本次创建的两个附件和空目录；不递归清理任何外部路径。
    if (Test-Path -LiteralPath $downloadDirectory -PathType Container) {
        foreach ($name in @($version.ApkName, "android-update.json")) {
            $downloadedPath = Join-Path $downloadDirectory $name
            if (Test-Path -LiteralPath $downloadedPath -PathType Leaf) { Remove-Item -LiteralPath $downloadedPath -Force }
        }
        Remove-Item -LiteralPath $downloadDirectory -Force
        Remove-Item -LiteralPath (Join-Path $verificationRoot "dist") -Force
        Remove-Item -LiteralPath $verificationRoot -Force
    }
}
