# Sync the workspace version into the three files that also carry it.
#
# The Windows script does this inline; the Android build needs it too, otherwise
# the APK reports the previous versionName while the file name says the new one.
# Keep this file pure ASCII (PowerShell 5.1 reads .ps1 with the ANSI code page).
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $root 'Cargo.toml'
$text = [System.IO.File]::ReadAllText($cargoToml)
if ($text -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw 'Cannot read version from Cargo.toml'
}
$version = $Matches[1]
Write-Host "  version: $version"

$utf8 = New-Object System.Text.UTF8Encoding($false)

$jsonTargets = @(
    'apps/desktop/src-tauri/tauri.conf.json',
    'apps/desktop/ui/package.json'
)
foreach ($rel in $jsonTargets) {
    $path = Join-Path $root $rel
    $raw = [System.IO.File]::ReadAllText($path)
    $new = [regex]::Replace($raw, '("version"\s*:\s*")[^"]+(")', ('${1}' + $version + '${2}'))
    if ($new -ne $raw) {
        [System.IO.File]::WriteAllText($path, $new, $utf8)
        Write-Host "  synced $rel"
    }
}

$desktopToml = Join-Path $root 'apps/desktop/src-tauri/Cargo.toml'
$raw = [System.IO.File]::ReadAllText($desktopToml)
$new = [regex]::Replace($raw, '(?m)^(version\s*=\s*")[^"]+(")', ('${1}' + $version + '${2}'))
if ($new -ne $raw) {
    [System.IO.File]::WriteAllText($desktopToml, $new, $utf8)
    Write-Host '  synced apps/desktop/src-tauri/Cargo.toml'
}
