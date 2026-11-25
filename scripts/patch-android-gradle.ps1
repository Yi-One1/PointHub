param(
    [string] $GradlePath = (Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\gen\android\app\build.gradle.kts')
)

# Tauri regenerates gen/android on `tauri android init`, and gen/ is gitignored.
# Keep the three build tweaks here (tracked) so a fresh checkout still builds a
# small, installable release APK:
#   1) release uses the debug keystore (self-distribution; no private key to manage)
#   2) native debug symbols are stripped (keeping them inflates the APK ~2x)
#   3) targetSdk pinned to 36: Android 17 (API 37) blocks local-network TCP unless
#      the app holds ACCESS_LOCAL_NETWORK. Targeting 36 keeps the implicit grant
#      via INTERNET, which is what the PC<->phone LAN pairing relies on.
$marker = '// PointHub: self-distribution patch (scripts/patch-android-gradle.ps1)'
$signLine = '            signingConfig = signingConfigs.getByName("debug")'

if (-not (Test-Path $GradlePath)) {
    Write-Host "  gradle file not found: $GradlePath"
    exit 1
}

$text = [System.IO.File]::ReadAllText($GradlePath)
$lines = $text -split "`n"

# Fast path: patch already in place and no stale symbol/dup lines to clean.
$pattern = '(?s)getByName\("release"\)\s*\{\s*// PointHub: self-distribution patch[^\r\n]*\s*signingConfig = signingConfigs\.getByName\("debug"\)'
$signCount = ([regex]::Matches($text, 'signingConfig = signingConfigs\.getByName\("debug"\)')).Count
if ($text -match $pattern -and $text -notmatch 'jniLibs\.keepDebugSymbols\.add' -and $signCount -eq 1 -and $text -match 'targetSdk\s*=\s*36\b') {
    Write-Host '  gradle patch already applied'
    exit 0
}

$out = New-Object System.Collections.Generic.List[string]
$inRelease = $false
$depth = 0
$hadSign = $false
$changed = $false

foreach ($line in $lines) {
    if ($line -match 'getByName\("release"\)') {
        $out.Add($line) | Out-Null
        # Rebuild the patch at the top of the release block; this also removes
        # duplicates left by earlier manual edits.
        $out.Add("            $marker") | Out-Null
        $out.Add($signLine) | Out-Null
        $hadSign = $true
        $changed = $true
        $inRelease = $true
        $depth = 1
        continue
    }

    if ($line -match 'jniLibs\.keepDebugSymbols\.add' -or $line.Contains($marker)) {
        $changed = $true
        continue
    }

    if ($inRelease -and $line.Contains('signingConfig = signingConfigs.getByName("debug")')) {
        # Already patched (or a duplicate): the line above covers it.
        $changed = $true
        continue
    }

    $out.Add($line) | Out-Null

    if ($inRelease) {
        $depth += ([regex]::Matches($line, '\{').Count - [regex]::Matches($line, '\}').Count)
        if ($depth -le 0) {
            $inRelease = $false
        }
    }
}

if (-not $hadSign) {
    Write-Host '  WARNING: release buildType not found, gradle file left unchanged'
    exit 1
}

if ($changed) {
    $newText = $out -join "`n"
    $newText = [regex]::Replace($newText, 'targetSdk\s*=\s*\d+', 'targetSdk = 36')
    [System.IO.File]::WriteAllText($GradlePath, $newText)
    Write-Host '  gradle patched: release signing + stripped native symbols + targetSdk 36'
} else {
    $newText = [regex]::Replace($text, 'targetSdk\s*=\s*\d+', 'targetSdk = 36')
    if ($newText -ne $text) {
        [System.IO.File]::WriteAllText($GradlePath, $newText)
        Write-Host '  gradle patched: targetSdk 36'
    } else {
        Write-Host '  gradle patch already applied'
    }
}
