# Copy the release APK into dist\ using the same base name as the Windows
# portable exe: <ChineseName>PointHub_v<version>.apk
# Artifacts in dist\ are shipped history: never overwrite an existing one.
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargo = Join-Path $root 'Cargo.toml'
$apkRoot = Join-Path $root 'apps\desktop\src-tauri\gen\android\app\build\outputs\apk'
$dist = Join-Path $root 'dist'

# Prefer the single-ABI arm64 release: it is what earlier releases shipped and
# keeps the download near 8 MB. Fall back to universal when that flavor was not
# built (`build-android.bat release` without --target builds all four ABIs).
$candidates = @(
    (Join-Path $apkRoot 'arm64\release\app-arm64-release.apk'),
    (Join-Path $apkRoot 'universal\release\app-universal-release.apk')
)
$src = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $src) {
    Write-Host '  release APK not found, skip copy'
    exit 0
}

$text = [System.IO.File]::ReadAllText($cargo)
if ($text -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    Write-Host '  cannot read version from Cargo.toml'
    exit 1
}
$version = $Matches[1]

# Keep the naming in sync with build-windows.ps1.
$cnName = [char]0x79EF + [char]0x5206 + [char]0x67A2 + [char]0x7EBD  # 积分枢纽
$name = "$cnName" + 'PointHub_v' + $version + '.apk'
$dst = Join-Path $dist $name

if (-not (Test-Path $dist)) {
    New-Item -ItemType Directory -Path $dist | Out-Null
}
if (Test-Path $dst) {
    Write-Host "  $name already exists in dist\, skipped (artifacts are immutable)"
    exit 0
}

Copy-Item -LiteralPath $src -Destination $dst
$sizeMb = [math]::Round((Get-Item -LiteralPath $dst).Length / 1MB, 1)
Write-Host "  copied -> dist\$name  ($sizeMb MB)"
